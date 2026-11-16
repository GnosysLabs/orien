//! The wallet's SOL swaps: ORN for SOL, the main way to swap. Nobody keeps
//! SOL in the app. A seller is paid at their own SOL address; a buyer pays
//! from their own SOL wallet to a one-time address this app makes for the
//! swap, and anything left over goes back to that wallet. The swap itself
//! is `orien_swap::sol`; this keeps each one, moves it along, and says
//! where it stands in plain words.

use crate::approval;
use crate::swaps::{AppOrien, now, qr};
use orien_crypto::Hash32;
use orien_net::store::write_replacing;
use orien_swap::sol::{SolOffer, SolReply, SolStage, SolSwap};
use orien_swap::solana::{self, LAMPORTS_PER_SOL, Solana};
use orien_swap::swap::Role;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::str::FromStr;

/// One SOL swap, as this app keeps it.
#[derive(Clone, Serialize, Deserialize)]
pub struct Kept {
    pub id: String,
    pub swap: SolSwap,
    /// Seller: the offer code, once the ORN are locked.
    pub offer: Option<String>,
    /// Buyer: the reply code, made as the offer is taken.
    pub reply: Option<String>,
    pub problem: Option<String>,
    pub created: u64,
    /// The swap board offer this swap runs for.
    #[serde(default)]
    pub board: Option<i64>,
    /// The wallet the swap runs with.
    #[serde(default)]
    pub wallet: Option<PathBuf>,
    /// Buyer: SOL seen at the one-time address so far, in lamports.
    #[serde(default)]
    pub arrived: u64,
}

/// What the window shows of a SOL swap.
#[derive(Serialize)]
pub struct View {
    pub id: String,
    pub coin: &'static str,
    pub role: &'static str,
    /// "locking", "reply", "paying", "claiming", "pay", "confirming",
    /// "claim", "refunding", "done" or "cancelled".
    pub stage: &'static str,
    pub status: String,
    pub orn: String,
    pub sol: String,
    /// Buyer, while paying: where, how much in all, a link and QR code for a
    /// phone's SOL wallet, and how much has arrived.
    pub pay_to: Option<String>,
    pub pay_amount: Option<String>,
    pub pay_link: Option<String>,
    pub pay_qr: Option<String>,
    pub arrived: Option<String>,
    /// The buyer's SOL has reached the swap.
    pub paid: bool,
    pub orn_claimed: bool,
    pub problem: Option<String>,
    pub created: u64,
    pub board: bool,
    pub finished: bool,
}

fn file(dir: &Path) -> PathBuf {
    dir.join("sol-swaps.json")
}

pub fn load(dir: &Path) -> Vec<Kept> {
    fs::read(file(dir)).ok().and_then(|bytes| serde_json::from_slice(&bytes).ok()).unwrap_or_default()
}

/// Saved privately and durably: the file holds each swap's secret and keys.
pub fn save(dir: &Path, all: &[Kept]) -> Result<(), String> {
    let text = serde_json::to_string_pretty(all).map_err(|error| error.to_string())?;
    write_replacing(&file(dir), text.as_bytes(), true).map_err(|error| format!("could not save the swap: {error}"))
}

/// Which Solana this copy of the app swaps on: Solana itself, or its free
/// test network for test copies.
static CLUSTER: std::sync::OnceLock<&'static str> = std::sync::OnceLock::new();

/// Set once as the app starts: test copies swap on Solana's test network.
pub fn set_cluster(test: bool) {
    let _ = CLUSTER.set(if test { "devnet" } else { "mainnet" });
}

pub fn cluster() -> &'static str {
    CLUSTER.get().copied().unwrap_or("mainnet")
}

/// Solana, for `cluster`: the test network for test copies of the app,
/// Solana itself otherwise.
pub fn solana(cluster: &str) -> Solana {
    if cluster == "devnet" { Solana::devnet() } else { Solana::mainnet() }
}

/// In plain words, for the app: an address that can never receive SOL.
pub const NOT_A_WALLET: &str = "That address can't receive SOL. Use your own wallet's address, such as the one in Phantom.";

/// What's wrong with `payout` as a seller's SOL address, if anything. An
/// address that doesn't exist yet is fine; a program never is. If Solana
/// can't be asked right now, it's allowed: the swap checks again before
/// any ORN are locked.
pub fn payout_problem(payout: &str) -> Option<String> {
    let Ok(key) = orien_swap::solana::address(payout.trim()) else {
        return Some("That is not a SOL address.".into());
    };
    match orien_swap::sol::payable(&solana(cluster()), &key) {
        Ok(None) => Some(NOT_A_WALLET.into()),
        _ => None,
    }
}

/// Solana's loader for programs; a live program's account belongs to it.
const UPGRADEABLE_LOADER: &str = "BPFLoaderUpgradeab1e11111111111111111111111";

