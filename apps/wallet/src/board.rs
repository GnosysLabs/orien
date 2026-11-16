//! The swap board on the marketplace server: offers of ORN for Bitcoin, and
//! the codes the two wallets pass each other through it. The board only
//! carries messages; the swap itself runs here, as any other swap does.
//!
//! The app signs in with the same account as the website (an emailed code),
//! keeps the session cookie in a private file, and, while it is open, moves
//! each of its board swaps along: the seller's wallet locks the ORN once
//! someone takes the offer and posts its offer code; the buyer's wallet
//! takes up that code and posts its reply; the seller's wallet takes up the
//! reply; and either side marks the swap finished at the end.

use crate::swaps::{self, AppOrien, Kept};
use orien_net::store::write_replacing;
use orien_swap::swap::Stage;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::fs;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::time::Duration;

/// The marketplace, live. Debug builds can be pointed at a test copy with
/// `ORIEN_MARKET`, such as http://localhost:7200.
const LIVE: &str = "https://orien.network";
const COOKIE: &str = "orien_market";

pub fn base() -> String {
    if cfg!(debug_assertions)
        && let Ok(base) = std::env::var("ORIEN_MARKET")
    {
        return base.trim_end_matches('/').to_string();
    }
    LIVE.to_string()
}

/// One of this wallet's board swaps, as the app remembers it.
#[derive(Clone, Serialize, Deserialize)]
pub struct Link {
    /// The offer's number on the board.
    pub offer: i64,
    pub selling: bool,
    /// The seller's payout address, or the buyer's refund address.
    pub bitcoin: String,
    /// The swap this app runs for it, once there is one.
    #[serde(default)]
    pub swap: Option<String>,
    /// Marked finished on the board.
    #[serde(default)]
    pub finished: bool,
    /// The wallet that posted or took the offer, which its swap runs with
    /// whichever is open later. None for links from before several wallets:
    /// the open one.
    #[serde(default)]
    pub wallet: Option<PathBuf>,
}

#[derive(Default, Serialize, Deserialize)]
struct Saved {
    /// The session cookie's value; private, like the wallet.
    cookie: Option<String>,
    email: Option<String>,
    #[serde(default)]
    links: Vec<Link>,
    /// This app's own name for itself on the board, made up once: of all
    /// the account's devices, only the one that claimed a taken offer runs
    /// it, so two can never lock the same ORN twice.
    #[serde(default)]
    device: Option<String>,
}

/// This app's device name, made up and saved the first time it's wanted.
fn device(dir: &Path) -> Result<String, String> {
    let mut saved = load(dir);
    if let Some(device) = &saved.device {
        return Ok(device.clone());
    }
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes).map_err(|error| error.to_string())?;
    let device: String = bytes.iter().map(|byte| format!("{byte:02x}")).collect();
    saved.device = Some(device.clone());
    save(dir, &saved)?;
    Ok(device)
}

/// Which of the app's wallets has `address`, if any: every device with the
/// same recovery phrase has the same wallet, under its own file.
fn wallet_with(dir: &Path, address: &str) -> Option<PathBuf> {
    crate::known_wallets(dir)
        .into_iter()
        .map(|w| w.path)
        .find(|path| orien_net::keys::Wallet::peek(path).is_ok_and(|(own, _)| own.to_string() == address))
}

fn file(dir: &Path) -> PathBuf {
    dir.join("board.json")
}

fn load(dir: &Path) -> Saved {
    fs::read(file(dir)).ok().and_then(|bytes| serde_json::from_slice(&bytes).ok()).unwrap_or_default()
}

fn save(dir: &Path, saved: &Saved) -> Result<(), String> {
    let text = serde_json::to_string_pretty(saved).map_err(|error| error.to_string())?;
    write_replacing(&file(dir), text.as_bytes(), true).map_err(|error| format!("could not save the swap board's details: {error}"))
}

