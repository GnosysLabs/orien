//! Swapping ORN for SOL: the Bitcoin swap's Orien half, with the Bitcoin
//! script replaced by orien-lock on Solana. See docs/sol-swaps.md.
//!
//! The seller sells ORN and is paid SOL at their own wallet's address. The
//! buyer pays from their own wallet (Phantom or any other) into a one-time
//! address this app holds for the swap; the app locks the SOL from there,
//! and anything left over, or refunded, goes back to the buyer's wallet.
//! The seller's app keeps a small fee key, topped up by the buyer's lock,
//! to pay the claim's network fee: nobody funds anything by hand.

use crate::solana::{self, Keypair, LOCK_LEN, Landed, Lock, Pubkey, Solana};
use crate::swap::{LockRef, Orien, Role, decode_code, encode_code};
use anyhow::{Context, Result, anyhow, bail};
use orien_crypto::Hash32;
use serde::{Deserialize, Serialize};

/// The smallest SOL swap: 0.15 SOL, about $10 when set.
pub const MIN_LAMPORTS: u64 = 150_000_000;
/// How long the seller's ORN stay locked, in epochs: long enough for a
/// buyer to pay from another wallet, and for them to claim the ORN after.
pub const ORN_EPOCHS: u64 = 4;
/// How long the buyer's SOL stay locked.
pub const SOL_LOCK_SECONDS: i64 = 3_600;
/// The SOL lock ends at least this long before the ORN lock, so that once
/// the secret is out the buyer always has time to claim the ORN.
pub const MARGIN_SECONDS: i64 = 3_600;
/// The seller claims only while at least this long is left on the SOL lock.
pub const SAFETY_SECONDS: i64 = 600;
/// A signature's network fee, in lamports.
pub const FEE: u64 = 5_000;
/// The seller's fee key is topped up to the rent minimum plus this many fees.
pub const FEE_KEY_FEES: u64 = 20;
/// Asked of the buyer on top of what's needed, in case Solana's numbers move
/// a little before the lock is made; it comes back with the leftovers.
pub const CUSHION: u64 = 10 * FEE;
/// A transaction not seen landed after this long is sent again.
pub const RESEND_SECONDS: i64 = 90;
/// The seller's wallet leaves the buyer's wallet this long to claim the ORN
/// itself before claiming them for the buyer.
pub const DELIVER_AFTER_SECONDS: i64 = 300;

/// What a SOL swap needs from Solana: [`Solana`] itself, or a test's
/// pretend one.
pub trait Sol {
    fn lock_program(&self) -> Option<Pubkey>;
    fn balance(&self, who: &Pubkey) -> Result<u64>;
    fn lock(&self, at: &Pubkey) -> Result<Option<Lock>>;
    fn time(&self) -> Result<i64>;
    fn rent(&self, len: u64) -> Result<u64>;
    fn blockhash(&self) -> Result<[u8; 32]>;
    fn send(&self, transaction: &[u8]) -> Result<String>;
    fn landed(&self, signature: &str) -> Result<Option<Landed>>;
    fn secret(&self, lock: &Pubkey, hash: &[u8; 32]) -> Result<Option<[u8; 32]>>;
}

impl Sol for Solana {
    fn lock_program(&self) -> Option<Pubkey> {
        self.lock_program
    }
    fn balance(&self, who: &Pubkey) -> Result<u64> {
        Solana::balance(self, who)
    }
    fn lock(&self, at: &Pubkey) -> Result<Option<Lock>> {
        Solana::lock(self, at)
    }
    fn time(&self) -> Result<i64> {
        Solana::time(self)
    }
    fn rent(&self, len: u64) -> Result<u64> {
        Solana::rent(self, len)
    }
    fn blockhash(&self) -> Result<[u8; 32]> {
        Solana::blockhash(self)
    }
    fn send(&self, transaction: &[u8]) -> Result<String> {
        Solana::send(self, transaction)
    }
    fn landed(&self, signature: &str) -> Result<Option<Landed>> {
        Solana::landed(self, signature)
    }
    fn secret(&self, lock: &Pubkey, hash: &[u8; 32]) -> Result<Option<[u8; 32]>> {
        Solana::secret(self, lock, hash)
    }
}

