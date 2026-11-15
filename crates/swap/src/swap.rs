//! A swap of ORN for Bitcoin between two people who don't trust each other,
//! step by step, as each side's wallet runs it.
//!
//! The seller has ORN and wants Bitcoin; the buyer has Bitcoin and wants
//! ORN. The seller makes the secret.
//!
//! 1. The buyer tells the seller their Orien address.
//! 2. The seller's wallet saves a new secret, locks the ORN for the buyer on
//!    Orien behind its hash for `ORN_HOURS`, and makes an **offer** code.
//! 3. The buyer's wallet checks, with validators holding more than two
//!    thirds of the stake, that the ORN really are locked for them with time
//!    to spare, makes a Bitcoin lock that the seller can take with the secret
//!    and the buyer can take back after `BTC_BLOCKS`, and makes a **reply**.
//!    The buyer pays the Bitcoin into it.
//! 4. The seller's wallet sees the Bitcoin, checked against its own
//!    transaction and buried `CONFIRMATIONS` deep by two explorers, and
//!    takes it, which shows the secret to everyone. It sends the claim again,
//!    with a higher fee if need be, until it is `SETTLED` deep.
//! 5. The buyer's wallet reads the secret from Bitcoin and claims the ORN.
//!
//! If the buyer never pays, the seller's ORN come back on their own. If the
//! seller never takes the Bitcoin, the buyer's wallet takes it back after
//! `BTC_BLOCKS`; and if the seller takes it even then, the buyer's wallet
//! reads the secret and claims the ORN. The Bitcoin lock ends at least
//! `MARGIN_HOURS` before the ORN lock, so the buyer always has time.

use crate::chain::Explorers;
use crate::htlc::{Coin, Lock, new_key, secret_in};
use anyhow::{Context, Result, anyhow, bail};
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use bitcoin::consensus::encode;
use bitcoin::hashes::{Hash, sha256};
use bitcoin::secp256k1::SecretKey;
use bitcoin::{Address, Amount, Network, OutPoint, PublicKey, Transaction, Txid};
use orien_crypto::Hash32;
use serde::{Deserialize, Serialize};
use std::str::FromStr;

/// How long the seller's ORN stay locked.
pub const ORN_HOURS: u64 = 48;
/// The buyer pays only if the ORN stay locked at least this much longer.
pub const ORN_HOURS_LEFT: u64 = 30;
/// How many Bitcoin blocks (about 12 hours) the buyer's Bitcoin stay locked.
pub const BTC_BLOCKS: u32 = 72;
/// Bitcoin's average time between blocks.
pub const BLOCK_MINUTES: u64 = 10;
/// The Bitcoin lock must end at least this long before the ORN lock.
pub const MARGIN_HOURS: u64 = 18;
/// The seller takes the Bitcoin only while at least this many blocks are
/// left before the buyer could take it back.
pub const SAFETY_BLOCKS: u32 = 6;
/// The seller waits for this many confirmations of the buyer's payment.
pub const CONFIRMATIONS: u32 = 3;
/// A claim or refund counts as settled at this depth.
pub const SETTLED: u32 = 3;
/// The buyer's refund doubles its fee every block, up to nine tenths of
/// the coins, once this few hours are left on the ORN lock.
pub const URGENT_HOURS: u64 = 6;
/// Coins smaller than this cost more to take back than they hold.
pub const DUST_SATS: u64 = 2_000;
/// A claim or refund still unmined after this many blocks is signed again
/// with a quarter more fee, whatever explorers say the network's rate is.
pub const BUMP_BLOCKS: u32 = 3;
/// The buyer stops watching for a payment this many blocks (about a day)
/// after the Bitcoin lock's deadline.
pub const GIVE_UP_BLOCKS: u32 = 144;

/// A locked payment on Orien: its sender and order number.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LockRef {
    pub from: Hash32,
    pub seq: u64,
}

/// A locked payment on Orien.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Waiting {
    pub to: Hash32,
    pub amount: u64,
    pub hash: [u8; 32],
    pub until: u64,
}

/// A locked payment as validators' signatures prove it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LockState {
    /// The epoch the proof holds for.
    pub epoch: u64,
    /// The payment, proven made and not yet claimed as far as the proof
    /// reaches.
    pub waiting: Option<Waiting>,
    /// The secret that claimed it, proven.
    pub secret: Option<[u8; 32]>,
}