/// An answer from the board, or why there is none: `None` when it wants
/// the app to sign in (again).
enum Failed {
    SignIn,
    Said(String),
}

impl From<Failed> for String {
    fn from(failed: Failed) -> String {
        match failed {
            Failed::SignIn => "signin: sign in to the swap board".into(),
            Failed::Said(text) => text,
        }
    }
}

fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new().timeout(Duration::from_secs(15)).user_agent("orien-wallet").build()
}

/// Asks the board, sending the session cookie and keeping any renewed one.
fn call(dir: &Path, method: &str, path: &str, body: Option<Value>) -> Result<Value, Failed> {
    let mut saved = load(dir);
    let mut request = agent().request(method, &format!("{}/api/market{path}", base()));
    if let Some(cookie) = &saved.cookie {
        request = request.set("Cookie", &format!("{COOKIE}={cookie}"));
    }
    let answer = match body {
        Some(body) => request.send_json(body),
        None => request.call(),
    };
    let response = match answer {
        Ok(response) => response,
        Err(ureq::Error::Status(401, _)) => {
            if saved.cookie.take().is_some() {
                let _ = save(dir, &saved);
            }
            return Err(Failed::SignIn);
        }
        Err(ureq::Error::Status(_, response)) => {
            let said = response.into_json::<Value>().ok().and_then(|v| v["error"].as_str().map(capital));
            return Err(Failed::Said(said.unwrap_or_else(|| "The swap board could not do that.".into())));
        }
        Err(_) => return Err(Failed::Said("The swap board can't be reached. Check your internet connection.".into())),
    };
    let renewed = response.all("set-cookie").into_iter().find_map(|header| {
        let (pair, _) = header.split_once(';').unwrap_or((header, ""));
        let (name, value) = pair.trim().split_once('=')?;
        (name == COOKIE).then(|| value.to_string())
    });
    if let Some(value) = renewed {
        saved.cookie = (!value.is_empty()).then_some(value);
        save(dir, &saved).map_err(Failed::Said)?;
    }
    response.into_json().map_err(|_| Failed::Said("The swap board sent an answer this app can't read.".into()))
}

fn capital(text: &str) -> String {
    let mut chars = text.chars();
    let mut out: String = chars.next().map(|c| c.to_uppercase().collect()).unwrap_or_default();
    out.extend(chars);
    if !out.ends_with('.') {
        out.push('.');
    }
    out
}

/// The open offers, cheapest first, and Bitcoin's dollar price.
pub fn offers(dir: &Path) -> Result<Value, String> {
    let mut answer = call(dir, "GET", "/swaps", None)?;
    // Where sellers' pictures come from.
    answer["base"] = json!(base());
    Ok(answer)
}

/// Who is signed in: `None` when no one is.
pub fn me(dir: &Path) -> Result<Option<Value>, String> {
    if load(dir).cookie.is_none() {
        return Ok(None);
    }
    match call(dir, "GET", "/me", None) {
        Ok(me) => {
            let account = &me["account"];
            Ok(Some(json!({
                "id": account["id"],
                "name": account["name"],
                "picture": account["picture"],
                // The wallet the account has proven, for selling on the
                // marketplace. Swaps don't need one.
                "wallet": account["wallet"],
                "email": load(dir).email,
            })))
        }
        Err(Failed::SignIn) => Ok(None),
        Err(failed) => Err(failed.into()),
    }
}

pub fn email_start(dir: &Path, email: &str) -> Result<(), String> {
    call(dir, "POST", "/email/start", Some(json!({ "email": email.trim() })))?;
    Ok(())
}

