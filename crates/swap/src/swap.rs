//! A swap of ORN for Bitcoin between two people who don't trust each other,
//! step by step, as each side's wallet runs it.
//!
//! The seller has ORN and wants Bitcoin; the buyer has Bitcoin and wants
//! ORN. The seller makes the secret.
//!
//! 1. The buyer tells the seller their Orien address.
//! 2. The seller's wallet locks the ORN for the buyer on Orien, behind the
//!    secret's hash, for `ORN_HOURS`, and makes an **offer** code.
//! 3. The buyer's wallet checks, on Orien, that the ORN really are locked
//!    for them with time to spare, makes a Bitcoin lock that the seller can
//!    take with the secret and the buyer can take back after `BTC_BLOCKS`,
//!    and makes a **reply** code. The buyer pays the Bitcoin into it.
//! 4. The seller's wallet sees the Bitcoin confirmed and takes it, which
//!    shows the secret to everyone.
//! 5. The buyer's wallet reads the secret and claims the ORN.
//!
//! If the buyer never pays, the seller's ORN come back on their own after
//! `ORN_HOURS`. If the seller never takes the Bitcoin, the buyer's wallet
//! takes it back after `BTC_BLOCKS`. The Bitcoin lock ends long before the
//! ORN lock, so the buyer always has time to claim after the secret appears.

use crate::chain::Chain;
use crate::htlc::{Coin, Lock, SPEND_VBYTES, new_key, secret_in};
use anyhow::{Context, Result, anyhow, bail};
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use bitcoin::hashes::{Hash, sha256};
use bitcoin::secp256k1::SecretKey;
use bitcoin::{Address, Amount, Network, OutPoint, PublicKey, Txid};
use orien_crypto::Hash32;
use serde::{Deserialize, Serialize};
use std::str::FromStr;

/// How many Orien epochs (hours, on the live network) the seller's ORN stay
/// locked.
pub const ORN_EPOCHS: u64 = 48;
/// How many Bitcoin blocks (about 12 hours) the buyer's Bitcoin stay locked.
pub const BTC_BLOCKS: u32 = 72;
/// The buyer pays only if the ORN stay locked at least this many epochs
/// longer: time for the Bitcoin lock to run out, and to claim after it.
pub const ORN_EPOCHS_LEFT: u64 = 30;
/// The seller takes the Bitcoin only while at least this many blocks are
/// left before the buyer could take it back, so the two never race.
pub const SAFETY_BLOCKS: u32 = 6;
/// The seller waits for this many confirmations of the buyer's payment.
pub const CONFIRMATIONS: u32 = 1;

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

/// What a swap needs from Orien: the wallet's own account, and its locked
/// payments.
pub trait Orien {
    /// The Orien network's id, so both sides know they are on the same one.
    fn network(&self) -> String;
    /// This wallet's address.
    fn me(&self) -> Hash32;
    fn epoch(&self) -> Result<u64>;
    fn waiting(&self, lock: &LockRef) -> Result<Option<Waiting>>;
    /// Locks `amount` for `to` behind `hash` until epoch `until`.
    fn lock(&self, to: &Hash32, amount: u64, hash: [u8; 32], until: u64) -> Result<LockRef>;
    /// Claims the locked payment with its secret.
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

fn encode<T: Serialize>(prefix: &str, value: &T) -> String {
    format!("{prefix}{}", URL_SAFE_NO_PAD.encode(serde_json::to_vec(value).expect("plain data")))
}

fn decode<T: for<'a> Deserialize<'a>>(prefix: &str, code: &str, what: &str) -> Result<T> {
    let body = code.trim().strip_prefix(prefix).ok_or_else(|| anyhow!("that is not a swap {what} code"))?;
    let bytes = URL_SAFE_NO_PAD.decode(body.trim()).map_err(|_| anyhow!("that {what} code is damaged"))?;
    serde_json::from_slice(&bytes).map_err(|_| anyhow!("that {what} code is damaged"))
}

impl Offer {
    pub fn code(&self) -> String {
        encode("orien-offer:", self)
    }
    pub fn read(code: &str) -> Result<Offer> {
        decode("orien-offer:", code, "offer")
    }
}

impl Reply {
    pub fn code(&self) -> String {
        encode("orien-reply:", self)
    }
    pub fn read(code: &str) -> Result<Reply> {
        decode("orien-reply:", code, "reply")
    }
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
    /// Seller: the ORN are locked; waiting for the buyer's reply.
    WaitingForReply,
    /// Both: waiting for the buyer's Bitcoin to arrive (and, for the seller,
    /// to be confirmed).
    WaitingForBitcoin,
    /// Buyer: paid; waiting for the seller to take the Bitcoin and so show
    /// the secret.
    WaitingForSecret,
    /// Buyer: the secret is known; claiming the ORN.
    ClaimingOrn,
    /// Done: each side has what it wanted.
    Done,
    /// The swap did not happen, and this side's coins came, or are coming,
    /// back.
    Cancelled { why: String },
}

/// One swap, as one side's wallet keeps it. Holds secrets: keep the file
/// private.
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
    pub lock: LockRef,
    /// The ORN lock's deadline epoch.
    pub until: u64,
    pub buyer: Hash32,
    /// Where this side's Bitcoin goes: the seller's payout, or the buyer's
    /// refund.
    pub payout: String,
    /// The buyer's payment into the Bitcoin lock, once seen.
    pub coin: Option<(String, u32, u64)>,
    /// The transaction this side sent to take Bitcoin out of the lock.
    pub spent_by: Option<String>,
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
        let theirs = PublicKey::from_str(self.their_key.as_deref().ok_or_else(|| anyhow!("waiting for the other side"))?)?;
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

