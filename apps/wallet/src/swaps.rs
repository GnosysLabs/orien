//! Swaps between ORN and Bitcoin, as the app runs them: kept in a private
//! file, and moved along while the app is open. The steps themselves live
//! in the `orien-swap` library.

use crate::send;
use orien_crypto::Hash32;
use orien_ledger::{Fees, Kind};
use orien_net::client::Client;
use orien_net::store::write_replacing;
use orien_swap::bitcoin;
use orien_swap::chain::{Esplora, Explorers};
use orien_swap::swap::{LockRef, LockState, Offer, Orien, Reply, Role, Stage, Swap, Waiting};
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
    /// About when the ORN lock ends (seconds since 1970): unclaimed ORN go
    /// back to the seller then, and the buyer must have claimed them before.
    #[serde(default)]
    pub orn_back_at: Option<u64>,
    /// About when the buyer can take the Bitcoin back, if the seller never
    /// took it.
    #[serde(default)]
    pub btc_back_at: Option<u64>,
    /// The swap board offer this swap runs for, if it came from the board.
    #[serde(default)]
    pub board: Option<i64>,
    /// The wallet the swap runs with, whichever is open later. Swaps made
    /// before the app kept several wallets have none: the first one.
    #[serde(default)]
    pub wallet: Option<PathBuf>,
    /// Bitcoin seen paid into the swap, confirmed or not: sats, and the
    /// fewest confirmations among the coins. So both sides show the payment
    /// the moment it's sent, and count its confirmations.
    #[serde(default)]
    pub seen: Option<(u64, u32)>,
}

/// What the window shows of a swap.
#[derive(Serialize)]
pub struct View {
    pub id: String,
    /// "sell" or "buy".
    pub role: &'static str,
    /// "locking", "reply", "bitcoin", "claiming", "secret", "refunding",
    /// "claim", "done" or "cancelled".
    pub stage: &'static str,
    pub status: String,
    pub orn: String,
    pub btc: String,
    pub offer: Option<String>,
    pub reply: Option<String>,
    /// For the buyer: where to pay, and how much, as a Bitcoin URI too.
    pub pay_to: Option<String>,
    pub pay_uri: Option<String>,
    /// The same, as a QR code (SVG) for a phone's Bitcoin wallet to scan.
    pub pay_qr: Option<String>,
    /// When the buyer must have paid by to be safe, seconds since 1970.
    pub pay_by: Option<u64>,
    /// The buyer's payment has been seen, though perhaps not confirmed.
    pub paid_seen: bool,
    /// Its confirmations so far, and how many are wanted.
    pub confirmations: Option<u32>,
    pub confirmations_needed: u32,
    pub problem: Option<String>,
    pub created: u64,
    /// The other side's Orien address: the buyer's, or the seller's.
    pub other: Option<String>,
    /// Buyer: Bitcoin has been seen paid into the swap.
    pub paid: bool,
    /// Buyer: the ORN are claimed.
    pub orn_claimed: bool,
    /// Buyer: the ORN deadline passed before they could be claimed.
    pub orn_missed: bool,
    pub orn_back_at: Option<u64>,
    pub btc_back_at: Option<u64>,
    /// The swap board passes this swap's codes: none to copy.
    pub board: bool,
    /// Nothing left for this app to do: for a seller, that includes having
    /// delivered the buyer's ORN, which goes on quietly after "done".
    pub finished: bool,
}

