//! Funded SOL buy orders backed by `programs/orien-order` on Solana.
//!
//! Neither buyer nor seller needs to stay online after sending their step:
//! - Buyer funds a one-time Solana address (QR code), Orien Wallet opens the
//!   escrow on Solana (`OpenOrder`) with `authority = wallet.solana_secret()`
//!   (so Cancel is 1-click), sweeps leftovers back to the buyer's refund
//!   wallet, and registers `{ order_pubkey, secret }` with the Market server.
//! - Seller reserves the order on Solana (0 SOL needed: server relays `Fill`
//!   when `min_bond == 0`), verifies finalized `STATE_FILLED` directly against
//!   Solana before locking a single ORN, locks ORN on Orien, and reports the
//!   lock so the server settles both chains in ~1 minute.

use crate::approval;
use crate::board;
use crate::solswaps::{self, show_sol};
use crate::swaps::{AppOrien, now, qr};
use orien_crypto::Hash32;
use orien_net::store::write_replacing;
use orien_swap::order::{
    self, DEFAULT_EXPIRY_SECONDS, DEFAULT_FILL_WINDOW, ORDER_LEN, OrderTerms, STATE_FILLED, STATE_OPEN,
};
use orien_swap::sol::{CUSHION, FEE, FEE_KEY_FEES, MIN_LAMPORTS, ORN_EPOCHS, RESEND_SECONDS};
use orien_swap::solana::{self, Keypair, LAMPORTS_PER_SOL, Landed, Pubkey, Solana};
use orien_swap::swap::{LockRef, Orien};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::str::FromStr;

/// Client-side default cap matching the server (10 SOL per order).
pub const MAX_ORDER_LAMPORTS: u64 = 10 * LAMPORTS_PER_SOL;
/// Minimum remaining seconds on `fill_deadline` required before a seller locks ORN.
const MIN_FILL_SAFETY_SECONDS: i64 = 1_800;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum OrderRole {
    Buyer,
    Seller,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum OrderStage {
    // Buyer
    Funding,
    Opening,
    Registering,
    Open,
    Cancelling,
    Settling,
    // Seller
    Reserving,
    LockingOrn,
    ReportingFill,
    // Terminal
    Done,
    Cancelled { why: String },
}

/// One funded buy order as Orien Wallet persists it in `funded-orders.json`.
#[derive(Clone, Serialize, Deserialize)]
pub struct KeptOrder {
    pub id: String,
    pub role: OrderRole,
    pub stage: OrderStage,
    pub cluster: String,
    pub program: String,
    #[serde(default)]
    pub server_id: Option<i64>,
    pub order_pubkey: String,
    pub orn: u64,
    pub lamports: u64,
    pub hash: [u8; 32],
    pub buyer_orien: String,
    pub buyer_sol: String,
    pub expiry: i64,
    pub max_fill_window: i64,
    #[serde(default)]
    pub wallet: Option<PathBuf>,
    pub created: u64,
    #[serde(default)]
    pub problem: Option<String>,

    // Buyer secrets & funding state (NEVER returned to UI or logged)
    #[serde(default)]
    pub secret: Option<[u8; 32]>,
    #[serde(default)]
    pub one_time: Option<String>,
    #[serde(default)]
    pub order_key: Option<String>,
    #[serde(default)]
    pub authority_key: Option<String>,
    #[serde(default)]
    pub to_pay: u64,
    #[serde(default)]
    pub arrived: u64,
    #[serde(default)]
    pub swept: bool,
    #[serde(default)]
    pub sent: Option<(String, i64)>,

    // Seller state
    #[serde(default)]
    pub seller_sol: Option<String>,
    #[serde(default)]
    pub lock_from: u64,
    #[serde(default)]
    pub lock_seq: Option<u64>,
    #[serde(default)]
    pub lock_until: u64,
    #[serde(default)]
    pub reported: bool,
    #[serde(default)]
    pub other_name: Option<String>,
}

/// Public UI view of a funded buy order, shaped identically to `solswaps::View`
/// so the existing Exchange list, detail screen, and QR pay card render it cleanly.
/// NEVER includes `secret`, `one_time`, `order_key`, or `authority_key`.
#[derive(Serialize)]
pub struct View {
    pub id: String,
    pub coin: &'static str,
    pub role: &'static str,
    pub funded_order: bool,
    pub server_id: Option<i64>,
    pub stage: &'static str,
    pub status: String,
    pub orn: String,
    pub sol: String,
    pub pay_to: Option<String>,
    /// The swap's one-time Solana address, kept after paying too, so the
    /// wallet's Solana activity can say which swap a payment was for.
    pub sol_address: Option<String>,
    /// Buyer: where refunds go, and so the wallet that pays.
    pub refund: Option<String>,
    pub pay_amount: Option<String>,
    pub pay_link: Option<String>,
    pub pay_qr: Option<String>,
    pub arrived: Option<String>,
    pub paid: bool,
    pub orn_claimed: bool,
    pub can_cancel: bool,
    pub other: Option<String>,
    pub expiry: Option<i64>,
    pub problem: Option<String>,
    pub created: u64,
    pub board: bool,
    pub finished: bool,
}

fn file(dir: &Path) -> PathBuf {
    dir.join("funded-orders.json")
}

pub fn load(dir: &Path) -> Vec<KeptOrder> {
    fs::read(file(dir)).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
}

pub fn save(dir: &Path, all: &[KeptOrder]) -> Result<(), String> {
    let text = serde_json::to_string_pretty(all).map_err(|e| e.to_string())?;
    write_replacing(&file(dir), text.as_bytes(), true).map_err(|e| format!("could not save the buy order: {e}"))
}

pub fn active(all: &[KeptOrder]) -> bool {
    all.iter().any(|o| !o.finished())
}

/// Which `orien-order` program ID this build uses on `cluster`.
pub fn program_id(cluster: &str) -> Result<Pubkey, String> {
    if cfg!(debug_assertions)
        && let Ok(val) = std::env::var("ORIEN_ORDER_PROGRAM")
        && !val.trim().is_empty()
    {
        return solana::address(val.trim()).map_err(|_| "Invalid ORIEN_ORDER_PROGRAM address.".to_string());
    }
    if cluster == "devnet" {
        return solana::address(order::DEVNET_ORDER_PROGRAM).map_err(|e| e.to_string());
    }
    match order::MAINNET_ORDER_PROGRAM {
        Some(id) => solana::address(id).map_err(|e| e.to_string()),
        None => Err("Funded buy orders are coming to mainnet soon (currently testing on Solana devnet).".into()),
    }
}

fn random32() -> Result<[u8; 32], String> {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).map_err(|e| format!("randomness failed: {e}"))?;
    Ok(bytes)
}