/// What a swap needs from Orien.
pub trait Orien {
    /// The Orien network's id, so both sides know they are on the same one.
    fn network(&self) -> String;
    /// This wallet's address.
    fn me(&self) -> Hash32;
    /// The current epoch as checkpoints prove it, checked back to the
    /// genesis file. It can lag, never run ahead: for deciding that a
    /// deadline has passed.
    fn epoch(&self) -> Result<u64>;
    /// The epoch it could be by now, by the clock as well: never earlier
    /// than the real one. For deciding how much time is left.
    fn latest_epoch(&self) -> Result<u64>;
    /// How long an epoch lasts, in seconds.
    fn epoch_seconds(&self) -> u64;
    /// The order number this wallet signs next.
    fn next_seq(&self) -> Result<u64>;
    /// A locked payment, as validators' signatures prove it; an error if
    /// nothing can be proven yet.
    fn proven(&self, lock: &LockRef) -> Result<LockState>;
    /// Locks `amount` for `to` behind `hash` until epoch `until`, as order
    /// number `seq` and no other, and waits until that is final. Finishes
    /// an identical lock already signed rather than signing a second, and
    /// refuses if `seq` is already used.
    fn lock(&self, seq: u64, to: &Hash32, amount: u64, hash: [u8; 32], until: u64) -> Result<LockRef>;
    /// Claims the locked payment with its secret, and waits until that is
    /// final.
    fn claim(&self, lock: &LockRef, amount: u64, secret: [u8; 32]) -> Result<()>;
}

/// What the seller sends the buyer.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Offer {
    pub network: String,
    pub orn: u64,
    pub sats: u64,
    pub hash: [u8; 32],
    pub lock: LockRef,
    pub buyer: Hash32,
    /// The seller's one-time Bitcoin key.
    pub key: String,
}

/// What the buyer sends back.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Reply {
    pub hash: [u8; 32],
    /// The buyer's one-time Bitcoin key.
    pub key: String,
    /// The block height from which the buyer can take the Bitcoin back.
    pub height: u32,
}

fn encode_code<T: Serialize>(prefix: &str, value: &T) -> String {
    format!("{prefix}{}", URL_SAFE_NO_PAD.encode(serde_json::to_vec(value).expect("plain data")))
}

fn decode_code<T: for<'a> Deserialize<'a>>(prefix: &str, code: &str, what: &str) -> Result<T> {
    let body = code.trim().strip_prefix(prefix).ok_or_else(|| anyhow!("that is not a swap {what} code"))?;
    let bytes = URL_SAFE_NO_PAD.decode(body.trim()).map_err(|_| anyhow!("that {what} code is damaged"))?;
    serde_json::from_slice(&bytes).map_err(|_| anyhow!("that {what} code is damaged"))
}

impl Offer {
    pub fn code(&self) -> String {
        encode_code("orien-offer:", self)
    }
    pub fn read(code: &str) -> Result<Offer> {
        decode_code("orien-offer:", code, "offer")
    }
}

impl Reply {
    pub fn code(&self) -> String {
        encode_code("orien-reply:", self)
    }
    pub fn read(code: &str) -> Result<Reply> {
        decode_code("orien-reply:", code, "reply")
    }
}

/// A Bitcoin key from a code: compressed, as P2WSH requires.
fn their_key(text: &str, what: &str) -> Result<PublicKey> {
    let key = PublicKey::from_str(text).map_err(|_| anyhow!("that {what}'s Bitcoin key is damaged"))?;
    if !key.compressed {
        bail!("that {what}'s Bitcoin key is not the compressed kind Bitcoin needs here");
    }
    Ok(key)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Role {
    /// Has ORN, wants Bitcoin; makes the secret.
    Seller,
    /// Has Bitcoin, wants ORN.
    Buyer,
}

/// Where a swap stands.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Stage {
    /// Seller: the secret is saved; locking the ORN.
    Preparing,
    /// Seller: the ORN are locked; waiting for the buyer's reply.
    WaitingForReply,
    /// Seller: waiting for the buyer's Bitcoin to be confirmed. Buyer:
    /// waiting to see the payment.
    WaitingForBitcoin,
    /// Seller: taking the Bitcoin, until the claim is settled.
    Claiming,
    /// Buyer: paid; waiting for the seller to take the Bitcoin.
    WaitingForSecret,
    /// Buyer: the seller did not take the Bitcoin in time; taking it back,
    /// still watching for the secret.
    Refunding,
    /// Buyer: the secret is known; claiming the ORN.
    ClaimingOrn,
    /// Done: each side has what it wanted.
    Done,
    /// The swap did not happen, and this side's coins came, or are coming,
    /// back.
    Cancelled { why: String },
}

/// A coin paid into the swap's Bitcoin lock, as a swap keeps it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Paid {
    pub txid: String,
    pub vout: u32,
    pub sats: u64,
}

impl Paid {
    fn of(coin: &Coin) -> Paid {
        Paid { txid: coin.outpoint.txid.to_string(), vout: coin.outpoint.vout, sats: coin.value.to_sat() }
    }

    fn coin(&self) -> Result<Coin> {
        let txid = Txid::from_str(&self.txid).map_err(|_| anyhow!("a saved coin is damaged"))?;
        Ok(Coin { outpoint: OutPoint { txid, vout: self.vout }, value: Amount::from_sat(self.sats) })
    }
}