/// Signs in with the emailed code. `Ok(false)` means the email has no
/// account yet, and a name is wanted to make one.
pub fn email_finish(dir: &Path, email: &str, code: &str, name: Option<&str>) -> Result<bool, String> {
    let mut body = json!({ "email": email.trim(), "code": code.trim() });
    if let Some(name) = name {
        body["name"] = json!(name.trim());
    }
    let answer = call(dir, "POST", "/email/finish", Some(body))?;
    if answer["needs_name"].as_bool() == Some(true) {
        return Ok(false);
    }
    let mut saved = load(dir);
    if saved.cookie.is_none() {
        return Err("The swap board did not sign this app in. Try again.".into());
    }
    saved.email = Some(email.trim().to_string());
    save(dir, &saved)?;
    // A fresh sign-in, perhaps to another account: tell it the wallets anew.
    *REPORTED.lock().unwrap() = None;
    Ok(true)
}

/// What the Market was last told about this app's wallets, so it is told
/// again only on a change.
static REPORTED: std::sync::Mutex<Option<Value>> = std::sync::Mutex::new(None);

/// Tells the Market which wallets this device has and their names here, so
/// the website can offer them by name ("Sell from"). Only while signed in;
/// each device's list replaces only its own. `empty` withdraws them all.
pub fn report_wallets(dir: &Path, empty: bool) -> Result<(), String> {
    if load(dir).cookie.is_none() {
        return Ok(());
    }
    let wallets: Vec<Value> = if empty {
        Vec::new()
    } else {
        crate::known_wallets(dir)
            .into_iter()
            .filter_map(|w| {
                let address = orien_net::keys::Wallet::peek(&w.path).ok()?.0.to_string();
                Some(json!({ "address": address, "name": w.name }))
            })
            .take(50)
            .collect()
    };
    let body = json!({ "device": device(dir)?, "wallets": wallets });
    if REPORTED.lock().unwrap().as_ref() == Some(&body) {
        return Ok(());
    }
    call(dir, "PUT", "/me/wallets", Some(body.clone())).map_err(String::from)?;
    *REPORTED.lock().unwrap() = if empty { None } else { Some(body) };
    Ok(())
}

pub fn sign_out(dir: &Path) -> Result<(), String> {
    let _ = report_wallets(dir, true);
    let _ = call(dir, "POST", "/signout", Some(json!({})));
    let mut saved = load(dir);
    saved.cookie = None;
    save(dir, &saved)
}

/// Links this wallet, `address`, to the signed-in account, as signing in
/// through the app does, so the account's buyers pay this wallet. Nothing
/// is paid: the app names its own address, so it can't be mistyped, and
/// naming someone else's wallet would only pay them. Returns whether a
/// link was made (or was already there); `false` when no one is signed in.
pub fn link_wallet(dir: &Path, address: &str) -> Result<bool, String> {
    if load(dir).cookie.is_none() {
        return Ok(false);
    }
    match call(dir, "POST", "/me/wallet", Some(json!({ "address": address, "app": true }))) {
        Ok(answer) if answer["wallet"].as_str() == Some(address) => Ok(true),
        Ok(_) => Err("The Market did not link this wallet.".into()),
        Err(Failed::SignIn) => Ok(false),
        Err(failed) => Err(failed.into()),
    }
}

fn bitcoin_address(text: &str, network: orien_swap::bitcoin::Network) -> Result<String, String> {
    let text = text.trim();
    orien_swap::bitcoin::Address::from_str(text)
        .ok()
        .and_then(|address| address.require_network(network).ok())
        .map(|_| text.to_string())
        .ok_or_else(|| "That is not a Bitcoin address.".into())
}

/// Puts an offer on the board, with the Bitcoin address to be paid at.
/// Posts an offer, approved with `password` (the wallet's, or None for a
/// wallet with no lock): when it's taken, this app accepts by itself.
pub fn post(dir: &Path, wallet: &Path, orn: u64, sats: u64, payout: &str, password: Option<&str>) -> Result<(), String> {
    let offer = post_on(dir, wallet, swaps::bitcoin().0, orn, sats, payout)?;
    crate::approval::approve(dir, offer, orn, wallet, password);
    Ok(())
}

