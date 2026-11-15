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
}

#[derive(Default, Serialize, Deserialize)]
struct Saved {
    /// The session cookie's value; private, like the wallet.
    cookie: Option<String>,
    email: Option<String>,
    #[serde(default)]
    links: Vec<Link>,
    /// When this app sent the payment that links its wallet to the account,
    /// so it waits for the marketplace to see it rather than send again.
    #[serde(default)]
    verify_sent: Option<u64>,
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
    Ok(true)
}

pub fn sign_out(dir: &Path) -> Result<(), String> {
    let _ = call(dir, "POST", "/signout", Some(json!({})));
    let mut saved = load(dir);
    saved.cookie = None;
    save(dir, &saved)
}

/// What linking this wallet to the account came to.
pub enum Linked {
    /// Not signed in, or the marketplace can't be reached: nothing to do now.
    Nothing,
    /// The account's wallet is this one.
    Done,
    /// The account is linked to another wallet; it is left as it is.
    Other,
    /// This wallet holds too little to send the amount; tried again later.
    TooLittle,
    /// Sent; the marketplace hasn't seen it yet.
    Sent,
    /// The wallet has a password, and the app needs it to send.
    Password(String),
}

/// The most the marketplace asks for linking a wallet (0.01 ORN), with room
/// for the fee.
const LINK_NEEDS: i128 = 11_000_000;

/// Links this wallet, `address`, to the signed-in account, as signing in
/// through the app does: the marketplace names an exact tiny amount, and
/// `pay` sends it from this wallet. Never switches an account already
/// linked to another wallet, and sends at most once until the marketplace
/// has seen it (or an hour has passed).
pub fn link_wallet(
    dir: &Path,
    address: &str,
    balance: Option<i128>,
    pay: impl FnOnce(&str, u64) -> Result<(), String>,
) -> Result<Linked, String> {
    if load(dir).cookie.is_none() {
        return Ok(Linked::Nothing);
    }
    let me = match call(dir, "GET", "/me", None) {
        Ok(me) => me,
        Err(Failed::SignIn) => return Ok(Linked::Nothing),
        Err(failed) => return Err(failed.into()),
    };
    match me["account"]["wallet"].as_str() {
        Some(wallet) if wallet == address => {
            let mut saved = load(dir);
            if saved.verify_sent.take().is_some() {
                save(dir, &saved)?;
            }
            return Ok(Linked::Done);
        }
        Some(_) => return Ok(Linked::Other),
        None => {}
    }
    // Sent already: wait for the marketplace to see it.
    if load(dir).verify_sent.is_some_and(|at| now() < at + 3_600) {
        let answer = call(dir, "GET", "/me/wallet", None)?;
        return Ok(if answer["wallet"].as_str() == Some(address) { Linked::Done } else { Linked::Sent });
    }
    if balance.is_none_or(|balance| balance < LINK_NEEDS) {
        return Ok(Linked::TooLittle);
    }
    let answer = call(dir, "POST", "/me/wallet", Some(json!({ "address": address })))?;
    let (Some(pay_to), Some(amount)) = (answer["waiting"]["pay_to"].as_str(), answer["waiting"]["amount"].as_u64()) else {
        return Err("The marketplace did not say what to send.".into());
    };
    match pay(pay_to, amount) {
        Ok(()) => {
            let mut saved = load(dir);
            saved.verify_sent = Some(now());
            save(dir, &saved)?;
            Ok(Linked::Sent)
        }
        Err(error) if error.starts_with("password:") => Ok(Linked::Password(error)),
        Err(error) => Err(error),
    }
}

fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs())
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
pub fn post(dir: &Path, orn: u64, sats: u64, payout: &str) -> Result<(), String> {
    post_on(dir, swaps::bitcoin().0, orn, sats, payout)
}

pub fn post_on(dir: &Path, network: orien_swap::bitcoin::Network, orn: u64, sats: u64, payout: &str) -> Result<(), String> {
    let payout = bitcoin_address(payout, network)?;
    let posted = call(dir, "POST", "/swaps", Some(json!({ "orn": orn, "sats": sats })))?;
    let offer = posted["offer"]["id"].as_i64().ok_or("The swap board did not say which offer it posted.")?;
    let mut saved = load(dir);
    saved.links.push(Link { offer, selling: true, bitcoin: payout, swap: None, finished: false });
    save(dir, &saved)
}

/// Takes an offer for this wallet, `address`, with the Bitcoin address a
/// refund would go to. The seller locks the ORN for `address`.
pub fn take(dir: &Path, offer: i64, address: &str, refund: &str) -> Result<(), String> {
    take_on(dir, swaps::bitcoin().0, offer, address, refund)
}

pub fn take_on(dir: &Path, network: orien_swap::bitcoin::Network, offer: i64, address: &str, refund: &str) -> Result<(), String> {
    let refund = bitcoin_address(refund, network)?;
    call(dir, "POST", &format!("/swaps/{offer}/take"), Some(json!({ "address": address })))?;
    let mut saved = load(dir);
    saved.links.retain(|link| link.offer != offer);
    saved.links.push(Link { offer, selling: false, bitcoin: refund, swap: None, finished: false });
    save(dir, &saved)
}

pub fn withdraw(dir: &Path, offer: i64) -> Result<(), String> {
    call(dir, "POST", &format!("/swaps/{offer}/close"), Some(json!({})))?;
    Ok(())
}

/// Board swaps of this account that have no swap in this app yet, as the
/// window shows them: offers on the board, and ones taken but not started.
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
        .cloned()
        .collect()
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
    let mut saved = load(dir);
    for item in mine.as_array().into_iter().flatten() {
        let Some(id) = item["offer"]["id"].as_i64() else { continue };
        let state = item["offer"]["state"].as_str().unwrap_or_default();
        let Some(link) = saved.links.iter_mut().find(|link| link.offer == id) else { continue };
        // The board says whether this side has reported the end already.
        if item["finished"].as_bool() == Some(true) {
            link.finished = true;
        }
        let kept = link.swap.as_ref().and_then(|swap| all.iter().position(|kept| &kept.id == swap));
        let result = match (link.selling, kept) {
            // Seller: someone took the offer. Start the swap: it is saved
            // with its secret, then the regular steps lock the ORN.
            (true, None) if state == "held" => {
                let buyer = item["taker_wallet"].as_str().unwrap_or_default();
                let (orn, sats) = (item["offer"]["orn"].as_u64().unwrap_or(0), item["offer"]["sats"].as_u64().unwrap_or(0));
                swaps::sell_on(orien, network, buyer, orn, sats, &link.bitcoin).map(|mut kept| {
                    kept.board = Some(id);
                    link.swap = Some(kept.id.clone());
                    all.push(kept);
                })
            }
            // Seller: the ORN are locked; pass the offer code on.
            (true, Some(index)) if state == "held" && item["offer_code"].is_null() => match all[index].offer.clone() {
                Some(code) => call(dir, "POST", &format!("/swaps/{id}/offer"), Some(json!({ "code": code }))).map(|_| ()).map_err(String::from),
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
    swaps::save(dir, all)?;
    save(dir, &saved)?;
    Ok(mine)
}