/// A claim or refund this side signed, sent again until it is mined.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Sending {
    /// The transaction, as hex.
    pub tx: String,
    /// The fee rate it pays, in satoshis per virtual byte.
    pub fee_rate: u64,
    /// The block height when it was signed: it is signed again with a
    /// higher fee at most once a block.
    pub signed_at: u32,
}

impl Sending {
    fn transaction(&self) -> Result<Transaction> {
        encode::deserialize_hex(&self.tx).map_err(|_| anyhow!("a saved transaction is damaged"))
    }

    fn spends(&self, outpoint: &OutPoint) -> bool {
        self.transaction().is_ok_and(|tx| tx.input.iter().any(|input| input.previous_output == *outpoint))
    }
}

/// One swap, as one side's wallet keeps it. Holds secrets: keep the file
/// private, and save it after every step.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Swap {
    pub role: Role,
    pub stage: Stage,
    /// The Bitcoin network: "bitcoin" for real swaps.
    pub bitcoin: String,
    pub orn: u64,
    pub sats: u64,
    pub hash: [u8; 32],
    /// The secret: the seller's from the start, the buyer's once revealed.
    pub secret: Option<[u8; 32]>,
    /// This side's one-time Bitcoin key, hex.
    pub key: String,
    /// The other side's one-time Bitcoin key, once known.
    pub their_key: Option<String>,
    /// The height from which the buyer can take the Bitcoin back.
    pub height: Option<u32>,
    /// The locked ORN, once locked.
    pub lock: Option<LockRef>,
    /// Seller: the first order number the lock could have, saved before it
    /// is signed, so that a lock made just before a crash is found again
    /// rather than made twice.
    #[serde(default)]
    pub lock_from: u64,
    /// The ORN lock's deadline epoch.
    pub until: u64,
    pub buyer: Hash32,
    /// Where this side's Bitcoin goes: the seller's payout, or the buyer's
    /// refund.
    pub payout: String,
    /// Every coin seen paid into the Bitcoin lock: the seller's claimed
    /// ones, or every one the buyer must take back if not claimed.
    #[serde(default)]
    pub coins: Vec<Paid>,
    /// The claim or refunds being sent.
    #[serde(default)]
    pub sending: Vec<Sending>,
    /// Buyer: the ORN are claimed; any Bitcoin left in the lock is taken
    /// back.
    #[serde(default)]
    pub orn_claimed: bool,
    /// Buyer: the ORN deadline passed before they could be claimed; the
    /// Bitcoin is taken back instead.
    #[serde(default)]
    pub orn_missed: bool,
}

fn hours(epochs: u64, epoch_seconds: u64) -> u64 {
    epochs.saturating_mul(epoch_seconds) / 3_600
}

fn blocks_hours(blocks: u32) -> u64 {
    (blocks as u64 * BLOCK_MINUTES).div_ceil(60)
}

/// The fee cap, in percent of the coins: higher once a deadline is close,
/// when getting mined matters more than the fee.
fn cap(urgent: bool) -> u64 {
    if urgent { 90 } else { 10 }
}

/// Keeps one claim or refund of `coins` on its way: signs it if there is
/// none, sends it again if any explorer lacks it, and once a block at most,
/// while it is not mined, signs it again with a higher fee: when the
/// network's has risen, every `BUMP_BLOCKS` blocks it stays unmined, and
/// every block when `urgent` says time is short. When the fee wanted is over
/// the cap it settles for less, but never below `floor`.
fn keep_sending(
    chain: &Explorers,
    slot: &mut Option<Sending>,
    coins: &[Coin],
    urgent: bool,
    floor: u64,
    make: impl Fn(&[Coin], u64, u64) -> Result<Transaction>,
) -> Result<()> {
    let tip = chain.height()?;
    let current = slot.as_ref().map(Sending::transaction).transpose()?;
    let same = current.as_ref().is_some_and(|tx| {
        tx.input.len() == coins.len() && coins.iter().all(|c| tx.input.iter().any(|i| i.previous_output == c.outpoint))
    });
    // Mined, as every explorer that answers agrees: nothing to do but wait.
    let depth = match &current {
        Some(tx) => chain.depth_least(&tx.compute_txid())?,
        None => None,
    };
    if same && depth.is_some_and(|d| d >= 1) {
        return Ok(());
    }
    let network = chain.fee_rate()?;
    let (paid, signed_at) = slot.as_ref().map_or((0, 0), |s| (s.fee_rate, s.signed_at));
    let waited = tip >= signed_at.saturating_add(BUMP_BLOCKS);
    let stuck = tip > signed_at && (network.saturating_mul(4) > paid.saturating_mul(5) || urgent || waited);
    if !same || stuck {
        let mut want = network;
        if waited {
            want = want.max(paid.saturating_add(paid / 4));
        }
        if urgent {
            want = want.max(paid.saturating_mul(2));
        }
        // A replacement must pay more than what it replaces.
        if current.is_some() {
            want = want.max(paid.saturating_add(1));
        }
        let mut rate = want;
        let error = loop {
            match make(coins, rate, cap(urgent)) {
                Ok(tx) => {
                    *slot = Some(Sending { tx: encode::serialize_hex(&tx), fee_rate: rate, signed_at: tip });
                    return chain.broadcast(&tx).context("could not send the transaction; it is sent again on the next step");
                }
                // Too dear: try a little less, as long as it beats what was paid.
                Err(error) => {
                    let less = rate.saturating_mul(3) / 4;
                    if less <= paid || less < floor.max(1) {
                        break error;
                    }
                    rate = less;
                }
            }
        };
        // Nothing better can be signed now: keep sending what there is.
        match (current, same) {
            (Some(tx), true) => {
                let _ = chain.broadcast(&tx);
                if let Some(sending) = slot {
                    sending.signed_at = tip;
                }
                return Err(error);
            }
            _ => return Err(error),
        }
    }
    if let (Some(tx), None) = (current, depth) {
        chain.broadcast(&tx).context("could not send it again; trying again on the next step")?;
    }
    Ok(())
}