    /// Seller: locks `orn` for `buyer` and makes the offer to send them.
    pub fn sell(orien: &dyn Orien, bitcoin: Network, buyer: Hash32, orn: u64, sats: u64, payout: &str) -> Result<(Swap, Offer)> {
        Address::from_str(payout)?.require_network(bitcoin).map_err(|_| anyhow!("{payout} is not a Bitcoin address"))?;
        if sats < 10_000 {
            bail!("a swap needs at least 10,000 satoshis, so that Bitcoin's fees leave something");
        }
        let secret = random_secret();
        let hash = sha256::Hash::hash(&secret).to_byte_array();
        let (key, public) = new_key();
        let until = orien.epoch()? + ORN_EPOCHS;
        let lock = orien.lock(&buyer, orn, hash, until)?;
        let swap = Swap {
            role: Role::Seller,
            stage: Stage::WaitingForReply,
            bitcoin: bitcoin.to_string(),
            orn,
            sats,
            hash,
            secret: Some(secret),
            key: key.display_secret().to_string(),
            their_key: None,
            height: None,
            lock,
            until,
            buyer,
            payout: payout.to_string(),
            coin: None,
            spent_by: None,
        };
        let offer = Offer { network: orien.network(), orn, sats, hash, lock, buyer, key: public.to_string() };
        Ok((swap, offer))
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
        PublicKey::from_str(&offer.key).map_err(|_| anyhow!("that offer's Bitcoin key is damaged"))?;
        let waiting = orien.waiting(&offer.lock)?.ok_or_else(|| anyhow!("the seller's ORN are not locked: the offer cannot be trusted"))?;
        if waiting.to != orien.me() || waiting.amount != offer.orn || waiting.hash != offer.hash {
            bail!("the ORN locked on Orien do not match the offer: it cannot be trusted");
        }
        if waiting.until < orien.epoch()? + ORN_EPOCHS_LEFT {
            bail!("the seller's ORN are locked for too short a time left to swap safely");
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
            lock: offer.lock,
            until: waiting.until,
            buyer: offer.buyer,
            payout: refund.to_string(),
            coin: None,
            spent_by: None,
        };
        let reply = Reply { hash: offer.hash, key: public.to_string(), height };
        Ok((swap, reply))
    }

    /// Seller: takes in the buyer's reply.
    pub fn accept(&mut self, chain: &dyn Chain, reply: &Reply) -> Result<()> {
        if self.role != Role::Seller || self.stage != Stage::WaitingForReply {
            bail!("this swap is not waiting for a reply");
        }
        if reply.hash != self.hash {
            bail!("that reply is for another swap");
        }
        PublicKey::from_str(&reply.key).map_err(|_| anyhow!("that reply's Bitcoin key is damaged"))?;
        let tip = chain.height()?;
        if reply.height < tip + BTC_BLOCKS / 3 || reply.height > tip + BTC_BLOCKS * 2 {
            bail!("the reply's Bitcoin deadline is too soon or too far off; ask the buyer for a fresh reply");
        }
        self.their_key = Some(reply.key.clone());
        self.height = Some(reply.height);
        self.stage = Stage::WaitingForBitcoin;
        Ok(())
    }

    /// Moves the swap on as far as it can go now. Safe to call as often as
    /// you like; it never sends anything twice.
    pub fn advance(&mut self, chain: &dyn Chain, orien: &dyn Orien) -> Result<()> {
        match (self.role, self.stage.clone()) {
            (Role::Seller, Stage::WaitingForReply) => {
                if orien.epoch()? >= self.until {
                    self.stage = Stage::Cancelled { why: "no reply came; the ORN came back".into() };
                }
            }
            (Role::Seller, Stage::WaitingForBitcoin) => self.seller_take(chain, orien)?,
            (Role::Buyer, Stage::WaitingForBitcoin) => {
                if let Some(coin) = self.paid(chain, 0)? {
                    self.coin = Some((coin.outpoint.txid.to_string(), coin.outpoint.vout, coin.value.to_sat()));
                    self.stage = Stage::WaitingForSecret;
                } else if chain.height()? >= self.height.unwrap_or(0) {
                    self.stage = Stage::Cancelled { why: "no Bitcoin was paid in time".into() };
                }
            }
            (Role::Buyer, Stage::WaitingForSecret) => self.buyer_watch(chain)?,
            (Role::Buyer, Stage::ClaimingOrn) => {
                let secret = self.secret.ok_or_else(|| anyhow!("the secret is missing"))?;
                if orien.waiting(&self.lock)?.is_some() {
                    orien.claim(&self.lock, self.orn, secret)?;
                }
                if orien.waiting(&self.lock)?.is_none() {
                    self.stage = Stage::Done;
                }
            }
            _ => {}
        }
        Ok(())
    }