/// What the seller sends the buyer, once the ORN are locked.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SolOffer {
    pub network: String,
    pub orn: u64,
    pub lamports: u64,
    pub hash: [u8; 32],
    pub lock: LockRef,
    pub buyer: Hash32,
    /// Where the SOL go: the seller's own wallet.
    pub payout: String,
    /// The seller's fee key, which the buyer's lock tops up.
    pub fee: String,
}

/// What the buyer sends back, at once: where the SOL will be locked.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SolReply {
    pub hash: [u8; 32],
    /// The lock account's address.
    pub lock: String,
    /// The buyer's own wallet, paid back after the deadline.
    pub refund: String,
}

impl SolOffer {
    pub fn code(&self) -> String {
        encode_code("orien-sol-offer:", self)
    }
    pub fn read(code: &str) -> Result<SolOffer> {
        decode_code("orien-sol-offer:", code, "offer")
    }
}

impl SolReply {
    pub fn code(&self) -> String {
        encode_code("orien-sol-reply:", self)
    }
    pub fn read(code: &str) -> Result<SolReply> {
        decode_code("orien-sol-reply:", code, "reply")
    }
}

/// Where a SOL swap stands.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SolStage {
    /// Seller: the secret is saved; locking the ORN.
    Preparing,
    /// Seller: the ORN are locked; waiting for the buyer's reply.
    WaitingForReply,
    /// Seller: waiting for the SOL to be locked, finalized.
    WaitingForSol,
    /// Seller: the claim is sent; waiting for it to be final.
    Claiming,
    /// Buyer: waiting for the SOL to arrive at the one-time address.
    WaitingForPayment,
    /// Buyer: the lock is sent; waiting for it to land.
    Locking,
    /// Buyer: the SOL are locked; waiting for the seller to take them,
    /// which reveals the secret.
    WaitingForSecret,
    /// Buyer: the secret is known; claiming the ORN.
    ClaimingOrn,
    /// Buyer: the deadline passed; taking the SOL back.
    Refunding,
    Done,
    Cancelled { why: String },
}

/// One SOL swap, as one side's wallet keeps it. Holds secrets: keep the
/// file private, and save it after every step.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SolSwap {
    pub role: Role,
    pub stage: SolStage,
    /// "devnet" for tests, "mainnet" for real swaps.
    pub cluster: String,
    pub orn: u64,
    pub lamports: u64,
    pub hash: [u8; 32],
    /// The seller's from the start; the buyer's once revealed.
    pub secret: Option<[u8; 32]>,
    pub lock: Option<LockRef>,
    /// Seller: the order number the ORN lock has, saved before signing.
    #[serde(default)]
    pub lock_from: u64,
    /// The ORN lock's deadline epoch.
    pub until: u64,
    pub buyer: Hash32,
    /// The seller's payout address.
    pub payout: String,
    /// Seller: its fee key's secret, hex. Buyer: the seller's fee address.
    pub fee: String,
    /// The SOL lock's address, once replied.
    pub sol_lock: Option<String>,
    /// Buyer: the lock account's secret key, hex, to sign its creation.
    #[serde(default)]
    pub sol_lock_key: Option<String>,
    /// Buyer: the one-time address's secret key, hex.
    #[serde(default)]
    pub one_time: Option<String>,
    /// Buyer: the buyer's own wallet.
    #[serde(default)]
    pub refund: String,
    /// Buyer: what to ask the buyer to send, in lamports.
    #[serde(default)]
    pub to_pay: u64,
    /// The SOL lock's deadline, a Unix time, once known.
    pub deadline: Option<i64>,
    /// The transaction on its way, and when it was sent (Solana's clock).
    #[serde(default)]
    pub sent: Option<(String, i64)>,
    /// Buyer: the ORN are claimed.
    #[serde(default)]
    pub orn_claimed: bool,
    /// Seller: the ORN reached the buyer, or their deadline passed.
    #[serde(default)]
    pub orn_delivered: bool,
    /// Seller: when the SOL were final (Solana's clock), to give the buyer's
    /// wallet a few minutes to claim the ORN first.
    #[serde(default)]
    pub paid_at: Option<i64>,
    /// Buyer: what was left at the one-time address has gone back.
    #[serde(default)]
    pub swept: bool,
}