/// The fee rate a transaction spending `coins` pays.
fn rate_of(tx: &Transaction, coins: &[Coin]) -> u64 {
    let inputs: u64 = coins.iter().filter(|c| tx.input.iter().any(|i| i.previous_output == c.outpoint)).map(|c| c.value.to_sat()).sum();
    let outputs: u64 = tx.output.iter().map(|o| o.value.to_sat()).sum();
    inputs.saturating_sub(outputs) / (tx.vsize() as u64).max(1)
}

impl Swap {
    fn network(&self) -> Result<Network> {
        Network::from_str(&self.bitcoin).map_err(|_| anyhow!("unknown Bitcoin network {}", self.bitcoin))
    }

    fn secret_key(&self) -> Result<SecretKey> {
        SecretKey::from_str(&self.key).map_err(|_| anyhow!("the swap's key is damaged"))
    }

    fn public_key(&self) -> Result<PublicKey> {
        Ok(PublicKey::new(self.secret_key()?.public_key(&bitcoin::secp256k1::Secp256k1::new())))
    }

    /// The Bitcoin lock, once both keys and the height are known.
    pub fn bitcoin_lock(&self) -> Result<Lock> {
        let theirs = their_key(self.their_key.as_deref().ok_or_else(|| anyhow!("waiting for the other side"))?, "swap")?;
        let ours = self.public_key()?;
        let (payee, payer) = match self.role {
            Role::Seller => (ours, theirs),
            Role::Buyer => (theirs, ours),
        };
        let height = self.height.ok_or_else(|| anyhow!("waiting for the other side"))?;
        Ok(Lock { hash: self.hash, payee, payer, height })
    }

    /// Where the buyer pays the Bitcoin.
    pub fn bitcoin_address(&self) -> Result<Address> {
        Ok(self.bitcoin_lock()?.address(self.network()?))
    }

    fn payout(&self) -> Result<Address> {
        Address::from_str(&self.payout)?
            .require_network(self.network()?)
            .map_err(|_| anyhow!("{} is not an address on this Bitcoin network", self.payout))
    }

    fn lock_ref(&self) -> Result<LockRef> {
        self.lock.ok_or_else(|| anyhow!("the ORN are not locked yet"))
    }

    /// The claim or refunds signed so far.
    pub fn transactions(&self) -> Vec<Transaction> {
        self.sending.iter().filter_map(|s| s.transaction().ok()).collect()
    }

    /// The coins remembered, plus any new ones in `seen`.
    fn remember(&mut self, seen: &[(Coin, u32)]) -> Result<Vec<Coin>> {
        for (coin, _) in seen {
            let paid = Paid::of(coin);
            if !self.coins.contains(&paid) {
                self.coins.push(paid);
            }
        }
        self.coins.iter().map(Paid::coin).collect()
    }

    /// Hours left on the ORN lock, at the latest the epoch could be.
    fn orn_hours_left(&self, orien: &dyn Orien) -> Result<u64> {
        Ok(hours(self.until.saturating_sub(orien.latest_epoch()?), orien.epoch_seconds()))
    }

