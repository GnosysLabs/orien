//! Swaps between ORN and Bitcoin, as the app runs them: kept in a private
//! file, and moved along while the app is open. The steps themselves live
//! in the `orien-swap` library.

use crate::send;
use orien_crypto::Hash32;
use orien_ledger::{Fees, Kind};
use orien_net::client::Client;
use orien_net::store::write_replacing;
use orien_swap::bitcoin;
use orien_swap::chain::Esplora;
use orien_swap::swap::{LockRef, Offer, Orien, Reply, Role, Stage, Swap, Waiting};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::str::FromStr;

/// One swap as the app keeps it: the swap, and the codes to show.
#[derive(Clone, Serialize, Deserialize)]
pub struct Kept {
    pub id: String,
    pub swap: Swap,
    pub offer: Option<String>,
    pub reply: Option<String>,
    /// The last thing that held it up, if anything did.
    pub problem: Option<String>,
    pub created: u64,
}

/// What the window shows of a swap.
#[derive(Serialize)]
pub struct View {
    pub id: String,
    /// "sell" or "buy".
    pub role: &'static str,
    /// "reply", "bitcoin", "secret", "claim", "done" or "cancelled".
    pub stage: &'static str,
    pub status: String,
    pub orn: String,
    pub btc: String,
    pub offer: Option<String>,
    pub reply: Option<String>,
    /// For the buyer: where to pay, and how much, as a Bitcoin URI too.
    pub pay_to: Option<String>,
    pub pay_uri: Option<String>,
    pub problem: Option<String>,
    pub created: u64,
}

/// The Bitcoin network swaps use: Bitcoin itself, unless `ORIEN_BITCOIN`
/// names a test network (testnet4, signet or regtest, with
/// `ORIEN_BITCOIN_API` for regtest's explorer).
pub fn bitcoin() -> (bitcoin::Network, Esplora) {
    match std::env::var("ORIEN_BITCOIN").as_deref() {
        Ok("testnet4") => (bitcoin::Network::Testnet4, Esplora::at("https://mempool.space/testnet4/api")),
        Ok("signet") => (bitcoin::Network::Signet, Esplora::at("https://mempool.space/signet/api")),
        Ok("regtest") => (
            bitcoin::Network::Regtest,
            Esplora::at(&std::env::var("ORIEN_BITCOIN_API").unwrap_or_else(|_| "http://127.0.0.1:3002".into())),
        ),
        _ => (bitcoin::Network::Bitcoin, Esplora::bitcoin()),
    }
}

fn file(dir: &Path) -> PathBuf {
    dir.join("swaps.json")
}

pub fn load(dir: &Path) -> Vec<Kept> {
    fs::read(file(dir)).ok().and_then(|bytes| serde_json::from_slice(&bytes).ok()).unwrap_or_default()
}

/// Saved privately and durably: the file holds each swap's secret and keys.
pub fn save(dir: &Path, swaps: &[Kept]) -> Result<(), String> {
    let text = serde_json::to_string_pretty(swaps).map_err(|error| error.to_string())?;
    write_replacing(&file(dir), text.as_bytes(), true).map_err(|error| format!("could not save the swap: {error}"))
}

fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

/// Bitcoin as people write it, "0.01", in satoshis.
pub fn read_btc(text: &str) -> Result<u64, String> {
    let text = text.trim();
    let (whole, fraction) = text.split_once('.').unwrap_or((text, ""));
    if whole.is_empty() && fraction.is_empty() || fraction.len() > 8 || !(whole.chars().chain(fraction.chars())).all(|c| c.is_ascii_digit()) {
        return Err("Enter an amount of Bitcoin, like 0.01".into());
    }
    let whole: u64 = if whole.is_empty() { 0 } else { whole.parse().map_err(|_| "That amount is too large.".to_string())? };
    let fraction: u64 = format!("{fraction:0<8}").parse().unwrap_or(0);
    whole.checked_mul(100_000_000).and_then(|sats| sats.checked_add(fraction)).ok_or_else(|| "That amount is too large.".into())
}

pub fn show_btc(sats: u64) -> String {
    let text = format!("{}.{:08}", sats / 100_000_000, sats % 100_000_000);
    text.trim_end_matches('0').trim_end_matches('.').to_string()
}

pub fn view(kept: &Kept) -> View {
    let swap = &kept.swap;
    let (stage, status) = match (&swap.role, &swap.stage) {
        (Role::Seller, Stage::WaitingForReply) => ("reply", "Your ORN are locked for the buyer. Send them the offer code, then paste their reply.".to_string()),
        (Role::Seller, Stage::WaitingForBitcoin) => ("bitcoin", "Waiting for the buyer's Bitcoin to arrive and be confirmed. Keep this app open.".to_string()),
        (Role::Buyer, Stage::WaitingForBitcoin) => ("bitcoin", "Send the Bitcoin below, then send the seller the reply code. Keep this app open.".to_string()),
        (_, Stage::WaitingForSecret) => ("secret", "Paid. Waiting for the seller to take the Bitcoin, which lets this app claim your ORN.".to_string()),
        (_, Stage::ClaimingOrn) => ("claim", "The seller took the Bitcoin. Claiming your ORN.".to_string()),
        (_, Stage::Done) => ("done", match swap.role {
            Role::Seller => "Done: the Bitcoin is on its way to your Bitcoin address.".to_string(),
            Role::Buyer => "Done: the ORN are in your wallet.".to_string(),
        }),
        (_, Stage::Cancelled { why }) => ("cancelled", format!("Cancelled: {why}.")),
        _ => ("bitcoin", String::new()),
    };
    let pay_to = (swap.role == Role::Buyer && stage == "bitcoin").then(|| swap.bitcoin_address().ok()).flatten();
    View {
        id: kept.id.clone(),
        role: if swap.role == Role::Seller { "sell" } else { "buy" },
        stage,
        status,
        orn: crate::money::show(swap.orn as i128),
        btc: show_btc(swap.sats),
        offer: kept.offer.clone(),
        reply: kept.reply.clone(),
        pay_uri: pay_to.as_ref().map(|address| format!("bitcoin:{address}?amount={}", show_btc(swap.sats))),
        pay_to: pay_to.map(|address| address.to_string()),
        problem: kept.problem.clone(),
        created: kept.created,
    }
}