fn random32() -> [u8; 32] {
    let mut bytes = [0u8; 32];
    bitcoin::secp256k1::rand::RngCore::fill_bytes(&mut bitcoin::secp256k1::rand::thread_rng(), &mut bytes);
    bytes
}

fn key_from_hex(text: &str) -> Result<Keypair> {
    let bytes: [u8; 32] = hex::decode(text).ok().and_then(|b| b.try_into().ok()).ok_or_else(|| anyhow!("a saved key is damaged"))?;
    Ok(Keypair::from_secret(&bytes))
}

/// How long is surely left on the ORN lock, in seconds: the epoch could be
/// one later than `latest_epoch` says, and nearly over.
fn orn_seconds_left(orien: &dyn Orien, until: u64) -> Result<i64> {
    let left = until.saturating_sub(orien.latest_epoch()?).saturating_sub(1);
    Ok((left.saturating_mul(orien.epoch_seconds())).min(i64::MAX as u64) as i64)
}

/// What the seller's fee key needs topping up by: nothing while it can pay a
/// few claims, else up to the rent minimum and `FEE_KEY_FEES` fees.
fn topup(sol: &dyn Sol, fee: &Pubkey) -> Result<u64> {
    let (balance, minimum) = (sol.balance(fee)?, sol.rent(0)?);
    Ok(if balance >= minimum + 5 * FEE { 0 } else { (minimum + FEE_KEY_FEES * FEE).saturating_sub(balance) })
}

impl SolSwap {
    fn program(&self, sol: &dyn Sol) -> Result<Pubkey> {
        sol.lock_program().ok_or_else(|| anyhow!("SOL swaps are not open on this network yet"))
    }

    fn sol_lock_address(&self) -> Result<Pubkey> {
        solana::address(self.sol_lock.as_deref().ok_or_else(|| anyhow!("no SOL lock yet"))?)
    }

    fn lock_ref(&self) -> Result<LockRef> {
        self.lock.ok_or_else(|| anyhow!("the ORN are not locked yet"))
    }

    /// Seller: a new swap selling `orn` to `buyer` for `lamports`, paid at
    /// `payout`; `fee_secret` is the wallet's own Solana fee key. Save it,
    /// then `step` it: that locks the ORN.
    pub fn sell(orien: &dyn Orien, cluster: &str, buyer: Hash32, orn: u64, lamports: u64, payout: &str, fee_secret: &[u8; 32]) -> Result<SolSwap> {
        solana::address(payout).map_err(|_| anyhow!("{payout} is not a Solana address"))?;
        if lamports < MIN_LAMPORTS {
            bail!("a SOL swap is at least {} SOL", MIN_LAMPORTS as f64 / solana::LAMPORTS_PER_SOL as f64);
        }
        let secret = random32();
        Ok(SolSwap {
            role: Role::Seller,
            stage: SolStage::Preparing,
            cluster: cluster.into(),
            orn,
            lamports,
            hash: orien_ledger::types::lock_hash(&secret),
            secret: Some(secret),
            lock: None,
            lock_from: orien.next_seq()?,
            until: orien.epoch()? + ORN_EPOCHS,
            buyer,
            payout: payout.into(),
            fee: hex::encode(fee_secret),
            sol_lock: None,
            sol_lock_key: None,
            one_time: None,
            refund: String::new(),
            to_pay: 0,
            deadline: None,
            sent: None,
            orn_claimed: false,
            orn_delivered: false,
            paid_at: None,
            swept: false,
        })
    }