pub fn post_on(dir: &Path, wallet: &Path, network: orien_swap::bitcoin::Network, orn: u64, sats: u64, payout: &str) -> Result<i64, String> {
    let payout = bitcoin_address(payout, network)?;
    // The payout address and the wallet go on the offer too, so any of the
    // account's devices with this wallet can run the swap once it's taken.
    let address = orien_net::keys::Wallet::peek(wallet).map_err(|error| error.to_string())?.0.to_string();
    let posted = call(dir, "POST", "/swaps", Some(json!({ "orn": orn, "sats": sats, "payout": payout, "wallet": address })))?;
    let offer = posted["offer"]["id"].as_i64().ok_or("The swap board did not say which offer it posted.")?;
    let mut saved = load(dir);
    saved.links.push(Link { offer, selling: true, bitcoin: payout, swap: None, finished: false, wallet: Some(wallet.to_path_buf()) });
    save(dir, &saved)?;
    Ok(offer)
}

/// Takes an offer for this wallet, `address`, with the Bitcoin address a
/// refund would go to. The seller locks the ORN for `address`.
pub fn take(dir: &Path, wallet: &Path, offer: i64, address: &str, refund: &str) -> Result<(), String> {
    take_on(dir, wallet, swaps::bitcoin().0, offer, address, refund)
}

pub fn take_on(dir: &Path, wallet: &Path, network: orien_swap::bitcoin::Network, offer: i64, address: &str, refund: &str) -> Result<(), String> {
    let refund = bitcoin_address(refund, network)?;
    // Version 2: the shorter locks (12 hours of ORN, 6 of Bitcoin). The
    // board turns away takes from wallets that would refuse them.
    call(dir, "POST", &format!("/swaps/{offer}/take"), Some(json!({ "address": address, "swap_version": 2 })))?;
    let mut saved = load(dir);
    saved.links.retain(|link| link.offer != offer);
    saved.links.push(Link { offer, selling: false, bitcoin: refund, swap: None, finished: false, wallet: Some(wallet.to_path_buf()) });
    save(dir, &saved)
}

pub fn withdraw(dir: &Path, offer: i64) -> Result<(), String> {
    call(dir, "POST", &format!("/swaps/{offer}/close"), Some(json!({})))?;
    crate::approval::forget(dir, offer);
    Ok(())
}

/// Board swaps of this account that have no swap in this app yet, as the
/// window shows them: offers on the board, and ones taken but not started.
/// Each says whether this app can run it (`here`): only the device that
/// posted or took an offer knows its Bitcoin address and wallet.
pub fn waiting(dir: &Path, mine: &Value) -> Vec<Value> {
    let saved = load(dir);
    mine.as_array()
        .into_iter()
        .flatten()
        .filter(|item| {
            let id = item["offer"]["id"].as_i64();
            !saved.links.iter().any(|link| Some(link.offer) == id && link.swap.is_some())
        })
        .filter(|item| matches!(item["offer"]["state"].as_str(), Some("open" | "held" | "swapping")))
        .map(|item| {
            let id = item["offer"]["id"].as_i64();
            let mut item = item.clone();
            // This app can run it: it posted or took it, or the offer names
            // a wallet this app has.
            let here = saved.links.iter().any(|link| Some(link.offer) == id)
                || item["wallet"].as_str().is_some_and(|address| item["payout"].is_string() && wallet_with(dir, address).is_some());
            // Another of the account's devices claimed it.
            let elsewhere = item["runner"].as_str().is_some_and(|runner| saved.device.as_deref() != Some(runner));
            item["here"] = json!(here);
            // Waiting for its wallet to be unlocked, and whether Touch ID can.
            let needs = NEEDS_UNLOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
            if let Some(wallet) = id.and_then(|id| needs.get(&id)) {
                item["needs_unlock"] = json!(true);
                item["unlock_wallet"] = json!(wallet.display().to_string());
                item["unlock_biometric"] = json!(crate::biometric::enabled(wallet));
            }
            item["elsewhere"] = json!(elsewhere);
            item
        })
        .collect()
}

