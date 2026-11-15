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

use crate::chain::{Chain, paid_to};
use crate::htlc::{Coin, Lock, new_key, secret_in};
use anyhow::{Context, Result, anyhow, bail};
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use bitcoin::consensus::encode;
use bitcoin::hashes::{Hash, sha256};
use bitcoin::secp256k1::SecretKey;
use bitcoin::{Address, Amount, Network, PublicKey, Transaction};
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

/// A locked payment on Orien: its sender and order number.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LockRef {
    pub from: Hash32,
    pub seq: u64,
}

/// A locked payment still waiting on Orien.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Waiting {
    pub to: Hash32,
    pub amount: u64,
    pub hash: [u8; 32],
    pub until: u64,
}

/// A locked payment as validators holding more than two thirds of the stake
/// report it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LockState {
    /// The latest epoch they were in.
    pub epoch: u64,
    pub waiting: Option<Waiting>,
    /// The secret that claimed it, if one has.
    pub secret: Option<[u8; 32]>,
}

/// What a swap needs from Orien.
pub trait Orien {
    /// The Orien network's id, so both sides know they are on the same one.
    fn network(&self) -> String;
    /// This wallet's address.
    fn me(&self) -> Hash32;
    fn epoch(&self) -> Result<u64>;
    /// How long an epoch lasts, in seconds.
    fn epoch_seconds(&self) -> u64;
    /// A locked payment, as validators holding more than two thirds of the
    /// stake agree on it; an error if they don't agree.
    fn locked(&self, lock: &LockRef, hash: &[u8; 32]) -> Result<LockState>;
    /// Locks `amount` for `to` behind `hash` until epoch `until`, and waits
    /// until that is final.
    fn lock(&self, to: &Hash32, amount: u64, hash: [u8; 32], until: u64) -> Result<LockRef>;
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
    /// The ORN lock's deadline epoch.
    pub until: u64,
    pub buyer: Hash32,
    /// Where this side's Bitcoin goes: the seller's payout, or the buyer's
    /// refund.
    pub payout: String,
    /// The latest claim or refund this side signed, as hex: sent again until
    /// it is settled.
    pub tx: Option<String>,
    /// The fee rate it pays, in satoshis per virtual byte.
    pub fee_rate: u64,
}

fn hours(epochs: u64, epoch_seconds: u64) -> u64 {
    epochs.saturating_mul(epoch_seconds) / 3_600
}