/// The Bitcoin network swaps use: Bitcoin itself, through two independent
/// explorers, unless `ORIEN_BITCOIN` names a test network (testnet4, signet
/// or regtest, with `ORIEN_BITCOIN_API` for regtest's explorer).
pub fn bitcoin() -> (bitcoin::Network, Explorers) {
    let one = |base: &str| Explorers::one(Esplora::at(base));
    match std::env::var("ORIEN_BITCOIN").as_deref() {
        Ok("testnet4") => (bitcoin::Network::Testnet4, one("https://mempool.space/testnet4/api")),
        Ok("signet") => (bitcoin::Network::Signet, one("https://mempool.space/signet/api")),
        Ok("regtest") => (
            bitcoin::Network::Regtest,
            one(&std::env::var("ORIEN_BITCOIN_API").unwrap_or_else(|_| "http://127.0.0.1:3002".into())),
        ),
        _ => (bitcoin::Network::Bitcoin, Explorers::bitcoin()),
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

pub(crate) fn now() -> u64 {
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

/// A QR code for `text`, as an SVG picture: dark squares on white, which
/// every Bitcoin wallet's scanner reads.
pub(crate) fn qr(text: &str) -> Option<String> {
    let code = qrcode::QrCode::new(text.as_bytes()).ok()?;
    Some(code.render::<qrcode::render::svg::Color>().min_dimensions(200, 200).quiet_zone(true).build())
}

pub fn show_btc(sats: u64) -> String {
    let text = format!("{}.{:08}", sats / 100_000_000, sats % 100_000_000);
    text.trim_end_matches('0').trim_end_matches('.').to_string()
}

pub fn view(kept: &Kept) -> View {
    let swap = &kept.swap;
    let needed = orien_swap::swap::CONFIRMATIONS;
    let paid_seen = kept.seen.is_some_and(|(sats, _)| sats >= swap.sats);
    let confirmations = kept.seen.filter(|_| paid_seen).map(|(_, depth)| depth);
    let (stage, status) = match (&swap.role, &swap.stage) {
        (_, Stage::Preparing) => ("locking", "Locking your ORN for the buyer.".to_string()),
        (Role::Seller, Stage::Claiming) => ("claiming", format!("Paid: {} BTC is sent to your Bitcoin address, and your Bitcoin wallet may show it already. Bitcoin confirms it within about an hour; this app then hands the buyer their ORN.", show_btc(swap.sats))),
        (_, Stage::Refunding) => ("refunding", "The seller did not take the Bitcoin in time. Taking it back; keep this app open.".to_string()),
        // Board swaps pass their codes by themselves: never a word about them.
        (Role::Seller, Stage::WaitingForReply) if kept.board.is_some() => ("reply", "Accepted: your ORN are locked for the buyer. Waiting for them to pay.".to_string()),
        (Role::Seller, Stage::WaitingForReply) => ("reply", "Your ORN are locked for the buyer. Send them the offer code, then paste their reply.".to_string()),
        (Role::Seller, Stage::WaitingForBitcoin) if paid_seen => ("bitcoin", format!("The buyer paid. Bitcoin is confirming it ({} of {}); then this app takes it. Keep Orien Wallet running.", confirmations.unwrap_or(0).min(needed), needed)),
        (Role::Seller, Stage::WaitingForBitcoin) => ("bitcoin", "Waiting for the buyer to pay. Keep Orien Wallet running.".to_string()),
        (Role::Buyer, Stage::WaitingForBitcoin) if kept.board.is_some() && !swap.coins.is_empty() => ("bitcoin", "Payment seen. Waiting for Bitcoin to confirm it; nothing more for you to do.".to_string()),
        (Role::Buyer, Stage::WaitingForBitcoin) if kept.board.is_some() => ("bitcoin", format!("The seller accepted. Pay exactly {} BTC to the address below, from any Bitcoin wallet. This app sees the payment by itself.", show_btc(swap.sats))),
        (Role::Buyer, Stage::WaitingForBitcoin) => ("bitcoin", "Send the seller the reply code below, then pay the Bitcoin.".to_string()),
        (Role::Buyer, Stage::WaitingForSecret) if swap.orn_claimed => ("secret", "Your ORN are claimed. Taking back the extra Bitcoin paid into the swap once its lock ends; keep this app open.".to_string()),
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
        pay_qr: pay_to.as_ref().and_then(|address| qr(&format!("bitcoin:{address}?amount={}", show_btc(swap.sats)))),
        // The Bitcoin lock sets the real limit: a payment must be confirmed,
        // with a safety margin, before the lock ends for the seller to take
        // it. Paid later, it comes back to the buyer once the lock ends.
        pay_by: (swap.role == Role::Buyer)
            .then_some(kept.btc_back_at)
            .flatten()
            .map(|end| end.saturating_sub((orien_swap::swap::CONFIRMATIONS + orien_swap::swap::SAFETY_BLOCKS) as u64 * orien_swap::swap::BLOCK_MINUTES * 60 + 30 * 60)),
        pay_to: pay_to.map(|address| address.to_string()),
        problem: kept.problem.clone(),
        created: kept.created,
        other: match swap.role {
            Role::Seller => Some(swap.buyer.to_string()),
            Role::Buyer => swap.lock.map(|lock| lock.from.to_string()),
        },
        paid: !swap.coins.is_empty(),
        orn_claimed: swap.orn_claimed,
        orn_missed: swap.orn_missed,
        orn_back_at: kept.orn_back_at,
        btc_back_at: kept.btc_back_at,
        board: kept.board.is_some(),
        paid_seen,
        confirmations,
        confirmations_needed: needed,
        finished: swap.finished(),
    }
}

/// Works out, once, about when each side's coins would come back: the ORN
/// lock's end from the newest checkpoint, and the Bitcoin lock's from its
/// length in blocks, counted from when the app first knew it.
fn note_times(orien: &AppOrien, kept: &mut Kept) {
    if kept.orn_back_at.is_none() && kept.swap.until > 0 {
        let head = orien.client.head();
        if head.time > 0 {
            let epochs = kept.swap.until.saturating_sub(head.epoch);
            kept.orn_back_at = Some(head.time + epochs * orien.epoch_seconds.max(1));
        }
    }
    if kept.btc_back_at.is_none() && kept.swap.height.is_some() {
        kept.btc_back_at = Some(now() + orien_swap::swap::BTC_BLOCKS as u64 * orien_swap::swap::BLOCK_MINUTES * 60);
    }
}

/// Orien, as the app reaches it: its client, and its wallet to sign with.
pub struct AppOrien<'a> {
    pub client: &'a Client,
    pub wallet_file: PathBuf,
    pub password: Option<String>,
    pub fees: Fees,
    pub network: String,
    pub epoch_seconds: u64,
}

impl<'a> AppOrien<'a> {
    /// The same, signing with another of the app's wallets.
    pub fn with_wallet(&self, wallet_file: &Path) -> AppOrien<'a> {
        AppOrien {
            client: self.client,
            wallet_file: wallet_file.to_path_buf(),
            password: self.password.clone(),
            fees: self.fees,
            network: self.network.clone(),
            epoch_seconds: self.epoch_seconds,
        }
    }

    /// The same, for the wallet a swap runs with.
    pub fn for_swap(&self, kept: &Kept) -> AppOrien<'a> {
        self.with_wallet(kept.wallet.as_deref().unwrap_or(&self.wallet_file))
    }

    /// The same, unlocked with `password`.
    pub fn with_password(&self, password: Option<String>) -> AppOrien<'a> {
        AppOrien { password, ..self.with_wallet(&self.wallet_file) }
    }

    fn send(&self, kind: Kind, to: Hash32, amount: u64, at: Option<u64>) -> anyhow::Result<send::Sent> {
        let mut wallet = crate::open_signing(&self.wallet_file, self.password.as_deref()).map_err(anyhow::Error::msg)?;
        let sent = send::send_order_at(self.client, &mut wallet, self.fees, kind.clone(), to, amount, at).map_err(anyhow::Error::msg)?;
        // That finished an earlier payment instead: now send this one.
        if sent.earlier {
            return send::send_order_at(self.client, &mut wallet, self.fees, kind, to, amount, at).map_err(anyhow::Error::msg);
        }
        Ok(sent)
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
        self.client.follow();
        Ok(self.client.epoch())
    }
    fn latest_epoch(&self) -> anyhow::Result<u64> {
        // Checkpoints could be held back from this wallet; the clock cannot.
        self.client.follow();
        let head = self.client.head();
        let passed = now().saturating_sub(head.time) / self.epoch_seconds.max(1);
        Ok(head.epoch + passed)
    }
    fn epoch_seconds(&self) -> u64 {
        self.epoch_seconds
    }
    fn next_seq(&self) -> anyhow::Result<u64> {
        let status = self.client.account(&self.me(), 0);
        if status.views.iter().all(Option::is_none) {
            anyhow::bail!("no validator could be reached");
        }
        // A payment signed here and not yet final is finished before
        // anything new is signed, so the next order comes after it.
        let pending = crate::open_signing(&self.wallet_file, self.password.as_deref())
            .ok()
            .and_then(|wallet| wallet.pending().map(|signed| signed.order.seq + 1));
        Ok(pending.map_or(status.seq, |after| after.max(status.seq)))
    }
    fn proven(&self, lock: &LockRef) -> anyhow::Result<LockState> {
        let proven = self
            .client
            .proven_lock(&lock.from, lock.seq)
            .ok_or_else(|| anyhow::anyhow!("the validators have not proven the locked ORN yet; trying again"))?;
        Ok(LockState {
            epoch: proven.epoch,
            waiting: proven.made.map(|e| Waiting { to: e.to, amount: e.amount, hash: e.hash, until: e.until }),
            secret: proven.secret,
        })
    }
    fn lock(&self, seq: u64, to: &Hash32, amount: u64, hash: [u8; 32], until: u64) -> anyhow::Result<LockRef> {
        let sent = self.send(Kind::Lock { hash, until }, *to, amount, Some(seq))?;
        Ok(LockRef { from: self.me(), seq: sent.seq })
    }
    fn still_waiting(&self, lock: &LockRef) -> anyhow::Result<Option<bool>> {
        // Any secret will do here: only whether the payment waits matters.
        let now = self.client.locked(&lock.from, lock.seq, &[0; 32]);
        if now.answered == 0 {
            anyhow::bail!("no validator could be reached");
        }
        Ok(if now.secret.is_some() {
            Some(false)
        } else if now.agreed {
            Some(now.waiting.is_some())
        } else {
            None
        })
    }
    fn claim(&self, lock: &LockRef, payee: &Hash32, amount: u64, secret: [u8; 32]) -> anyhow::Result<()> {
        self.send(Kind::Claim { from: lock.from, seq: lock.seq, secret }, *payee, amount, None)?;
        Ok(())
    }
}