    /// Seller: the offer, once the ORN are locked.
    pub fn offer(&self, network: String) -> Result<SolOffer> {
        Ok(SolOffer {
            network,
            orn: self.orn,
            lamports: self.lamports,
            hash: self.hash,
            lock: self.lock_ref()?,
            buyer: self.buyer,
            payout: self.payout.clone(),
            fee: solana::text(&key_from_hex(&self.fee)?.pubkey()),
        })
    }

    /// Seller: takes in the buyer's reply.
    pub fn accept(&mut self, reply: &SolReply) -> Result<()> {
        if self.role != Role::Seller || self.stage != SolStage::WaitingForReply {
            bail!("this swap is not waiting for a reply");
        }
        if reply.hash != self.hash {
            bail!("that reply is for another swap");
        }
        solana::address(&reply.lock)?;
        self.sol_lock = Some(reply.lock.clone());
        self.stage = SolStage::WaitingForSol;
        Ok(())
    }

    /// Buyer: checks the offer against what validators prove on Orien, and
    /// makes the swap and its reply. The buyer then sends `to_pay` lamports
    /// to `pay_address()` from their own wallet, `refund`, which must have
    /// held SOL before (Solana refuses tiny amounts to brand-new addresses).
    pub fn buy(orien: &dyn Orien, sol: &dyn Sol, cluster: &str, offer: &SolOffer, refund: &str) -> Result<(SolSwap, SolReply)> {
        let refund_key = solana::address(refund).map_err(|_| anyhow!("{refund} is not a Solana address"))?;
        let fee_key = solana::address(&offer.fee).map_err(|_| anyhow!("that offer's fee address is damaged"))?;
        solana::address(&offer.payout).map_err(|_| anyhow!("that offer's payout address is damaged"))?;
        if offer.network != orien.network() {
            bail!("that offer is for another Orien network");
        }
        if offer.buyer != orien.me() {
            bail!("that offer is for {}, not this wallet", offer.buyer);
        }
        if offer.lamports < MIN_LAMPORTS {
            bail!("that offer is below the smallest SOL swap");
        }
        // orien-lock refuses a lock that pays the same address both ways.
        if solana::address(&offer.payout)? == refund_key {
            bail!("your refund address is the seller's payout address: give your own wallet's address");
        }
        if sol.balance(&refund_key)? == 0 {
            bail!("{refund} has never held SOL: give the address of the wallet you'll pay from");
        }
        let state = orien.proven(&offer.lock).context("the seller's locked ORN cannot be proven yet")?;
        if state.secret.is_some() {
            bail!("the ORN in that offer were already claimed");
        }
        let waiting = state.waiting.ok_or_else(|| anyhow!("the seller's ORN are not locked: the offer cannot be trusted"))?;
        if waiting.to != orien.me() || waiting.amount != offer.orn || waiting.hash != offer.hash {
            bail!("the ORN locked on Orien do not match the offer: it cannot be trusted");
        }
        if orn_seconds_left(orien, waiting.until)? < SOL_LOCK_SECONDS + MARGIN_SECONDS {
            bail!("the seller's ORN are locked for too short a time left to swap safely");
        }
        let (one_time, lock) = (random32(), random32());
        let needed = offer.lamports + sol.rent(LOCK_LEN)? + sol.rent(0)? + 3 * FEE + topup(sol, &fee_key)?;
        let lock_address = solana::text(&Keypair::from_secret(&lock).pubkey());
        let swap = SolSwap {
            role: Role::Buyer,
            stage: SolStage::WaitingForPayment,
            cluster: cluster.into(),
            orn: offer.orn,
            lamports: offer.lamports,
            hash: offer.hash,
            secret: None,
            lock: Some(offer.lock),
            lock_from: 0,
            until: waiting.until,
            buyer: offer.buyer,
            payout: offer.payout.clone(),
            fee: offer.fee.clone(),
            sol_lock: Some(lock_address.clone()),
            sol_lock_key: Some(hex::encode(lock)),
            one_time: Some(hex::encode(one_time)),
            refund: refund.into(),
            to_pay: needed + CUSHION,
            deadline: None,
            sent: None,
            orn_claimed: false,
            orn_delivered: false,
            paid_at: None,
            swept: false,
        };
        let reply = SolReply { hash: offer.hash, lock: lock_address, refund: refund.into() };
        Ok((swap, reply))
    }