fn key_from_hex(text: &str) -> Result<Keypair, String> {
    let bytes: [u8; 32] = hex::decode(text)
        .ok()
        .and_then(|b| b.try_into().ok())
        .ok_or_else(|| "a saved Solana key is damaged".to_string())?;
    Ok(Keypair::from_secret(&bytes))
}

fn authority_topup(sol: &Solana, auth: &Pubkey) -> Result<u64, String> {
    let balance = sol.balance(auth).map_err(|e| e.to_string())?;
    let minimum = sol.rent(0).map_err(|e| e.to_string())?;
    Ok(if balance >= minimum + 5 * FEE {
        0
    } else {
        (minimum + FEE_KEY_FEES * FEE).saturating_sub(balance)
    })
}

impl KeptOrder {
    pub fn finished(&self) -> bool {
        match (&self.role, &self.stage) {
            (OrderRole::Buyer, OrderStage::Done | OrderStage::Cancelled { .. }) => self.swept,
            (OrderRole::Seller, OrderStage::Done | OrderStage::Cancelled { .. }) => true,
            _ => false,
        }
    }

    pub fn pay_address(&self) -> Option<String> {
        let hex = self.one_time.as_deref()?;
        let kp = key_from_hex(hex).ok()?;
        Some(solana::text(&kp.pubkey()))
    }

    pub fn pay_link(&self) -> Option<String> {
        let addr = self.pay_address()?;
        Some(format!("solana:{addr}?amount={}", show_sol(self.to_pay)))
    }
}