    /// Seller: a new swap, with its secret, ready to lock `orn` for `buyer`.
    /// Save it, then `advance` it: that locks the ORN. The secret is saved
    /// before anything is signed, so it can never be lost.
    pub fn sell(orien: &dyn Orien, bitcoin: Network, buyer: Hash32, orn: u64, sats: u64, payout: &str) -> Result<Swap> {
        Address::from_str(payout)?.require_network(bitcoin).map_err(|_| anyhow!("{payout} is not a Bitcoin address"))?;
        if sats < 20_000 {
            bail!("a swap needs at least 20,000 satoshis (0.0002 BTC), so that Bitcoin's fees leave enough");
        }
        let seconds = orien.epoch_seconds().max(1);
        let epochs = (ORN_HOURS * 3_600).div_ceil(seconds).clamp(2, 720);
        if hours(epochs, seconds) < ORN_HOURS_LEFT + 1 {
            bail!("this Orien network's epochs are too long for a swap");
        }
        let mut secret = [0u8; 32];
        bitcoin::secp256k1::rand::RngCore::fill_bytes(&mut bitcoin::secp256k1::rand::thread_rng(), &mut secret);
        let (key, _) = new_key();
        Ok(Swap {
            role: Role::Seller,
            stage: Stage::Preparing,
            bitcoin: bitcoin.to_string(),
            orn,
            sats,
            hash: sha256::Hash::hash(&secret).to_byte_array(),
            secret: Some(secret),
            key: key.display_secret().to_string(),
            their_key: None,
            height: None,
            lock: None,
            lock_from: orien.next_seq()?,
            until: orien.epoch()? + epochs,
            buyer,
            payout: payout.to_string(),
            coins: Vec::new(),
            sending: Vec::new(),
            orn_claimed: false,
            orn_missed: false,
        })
    }

    /// Seller: the offer to send the buyer, once the ORN are locked.
    pub fn offer(&self, network: String) -> Result<Offer> {
        Ok(Offer {
            network,
            orn: self.orn,
            sats: self.sats,
            hash: self.hash,
            lock: self.lock_ref()?,
            buyer: self.buyer,
            key: self.public_key()?.to_string(),
        })
    }

    /// Buyer: checks the offer against what validators' signatures prove on
    /// Orien, and makes the reply. The buyer then pays `sats` to
    /// `bitcoin_address()`.
    pub fn buy(orien: &dyn Orien, chain: &Explorers, bitcoin: Network, offer: &Offer, refund: &str) -> Result<(Swap, Reply)> {
        Address::from_str(refund)?.require_network(bitcoin).map_err(|_| anyhow!("{refund} is not a Bitcoin address"))?;
        if offer.network != orien.network() {
            bail!("that offer is for another Orien network");
        }
        if offer.buyer != orien.me() {
            bail!("that offer is for {}, not this wallet", offer.buyer);
        }
        their_key(&offer.key, "offer")?;
        let state = orien.proven(&offer.lock).context("the seller's locked ORN cannot be proven yet")?;
        if state.secret.is_some() {
            bail!("the ORN in that offer were already claimed");
        }
        let waiting = state.waiting.ok_or_else(|| anyhow!("the seller's ORN are not locked: the offer cannot be trusted"))?;
        if waiting.to != orien.me() || waiting.amount != offer.orn || waiting.hash != offer.hash {
            bail!("the ORN locked on Orien do not match the offer: it cannot be trusted");
        }
        let epoch = state.epoch.max(orien.latest_epoch()?);
        let left = hours(waiting.until.saturating_sub(epoch), orien.epoch_seconds());
        if left < ORN_HOURS_LEFT || left < blocks_hours(BTC_BLOCKS) + MARGIN_HOURS {
            bail!("the seller's ORN are locked for too short a time left to swap safely ({left} hours)");
        }
        let (key, public) = new_key();
        let height = chain.agreed_height()? + BTC_BLOCKS;
        let swap = Swap {
            role: Role::Buyer,
            stage: Stage::WaitingForBitcoin,
            bitcoin: bitcoin.to_string(),
            orn: offer.orn,
            sats: offer.sats,
            hash: offer.hash,
            secret: None,
            key: key.display_secret().to_string(),
            their_key: Some(offer.key.clone()),
            height: Some(height),
            lock: Some(offer.lock),
            lock_from: 0,
            until: waiting.until,
            buyer: offer.buyer,
            payout: refund.to_string(),
            coins: Vec::new(),
            sending: Vec::new(),
            orn_claimed: false,
            orn_missed: false,
        };
        let reply = Reply { hash: offer.hash, key: public.to_string(), height };
        Ok((swap, reply))
    }

    /// Seller: takes in the buyer's reply.
    pub fn accept(&mut self, chain: &Explorers, orien: &dyn Orien, reply: &Reply) -> Result<()> {
        if self.role != Role::Seller || self.stage != Stage::WaitingForReply {
            bail!("this swap is not waiting for a reply");
        }
        if reply.hash != self.hash {
            bail!("that reply is for another swap");
        }
        their_key(&reply.key, "reply")?;
        let tip = chain.height()?;
        if reply.height < tip + BTC_BLOCKS / 3 || reply.height > tip + BTC_BLOCKS * 2 {
            bail!("the reply's Bitcoin deadline is too soon or too far off; ask the buyer for a fresh reply");
        }
        // The Bitcoin lock must end well before the ORN lock.
        if blocks_hours(reply.height - tip) + MARGIN_HOURS > self.orn_hours_left(orien)? {
            bail!("too little time is left on the ORN lock for this reply; start a new swap");
        }
        self.their_key = Some(reply.key.clone());
        self.height = Some(reply.height);
        self.stage = Stage::WaitingForBitcoin;
        Ok(())
    }

