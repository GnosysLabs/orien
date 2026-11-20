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
    /// The seller's payout address, or the buyer's refund address: a
    /// Bitcoin one, or for a SOL offer a Solana one.
    pub bitcoin: String,
    /// "btc" or "sol". Links from before SOL swaps are Bitcoin ones.
    #[serde(default = "btc")]
    pub coin: String,
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

fn btc() -> String {
    "btc".into()
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
    // SOL offers first, then Bitcoin ones.
    let mut answer = call(dir, "GET", "/swaps?coins=sol,btc", None)?;
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
                "bio": account["bio"],
                "banner": account["banner"],
                "contact": account["contact"],
                // The wallet the account has proven, for selling on the
                // marketplace. Swaps don't need one.
                "wallet": account["wallet"],
                "wallet_verified": account["wallet_verified"],
                // Conversations with something new from the other side.
                "unread": me["unread"],
                // Their own listings, open first.
                "listings": me["listings"],
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

/// The Market sign-in, if this app is signed in: for registering the
/// iPhone's push token to the account too, and for the Safari extension.
#[allow(dead_code)]
pub fn session(dir: &Path) -> Option<String> {
    load(dir).cookie
}

/// A single-use pass, good for a minute, that signs a browser in to this
/// Orien Network account on orien.network: Sign in with Orien Wallet.
pub fn sign_in_pass(dir: &Path) -> Result<String, String> {
    let answer = call(dir, "POST", "/oidc/pass", Some(json!({})))?;
    answer["pass"].as_str().filter(|p| p.starts_with("ornp_")).map(str::to_string).ok_or_else(|| "Orien Network gave no sign-in pass.".into())
}

/// Deletes the signed-in account for good: profile, listings and messages.
/// `confirm` is the account's name, typed to be sure. Signed out after.
pub fn delete_account(dir: &Path, confirm: &str) -> Result<(), String> {
    call(dir, "POST", "/me/delete", Some(json!({ "confirm": confirm.trim() })))?;
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
    saved.links.push(Link { offer, selling: true, bitcoin: payout, coin: btc(), swap: None, finished: false, wallet: Some(wallet.to_path_buf()) });
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
    // Version 3: the shorter locks, and SOL swaps. The board turns away
    // takes from wallets that would refuse them.
    call(dir, "POST", &format!("/swaps/{offer}/take"), Some(json!({ "address": address, "swap_version": 3 })))?;
    let mut saved = load(dir);
    saved.links.retain(|link| link.offer != offer);
    saved.links.push(Link { offer, selling: false, bitcoin: refund, coin: btc(), swap: None, finished: false, wallet: Some(wallet.to_path_buf()) });
    save(dir, &saved)
}

/// Puts a SOL offer on the board: `orn` units for `lamports`, paid at
/// `payout`, the seller's own SOL wallet. Approved with `password`, as for
/// Bitcoin: when it's taken, this app accepts by itself.
pub fn post_sol(dir: &Path, wallet: &Path, orn: u64, lamports: u64, payout: &str, password: Option<&str>) -> Result<(), String> {
    let payout = payout.trim();
    crate::solswaps::payout_problem(payout).map_or(Ok(()), Err)?;
    let address = orien_net::keys::Wallet::peek(wallet).map_err(|error| error.to_string())?.0.to_string();
    let posted = call(dir, "POST", "/swaps", Some(json!({ "coin": "sol", "orn": orn, "lamports": lamports, "payout": payout, "wallet": address })))?;
    let offer = posted["offer"]["id"].as_i64().ok_or("The swap board did not say which offer it posted.")?;
    let mut saved = load(dir);
    saved.links.push(Link { offer, selling: true, bitcoin: payout.into(), coin: "sol".into(), swap: None, finished: false, wallet: Some(wallet.to_path_buf()) });
    save(dir, &saved)?;
    crate::approval::approve(dir, offer, orn, wallet, password);
    Ok(())
}

/// Takes a SOL offer for this wallet, `address`. The buyer pays from
/// `refund`, their own SOL wallet, and anything left over or refunded goes
/// back there; it must already hold SOL.
pub fn take_sol(dir: &Path, wallet: &Path, offer: i64, address: &str, refund: &str) -> Result<(), String> {
    let refund = refund.trim();
    let key = orien_swap::solana::address(refund).map_err(|_| "That is not a SOL address.".to_string())?;
    let solana = crate::solswaps::solana(crate::solswaps::cluster());
    match orien_swap::sol::payable(&solana, &key).map_err(|error| error.to_string())? {
        None => return Err(crate::solswaps::NOT_A_WALLET.into()),
        Some(0) => return Err("That SOL address has never held SOL. Give the address of the SOL wallet you'll pay from.".into()),
        Some(_) => {}
    }
    call(dir, "POST", &format!("/swaps/{offer}/take"), Some(json!({ "address": address, "swap_version": 3 })))?;
    let mut saved = load(dir);
    saved.links.retain(|link| link.offer != offer);
    saved.links.push(Link { offer, selling: false, bitcoin: refund.into(), coin: "sol".into(), swap: None, finished: false, wallet: Some(wallet.to_path_buf()) });
    save(dir, &saved)
}

/// What proving the payout wallet takes: send `orn` ORN from `address` to
/// `to`. The same amount until it's paid.
pub fn verify(dir: &Path, address: &str) -> Result<Value, String> {
    Ok(call(dir, "POST", "/me/wallet/verify", Some(json!({ "address": address.trim().to_lowercase() })))?["proof"].clone())
}

/// Makes `address` the wallet Market buyers pay (a verified one, once the
/// account has any).
pub fn set_paid_wallet(dir: &Path, address: &str) -> Result<(), String> {
    call(dir, "POST", "/me/wallet", Some(json!({ "address": address.trim().to_lowercase(), "app": true })))?;
    Ok(())
}

/// Takes a wallet off the profile, verified or waiting.
pub fn remove_wallet(dir: &Path, address: &str) -> Result<(), String> {
    call(dir, "POST", "/me/wallet/remove", Some(json!({ "address": address.trim().to_lowercase() })))?;
    Ok(())
}

/// The account's wallets and where each stands: verified, or waiting for
/// its payment.
pub fn wallet_states(dir: &Path) -> Result<Value, String> {
    Ok(call(dir, "GET", "/me/wallet", None)?["wallets"].clone())
}

/// Saves the signed-in account's name, bio and public usernames.
pub fn save_profile(dir: &Path, name: &str, bio: &str, telegram: &str, x: &str, discord: &str) -> Result<(), String> {
    call(dir, "PUT", "/me", Some(json!({
        "name": name.trim(),
        "bio": bio.trim(),
        "contact": { "telegram": telegram.trim(), "x": x.trim(), "discord": discord.trim() },
    })))?;
    Ok(())
}

/// Takes the profile's banner away.
pub fn remove_banner(dir: &Path) -> Result<(), String> {
    call(dir, "DELETE", "/me/banner", None)?;
    Ok(())
}

/// Sends a new profile picture, the file at `path`; the server makes it a
/// square and checks it.
pub fn set_picture(dir: &Path, path: &Path, banner: bool) -> Result<(), String> {
    let bytes = std::fs::read(path).map_err(|error| format!("could not read that picture: {error}"))?;
    if bytes.len() > 10 * 1024 * 1024 {
        return Err("That picture is too big. Pick one under 10 MB.".into());
    }
    let saved = load(dir);
    let mut request = agent().post(&format!("{}/api/market/me/{}", base(), if banner { "banner" } else { "picture" }));
    if let Some(cookie) = &saved.cookie {
        request = request.set("Cookie", &format!("{COOKIE}={cookie}"));
    }
    match request.send_bytes(&bytes) {
        Ok(_) => Ok(()),
        Err(ureq::Error::Status(401, _)) => Err("Sign in again to change your picture.".into()),
        Err(ureq::Error::Status(_, response)) => Err(response
            .into_json::<Value>()
            .ok()
            .and_then(|v| v["error"].as_str().map(capital))
            .unwrap_or_else(|| "That picture couldn't be used.".into())),
        Err(error) => Err(error.to_string()),
    }
}

/// A marketplace picture, as a data: URL, kept on disk after the first
/// fetch: picture names are new for every new picture, so a kept one never
/// goes stale. None for a name that isn't a picture's or can't be fetched.
pub fn image(dir: &Path, name: &str) -> Option<String> {
    use base64::Engine;
    if name.is_empty() || name.len() > 100 || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-' || c == '_') || name.starts_with('.') {
        return None;
    }
    let kept = dir.join("pictures");
    let file = kept.join(name);
    let bytes = match std::fs::read(&file) {
        Ok(bytes) => bytes,
        Err(_) => {
            let response = agent().get(&format!("{}/api/market/images/{name}", base())).call().ok()?;
            let mut bytes = Vec::new();
            std::io::Read::read_to_end(&mut std::io::Read::take(response.into_reader(), 5 * 1024 * 1024), &mut bytes).ok()?;
            let _ = std::fs::create_dir_all(&kept);
            let _ = std::fs::write(&file, &bytes);
            bytes
        }
    };
    let kind = if bytes.starts_with(&[0x89, b'P', b'N', b'G']) { "image/png" } else if bytes.starts_with(b"RIFF") { "image/webp" } else { "image/jpeg" };
    Some(format!("data:{kind};base64,{}", base64::engine::general_purpose::STANDARD.encode(&bytes)))
}