/// Buyer: creates a new funded SOL buy order locally and returns its ID so the
/// UI can immediately show the QR code to fund it.
pub fn create_buy_order(
    dir: &Path,
    wallet: &Path,
    orn: u64,
    lamports: u64,
    refund: &str,
    password: Option<&str>,
) -> Result<String, String> {
    if orn == 0 {
        return Err("Enter an amount of ORN to buy.".into());
    }
    if lamports < MIN_LAMPORTS {
        return Err(format!("Smallest buy order is {} SOL.", show_sol(MIN_LAMPORTS)));
    }
    if lamports > MAX_ORDER_LAMPORTS {
        return Err(format!("Buy orders are limited to {} SOL per order for now.", show_sol(MAX_ORDER_LAMPORTS)));
    }
    let cluster = solswaps::cluster();
    let program = program_id(cluster)?;
    let refund = board::check_sol_refund(refund)?;
    let buyer_orien = orien_net::keys::Wallet::peek(wallet).map_err(|e| e.to_string())?.0.to_string();
    let auth_secret = board::fee_secret(wallet, password)?;
    let auth_kp = Keypair::from_secret(&auth_secret);

    let sol = solswaps::solana(cluster);
    let order_rent = sol.rent(ORDER_LEN).map_err(|e| format!("Could not reach Solana: {e}"))?;
    let rent0 = sol.rent(0).map_err(|e| format!("Could not reach Solana: {e}"))?;
    let auth_topup = authority_topup(&sol, &auth_kp.pubkey())?;
    let needed = lamports + order_rent + rent0 + 3 * FEE + auth_topup;

    let secret = random32()?;
    let hash = orien_ledger::types::lock_hash(&secret);
    let one_time = random32()?;
    let order_key = random32()?;
    let order_pubkey = solana::text(&Keypair::from_secret(&order_key).pubkey());
    let id = hex::encode(hash);

    let kept = KeptOrder {
        id: id.clone(),
        role: OrderRole::Buyer,
        stage: OrderStage::Funding,
        cluster: cluster.into(),
        program: solana::text(&program),
        server_id: None,
        order_pubkey,
        orn,
        lamports,
        hash,
        buyer_orien,
        buyer_sol: refund,
        expiry: 0,
        max_fill_window: DEFAULT_FILL_WINDOW,
        wallet: Some(wallet.to_path_buf()),
        created: now(),
        problem: None,
        secret: Some(secret),
        one_time: Some(hex::encode(one_time)),
        order_key: Some(hex::encode(order_key)),
        authority_key: Some(hex::encode(auth_secret)),
        to_pay: needed + CUSHION,
        arrived: 0,
        swept: false,
        sent: None,
        seller_sol: None,
        lock_from: 0,
        lock_seq: None,
        lock_until: 0,
        reported: false,
        other_name: None,
    };

    let mut all = load(dir);
    all.push(kept);
    save(dir, &all)?;
    // Approved just now: paid at once from this wallet's own Solana balance,
    // when that's where refunds go (and so the wallet it pays from).
    if let Ok(opened) = orien_net::keys::Wallet::open(wallet, password) {
        let secret = opened.solana_devnet_secret();
        let index = all.len() - 1;
        if all[index].buyer_sol == solana::text(&Keypair::from_secret(&secret).pubkey())
            && let Some(to) = all[index].pay_address()
        {
            match solswaps::pay_from_wallet(&sol, &secret, &to, all[index].to_pay) {
                Ok(_) => {}
                Err(error) => all[index].problem = Some(error),
            }
            save(dir, &all)?;
        }
    }
    Ok(id)
}