/// Taken offers this app would start but for a locked wallet: offer, and
/// the wallet that must be unlocked.
pub static NEEDS_UNLOCK: std::sync::Mutex<std::collections::BTreeMap<i64, PathBuf>> =
    std::sync::Mutex::new(std::collections::BTreeMap::new());

/// Whether this device runs offer `id`, asking the board to make it the
/// runner if no other device is.
fn runs(dir: &Path, id: i64, device: &str) -> bool {
    call(dir, "POST", &format!("/swaps/{id}/run"), Some(json!({ "device": device })))
        .is_ok_and(|answer| answer["run"].as_bool() == Some(true))
}

/// This account's swaps on the board, as the board sees them.
pub fn mine(dir: &Path) -> Result<Value, String> {
    if load(dir).cookie.is_none() {
        return Ok(json!([]));
    }
    match call(dir, "GET", "/swaps/mine", None) {
        Ok(mine) => Ok(mine["swaps"].clone()),
        Err(Failed::SignIn) => Ok(json!([])),
        Err(failed) => Err(failed.into()),
    }
}

/// Moves this wallet's board swaps along as far as the board allows now,
/// starting swaps in `all` as their turn comes. Returns the board's list.
pub fn drive(orien: &AppOrien, dir: &Path, all: &mut Vec<Kept>) -> Result<Value, String> {
    let (network, chain) = swaps::bitcoin();
    drive_on(orien, network, &chain, dir, all)
}