/// Whether orien-lock is live on `cluster`: its account is there, owned by
/// the loader, and marks a deployed program. Asks Solana, so not on the
/// main thread. Once live, it stays live.
pub fn lock_live(cluster: &str) -> bool {
    static LIVE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    if cluster == "devnet" || LIVE.load(std::sync::atomic::Ordering::Relaxed) {
        return true;
    }
    let solana = solana(cluster);
    let (Some(program), Ok(loader)) = (solana.lock_program, orien_swap::solana::address(UPGRADEABLE_LOADER)) else {
        return false;
    };
    let live = matches!(solana.account(&program), Ok(Some(account)) if account.executable && account.owner == loader);
    if live {
        LIVE.store(true, std::sync::atomic::Ordering::Relaxed);
    }
    live
}

/// "0.15", from lamports.
pub fn show_sol(lamports: u64) -> String {
    let text = format!("{}.{:09}", lamports / LAMPORTS_PER_SOL, lamports % LAMPORTS_PER_SOL);
    text.trim_end_matches('0').trim_end_matches('.').to_string()
}

/// Lamports, from "0.15".
pub fn read_sol(text: &str) -> Result<u64, String> {
    let text = text.trim();
    let (whole, fraction) = text.split_once('.').unwrap_or((text, ""));
    if (whole.is_empty() && fraction.is_empty()) || !whole.chars().all(|c| c.is_ascii_digit()) || !fraction.chars().all(|c| c.is_ascii_digit()) || fraction.len() > 9 {
        return Err("Enter an amount of SOL, like 0.5".into());
    }
    let whole: u64 = if whole.is_empty() { 0 } else { whole.parse().map_err(|_| "That amount is too large.".to_string())? };
    let fraction: u64 = format!("{fraction:0<9}").parse().unwrap_or(0);
    whole.checked_mul(LAMPORTS_PER_SOL).and_then(|l| l.checked_add(fraction)).ok_or_else(|| "That amount is too large.".into())
}