/// Seller: starts filling an open funded buy order from the order book.
/// Zero SOL required in the seller's wallet: the wallet reserves via the server
/// relayer, verifies finalized `STATE_FILLED` directly against Solana, then
/// locks ORN on Orien and triggers ~1-minute settlement.
#[allow(clippy::too_many_arguments)]
pub fn fill_buy_order(
    orien: &AppOrien,
    dir: &Path,
    wallet: &Path,
    server_id: i64,
    order_pubkey: &str,
    orn: u64,
    lamports: u64,
    hash_hex: &str,
    buyer_orien: &str,
    buyer_name: Option<String>,
    payout: &str,
    password: Option<&str>,
) -> Result<String, String> {
    let payout = payout.trim();
    if let Some(prob) = solswaps::payout_problem(payout) {
        return Err(prob);
    }
    let cluster = solswaps::cluster();
    let program = program_id(cluster)?;
    let order_pk = solana::address(order_pubkey.trim()).map_err(|_| "That order has an invalid Solana address.".to_string())?;
    let hash_bytes = hex::decode(hash_hex.trim()).map_err(|_| "Invalid order hash.".to_string())?;
    let hash: [u8; 32] = hash_bytes.try_into().map_err(|_| "Invalid order hash length.".to_string())?;
    let buyer_orien_id = Hash32::from_str(buyer_orien.trim()).map_err(|_| "Invalid buyer Orien address.".to_string())?;
    if buyer_orien_id == orien.me() {
        return Err("You cannot fill your own buy order.".into());
    }

    // Verify the funded order really exists and matches on Solana before starting!
    let sol = solswaps::solana(cluster);
    let acct = sol
        .order(&program, &order_pk)
        .map_err(|e| format!("Could not check Solana: {e}"))?
        .ok_or_else(|| "That buy order is no longer open on Solana.".to_string())?;
    if acct.state != STATE_OPEN {
        return Err("Someone else is already filling that order.".into());
    }
    if acct.terms.hash != hash
        || acct.terms.orn_amount != orn
        || acct.terms.amount != lamports
        || acct.terms.buyer_orien != buyer_orien_id.0
    {
        return Err("That buy order does not match Solana's on-chain record.".into());
    }
    if solana::text(&acct.terms.buyer) == payout {
        return Err("That SOL address is the buyer's own. Give your own SOL wallet address.".into());
    }
    if acct.terms.min_bond > 0 {
        return Err("This order requires a SOL bond to fill.".into());
    }

    let next_seq = orien.next_seq().map_err(|e| e.to_string())?;
    let until = orien.epoch().map_err(|e| e.to_string())? + ORN_EPOCHS;

    // Remember approval so background stepping locks ORN without re-asking password.
    approval::approve(dir, server_id, orn, wallet, password);

    let id = format!("fill-{}", hex::encode(hash));
    let kept = KeptOrder {
        id: id.clone(),
        role: OrderRole::Seller,
        stage: OrderStage::Reserving,
        cluster: cluster.into(),
        program: solana::text(&program),
        server_id: Some(server_id),
        order_pubkey: solana::text(&order_pk),
        orn,
        lamports,
        hash,
        buyer_orien: buyer_orien_id.to_string(),
        buyer_sol: solana::text(&acct.terms.buyer),
        expiry: acct.terms.expiry,
        max_fill_window: acct.terms.max_fill_window,
        wallet: Some(wallet.to_path_buf()),
        created: now(),
        problem: None,
        secret: None,
        one_time: None,
        order_key: None,
        authority_key: None,
        to_pay: 0,
        arrived: 0,
        swept: true,
        sent: None,
        seller_sol: Some(payout.to_string()),
        lock_from: next_seq,
        lock_seq: None,
        lock_until: until,
        reported: false,
        other_name: buyer_name,
    };

    let mut all = load(dir);
    all.retain(|o| o.id != id);
    all.push(kept);
    save(dir, &all)?;
    Ok(id)
}

/// Buyer: 1-click cancel for a funded buy order (either before funding or while open on Solana).
pub fn cancel_buy_order(dir: &Path, id: &str) -> Result<(), String> {
    let mut all = load(dir);
    let Some(order) = all.iter_mut().find(|o| o.id == id) else {
        return Err("Could not find that buy order.".into());
    };
    if order.role != OrderRole::Buyer {
        return Err("Only the buyer can cancel a buy order.".into());
    }
    match order.stage {
        OrderStage::Funding => {
            order.stage = OrderStage::Cancelled { why: "cancelled before funding; anything sent goes back to your SOL wallet".into() };
            order.problem = None;
            save(dir, &all)?;
            Ok(())
        }
        OrderStage::Open | OrderStage::Registering | OrderStage::Settling => {
            let sol = solswaps::solana(&order.cluster);
            let program = solana::address(&order.program).map_err(|e| e.to_string())?;
            let order_pk = solana::address(&order.order_pubkey).map_err(|e| e.to_string())?;
            let buyer_pk = solana::address(&order.buyer_sol).map_err(|e| e.to_string())?;
            let auth = key_from_hex(order.authority_key.as_deref().ok_or("Missing cancel authority key.")?)?;
            let Some(acct) = sol.order(&program, &order_pk).map_err(|e| e.to_string())? else {
                if let Some(sid) = order.server_id {
                    let _ = board::sync_funded_order(dir, sid);
                }
                order.stage = OrderStage::Cancelled { why: "order already closed on Solana".into() };
                save(dir, &all)?;
                return Ok(());
            };
            let t = sol.time().map_err(|e| e.to_string())?;
            if acct.state == STATE_FILLED && t < acct.fill_deadline {
                return Err("A seller is actively filling this order right now. It settles in about a minute.".into());
            }
            let ix = order::cancel(&program, &order_pk, &auth.pubkey(), &buyer_pk);
            let bh = sol.blockhash().map_err(|e| e.to_string())?;
            let tx = solana::transaction(&auth, &[], &[ix], bh).map_err(|e| e.to_string())?;
            let sig = sol.send(&tx).map_err(|e| format!("Could not send cancel transaction: {e}"))?;
            order.sent = Some((sig, t));
            order.stage = OrderStage::Cancelling;
            order.problem = None;
            save(dir, &all)?;
            Ok(())
        }
        OrderStage::Cancelling => Ok(()),
        _ => Err("That order cannot be cancelled right now.".into()),
    }
}