    /// Buyer: where to send the SOL.
    pub fn pay_address(&self) -> Result<String> {
        Ok(solana::text(&key_from_hex(self.one_time.as_deref().ok_or_else(|| anyhow!("not a buyer's swap"))?)?.pubkey()))
    }

    /// Buyer: a `solana:` link for a QR code, asking for `to_pay`.
    pub fn pay_link(&self) -> Result<String> {
        let amount = format!("{}.{:09}", self.to_pay / solana::LAMPORTS_PER_SOL, self.to_pay % solana::LAMPORTS_PER_SOL);
        Ok(format!("solana:{}?amount={}", self.pay_address()?, amount.trim_end_matches('0').trim_end_matches('.')))
    }

    /// Moves the swap along as far as it can go now. Call it every few
    /// seconds while the swap is open, and save the swap after each call.
    pub fn step(&mut self, sol: &dyn Sol, orien: &dyn Orien) -> Result<()> {
        match (self.role, self.stage.clone()) {
            (Role::Seller, SolStage::Preparing) => self.seller_lock(orien),
            (Role::Seller, SolStage::WaitingForReply) => {
                if orien.epoch()? >= self.until {
                    self.stage = SolStage::Cancelled { why: "no reply came; the ORN came back".into() };
                }
                Ok(())
            }
            (Role::Seller, SolStage::WaitingForSol) => self.seller_wait(sol, orien),
            (Role::Seller, SolStage::Claiming) => self.seller_settle(sol, orien),
            (Role::Seller, SolStage::Done) if !self.orn_delivered => {
                self.seller_deliver(sol, orien);
                Ok(())
            }
            (Role::Buyer, SolStage::WaitingForPayment) => self.buyer_pay(sol, orien),
            (Role::Buyer, SolStage::Locking) => self.buyer_locking(sol),
            (Role::Buyer, SolStage::WaitingForSecret) => self.buyer_wait(sol),
            (Role::Buyer, SolStage::ClaimingOrn) => self.buyer_claim(sol, orien),
            (Role::Buyer, SolStage::Refunding) => self.buyer_refund(sol),
            (Role::Buyer, SolStage::Done | SolStage::Cancelled { .. }) if !self.swept => self.sweep(sol),
            _ => Ok(()),
        }
    }

    /// Whether nothing is left for this wallet to do.
    pub fn finished(&self) -> bool {
        match (&self.stage, self.role) {
            (SolStage::Cancelled { .. }, Role::Seller) => true,
            (SolStage::Done, Role::Seller) => self.orn_delivered,
            (SolStage::Done | SolStage::Cancelled { .. }, Role::Buyer) => self.swept,
            _ => false,
        }
    }

    // ---- Seller ----

    /// Locks the ORN as order number `lock_from` and no other, as the
    /// Bitcoin swap does: a lock made just before a crash is found again.
    fn seller_lock(&mut self, orien: &dyn Orien) -> Result<()> {
        let lock = LockRef { from: orien.me(), seq: self.lock_from };
        if let Ok(state) = orien.proven(&lock)
            && state.waiting.is_some_and(|w| w.hash == self.hash && w.to == self.buyer && w.amount == self.orn)
        {
            self.lock = Some(lock);
            self.stage = SolStage::WaitingForReply;
            return Ok(());
        }
        if orn_seconds_left(orien, self.until)? < SOL_LOCK_SECONDS + MARGIN_SECONDS {
            self.stage = SolStage::Cancelled { why: "it was not started in time".into() };
            return Ok(());
        }
        self.lock = Some(orien.lock(self.lock_from, &self.buyer, self.orn, self.hash, self.until)?);
        self.stage = SolStage::WaitingForReply;
        Ok(())
    }