/// A Market read, with the sign-in cookie when there is one (so a listing
/// says whether it's yours), and `query` added to the address.
fn market_get(dir: &Path, path: &str, query: &[(&str, &str)]) -> Result<Value, String> {
    let saved = load(dir);
    let mut request = agent().get(&format!("{}/api/market{path}", base()));
    for (key, value) in query {
        request = request.query(key, value);
    }
    if let Some(cookie) = &saved.cookie {
        request = request.set("Cookie", &format!("{COOKIE}={cookie}"));
    }
    match request.call() {
        Ok(answer) => answer.into_json().map_err(|error| error.to_string()),
        Err(ureq::Error::Status(404, _)) => Err("This is gone or closed.".into()),
        Err(ureq::Error::Status(_, response)) => Err(response
            .into_json::<Value>()
            .ok()
            .and_then(|v| v["error"].as_str().map(capital))
            .unwrap_or_else(|| "The Market could not do that.".into())),
        Err(_) => Err("Can't reach Orien Network right now.".into()),
    }
}

/// A page of listings, newest first: in `category` (empty for all), with
/// `text` in the title or description, from `offset`.
pub fn listings(dir: &Path, category: &str, text: &str, offset: u32) -> Result<Value, String> {
    let offset = offset.to_string();
    market_get(dir, "/listings", &[("category", category), ("q", text.trim()), ("offset", &offset)])
}