fn sweep_one_time(order: &mut KeptOrder, sol: &Solana) {
    if order.swept {
        return;
    }
    let Some(hex_key) = order.one_time.as_deref() else {
        order.swept = true;
        return;
    };
    let Ok(one_time) = key_from_hex(hex_key) else { return };
    let Ok(balance) = sol.balance(&one_time.pubkey()) else { return };
    if balance <= FEE {
        order.swept = true;
        return;
    }
    let Ok(refund) = solana::address(&order.buyer_sol) else { return };
    let Ok(bh) = sol.blockhash() else { return };
    let ix = solana::transfer(&one_time.pubkey(), &refund, balance - FEE);
    if let Ok(tx) = solana::transaction(&one_time, &[], &[ix], bh)
        && sol.send(&tx).is_ok()
    {
        order.swept = true;
    }
}

fn step_one(orien: &AppOrien, dir: &Path, order: &mut KeptOrder) -> Result<(), String> {
    let sol = solswaps::solana(&order.cluster);
    let program = solana::address(&order.program).map_err(|e| e.to_string())?;
    let order_pk = solana::address(&order.order_pubkey).map_err(|e| e.to_string())?;

    match (&order.role, &order.stage.clone()) {
        // ---- Buyer ----
        (OrderRole::Buyer, OrderStage::Funding) => {
            if let Ok(Some(acct)) = sol.order(&program, &order_pk) {
                order.expiry = acct.terms.expiry;
                order.stage = OrderStage::Registering;
                return Ok(());
            }
            let one_time = key_from_hex(order.one_time.as_deref().ok_or("missing funding key")?)?;
            let balance = sol.balance(&one_time.pubkey()).map_err(|e| e.to_string())?;
            order.arrived = balance;
            let auth = key_from_hex(order.authority_key.as_deref().ok_or("missing authority key")?)?;
            let order_kp = key_from_hex(order.order_key.as_deref().ok_or("missing order key")?)?;
            let deposit = sol.rent(ORDER_LEN).map_err(|e| e.to_string())?;
            let rent0 = sol.rent(0).map_err(|e| e.to_string())?;
            let topup = authority_topup(&sol, &auth.pubkey())?;
            let needed = order.lamports + deposit + topup + FEE + rent0 + 2 * FEE;
            if balance < needed {
                return Ok(());
            }
            let t = sol.time().map_err(|e| e.to_string())?;
            let expiry = t + DEFAULT_EXPIRY_SECONDS;
            let buyer_pk = solana::address(&order.buyer_sol).map_err(|e| e.to_string())?;
            let buyer_orien = Hash32::from_str(&order.buyer_orien).map_err(|_| "bad buyer_orien".to_string())?;
            let terms = OrderTerms {
                hash: order.hash,
                buyer: buyer_pk,
                authority: auth.pubkey(),
                buyer_orien: buyer_orien.0,
                orn_amount: order.orn,
                amount: order.lamports,
                expiry,
                min_bond: 0,
                max_fill_window: order.max_fill_window,
            };
            let mut ixs = order::open_order(&program, &one_time.pubkey(), &order_kp.pubkey(), &terms, deposit);
            if topup > 0 {
                ixs.push(solana::transfer(&one_time.pubkey(), &auth.pubkey(), topup));
            }
            let bh = sol.blockhash().map_err(|e| e.to_string())?;
            let tx = solana::transaction(&one_time, &[&order_kp], &ixs, bh).map_err(|e| e.to_string())?;
            let sig = sol.send(&tx).map_err(|e| format!("Could not open buy order on Solana: {e}"))?;
            order.expiry = expiry;
            order.sent = Some((sig, t));
            order.stage = OrderStage::Opening;
            Ok(())
        }
        (OrderRole::Buyer, OrderStage::Opening) => {
            if let Ok(Some(acct)) = sol.order(&program, &order_pk) {
                order.expiry = acct.terms.expiry;
                sweep_one_time(order, &sol);
                order.stage = OrderStage::Registering;
                return Ok(());
            }
            if let Some((sig, sent_at)) = &order.sent {
                match sol.landed(sig) {
                    Ok(Some(Landed::Finalized)) => {}
                    Ok(Some(_)) => return Ok(()),
                    Err(_) => {
                        order.stage = OrderStage::Funding;
                        order.sent = None;
                    }
                    Ok(None) => {
                        if sol.time().is_ok_and(|t| t >= sent_at + RESEND_SECONDS) {
                            order.stage = OrderStage::Funding;
                            order.sent = None;
                        }
                    }
                }
            }
            Ok(())
        }
        (OrderRole::Buyer, OrderStage::Registering) => {
            sweep_one_time(order, &sol);
            let secret = order.secret.ok_or("missing order secret")?;
            let server_id = board::register_funded_order(dir, &order.order_pubkey, &hex::encode(secret))?;
            order.server_id = Some(server_id);
            order.stage = OrderStage::Open;
            Ok(())
        }
        (OrderRole::Buyer, OrderStage::Open | OrderStage::Settling) => {
            sweep_one_time(order, &sol);
            match sol.order(&program, &order_pk).map_err(|e| e.to_string())? {
                Some(acct) => {
                    let t = sol.time().map_err(|e| e.to_string())?;
                    if acct.state == STATE_FILLED && t < acct.fill_deadline {
                        order.stage = OrderStage::Settling;
                    } else if acct.state == STATE_OPEN {
                        order.stage = OrderStage::Open;
                    }
                    Ok(())
                }
                None => {
                    // Closed on Solana: check server to distinguish Done vs Cancelled/Expired.
                    if let Some(sid) = order.server_id
                        && let Ok(synced) = board::sync_funded_order(dir, sid)
                    {
                        match synced["state"].as_str() {
                            Some("done") => {
                                order.stage = OrderStage::Done;
                                return Ok(());
                            }
                            Some("expired") => {
                                order.stage = OrderStage::Cancelled { why: "order expired; SOL returned to your wallet".into() };
                                return Ok(());
                            }
                            Some("cancelled") => {
                                order.stage = OrderStage::Cancelled { why: "cancelled; SOL returned to your wallet".into() };
                                return Ok(());
                            }
                            _ => {}
                        }
                    }
                    // Fallback if offline: check if expired or settled.
                    let t = sol.time().unwrap_or(0);
                    if order.expiry > 0 && t >= order.expiry {
                        order.stage = OrderStage::Cancelled { why: "order expired; SOL returned to your wallet".into() };
                    } else {
                        order.stage = OrderStage::Done;
                    }
                    Ok(())
                }
            }
        }
        (OrderRole::Buyer, OrderStage::Cancelling) => {
            sweep_one_time(order, &sol);
            if sol.order(&program, &order_pk).map_err(|e| e.to_string())?.is_none() {
                if let Some(sid) = order.server_id {
                    let _ = board::sync_funded_order(dir, sid);
                }
                order.stage = OrderStage::Cancelled { why: "cancelled; SOL returned to your wallet".into() };
                return Ok(());
            }
            let t = sol.time().map_err(|e| e.to_string())?;
            let resend = order.sent.as_ref().is_none_or(|(_, sent_at)| t >= sent_at + RESEND_SECONDS);
            if resend {
                let auth = key_from_hex(order.authority_key.as_deref().ok_or("missing authority key")?)?;
                let buyer_pk = solana::address(&order.buyer_sol).map_err(|e| e.to_string())?;
                let ix = order::cancel(&program, &order_pk, &auth.pubkey(), &buyer_pk);
                let bh = sol.blockhash().map_err(|e| e.to_string())?;
                let tx = solana::transaction(&auth, &[], &[ix], bh).map_err(|e| e.to_string())?;
                let sig = sol.send(&tx).map_err(|e| e.to_string())?;
                order.sent = Some((sig, t));
            }
            Ok(())
        }
        (OrderRole::Buyer, OrderStage::Done | OrderStage::Cancelled { .. }) => {
            sweep_one_time(order, &sol);
            Ok(())
        }

        // ---- Seller ----
        (OrderRole::Seller, OrderStage::Reserving) => {
            let seller_sol = order.seller_sol.as_deref().ok_or("missing seller SOL address")?;
            let seller_pk = solana::address(seller_sol).map_err(|e| e.to_string())?;
            let Some(acct) = sol.order(&program, &order_pk).map_err(|e| e.to_string())? else {
                order.stage = OrderStage::Cancelled { why: "that buy order was cancelled or expired".into() };
                return Ok(());
            };
            let t = sol.time().map_err(|e| e.to_string())?;
            if acct.state == STATE_FILLED {
                if acct.seller != seller_pk {
                    order.stage = OrderStage::Cancelled { why: "another seller filled that buy order first".into() };
                    return Ok(());
                }
                let buyer_orien = Hash32::from_str(&order.buyer_orien).map_err(|_| "bad buyer_orien".to_string())?;
                if acct.terms.hash != order.hash
                    || acct.terms.orn_amount != order.orn
                    || acct.terms.amount != order.lamports
                    || acct.terms.buyer_orien != buyer_orien.0
                    || t + MIN_FILL_SAFETY_SECONDS >= acct.fill_deadline
                {
                    order.stage = OrderStage::Cancelled { why: "that order's fill window is too short to settle safely".into() };
                    return Ok(());
                }
                // Finalized on Solana with our payout address! Now safe to lock ORN.
                order.stage = OrderStage::LockingOrn;
                return Ok(());
            }
            if acct.state == STATE_OPEN {
                let resend = order.sent.as_ref().is_none_or(|(_, sent_at)| t >= sent_at + RESEND_SECONDS);
                if resend {
                    let sid = order.server_id.ok_or("missing server order id")?;
                    let sig = board::reserve_funded_order(dir, sid, seller_sol, &orien.me().to_string())?;
                    order.sent = Some((sig.unwrap_or_else(|| "reserved".into()), t));
                }
            }
            Ok(())
        }
        (OrderRole::Seller, OrderStage::LockingOrn) => {
            let seller_sol = order.seller_sol.as_deref().ok_or("missing seller SOL address")?;
            let seller_pk = solana::address(seller_sol).map_err(|e| e.to_string())?;
            let buyer_orien = Hash32::from_str(&order.buyer_orien).map_err(|_| "bad buyer_orien".to_string())?;
            // Crash recovery: did we already lock ORN at lock_from?
            let lock_ref = LockRef { from: orien.me(), seq: order.lock_from };
            if let Ok(state) = orien.proven(&lock_ref)
                && (state.secret.is_some()
                    || state.waiting.is_some_and(|w| w.hash == order.hash && w.to == buyer_orien && w.amount == order.orn))
            {
                order.lock_seq = Some(order.lock_from);
                order.stage = OrderStage::ReportingFill;
                return Ok(());
            }
            // Re-verify finalized Solana fill immediately before locking a single ORN!
            let Some(acct) = sol.order(&program, &order_pk).map_err(|e| e.to_string())? else {
                order.stage = OrderStage::Cancelled { why: "that buy order closed before your ORN were locked".into() };
                return Ok(());
            };
            let t = sol.time().map_err(|e| e.to_string())?;
            if acct.state != STATE_FILLED || acct.seller != seller_pk || t + MIN_FILL_SAFETY_SECONDS >= acct.fill_deadline {
                order.stage = OrderStage::Cancelled { why: "your reservation lapsed before locking ORN".into() };
                return Ok(());
            }
            let locked = orien
                .lock(order.lock_from, &buyer_orien, order.orn, order.hash, order.lock_until)
                .map_err(|e| e.to_string())?;
            order.lock_seq = Some(locked.seq);
            order.stage = OrderStage::ReportingFill;
            Ok(())
        }
        (OrderRole::Seller, OrderStage::ReportingFill) => {
            let sid = order.server_id.ok_or("missing server order id")?;
            if !order.reported {
                let seq = order.lock_seq.ok_or("missing lock sequence")?;
                // Wait until validators prove the lock, then report to server.
                if let Err(e) = board::report_funded_fill(dir, sid, &orien.me().to_string(), seq) {
                    // Normal while validators finalize the ORN lock (~10-20s).
                    if e.to_lowercase().contains("not proven") || e.to_lowercase().contains("finalized") {
                        return Ok(());
                    }
                    return Err(e);
                }
                order.reported = true;
            }
            if sol.order(&program, &order_pk).map_err(|e| e.to_string())?.is_none() {
                let _ = board::sync_funded_order(dir, sid);
                order.stage = OrderStage::Done;
            }
            Ok(())
        }
        (OrderRole::Seller, OrderStage::Done | OrderStage::Cancelled { .. }) => Ok(()),
        _ => Ok(()),
    }
}