    /// Whether the SOL lock is the one agreed: the right secret's hash,
    /// paying this payout at least the price, and ending in time.
    fn lock_is_right(&self, lock: &Lock, orien: &dyn Orien, now: i64) -> Result<bool> {
        let payout = solana::address(&self.payout)?;
        let orn_end = now.saturating_add(orn_seconds_left(orien, self.until)?);
        Ok(lock.hash == self.hash
            && lock.seller == payout
            && lock.amount >= self.lamports
            && now + SAFETY_SECONDS < lock.deadline
            && lock.deadline.saturating_add(MARGIN_SECONDS) <= orn_end
            && orien.epoch()? + 1 < self.until)
    }

    fn send_claim(&mut self, sol: &dyn Sol, lock: &Lock, now: i64) -> Result<()> {
        let program = self.program(sol)?;
        let secret = self.secret.ok_or_else(|| anyhow!("the secret is missing"))?;
        let fee = key_from_hex(&self.fee)?;
        let at = self.sol_lock_address()?;
        let tx = solana::transaction(&fee, &[], &[solana::claim(&program, &at, lock, &secret)], sol.blockhash()?)?;
        // Signed means the secret may be out: from here on, settle it.
        self.sent = Some((solana::signature_of(&tx), now));
        self.deadline = Some(lock.deadline);
        self.stage = SolStage::Claiming;
        sol.send(&tx).map(|_| ()).context("could not send the claim; it is sent again on the next step")
    }

    /// Waits for the SOL lock, finalized, and takes the SOL while it is
    /// safe to reveal the secret.
    fn seller_wait(&mut self, sol: &dyn Sol, orien: &dyn Orien) -> Result<()> {
        let at = self.sol_lock_address()?;
        if let Some(lock) = sol.lock(&at)? {
            let now = sol.time()?;
            if self.lock_is_right(&lock, orien, now)? {
                return self.send_claim(sol, &lock, now);
            }
        }
        if orien.epoch()? >= self.until {
            self.stage = SolStage::Cancelled { why: "the SOL did not arrive in time; your ORN came back".into() };
        }
        Ok(())
    }

    /// The claim is out: sends it again until it is final, or until the
    /// lock is gone because it was claimed.
    fn seller_settle(&mut self, sol: &dyn Sol, orien: &dyn Orien) -> Result<()> {
        let (signature, sent_at) = self.sent.clone().ok_or_else(|| anyhow!("no claim was sent"))?;
        let now = sol.time()?;
        match sol.landed(&signature) {
            Ok(Some(Landed::Finalized)) => {
                self.stage = SolStage::Done;
                self.paid_at = Some(now);
                return Ok(());
            }
            Ok(Some(_)) => return Ok(()),
            Ok(None) | Err(_) => {}
        }
        let at = self.sol_lock_address()?;
        match sol.lock(&at)? {
            // Still locked: send the claim again, while the deadline allows.
            Some(lock) if now >= sent_at + RESEND_SECONDS && now < lock.deadline => self.send_claim(sol, &lock, now),
            Some(_) => Ok(()),
            // Gone: claimed (by this claim or another with the secret), or
            // refunded after the deadline.
            None => {
                if sol.secret(&at, &self.hash)?.is_some() {
                    self.stage = SolStage::Done;
                    self.paid_at = Some(now);
                } else if self.deadline.is_some_and(|d| now >= d) && orien.epoch()? >= self.until {
                    self.stage = SolStage::Cancelled { why: "the SOL went back to the buyer; your ORN came back".into() };
                }
                Ok(())
            }
        }
    }