    /// Moves the swap on as far as it can go now. Safe to call as often as
    /// you like; save the swap after each call.
    pub fn advance(&mut self, chain: &Explorers, orien: &dyn Orien) -> Result<()> {
        match (self.role, self.stage.clone()) {
            (Role::Seller, Stage::Preparing) => self.seller_lock(orien)?,
            (Role::Seller, Stage::WaitingForReply) => {
                if orien.epoch()? >= self.until {
                    self.stage = Stage::Cancelled { why: "no reply came; the ORN came back".into() };
                }
            }
            (Role::Seller, Stage::WaitingForBitcoin) => self.seller_wait(chain, orien)?,
            (Role::Seller, Stage::Claiming) => self.seller_settle(chain)?,
            (Role::Buyer, Stage::WaitingForBitcoin) => {
                let lock = self.bitcoin_lock()?;
                let seen = chain.coins_any(&lock.address(self.network()?))?;
                let coins = self.remember(&seen)?;
                let paid: u64 = coins.iter().map(|coin| coin.value.to_sat()).sum();
                if paid >= self.sats {
                    self.stage = Stage::WaitingForSecret;
                }
                if !coins.is_empty() {
                    // Paid, or paid short: watched, and taken back if need be.
                    self.buyer_watch(chain, orien)?;
                } else if let Ok(height) = chain.agreed_height()
                    && height >= lock.height.saturating_add(GIVE_UP_BLOCKS)
                {
                    self.stage = Stage::Cancelled { why: "no Bitcoin was paid".into() };
                }
            }
            (Role::Buyer, Stage::WaitingForSecret | Stage::Refunding) => self.buyer_watch(chain, orien)?,
            (Role::Buyer, Stage::ClaimingOrn) => self.buyer_claim(chain, orien)?,
            _ => {}
        }
        Ok(())
    }

    /// Seller: locks the ORN as order number `lock_from`, saved before
    /// anything was signed, and at no other number: a lock made just before
    /// a crash is found there, or that number is seen used, and the ORN are
    /// never locked twice.
    fn seller_lock(&mut self, orien: &dyn Orien) -> Result<()> {
        let lock = LockRef { from: orien.me(), seq: self.lock_from };
        if let Ok(state) = orien.proven(&lock)
            && state.waiting.is_some_and(|w| w.hash == self.hash && w.to == self.buyer && w.amount == self.orn)
        {
            self.lock = Some(lock);
            self.stage = Stage::WaitingForReply;
            return Ok(());
        }
        if orien.latest_epoch()? + 2 > self.until {
            self.stage = Stage::Cancelled { why: "it was not started in time".into() };
            return Ok(());
        }
        self.lock = Some(orien.lock(self.lock_from, &self.buyer, self.orn, self.hash, self.until)?);
        self.stage = Stage::WaitingForReply;
        Ok(())
    }

    /// Whether a reported spend of the lock's coins is really signed: a
    /// claim by the seller's key or a refund by the buyer's.
    fn genuine(&self, tx: &Transaction, coins: &[Coin]) -> bool {
        let Ok(lock) = self.bitcoin_lock() else { return false };
        let key = if secret_in(tx, &self.hash).is_some() { lock.payee } else { lock.payer };
        lock.signed_by(tx, coins, &key)
    }