/// Advances all active funded buy orders by one step, saving after each.
pub fn step_all(orien: &AppOrien, dir: &Path, all: &mut [KeptOrder]) -> Result<(), String> {
    for index in 0..all.len() {
        if all[index].finished() {
            continue;
        }
        let mut orien = orien.with_wallet(all[index].wallet.as_deref().unwrap_or(&orien.wallet_file));
        if orien.password.is_none()
            && let Some(sid) = all[index].server_id
            && let Some(password) = approval::approved(dir, sid, all[index].orn, &orien.wallet_file)
        {
            orien = orien.with_password(password);
        }
        all[index].problem = step_one(&orien, dir, &mut all[index]).err();
        save(dir, all)?;
    }
    Ok(())
}

pub fn view(o: &KeptOrder) -> View {
    let sol = show_sol(o.lamports);
    let (stage, status) = match (&o.role, &o.stage) {
        (OrderRole::Buyer, OrderStage::Funding) => (
            "pay",
            format!(
                "Send exactly {} SOL from your SOL wallet to fund this buy order. Once funded, it stays live on the order book even when Orien Wallet is closed.",
                show_sol(o.to_pay)
            ),
        ),
        (OrderRole::Buyer, OrderStage::Opening) => (
            "locking",
            "Payment received. Locking your SOL on Solana now. Keep this page open for about 20 seconds.".to_string(),
        ),
        (OrderRole::Buyer, OrderStage::Registering) => (
            "locking",
            "Locked on Solana. Adding your buy order to the order book now. Keep this page open for a few more seconds.".to_string(),
        ),
        (OrderRole::Buyer, OrderStage::Open) => (
            "open",
            "Your buy order is live on the order book. You are all set to leave this page or close Orien Wallet. Any extra SOL has been sent back to your wallet, and when a seller takes your order your ORN will arrive automatically.".to_string(),
        ),
        (OrderRole::Buyer, OrderStage::Cancelling) => (
            "cancelling",
            "Cancelling your buy order on Solana and returning your SOL to your wallet. Keep this page open for about 15 seconds.".to_string(),
        ),
        (OrderRole::Buyer, OrderStage::Settling) => (
            "settling",
            "A seller filled your order. Delivering your ORN automatically, usually within a minute.".to_string(),
        ),
        (OrderRole::Buyer, OrderStage::Done) => ("done", format!("Done: {} ORN are in your wallet.", crate::money::show(o.orn as i128))),
        (OrderRole::Buyer, OrderStage::Cancelled { why }) => ("cancelled", format!("Stopped: {why}.")),

        (OrderRole::Seller, OrderStage::Reserving) => ("reserving", "Reserving the buyer's locked SOL on Solana before locking your ORN. Keep this page open for about 15 seconds.".to_string()),
        (OrderRole::Seller, OrderStage::LockingOrn) => ("locking", "Buyer's SOL reserved. Locking your ORN now. Keep this page open for a few more seconds.".to_string()),
        (OrderRole::Seller, OrderStage::ReportingFill) => (
            "settling",
            format!("Your ORN are locked. You can leave this page; {sol} SOL arrives in your SOL wallet in about a minute."),
        ),
        (OrderRole::Seller, OrderStage::Done) => ("done", format!("Done: {sol} SOL is in your SOL wallet.")),
        (OrderRole::Seller, OrderStage::Cancelled { why }) => ("cancelled", format!("Stopped: {why}.")),
        _ => ("locking", String::new()),
    };
    let paying = o.role == OrderRole::Buyer && o.stage == OrderStage::Funding;
    let pay_to = paying.then(|| o.pay_address()).flatten();
    let pay_link = paying.then(|| o.pay_link()).flatten();
    let can_cancel = o.role == OrderRole::Buyer && matches!(o.stage, OrderStage::Funding | OrderStage::Open | OrderStage::Registering);
    View {
        id: o.id.clone(),
        coin: "sol",
        role: if o.role == OrderRole::Seller { "sell" } else { "buy" },
        funded_order: true,
        server_id: o.server_id,
        stage,
        status,
        orn: crate::money::show(o.orn as i128),
        sol,
        pay_qr: pay_link.as_deref().and_then(qr),
        pay_amount: paying.then(|| show_sol(o.to_pay)),
        arrived: (paying && o.arrived > 0).then(|| show_sol(o.arrived)),
        pay_to,
        sol_address: o.pay_address(),
        refund: (o.role == OrderRole::Buyer).then(|| o.buyer_sol.clone()),
        pay_link,
        paid: o.role == OrderRole::Buyer && !paying && !matches!(o.stage, OrderStage::Cancelled { .. }),
        orn_claimed: o.role == OrderRole::Buyer && o.stage == OrderStage::Done,
        can_cancel,
        other: o.other_name.clone(),
        expiry: (o.expiry > 0).then_some(o.expiry),
        problem: o.problem.clone(),
        created: o.created,
        board: true,
        finished: o.finished(),
    }
}