    /// Claims the ORN for the buyer with the secret, a few minutes after the
    /// SOL were final, so they arrive even if the buyer's wallet is closed.
    fn seller_deliver(&mut self, sol: &dyn Sol, orien: &dyn Orien) {
        let (Some(secret), Ok(lock)) = (self.secret, self.lock_ref()) else { return };
        if orien.epoch().is_ok_and(|epoch| epoch >= self.until) {
            self.orn_delivered = true;
            return;
        }
        if let (Some(paid), Ok(now)) = (self.paid_at, sol.time())
            && now < paid + DELIVER_AFTER_SECONDS
        {
            return;
        }
        match orien.still_waiting(&lock) {
            Ok(Some(true)) => {}
            Ok(Some(false)) => {
                self.orn_delivered = true;
                return;
            }
            _ => return,
        }
        if orien.claim(&lock, &self.buyer, self.orn, secret).is_ok() {
            self.orn_delivered = true;
        }
    }

    // ---- Buyer ----

    fn one_time_key(&self) -> Result<Keypair> {
        key_from_hex(self.one_time.as_deref().ok_or_else(|| anyhow!("not a buyer's swap"))?)
    }

    /// Waits for the buyer's SOL at the one-time address, then locks them.
    fn buyer_pay(&mut self, sol: &dyn Sol, orien: &dyn Orien) -> Result<()> {
        let program = self.program(sol)?;
        let at = self.sol_lock_address()?;
        // A lock sent just before a crash, already there.
        if let Some(lock) = sol.lock(&at)? {
            self.deadline = Some(lock.deadline);
            self.stage = SolStage::WaitingForSecret;
            return Ok(());
        }
        let one_time = self.one_time_key()?;
        let balance = sol.balance(&one_time.pubkey())?;
        let now = sol.time()?;
        // Too late to lock safely: whatever came goes back.
        if orn_seconds_left(orien, self.until)? < SOL_LOCK_SECONDS + MARGIN_SECONDS {
            self.stage = SolStage::Cancelled { why: "the SOL did not arrive in time; anything sent goes back to your wallet".into() };
            return Ok(());
        }
        let fee_key = solana::address(&self.fee)?;
        let (deposit, minimum, topup) = (sol.rent(LOCK_LEN)?, sol.rent(0)?, topup(sol, &fee_key)?);
        // The lock, its deposit, the top-up and this transaction's fee, and
        // enough left here to stay alive and pay for a refund and the sweep.
        let needed = self.lamports + deposit + topup + FEE + minimum + 2 * FEE;
        if balance < needed {
            return Ok(());
        }
        let deadline = now + SOL_LOCK_SECONDS;
        let lock_key = key_from_hex(self.sol_lock_key.as_deref().ok_or_else(|| anyhow!("the lock's key is missing"))?)?;
        let terms = Lock {
            hash: self.hash,
            seller: solana::address(&self.payout)?,
            buyer: solana::address(&self.refund)?,
            deadline,
            amount: self.lamports,
            lamports: 0,
        };
        let mut instructions = solana::lock(&program, &one_time.pubkey(), &at, &terms, deposit);
        if topup > 0 {
            instructions.push(solana::transfer(&one_time.pubkey(), &fee_key, topup));
        }
        let tx = solana::transaction(&one_time, &[&lock_key], &instructions, sol.blockhash()?)?;
        self.sent = Some((solana::signature_of(&tx), now));
        self.deadline = Some(deadline);
        self.stage = SolStage::Locking;
        sol.send(&tx).map(|_| ()).context("could not send the lock; it is tried again on the next step")
    }