fn blocks_hours(blocks: u32) -> u64 {
    (blocks as u64 * BLOCK_MINUTES).div_ceil(60)
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
            until: orien.epoch()? + epochs,
            buyer,
            payout: payout.to_string(),
            tx: None,
            fee_rate: 0,
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

    /// Buyer: checks the offer against Orien and makes the reply. The buyer
    /// then pays `sats` to `bitcoin_address()`.
    pub fn buy(orien: &dyn Orien, chain: &dyn Chain, bitcoin: Network, offer: &Offer, refund: &str) -> Result<(Swap, Reply)> {
        Address::from_str(refund)?.require_network(bitcoin).map_err(|_| anyhow!("{refund} is not a Bitcoin address"))?;
        if offer.network != orien.network() {
            bail!("that offer is for another Orien network");
        }
        if offer.buyer != orien.me() {
            bail!("that offer is for {}, not this wallet", offer.buyer);
        }
        their_key(&offer.key, "offer")?;
        let state = orien.locked(&offer.lock, &offer.hash)?;
        let waiting = state.waiting.ok_or_else(|| anyhow!("the seller's ORN are not locked: the offer cannot be trusted"))?;
        if waiting.to != orien.me() || waiting.amount != offer.orn || waiting.hash != offer.hash {
            bail!("the ORN locked on Orien do not match the offer: it cannot be trusted");
        }
        let epoch = state.epoch.max(orien.epoch()?);
        let left = hours(waiting.until.saturating_sub(epoch), orien.epoch_seconds());
        if left < ORN_HOURS_LEFT || left < blocks_hours(BTC_BLOCKS) + MARGIN_HOURS {
            bail!("the seller's ORN are locked for too short a time left to swap safely ({left} hours)");
        }
        let (key, public) = new_key();
        let height = chain.height()? + BTC_BLOCKS;
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
            until: waiting.until,
            buyer: offer.buyer,
            payout: refund.to_string(),
            tx: None,
            fee_rate: 0,
        };
        let reply = Reply { hash: offer.hash, key: public.to_string(), height };
        Ok((swap, reply))
    }

    /// Seller: takes in the buyer's reply.
    pub fn accept(&mut self, chain: &dyn Chain, orien: &dyn Orien, reply: &Reply) -> Result<()> {
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
        let left = hours(self.until.saturating_sub(orien.epoch()?), orien.epoch_seconds());
        if blocks_hours(reply.height - tip) + MARGIN_HOURS > left {
            bail!("too little time is left on the ORN lock for this reply; start a new swap");
        }
        self.their_key = Some(reply.key.clone());
        self.height = Some(reply.height);
        self.stage = Stage::WaitingForBitcoin;
        Ok(())
    }

    /// Moves the swap on as far as it can go now. Safe to call as often as
    /// you like; save the swap after each call.
    pub fn advance(&mut self, chain: &dyn Chain, orien: &dyn Orien) -> Result<()> {
        match (self.role, self.stage.clone()) {
            (Role::Seller, Stage::Preparing) => {
                if orien.epoch()? + 2 > self.until {
                    self.stage = Stage::Cancelled { why: "it was not started in time".into() };
                } else {
                    self.lock = Some(orien.lock(&self.buyer, self.orn, self.hash, self.until)?);
                    self.stage = Stage::WaitingForReply;
                }
            }
            (Role::Seller, Stage::WaitingForReply) => {
                if orien.epoch()? >= self.until {
                    self.stage = Stage::Cancelled { why: "no reply came; the ORN came back".into() };
                }
            }
            (Role::Seller, Stage::WaitingForBitcoin) => self.seller_wait(chain, orien)?,
            (Role::Seller, Stage::Claiming) => self.seller_settle(chain)?,
            (Role::Buyer, Stage::WaitingForBitcoin) => {
                if !paid_to(chain, &self.bitcoin_address()?)?.is_empty() {
                    self.stage = Stage::WaitingForSecret;
                } else if chain.height()? >= self.height.unwrap_or(0) {
                    self.stage = Stage::Cancelled { why: "no Bitcoin was paid in time".into() };
                }
            }
            (Role::Buyer, Stage::WaitingForSecret | Stage::Refunding) => self.buyer_watch(chain)?,
            (Role::Buyer, Stage::ClaimingOrn) => self.buyer_claim(orien)?,
            _ => {}
        }
        Ok(())
    }

    fn unspent(&self, chain: &dyn Chain, coins: &[(Coin, u32)]) -> Result<Vec<(Coin, u32)>> {
        let mut unspent = Vec::new();
        for (coin, depth) in coins {
            if chain.spender(&coin.outpoint)?.is_none() {
                unspent.push((*coin, *depth));
            }
        }
        Ok(unspent)
    }

    /// Signs `make` at the network's fee rate and, if that works, moves to
    /// `stage`, keeps the transaction, and sends it. Kept before it is sent;
    /// the caller saves the swap.
    fn send(&mut self, chain: &dyn Chain, stage: Stage, make: impl Fn(u64) -> Result<Transaction>) -> Result<()> {
        let rate = chain.fee_rate()?.max(self.fee_rate);
        let tx = make(rate)?;
        self.stage = stage;
        self.tx = Some(encode::serialize_hex(&tx));
        self.fee_rate = rate;
        chain.broadcast(&tx).context("could not send the transaction; it is sent again on the next step")
    }

    fn our_tx(&self) -> Result<Option<Transaction>> {
        self.tx.as_deref().map(|hex| encode::deserialize_hex(hex).map_err(|_| anyhow!("the saved transaction is damaged"))).transpose()
    }

    fn seller_wait(&mut self, chain: &dyn Chain, orien: &dyn Orien) -> Result<()> {
        let lock = self.bitcoin_lock()?;
        let tip = chain.height()?;
        let coins = self.unspent(chain, &paid_to(chain, &lock.address(self.network()?))?)?;
        let settled: Vec<Coin> = coins.iter().filter(|(_, depth)| *depth >= CONFIRMATIONS).map(|(coin, _)| *coin).collect();
        let total: Amount = settled.iter().map(|coin| coin.value).sum();
        if total >= Amount::from_sat(self.sats) && tip + SAFETY_BLOCKS < lock.height {
            let secret = self.secret.ok_or_else(|| anyhow!("the secret is missing"))?;
            let (key, payout) = (self.secret_key()?, self.payout()?);
            self.send(chain, Stage::Claiming, |rate| lock.claim(&settled, &secret, &key, &payout, rate))?;
        } else if tip + SAFETY_BLOCKS >= lock.height {
            self.stage = Stage::Cancelled {
                why: "the Bitcoin did not arrive in time; your ORN come back when their lock ends".into(),
            };
        } else if orien.epoch()? >= self.until {
            self.stage = Stage::Cancelled { why: "the ORN came back".into() };
        }
        Ok(())
    }

    /// Seller: sends the claim again, with a higher fee if it is stuck, until
    /// every coin it takes is settled.
    fn seller_settle(&mut self, chain: &dyn Chain) -> Result<()> {
        let claim = self.our_tx()?.ok_or_else(|| anyhow!("the claim is missing"))?;
        let mut settled = true;
        for input in &claim.input {
            match chain.spender(&input.previous_output)? {
                Some(tx) if secret_in(&tx, &self.hash).is_some() => {
                    if chain.confirmations(&tx.compute_txid())?.unwrap_or(0) < SETTLED {
                        settled = false;
                    }
                }
                Some(_) => {
                    self.stage = Stage::Cancelled { why: "the buyer took the Bitcoin back before the claim was mined".into() };
                    return Ok(());
                }
                None => settled = false,
            }
        }
        if settled {
            self.stage = Stage::Done;
            return Ok(());
        }
        let txid = claim.compute_txid();
        let lock = self.bitcoin_lock()?;
        let depth = chain.confirmations(&txid)?;
        let late = chain.height()? + SAFETY_BLOCKS >= lock.height;
        let rate = chain.fee_rate()?;
        // Not mined yet: raise the fee if the network's has risen, and
        // double it once the buyer's deadline is close.
        if depth == Some(0) && (rate * 4 > self.fee_rate * 5 || late) {
            let coins: Vec<Coin> = claim
                .input
                .iter()
                .map(|input| -> Result<Coin> {
                    let tx = chain.transaction(&input.previous_output.txid)?.ok_or_else(|| anyhow!("a paid coin vanished"))?;
                    let value = tx.output.get(input.previous_output.vout as usize).ok_or_else(|| anyhow!("a paid coin vanished"))?.value;
                    Ok(Coin { outpoint: input.previous_output, value })
                })
                .collect::<Result<_>>()?;
            let secret = self.secret.ok_or_else(|| anyhow!("the secret is missing"))?;
            let (key, payout) = (self.secret_key()?, self.payout()?);
            let floor = if late { self.fee_rate * 2 } else { 0 };
            self.fee_rate = self.fee_rate.max(floor);
            return self.send(chain, Stage::Claiming, |rate| lock.claim(&coins, &secret, &key, &payout, rate));
        }
        if depth.is_none() {
            chain.broadcast(&claim).context("could not send the claim again")?;
        }
        Ok(())
    }

    /// Buyer: watches every coin paid to the lock. The secret, wherever it
    /// shows up, means claiming the ORN; from the deadline on, every coin
    /// still there is taken back.
    fn buyer_watch(&mut self, chain: &dyn Chain) -> Result<()> {
        let lock = self.bitcoin_lock()?;
        let coins = paid_to(chain, &lock.address(self.network()?))?;
        let mut unspent = Vec::new();
        let mut refunded = true;
        for (coin, _) in &coins {
            match chain.spender(&coin.outpoint)? {
                Some(tx) => {
                    if let Some(secret) = secret_in(&tx, &self.hash) {
                        self.secret = Some(secret);
                        self.stage = Stage::ClaimingOrn;
                        return Ok(());
                    }
                    if chain.confirmations(&tx.compute_txid())?.unwrap_or(0) < SETTLED {
                        refunded = false;
                    }
                }
                None => {
                    unspent.push(*coin);
                    refunded = false;
                }
            }
        }
        if self.stage == Stage::Refunding && refunded && !coins.is_empty() {
            self.stage = Stage::Cancelled { why: "the seller did not take the Bitcoin; it came back".into() };
            return Ok(());
        }
        if chain.height()? < lock.height || unspent.is_empty() {
            return Ok(());
        }
        // From the deadline on: take back every coin still there, sending
        // the refund again (with a higher fee if it is stuck) until it is
        // mined.
        let current = self.our_tx()?;
        let same = current.as_ref().is_some_and(|tx| {
            tx.input.len() == unspent.len() && tx.input.iter().all(|input| unspent.iter().any(|c| c.outpoint == input.previous_output))
        });
        let depth = match &current {
            Some(tx) if same => chain.confirmations(&tx.compute_txid())?,
            _ => None,
        };
        let rate = chain.fee_rate()?;
        if !same || depth == Some(0) && rate * 4 > self.fee_rate * 5 {
            let (key, payout) = (self.secret_key()?, self.payout()?);
            return self.send(chain, Stage::Refunding, |rate| lock.refund(&unspent, &key, &payout, rate));
        }
        if let (Some(tx), None) = (current, depth) {
            chain.broadcast(&tx).context("could not send the refund again")?;
        }
        Ok(())
    }

    /// Buyer: claims the ORN with the secret, and is done only once the
    /// claim is final or validators holding more than two thirds of the
    /// stake show the ORN claimed with it.
    fn buyer_claim(&mut self, orien: &dyn Orien) -> Result<()> {
        let secret = self.secret.ok_or_else(|| anyhow!("the secret is missing"))?;
        let lock = self.lock_ref()?;
        let state = orien.locked(&lock, &self.hash)?;
        if state.waiting.is_some() {
            orien.claim(&lock, self.orn, secret)?;
            self.stage = Stage::Done;
        } else if state.secret == Some(secret) {
            self.stage = Stage::Done;
        } else if state.epoch.max(orien.epoch()?) >= self.until {
            self.stage = Stage::Cancelled { why: "the ORN were not claimed before their deadline".into() };
        } else {
            bail!("the validators show the ORN neither waiting nor claimed yet; trying again");
        }
        Ok(())
    }
}