/// Starts selling `orn` ORN for `sats` to the Orien address `buyer`.
pub fn sell(orien: &AppOrien, buyer: &str, orn: u64, sats: u64, payout: &str) -> Result<Kept, String> {
    sell_on(orien, bitcoin().0, buyer, orn, sats, payout)
}

/// `sell`, on a given Bitcoin network.
pub fn sell_on(orien: &AppOrien, network: bitcoin::Network, buyer: &str, orn: u64, sats: u64, payout: &str) -> Result<Kept, String> {
    let buyer = Hash32::from_str(buyer.trim()).map_err(|_| "That is not an Orien address. An address is 64 letters and digits.".to_string())?;
    if buyer == orien.me() {
        return Err("That is this wallet's own address.".into());
    }
    let swap = Swap::sell(orien, network, buyer, orn, sats, payout.trim()).map_err(|error| error.to_string())?;
    let mut kept = Kept { id: hex(&swap.hash), swap, offer: None, reply: None, problem: None, created: now(), orn_back_at: None, btc_back_at: None, board: None, wallet: Some(orien.wallet_file.clone()), seen: None };
    note_times(orien, &mut kept);
    Ok(kept)
}

/// Takes up an offer: checks it on Orien and makes the reply.
pub fn buy(orien: &AppOrien, offer: &str, refund: &str) -> Result<Kept, String> {
    let (network, chain) = bitcoin();
    buy_on(orien, network, &chain, offer, refund)
}