    fn buyer_locking(&mut self, sol: &dyn Sol) -> Result<()> {
        let (signature, sent_at) = self.sent.clone().ok_or_else(|| anyhow!("no lock was sent"))?;
        match sol.landed(&signature) {
            Ok(Some(level)) if level >= Landed::Confirmed => {
                self.stage = SolStage::WaitingForSecret;
                Ok(())
            }
            Ok(Some(_)) => Ok(()),
            // Refused, or lost: back to waiting for the money, which checks
            // whether the lock is there after all before locking again.
            Err(_) => {
                self.stage = SolStage::WaitingForPayment;
                Ok(())
            }
            Ok(None) => {
                if sol.time()? >= sent_at + RESEND_SECONDS {
                    self.stage = SolStage::WaitingForPayment;
                }
                Ok(())
            }
        }
    }

    /// Watches for the seller's claim, which reveals the secret.
    fn buyer_wait(&mut self, sol: &dyn Sol) -> Result<()> {
        let at = self.sol_lock_address()?;
        if let Some(secret) = sol.secret(&at, &self.hash)? {
            self.secret = Some(secret);
            self.stage = SolStage::ClaimingOrn;
            return Ok(());
        }
        if self.deadline.is_some_and(|deadline| sol.time().is_ok_and(|now| now >= deadline)) {
            self.stage = SolStage::Refunding;
            self.sent = None;
        }
        Ok(())
    }

    fn buyer_claim(&mut self, sol: &dyn Sol, orien: &dyn Orien) -> Result<()> {
        let secret = self.secret.ok_or_else(|| anyhow!("the secret is missing"))?;
        let lock = self.lock_ref()?;
        if let Err(error) = orien.claim(&lock, &orien.me(), self.orn, secret) {
            // Already claimed for us, by the seller's wallet: fine.
            if orien.proven(&lock).ok().is_none_or(|p| p.secret != Some(secret)) {
                if orien.epoch()? >= self.until {
                    self.stage = SolStage::Done;
                    return self.sweep(sol);
                }
                return Err(error);
            }
        }
        self.orn_claimed = true;
        self.stage = SolStage::Done;
        self.sweep(sol)
    }

    /// After the deadline: takes the SOL back, to the buyer's own wallet.
    fn buyer_refund(&mut self, sol: &dyn Sol) -> Result<()> {
        let at = self.sol_lock_address()?;
        let Some(lock) = sol.lock(&at)? else {
            // Gone: refunded, or claimed at the last moment.
            if let Some(secret) = sol.secret(&at, &self.hash)? {
                self.secret = Some(secret);
                self.stage = SolStage::ClaimingOrn;
            } else {
                self.stage = SolStage::Cancelled { why: "the seller did not take the SOL; they went back to your wallet".into() };
            }
            return Ok(());
        };
        let now = sol.time()?;
        if let Some((signature, sent_at)) = &self.sent
            && (sol.landed(signature).ok().flatten().is_some() || now < sent_at + RESEND_SECONDS)
        {
            return Ok(());
        }
        let program = self.program(sol)?;
        let one_time = self.one_time_key()?;
        let tx = solana::transaction(&one_time, &[], &[solana::refund(&program, &at, &lock)], sol.blockhash()?)?;
        self.sent = Some((solana::signature_of(&tx), now));
        sol.send(&tx).map(|_| ()).context("could not send the refund; it is sent again on the next step")
    }

    /// Sends whatever is left at the one-time address back to the buyer's
    /// wallet, leaving it empty, as Solana requires of a small balance.
    fn sweep(&mut self, sol: &dyn Sol) -> Result<()> {
        let one_time = self.one_time_key()?;
        let balance = sol.balance(&one_time.pubkey())?;
        if balance <= FEE {
            self.swept = true;
            return Ok(());
        }
        let refund = solana::address(&self.refund)?;
        let tx = solana::transaction(&one_time, &[], &[solana::transfer(&one_time.pubkey(), &refund, balance - FEE)], sol.blockhash()?)?;
        sol.send(&tx).map(|_| ()).context("could not send the leftovers back; tried again on the next step")
    }
}