    /// Seller: waits for the buyer's Bitcoin, buried deep enough as every
    /// explorer agrees, and takes it while the buyer cannot yet take it
    /// back and the ORN are still locked.
    fn seller_wait(&mut self, chain: &Explorers, orien: &dyn Orien) -> Result<()> {
        let lock = self.bitcoin_lock()?;
        let address = lock.address(self.network()?);
        // A claim of ours already out there, sent just before a crash: go
        // on settling it, whatever happens to the explorers.
        let seen: Vec<Coin> = chain.coins_any(&address)?.into_iter().map(|(coin, _)| coin).collect();
        for coin in &seen {
            for tx in chain.spenders(&coin.outpoint)? {
                if secret_in(&tx, &self.hash).is_some() && self.genuine(&tx, &seen) {
                    let coins: Vec<Coin> =
                        seen.iter().copied().filter(|c| tx.input.iter().any(|i| i.previous_output == c.outpoint)).collect();
                    self.coins = coins.iter().map(Paid::of).collect();
                    self.sending = vec![Sending { tx: encode::serialize_hex(&tx), fee_rate: rate_of(&tx, &coins), signed_at: 0 }];
                    self.stage = Stage::Claiming;
                    return Ok(());
                }
            }
        }
        let tip = chain.height()?;
        let mut settled = Vec::new();
        for (coin, depth) in chain.coins_all(&address)? {
            if depth >= CONFIRMATIONS && chain.spenders(&coin.outpoint)?.is_empty() {
                settled.push(coin);
            }
        }
        // Only as many coins as the price, largest first: tiny coins paid
        // in would make the claim large, and its fee too dear to be mined.
        settled.sort_by_key(|coin| std::cmp::Reverse(coin.value));
        let mut take = Vec::new();
        let mut total = Amount::ZERO;
        for coin in settled {
            if total >= Amount::from_sat(self.sats) {
                break;
            }
            total += coin.value;
            take.push(coin);
        }
        let in_time = tip + SAFETY_BLOCKS < lock.height && orien.epoch()? + 1 < self.until;
        if total >= Amount::from_sat(self.sats) && in_time {
            let secret = self.secret.ok_or_else(|| anyhow!("the secret is missing"))?;
            let (key, payout) = (self.secret_key()?, self.payout()?);
            // The secret goes out only in a claim paying the network's fee,
            // with room to raise it three times over if it gets stuck.
            let network = chain.fee_rate()?;
            lock.claim(&take, &secret, &key, &payout, network.saturating_mul(3), cap(false)).map_err(|_| {
                anyhow!("Bitcoin's fees are too high right now to take the Bitcoin safely; waiting for them to fall")
            })?;
            let mut slot = None;
            let sent = keep_sending(chain, &mut slot, &take, false, network, |coins, rate, cap| {
                lock.claim(coins, &secret, &key, &payout, rate, cap)
            });
            // Signed means the secret may be out: from here on, settle it.
            if let Some(sending) = slot {
                self.coins = take.iter().map(Paid::of).collect();
                self.sending = vec![sending];
                self.stage = Stage::Claiming;
            }
            return sent;
        }
        // Too late to reveal the secret safely: as the explorers agree, so
        // that one cannot call it off early.
        if let Ok(height) = chain.agreed_height()
            && height + SAFETY_BLOCKS >= lock.height
        {
            self.stage = Stage::Cancelled {
                why: "the Bitcoin did not arrive in time; your ORN come back when their lock ends".into(),
            };
        } else if orien.epoch()? >= self.until {
            self.stage = Stage::Cancelled { why: "the ORN came back".into() };
        }
        Ok(())
    }

    /// Seller: sends the claim again, with a higher fee if it is stuck, until
    /// every coin it takes is settled as every explorer agrees.
    fn seller_settle(&mut self, chain: &Explorers) -> Result<()> {
        let lock = self.bitcoin_lock()?;
        let coins: Vec<Coin> = self.coins.iter().map(Paid::coin).collect::<Result<_>>()?;
        let mut settled = true;
        let mut mined = false;
        for coin in &coins {
            let mut done = false;
            for tx in chain.spenders(&coin.outpoint)? {
                // What no one signed is no one's spend.
                if !self.genuine(&tx, &coins) {
                    continue;
                }
                let txid = tx.compute_txid();
                let depth = chain.depth_all(&txid).ok().flatten().unwrap_or(0);
                if secret_in(&tx, &self.hash).is_some() {
                    // Ours: this claim, or an earlier version that got mined.
                    done |= depth >= SETTLED;
                    mined |= chain.depth_least(&txid)?.is_some_and(|d| d >= 1);
                } else if depth >= SETTLED {
                    self.stage = Stage::Cancelled { why: "the buyer took the Bitcoin back before the claim was mined".into() };
                    return Ok(());
                }
            }
            settled &= done;
        }
        if settled {
            self.stage = Stage::Done;
            return Ok(());
        }
        // A claim is mined, as every explorer that answers agrees: wait for
        // it to settle rather than sign one that could no longer be mined.
        if mined {
            return Ok(());
        }
        let late = chain.height()? + SAFETY_BLOCKS >= lock.height;
        let secret = self.secret.ok_or_else(|| anyhow!("the secret is missing"))?;
        let (key, payout) = (self.secret_key()?, self.payout()?);
        let mut slot = self.sending.pop();
        let sent = keep_sending(chain, &mut slot, &coins, late, 0, |coins, rate, cap| lock.claim(coins, &secret, &key, &payout, rate, cap));
        self.sending = slot.into_iter().collect();
        sent
    }