/// `drive`, on a given Bitcoin network.
pub fn drive_on(
    orien: &AppOrien,
    network: orien_swap::bitcoin::Network,
    chain: &orien_swap::chain::Explorers,
    dir: &Path,
    all: &mut Vec<Kept>,
) -> Result<Value, String> {
    let mine = mine(dir)?;
    let device = device(dir)?;
    let mut saved = load(dir);
    for item in mine.as_array().into_iter().flatten() {
        let Some(id) = item["offer"]["id"].as_i64() else { continue };
        let state = item["offer"]["state"].as_str().unwrap_or_default();
        // A taken offer posted elsewhere (another device, or the website)
        // that names a wallet this app has: this app can run it, if it gets
        // there first.
        if state == "held"
            && !saved.links.iter().any(|link| link.offer == id)
            && let (Some(payout), Some(address)) = (item["payout"].as_str(), item["wallet"].as_str())
            && let Some(wallet) = wallet_with(dir, address)
            && let Ok(bitcoin) = bitcoin_address(payout, network)
        {
            saved.links.push(Link { offer: id, selling: true, bitcoin, swap: None, finished: false, wallet: Some(wallet) });
        }
        let Some(link) = saved.links.iter_mut().find(|link| link.offer == id) else { continue };
        // The board says whether this side has reported the end already.
        if item["finished"].as_bool() == Some(true) {
            link.finished = true;
        }
        let kept = link.swap.as_ref().and_then(|swap| all.iter().position(|kept| &kept.id == swap));
        // The wallet that posted or took the offer, not necessarily the open one.
        let orien = &orien.with_wallet(link.wallet.as_deref().unwrap_or(&orien.wallet_file));
        let result = match (link.selling, kept) {
            // Seller: someone took the offer. Start the swap: it is saved
            // with its secret, then the regular steps lock the ORN.
            // Only once the board says this device runs it: the first of the
            // account's devices to ask.
            (true, None) if state == "held" && !runs(dir, id, &device) => Ok(()),
            (true, None) if state == "held" => {
                let buyer = item["taker_wallet"].as_str().unwrap_or_default();
                let (orn, sats) = (item["offer"]["orn"].as_u64().unwrap_or(0), item["offer"]["sats"].as_u64().unwrap_or(0));
                let wallet = link.wallet.clone().unwrap_or_else(|| orien.wallet_file.clone());
                // Approved when posted, for exactly this many ORN: no one
                // needs to be at the computer.
                let approved = crate::approval::approved(dir, id, orn, &wallet);
                let orien = &match (&orien.password, approved) {
                    (None, Some(password)) => orien.with_password(password),
                    _ => orien.with_wallet(&orien.wallet_file),
                };
                let started = swaps::sell_on(orien, network, buyer, orn, sats, &link.bitcoin).map(|mut kept| {
                    kept.board = Some(id);
                    link.swap = Some(kept.id.clone());
                    all.push(kept);
                });
                // A locked wallet waits for its owner: Touch ID, or the
                // password typed, and the next step goes on.
                let mut needs = NEEDS_UNLOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
                match started {
                    Err(error) if error.starts_with("password:") => {
                        needs.insert(id, wallet);
                        Ok(())
                    }
                    other => {
                        needs.remove(&id);
                        other
                    }
                }
            }
            // Seller: the ORN are locked; pass the offer code on.
            (true, Some(index)) if state == "held" && item["offer_code"].is_null() => match all[index].offer.clone() {
                Some(code) => call(dir, "POST", &format!("/swaps/{id}/offer"), Some(json!({ "code": code, "device": device }))).map(|_| ()).map_err(String::from),
                None => Ok(()),
            },
            // Seller: the buyer replied; take it up.
            (true, Some(index)) if matches!(all[index].swap.stage, Stage::WaitingForReply) => match item["reply_code"].as_str() {
                Some(reply) => swaps::accept_on(orien, chain, &mut all[index], reply),
                None => Ok(()),
            },
            // Buyer: the seller's wallet locked the ORN; check its code and
            // reply. Paying the Bitcoin is the buyer's to do.
            (false, None) if state == "swapping" => match item["offer_code"].as_str() {
                Some(code) => swaps::buy_on(orien, network, chain, code, &link.bitcoin).map(|mut kept| {
                    kept.board = Some(id);
                    link.swap = Some(kept.id.clone());
                    all.push(kept);
                }),
                None => Ok(()),
            },
            (false, Some(index)) if state == "swapping" && item["reply_code"].is_null() => match all[index].reply.clone() {
                Some(code) => call(dir, "POST", &format!("/swaps/{id}/reply"), Some(json!({ "code": code }))).map(|_| ()).map_err(String::from),
                None => Ok(()),
            },
            _ => Ok(()),
        };
        if let Err(error) = result {
            if let Some(index) = link.swap.as_ref().and_then(|swap| all.iter().position(|kept| &kept.id == swap)) {
                all[index].problem = Some(error);
            } else {
                eprintln!("swap board offer {id}: {error}");
            }
        }
        // Either side, at the end.
        if let Some(index) = link.swap.as_ref().and_then(|swap| all.iter().position(|kept| &kept.id == swap))
            && !link.finished
            && state == "swapping"
        {
            let done = match &all[index].swap.stage {
                Stage::Done => Some(true),
                Stage::Cancelled { .. } => Some(false),
                _ => None,
            };
            if let Some(done) = done
                && call(dir, "POST", &format!("/swaps/{id}/finish"), Some(json!({ "done": done }))).is_ok()
            {
                link.finished = true;
            }
        }
        if matches!(state, "done" | "cancelled") {
            link.finished = true;
        }
    }
    // Approvals last as long as their offer: on the board, or its swap not
    // yet finished.
    let mut live: Vec<i64> = mine
        .as_array()
        .into_iter()
        .flatten()
        .filter(|item| matches!(item["offer"]["state"].as_str(), Some("open" | "held" | "swapping")))
        .filter_map(|item| item["offer"]["id"].as_i64())
        .collect();
    live.extend(all.iter().filter(|kept| !kept.swap.finished()).filter_map(|kept| kept.board));
    crate::approval::keep_only(dir, &live);
    swaps::save(dir, all)?;
    save(dir, &saved)?;
    Ok(mine)
}