/// `buy`, on a given Bitcoin network.
pub fn buy_on(orien: &AppOrien, network: bitcoin::Network, chain: &Explorers, offer: &str, refund: &str) -> Result<Kept, String> {
    let offer = Offer::read(offer).map_err(|error| error.to_string())?;
    let (swap, reply) = Swap::buy(orien, chain, network, &offer, refund.trim()).map_err(|error| error.to_string())?;
    let mut kept =
        Kept {
        id: hex(&swap.hash),
        swap,
        offer: None,
        reply: Some(reply.code()),
        problem: None,
        created: now(),
        orn_back_at: None,
        btc_back_at: None,
        board: None,
        wallet: Some(orien.wallet_file.clone()),
        seen: None,
    };
    note_times(orien, &mut kept);
    Ok(kept)
}

/// The seller takes in the buyer's reply.
pub fn accept(orien: &AppOrien, kept: &mut Kept, reply: &str) -> Result<(), String> {
    accept_on(orien, &bitcoin().1, kept, reply)
}

pub fn accept_on(orien: &AppOrien, chain: &Explorers, kept: &mut Kept, reply: &str) -> Result<(), String> {
    let reply = Reply::read(reply).map_err(|error| error.to_string())?;
    let orien = &orien.for_swap(kept);
    kept.swap.accept(chain, orien, &reply).map_err(|error| error.to_string())?;
    note_times(orien, kept);
    Ok(())
}