    /// Buyer: watches every coin paid to the lock, believing only spends
    /// that are really signed. The secret, wherever it shows up, means
    /// claiming the ORN; from the deadline on, every coin not claimed is
    /// taken back, each on its own, so that no coin can hold up another.
    fn buyer_watch(&mut self, chain: &Explorers, orien: &dyn Orien) -> Result<()> {
        let lock = self.bitcoin_lock()?;
        let seen = chain.coins_any(&lock.address(self.network()?))?;
        let coins = self.remember(&seen)?;
        let mut open = Vec::new();
        let mut finished = true;
        let mut learned = None;
        for coin in &coins {
            let spenders: Vec<Transaction> =
                chain.spenders(&coin.outpoint)?.into_iter().filter(|tx| self.genuine(tx, &coins)).collect();
            if let Some(secret) = spenders.iter().find_map(|tx| secret_in(tx, &self.hash)) {
                // Taken by the seller, and the ORN are ours: settled.
                if self.orn_claimed {
                    continue;
                }
                if !self.orn_missed {
                    learned = Some(secret);
                }
            }
            let mut refunded = false;
            for tx in spenders.iter().filter(|tx| secret_in(tx, &self.hash).is_none()) {
                if chain.depth_all(&tx.compute_txid()).ok().flatten().unwrap_or(0) >= SETTLED {
                    refunded = true;
                } else if !self.sending.iter().any(|s| s.spends(&coin.outpoint)) {
                    // Our own refund, sent just before a crash.
                    self.sending.push(Sending { tx: encode::serialize_hex(tx), fee_rate: rate_of(tx, &[*coin]), signed_at: 0 });
                }
            }
            if refunded {
                continue;
            }
            open.push(*coin);
            // Dust, and coins no explorer can vouch for, cannot be taken
            // back; they do not keep the swap open.
            if coin.value.to_sat() >= DUST_SATS && chain.depth_least(&coin.outpoint.txid).ok().flatten().is_some() {
                finished = false;
            }
        }
        if let Some(secret) = learned {
            self.secret = Some(secret);
            self.stage = Stage::ClaimingOrn;
        } else if finished && !coins.is_empty() {
            self.stage = if self.orn_claimed {
                Stage::Done
            } else if self.orn_missed {
                Stage::Cancelled { why: "the ORN were not claimed before their deadline; the Bitcoin came back".into() }
            } else {
                Stage::Cancelled { why: "the seller did not take the Bitcoin; it came back".into() }
            };
            return Ok(());
        }
        self.refund(chain, orien, &lock, open)
    }

    /// Buyer: from the deadline on, takes back each coin still there once
    /// it is mined (a coin still waiting could hold its refund up). Near the
    /// ORN deadline the fee rises every block: the refund must be mined
    /// before the seller could take the Bitcoin after the ORN went back.
    fn refund(&mut self, chain: &Explorers, orien: &dyn Orien, lock: &Lock, open: Vec<Coin>) -> Result<()> {
        if chain.height()? < lock.height {
            return Ok(());
        }
        if !self.orn_claimed && self.stage != Stage::ClaimingOrn {
            self.stage = Stage::Refunding;
        }
        let urgent = !self.orn_claimed && self.orn_hours_left(orien).unwrap_or(0) <= URGENT_HOURS;
        let (key, payout) = (self.secret_key()?, self.payout()?);
        let mut problem = None;
        for coin in open {
            if chain.depth_any(&coin.outpoint.txid)?.is_none_or(|d| d < 1) {
                continue;
            }
            let at = self.sending.iter().position(|s| s.spends(&coin.outpoint));
            let mut slot = at.map(|i| self.sending.remove(i));
            let sent = keep_sending(chain, &mut slot, &[coin], urgent, 0, |coins, rate, cap| lock.refund(coins, &key, &payout, rate, cap));
            self.sending.extend(slot);
            if let Err(error) = sent {
                problem.get_or_insert(error);
            }
        }
        problem.map_or(Ok(()), Err)
    }

    /// Buyer: claims the ORN with the secret. Done once the claim is final,
    /// or the latest checkpoint proves the ORN claimed with it, since only
    /// this wallet can claim them. Until then any refund under way keeps
    /// going, and if the ORN deadline passes the Bitcoin is taken back.
    fn buyer_claim(&mut self, chain: &Explorers, orien: &dyn Orien) -> Result<()> {
        let secret = self.secret.ok_or_else(|| anyhow!("the secret is missing"))?;
        let lock = self.lock_ref()?;
        if let Err(error) = orien.claim(&lock, self.orn, secret) {
            let proven = orien.proven(&lock).ok();
            if proven.as_ref().is_none_or(|p| p.secret != Some(secret)) {
                if orien.epoch()? >= self.until {
                    self.orn_missed = true;
                    self.stage = Stage::Refunding;
                    return self.buyer_watch(chain, orien);
                }
                let _ = self.buyer_watch(chain, orien);
                return Err(error);
            }
        }
        self.orn_claimed = true;
        // Any Bitcoin the seller left in the lock comes back.
        self.stage = Stage::WaitingForSecret;
        self.buyer_watch(chain, orien)
    }
}