/// A signed-in Market call for the Market screens: listings, messages and
/// reports only, so the page can't reach anything else with the cookie.
pub fn market_call(dir: &Path, method: &str, path: &str, body: Option<Value>) -> Result<Value, String> {
    let allowed = ["/listings", "/conversations", "/reports", "/accounts"].iter().any(|p| path == *p || path.starts_with(&format!("{p}/")) || path.starts_with(&format!("{p}?")));
    if !allowed || path.contains("..") || !["GET", "POST", "PUT", "DELETE"].contains(&method) {
        return Err("not a Market call".into());
    }
    call(dir, method, path, body).map_err(String::from)
}

/// Adds the picture at `path` to listing `id`; the Market crops it to 16:9.
pub fn listing_picture(dir: &Path, id: i64, path: &Path) -> Result<Value, String> {
    let bytes = std::fs::read(path).map_err(|error| format!("could not read that picture: {error}"))?;
    if bytes.len() > 8 * 1024 * 1024 {
        return Err("That picture is too big. Pick one under 8 MB.".into());
    }
    let saved = load(dir);
    let mut request = agent().post(&format!("{}/api/market/listings/{id}/pictures", base()));
    if let Some(cookie) = &saved.cookie {
        request = request.set("Cookie", &format!("{COOKIE}={cookie}"));
    }
    match request.send_bytes(&bytes) {
        Ok(answer) => answer.into_json().map_err(|error| error.to_string()),
        Err(ureq::Error::Status(401, _)) => Err("signin: sign in to the swap board".into()),
        Err(ureq::Error::Status(_, response)) => Err(response
            .into_json::<Value>()
            .ok()
            .and_then(|v| v["error"].as_str().map(capital))
            .unwrap_or_else(|| "That picture couldn't be used.".into())),
        Err(error) => Err(error.to_string()),
    }
}

/// One listing, its seller, and whether it's this account's.
pub fn listing(dir: &Path, id: i64) -> Result<Value, String> {
    market_get(dir, &format!("/listings/{id}"), &[])
}