/// Orien, as the app reaches it: its client, and its wallet to sign with.
pub struct AppOrien<'a> {
    pub client: &'a Client,
    pub wallet_file: PathBuf,
    pub password: Option<String>,
    pub fees: Fees,
    pub network: String,
}

impl AppOrien<'_> {
    fn send(&self, kind: Kind, to: Hash32, amount: u64) -> anyhow::Result<send::Sent> {
        let mut wallet = crate::open_signing(&self.wallet_file, self.password.as_deref()).map_err(anyhow::Error::msg)?;
        send::send_order(self.client, &mut wallet, self.fees, kind, to, amount).map_err(anyhow::Error::msg)
    }
}

impl Orien for AppOrien<'_> {
    fn network(&self) -> String {
        self.network.clone()
    }
    fn me(&self) -> Hash32 {
        orien_net::keys::Wallet::peek(&self.wallet_file).map(|(address, _)| address).unwrap_or_default()
    }
    fn epoch(&self) -> anyhow::Result<u64> {
        Ok(self.client.epoch())
    }
    fn waiting(&self, lock: &LockRef) -> anyhow::Result<Option<Waiting>> {
        let locked = self.client.locked(&lock.from, lock.seq, &[0; 32]);
        if locked.answered == 0 {
            anyhow::bail!("no validator could be reached");
        }
        Ok(locked.waiting.map(|e| Waiting { to: e.to, amount: e.amount, hash: e.hash, until: e.until }))
    }
    fn lock(&self, to: &Hash32, amount: u64, hash: [u8; 32], until: u64) -> anyhow::Result<LockRef> {
        let sent = self.send(Kind::Lock { hash, until }, *to, amount)?;
        Ok(LockRef { from: self.me(), seq: sent.seq })
    }
    fn claim(&self, lock: &LockRef, amount: u64, secret: [u8; 32]) -> anyhow::Result<()> {
        self.send(Kind::Claim { from: lock.from, seq: lock.seq, secret }, self.me(), amount)?;
        Ok(())
    }
}

/// Starts selling `orn` ORN for `sats` to the Orien address `buyer`.
pub fn sell(orien: &AppOrien, buyer: &str, orn: u64, sats: u64, payout: &str) -> Result<Kept, String> {
    let buyer = Hash32::from_str(buyer.trim()).map_err(|_| "That is not an Orien address. An address is 64 letters and digits.".to_string())?;
    if buyer == orien.me() {
        return Err("That is this wallet's own address.".into());
    }
    let (network, _) = bitcoin();
    let (swap, offer) = Swap::sell(orien, network, buyer, orn, sats, payout.trim()).map_err(|error| error.to_string())?;
    Ok(Kept { id: hex(&swap.hash), swap, offer: Some(offer.code()), reply: None, problem: None, created: now() })
}

/// Takes up an offer: checks it on Orien and makes the reply.
pub fn buy(orien: &AppOrien, offer: &str, refund: &str) -> Result<Kept, String> {
    let offer = Offer::read(offer).map_err(|error| error.to_string())?;
    let (network, chain) = bitcoin();
    let (swap, reply) = Swap::buy(orien, &chain, network, &offer, refund.trim()).map_err(|error| error.to_string())?;
    Ok(Kept { id: hex(&swap.hash), swap, offer: None, reply: Some(reply.code()), problem: None, created: now() })
}

/// The seller takes in the buyer's reply.
pub fn accept(kept: &mut Kept, reply: &str) -> Result<(), String> {
    let reply = Reply::read(reply).map_err(|error| error.to_string())?;
    let (_, chain) = bitcoin();
    kept.swap.accept(&chain, &reply).map_err(|error| error.to_string())
}

/// Moves every unfinished swap along as far as it can go now.
pub fn step(orien: &AppOrien, swaps: &mut [Kept]) {
    let (_, chain) = bitcoin();
    for kept in swaps.iter_mut().filter(|kept| !matches!(kept.swap.stage, Stage::Done | Stage::Cancelled { .. })) {
        kept.problem = kept.swap.advance(&chain, orien).err().map(|error| error.to_string());
    }
}

/// Whether any swap is waiting on something the app does.
pub fn active(swaps: &[Kept]) -> bool {
    swaps.iter().any(|kept| !matches!(kept.swap.stage, Stage::Done | Stage::Cancelled { .. }))
}

fn hex(bytes: &[u8; 32]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bitcoin_amounts_read_and_show() {
        assert_eq!(read_btc("0.01"), Ok(1_000_000));
        assert_eq!(read_btc("1"), Ok(100_000_000));
        assert_eq!(read_btc(".5"), Ok(50_000_000));
        assert_eq!(read_btc("0.00000001"), Ok(1));
        assert!(read_btc("0.000000001").is_err());
        assert!(read_btc("abc").is_err());
        assert_eq!(show_btc(1_000_000), "0.01");
        assert_eq!(show_btc(100_000_000), "1");
        assert_eq!(show_btc(1), "0.00000001");
    }
}