    /// A payment into the Bitcoin lock of at least `sats`, with at least
    /// `confirmations`.
    fn paid(&self, chain: &dyn Chain, confirmations: u32) -> Result<Option<Coin>> {
        let address = self.bitcoin_address()?;
        Ok(chain
            .coins(&address)?
            .into_iter()
            .filter(|(coin, confirmed)| coin.value >= Amount::from_sat(self.sats) && *confirmed >= confirmations)
            .map(|(coin, _)| coin)
            .next())
    }

    fn fee(&self, chain: &dyn Chain) -> Result<Amount> {
        Ok(Amount::from_sat(chain.fee_rate()?.saturating_mul(SPEND_VBYTES)))
    }

    fn seller_take(&mut self, chain: &dyn Chain, orien: &dyn Orien) -> Result<()> {
        let lock = self.bitcoin_lock()?;
        let height = lock.height;
        let tip = chain.height()?;
        if let Some(txid) = &self.spent_by {
            // Taken already: done once the network has it.
            let coin = self.coin_outpoint()?;
            if chain.spender(&coin)?.is_some_and(|tx| tx.compute_txid().to_string() == *txid) {
                self.stage = Stage::Done;
            }
            return Ok(());
        }
        match self.paid(chain, CONFIRMATIONS)? {
            Some(coin) if tip + SAFETY_BLOCKS < height => {
                let secret = self.secret.ok_or_else(|| anyhow!("the secret is missing"))?;
                let tx = lock.claim(&coin, &secret, &self.secret_key()?, &self.payout()?, self.fee(chain)?)?;
                self.coin = Some((coin.outpoint.txid.to_string(), coin.outpoint.vout, coin.value.to_sat()));
                // Recorded before it is sent, so that it is never sent twice
                // with different fees.
                self.spent_by = Some(tx.compute_txid().to_string());
                chain.broadcast(&tx).context("could not send the transaction that takes the Bitcoin")?;
            }
            Some(_) | None if tip + SAFETY_BLOCKS >= height => {
                self.stage = Stage::Cancelled {
                    why: "the Bitcoin did not arrive in time; the ORN come back when their lock ends".into(),
                };
            }
            _ => {
                if orien.epoch()? >= self.until {
                    self.stage = Stage::Cancelled { why: "the ORN came back".into() };
                }
            }
        }
        Ok(())
    }

    fn buyer_watch(&mut self, chain: &dyn Chain) -> Result<()> {
        let coin = self.coin_outpoint()?;
        match chain.spender(&coin)? {
            Some(tx) => match secret_in(&tx, &self.hash) {
                Some(secret) => {
                    self.secret = Some(secret);
                    self.stage = Stage::ClaimingOrn;
                }
                // Spent without the secret: the buyer's own refund.
                None if self.spent_by.as_deref() == Some(&tx.compute_txid().to_string()) => {
                    self.stage = Stage::Cancelled { why: "the seller did not take the Bitcoin; it came back".into() };
                }
                None => bail!("the Bitcoin lock was spent in a way this swap does not know"),
            },
            None if chain.height()? >= self.height.unwrap_or(u32::MAX) && self.spent_by.is_none() => {
                let (_, _, value) = self.coin.clone().expect("a buyer watching has paid");
                let lock = self.bitcoin_lock()?;
                let tx = lock.refund(&Coin { outpoint: coin, value: Amount::from_sat(value) }, &self.secret_key()?, &self.payout()?, self.fee(chain)?)?;
                self.spent_by = Some(tx.compute_txid().to_string());
                chain.broadcast(&tx).context("could not send the refund")?;
            }
            None => {}
        }
        Ok(())
    }

    fn coin_outpoint(&self) -> Result<OutPoint> {
        let (txid, vout, _) = self.coin.clone().ok_or_else(|| anyhow!("no Bitcoin paid yet"))?;
        Ok(OutPoint { txid: Txid::from_str(&txid)?, vout })
    }
}

/// 32 random bytes from the system, for the secret.
fn random_secret() -> [u8; 32] {
    let mut bytes = [0u8; 32];
    bitcoin::secp256k1::rand::RngCore::fill_bytes(&mut bitcoin::secp256k1::rand::thread_rng(), &mut bytes);
    bytes
}