/// A seller's profile and what they offer.
pub fn profile(dir: &Path, id: i64) -> Result<Value, String> {
    market_get(dir, &format!("/profiles/{id}"), &[])
}

/// Profiles whose name matches `text`, with their verified wallets, for
/// paying someone by name. Public, so asked without the sign-in cookie.
pub fn search(text: &str) -> Result<Value, String> {
    let answer = agent()
        .get(&format!("{}/api/market/profiles", base()))
        .query("q", text.trim())
        .call()
        .map_err(|error| error.to_string())?;
    let body: Value = answer.into_json().map_err(|error| error.to_string())?;
    Ok(body["profiles"].clone())
}

/// Marketplace profiles for wallet addresses: name and picture for each
/// address an account has proven, keyed by address; others are left out.
/// Public, so asked without the sign-in cookie.
pub fn profiles(addresses: &[String]) -> Result<Value, String> {
    let addresses: Vec<String> = addresses.iter().map(|a| a.trim().to_lowercase()).filter(|a| a.len() == 64).take(100).collect();
    if addresses.is_empty() {
        return Ok(json!({}));
    }
    let answer = agent()
        .post(&format!("{}/api/market/wallets/lookup", base()))
        .send_json(json!({ "addresses": addresses }))
        .map_err(|error| error.to_string())?;
    let body: Value = answer.into_json().map_err(|error| error.to_string())?;
    Ok(body["profiles"].clone())
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

/// Whether this device can start a taken offer at once: it holds the
/// approval given when the offer was posted, or the wallet is open here
/// without a password. Only then does it claim the swap, so a device that
/// would sit waiting for a password never keeps the one that can go ahead
/// from it. When it can't, it says the wallet needs unlocking here.
fn ready(orien: &AppOrien, dir: &Path, id: i64, item: &Value, wallet: Option<&Path>) -> bool {
    let wallet = wallet.map(Path::to_path_buf).unwrap_or_else(|| orien.wallet_file.clone());
    let orn = item["offer"]["orn"].as_u64().unwrap_or(0);
    let password = match crate::approval::approved(dir, id, orn, &wallet) {
        Some(password) => password,
        None if wallet == orien.wallet_file => orien.password.clone(),
        None => None,
    };
    let ready = crate::open_signing(&wallet, password.as_deref()).is_ok();
    let mut needs = NEEDS_UNLOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    if ready {
        needs.remove(&id);
    } else {
        needs.insert(id, wallet);
    }
    ready
}

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
            saved.links.push(Link { offer: id, selling: true, bitcoin, coin: btc(), swap: None, finished: false, wallet: Some(wallet) });
        }
        let Some(link) = saved.links.iter_mut().find(|link| link.offer == id) else { continue };
        // SOL offers are run by `drive_sol`.
        if link.coin == "sol" {
            continue;
        }
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
            (true, None) if state == "held" && !(ready(orien, dir, id, item, link.wallet.as_deref()) && runs(dir, id, &device)) => Ok(()),
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
    live.extend(crate::solswaps::load(dir).iter().filter(|kept| !kept.swap.finished()).filter_map(|kept| kept.board));
    crate::approval::keep_only(dir, &live);
    swaps::save(dir, all)?;
    save(dir, &saved)?;
    Ok(mine)
}

/// The wallet's own Solana fee key, which pays the small Solana fees of its
/// SOL sales: opened with the password, or Touch ID a moment ago.
fn fee_secret(wallet: &Path, password: Option<&str>) -> Result<[u8; 32], String> {
    match crate::open_signing(wallet, password)? {
        orien_net::unlock::Signing::Here(wallet) => Ok(wallet.solana_secret()),
        _ => Err("password: needed".into()),
    }
}