fn hex32(bytes: &[u8; 32]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Seller: a new swap selling `orn` units to `buyer` for `lamports`, paid at
/// `payout`, with the wallet's own Solana fee key.
pub fn sell(orien: &AppOrien, cluster: &str, buyer: &str, orn: u64, lamports: u64, payout: &str, fee_secret: &[u8; 32]) -> Result<Kept, String> {
    let buyer = Hash32::from_str(buyer.trim()).map_err(|_| "That is not an Orien address.".to_string())?;
    let swap = SolSwap::sell(orien, cluster, buyer, orn, lamports, payout.trim(), fee_secret).map_err(|error| error.to_string())?;
    Ok(Kept { id: hex32(&swap.hash), swap, offer: None, reply: None, problem: None, created: now(), board: None, wallet: Some(orien.wallet_file.clone()), arrived: 0 })
}

/// Buyer: takes `offer`, a seller's offer code; anything left over and any
/// refund go to `refund`, the buyer's own SOL wallet.
pub fn buy(orien: &AppOrien, cluster: &str, offer: &str, refund: &str) -> Result<Kept, String> {
    let offer = SolOffer::read(offer.trim()).map_err(|error| error.to_string())?;
    // The Solana lock refuses a buyer and seller with the same wallet.
    if offer.payout == refund.trim() {
        return Err("That SOL address is the seller's own. Give the address of a different SOL wallet, the one you'll pay from.".into());
    }
    let sol = solana(cluster);
    let (swap, reply): (SolSwap, SolReply) = SolSwap::buy(orien, &sol, cluster, &offer, refund.trim()).map_err(|error| error.to_string())?;
    Ok(Kept {
        id: hex32(&swap.hash),
        swap,
        offer: None,
        reply: Some(reply.code()),
        problem: None,
        created: now(),
        board: None,
        wallet: Some(orien.wallet_file.clone()),
        arrived: 0,
    })
}

/// Seller: takes up the buyer's reply code.
pub fn accept(kept: &mut Kept, reply: &str) -> Result<(), String> {
    let reply = SolReply::read(reply.trim()).map_err(|error| error.to_string())?;
    kept.swap.accept(&reply).map_err(|error| error.to_string())
}

/// Moves one swap along, as far as it goes now, and notes what changed.
pub fn advance(orien: &AppOrien, kept: &mut Kept) {
    let sol = solana(&kept.swap.cluster);
    kept.problem = kept.swap.step(&sol, orien).err().map(|error| format!("{error:#}"));
    if kept.offer.is_none() && kept.swap.role == Role::Seller && kept.swap.lock.is_some() {
        kept.offer = kept.swap.offer(orien.network.clone()).ok().map(|offer| offer.code());
    }
    // How much of the buyer's payment has arrived, to show as it comes in.
    if kept.swap.stage == SolStage::WaitingForPayment
        && let Ok(address) = kept.swap.pay_address()
        && let Ok(key) = solana::address(&address)
        && let Ok(balance) = sol.balance(&key)
    {
        kept.arrived = balance;
    }
}

/// Moves every unfinished swap along, saving after each. A board swap
/// approved when posted (sellers) or taken (buyers) goes on with that
/// approval.
pub fn step_all(orien: &AppOrien, dir: &Path, all: &mut [Kept]) -> Result<(), String> {
    for index in 0..all.len() {
        if all[index].swap.finished() {
            continue;
        }
        let mut orien = orien.with_wallet(all[index].wallet.as_deref().unwrap_or(&orien.wallet_file));
        if orien.password.is_none()
            && let Some(offer) = all[index].board
            && let Some(password) = approval::approved(dir, offer, all[index].swap.orn, &orien.wallet_file)
        {
            orien = orien.with_password(password);
        }
        advance(&orien, &mut all[index]);
        save(dir, all)?;
    }
    Ok(())
}

pub fn active(all: &[Kept]) -> bool {
    all.iter().any(|kept| !kept.swap.finished())
}

pub fn view(kept: &Kept) -> View {
    let swap = &kept.swap;
    let sol = show_sol(swap.lamports);
    let (stage, status) = match (&swap.role, &swap.stage) {
        (Role::Seller, SolStage::Preparing) => ("locking", "Locking your ORN for the buyer.".to_string()),
        (Role::Seller, SolStage::WaitingForReply) => ("reply", "Accepted: your ORN are locked for the buyer. Waiting for them to pay.".to_string()),
        (Role::Seller, SolStage::WaitingForSol) => ("paying", "The buyer is paying. This app takes the SOL as soon as it's locked, usually within a minute.".to_string()),
        (Role::Seller, SolStage::Claiming) => ("claiming", format!("Paid: {sol} SOL is on its way to your SOL address.")),
        (Role::Seller, SolStage::Done) => ("done", format!("Done: {sol} SOL is in your SOL wallet.")),
        (Role::Buyer, SolStage::WaitingForPayment) => ("pay", format!("The seller accepted. Send exactly {} SOL from your SOL wallet to the address below. This app sees it by itself.", show_sol(swap.to_pay))),
        (Role::Buyer, SolStage::Locking) => ("locking", "Payment received. Locking it for the swap.".to_string()),
        (Role::Buyer, SolStage::WaitingForSecret) => ("confirming", "Your SOL is locked for the seller. Their wallet takes it and your ORN arrive, usually within a minute.".to_string()),
        (Role::Buyer, SolStage::ClaimingOrn) => ("claim", "The seller took the SOL. Claiming your ORN.".to_string()),
        (Role::Buyer, SolStage::Refunding) => ("refunding", "The seller didn't take the SOL in time. Sending it back to your SOL wallet.".to_string()),
        (Role::Buyer, SolStage::Done) if swap.orn_claimed => ("done", "Done: the ORN are in your wallet, and anything left over went back to your SOL wallet.".to_string()),
        (Role::Buyer, SolStage::Done) => ("done", "Done: your SOL went back to your SOL wallet.".to_string()),
        (_, SolStage::Cancelled { why }) => ("cancelled", format!("Stopped: {why}.")),
        _ => ("locking", String::new()),
    };
    let paying = swap.role == Role::Buyer && swap.stage == SolStage::WaitingForPayment;
    let pay_to = paying.then(|| swap.pay_address().ok()).flatten();
    let pay_link = paying.then(|| swap.pay_link().ok()).flatten();
    View {
        id: kept.id.clone(),
        coin: "sol",
        role: if swap.role == Role::Seller { "sell" } else { "buy" },
        stage,
        status,
        orn: crate::money::show(swap.orn as i128),
        sol,
        pay_qr: pay_link.as_deref().and_then(qr),
        pay_amount: paying.then(|| show_sol(swap.to_pay)),
        arrived: (paying && kept.arrived > 0).then(|| show_sol(kept.arrived)),
        pay_to,
        pay_link,
        paid: swap.role == Role::Buyer && !paying && !matches!(swap.stage, SolStage::Cancelled { .. }),
        orn_claimed: swap.orn_claimed,
        problem: kept.problem.clone(),
        created: kept.created,
        board: kept.board.is_some(),
        finished: swap.finished(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sol_amounts_read_and_show() {
        assert_eq!(read_sol("0.15"), Ok(150_000_000));
        assert_eq!(read_sol("1"), Ok(LAMPORTS_PER_SOL));
        assert_eq!(read_sol(".5"), Ok(500_000_000));
        assert!(read_sol("0.0000000001").is_err());
        assert!(read_sol("abc").is_err());
        assert_eq!(show_sol(150_000_000), "0.15");
        assert_eq!(show_sol(2 * LAMPORTS_PER_SOL), "2");
        assert_eq!(show_sol(1), "0.000000001");
    }
}

#[cfg(test)]
mod live_checks {
    /// Asks Solana itself: run with --ignored.
    #[test]
    #[ignore]
    fn payouts_and_the_lock_on_solana() {
        assert!(super::payout_problem("TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA").is_some(), "a program is refused");
        assert!(super::payout_problem("not an address").is_some());
        assert!(super::payout_problem("9WzDXwBbmkg8ZTbNMqUxvQRAyrZzDsGYdLVL9zYtAWWM").is_none(), "a plain wallet is fine");
        assert!(super::lock_live("mainnet"), "live on Solana");
        assert!(super::lock_live("devnet"));
    }
}