/// Moves every unfinished swap along as far as it can go now, saving after
/// each: a swap's step may sign a Bitcoin transaction or an order, and what
/// it signed must be on disk before the next one starts.
pub fn step(orien: &AppOrien, dir: &Path, swaps: &mut [Kept]) -> Result<(), String> {
    step_on(orien, &bitcoin().1, dir, swaps)
}

pub fn step_on(orien: &AppOrien, chain: &Explorers, dir: &Path, swaps: &mut [Kept]) -> Result<(), String> {
    for index in 0..swaps.len() {
        // A seller's swap that is done still claims the buyer's ORN for them.
        if swaps[index].swap.finished() {
            continue;
        }
        let mut orien = orien.for_swap(&swaps[index]);
        // A board swap approved when it was posted (sellers) or taken
        // (buyers): it goes on with that approval, and with nothing else.
        if orien.password.is_none()
            && let Some(offer) = swaps[index].board
            && let Some(password) = crate::approval::approved(dir, offer, swaps[index].swap.orn, &orien.wallet_file)
        {
            orien = orien.with_password(password);
        }
        advance(&orien, chain, &mut swaps[index]);
        save(dir, swaps)?;
    }
    Ok(())
}

/// Moves one swap along, and makes the offer code once the ORN are locked.
pub fn advance(orien: &AppOrien, chain: &Explorers, kept: &mut Kept) {
    kept.problem = kept.swap.advance(chain, orien).err().map(|error| error.to_string());
    note_times(orien, kept);
    // While Bitcoin is being paid and confirmed, look at the swap's address
    // directly, so both sides see the payment from the moment it's sent.
    let watching = matches!(
        (&kept.swap.role, &kept.swap.stage),
        (Role::Seller, Stage::WaitingForBitcoin) | (Role::Buyer, Stage::WaitingForBitcoin | Stage::WaitingForSecret)
    );
    if watching && let Ok(address) = kept.swap.bitcoin_address() && let Ok(coins) = chain.coins_any(&address) {
        let sats: u64 = coins.iter().map(|(coin, _)| coin.value.to_sat()).sum();
        let depth = coins.iter().map(|(_, depth)| *depth).min().unwrap_or(0);
        kept.seen = (sats > 0).then_some((sats, depth));
    }
    if kept.offer.is_none() && kept.swap.role == Role::Seller && kept.swap.lock.is_some() {
        kept.offer = kept.swap.offer(orien.network()).ok().map(|offer| offer.code());
    }
}

/// Whether any swap is waiting on something the app does.
pub fn active(swaps: &[Kept]) -> bool {
    swaps.iter().any(|kept| !kept.swap.finished())
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

#[cfg(test)]
mod live {
    #[test]
    #[ignore = "reaches the Bitcoin explorers"]
    fn smallest_swap_now() {
        let (_, chain) = super::bitcoin();
        let rate = chain.fee_rate().unwrap();
        println!("fee rate {rate} sat/vB; smallest swap now {} BTC", super::show_btc(orien_swap::swap::smallest_now(rate)));
    }
}