/// Moves this account's SOL board swaps along, as `drive` does for Bitcoin
/// ones, with `mine`, the board's list from `drive`.
pub fn drive_sol(orien: &AppOrien, dir: &Path, mine: &Value, sol: &mut Vec<crate::solswaps::Kept>) -> Result<(), String> {
    use crate::solswaps;
    use orien_swap::sol::SolStage;
    let device = device(dir)?;
    let cluster = solswaps::cluster();
    let mut saved = load(dir);
    for item in mine.as_array().into_iter().flatten() {
        if item["offer"]["coin"].as_str() != Some("sol") {
            continue;
        }
        let Some(id) = item["offer"]["id"].as_i64() else { continue };
        let state = item["offer"]["state"].as_str().unwrap_or_default();
        // Posted elsewhere, naming a wallet this app has: this app can run it.
        if state == "held"
            && !saved.links.iter().any(|link| link.offer == id)
            && let (Some(payout), Some(address)) = (item["payout"].as_str(), item["wallet"].as_str())
            && let Some(wallet) = wallet_with(dir, address)
            && orien_swap::solana::address(payout).is_ok()
        {
            saved.links.push(Link { offer: id, selling: true, bitcoin: payout.into(), coin: "sol".into(), swap: None, finished: false, wallet: Some(wallet) });
        }
        let Some(link) = saved.links.iter_mut().find(|link| link.offer == id) else { continue };
        if item["finished"].as_bool() == Some(true) {
            link.finished = true;
        }
        let kept = link.swap.as_ref().and_then(|swap| sol.iter().position(|kept| &kept.id == swap));
        let wallet = link.wallet.clone().unwrap_or_else(|| orien.wallet_file.clone());
        let orn = item["offer"]["orn"].as_u64().unwrap_or(0);
        let result = match (link.selling, kept) {
            (true, None) if state == "held" && !(ready(orien, dir, id, item, Some(&wallet)) && runs(dir, id, &device)) => Ok(()),
            // Seller: someone took the offer. Start the swap, approved when
            // posted; the steps then lock the ORN.
            (true, None) if state == "held" => {
                let lamports = item["offer"]["lamports"].as_u64().unwrap_or(0);
                let buyer = item["taker_wallet"].as_str().unwrap_or_default();
                let password = orien.password.clone().or_else(|| crate::approval::approved(dir, id, orn, &wallet).flatten());
                let orien = orien.with_wallet(&wallet).with_password(password.clone());
                let started = fee_secret(&wallet, password.as_deref()).and_then(|fee| {
                    solswaps::sell(&orien, cluster, buyer, orn, lamports, &link.bitcoin, &fee).map(|mut kept| {
                        kept.board = Some(id);
                        link.swap = Some(kept.id.clone());
                        sol.push(kept);
                    })
                });
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
            (true, Some(index)) if state == "held" && item["offer_code"].is_null() => match sol[index].offer.clone() {
                Some(code) => call(dir, "POST", &format!("/swaps/{id}/offer"), Some(json!({ "code": code, "device": device }))).map(|_| ()).map_err(String::from),
                None => Ok(()),
            },
            // Seller: the buyer's wallet replied; take it up.
            (true, Some(index)) if sol[index].swap.stage == SolStage::WaitingForReply => match item["reply_code"].as_str() {
                Some(reply) => solswaps::accept(&mut sol[index], reply),
                None => Ok(()),
            },
            // Buyer: the seller's ORN are locked; check them and reply at
            // once. Paying is the buyer's to do, from their own wallet.
            (false, None) if state == "swapping" => match item["offer_code"].as_str() {
                Some(code) => solswaps::buy(&orien.with_wallet(&wallet), cluster, code, &link.bitcoin).map(|mut kept| {
                    kept.board = Some(id);
                    link.swap = Some(kept.id.clone());
                    sol.push(kept);
                }),
                None => Ok(()),
            },
            (false, Some(index)) if state == "swapping" && item["reply_code"].is_null() => match sol[index].reply.clone() {
                Some(code) => call(dir, "POST", &format!("/swaps/{id}/reply"), Some(json!({ "code": code }))).map(|_| ()).map_err(String::from),
                None => Ok(()),
            },
            _ => Ok(()),
        };
        if let Err(error) = result {
            match link.swap.as_ref().and_then(|swap| sol.iter().position(|kept| &kept.id == swap)) {
                Some(index) => sol[index].problem = Some(error),
                None => eprintln!("swap board SOL offer {id}: {error}"),
            }
        }
        // Either side, at the end.
        if let Some(index) = link.swap.as_ref().and_then(|swap| sol.iter().position(|kept| &kept.id == swap))
            && !link.finished
            && state == "swapping"
        {
            let done = match &sol[index].swap.stage {
                SolStage::Done => Some(true),
                SolStage::Cancelled { .. } => Some(false),
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
    solswaps::save(dir, sol)?;
    save(dir, &saved)
}
