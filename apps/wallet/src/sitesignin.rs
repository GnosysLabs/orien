//! Signing in to a website with the wallet. A website asks through the
//! browser extension; the app comes forward and asks "Sign in to
//! example.com?", and only on Sign in does the wallet sign the website's
//! one-time value with its sign-in key (see `orien_crypto::signin`). The
//! website checks the proof itself. No ORN moves and no payment key is
//! used.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Mutex;
use std::sync::mpsc::{Sender, channel};
use std::time::Duration;
use tauri::{AppHandle, Emitter};

/// How long a website waits for an answer.
const WAIT: Duration = Duration::from_secs(150);
/// How long a sign-in stays good, in seconds.
const GOOD_FOR: u64 = 300;

/// The questions waiting for an answer, by number, each with where its
/// answer goes and what it asks.
static WAITING: Mutex<Option<HashMap<u64, (Sender<Result<serde_json::Value, String>>, Asked)>>> = Mutex::new(None);
static NEXT: Mutex<u64> = Mutex::new(1);

#[derive(Clone)]
struct Asked {
    origin: String,
    nonce: String,
}

/// Which one-time sign-in key this device uses next, for one wallet. Each
/// device picks its own bottom tree at random and counts within it, so
/// devices sharing a wallet never share a key.
///
/// After the first sign-in the device also keeps the wallet's sign-in key
/// (hex) with the spare key's binding and the first key's commitment, so
/// later sign-ins need neither the password nor Touch ID: the key signs in
/// to websites and can never move ORN.
#[derive(Clone, Default, Serialize, Deserialize)]
pub struct Count {
    pub slot: u32,
    pub next: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub binding: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub first_key: Option<String>,
}

type Kept = (orien_crypto::signin::SignInKey, orien_crypto::keytree::TreeSignature, orien_crypto::Hash32);

/// The sign-in key this device keeps for the wallet at `address`, if any.
fn kept(dir: &std::path::Path, address: &str) -> Option<Kept> {
    let saved = crate::settings(dir);
    let count = saved.sign_ins.get(address)?;
    let secret: [u8; 32] = hex::decode(count.key.as_ref()?).ok()?.try_into().ok()?;
    let binding = borsh::from_slice(&hex::decode(count.binding.as_ref()?).ok()?).ok()?;
    let first_key = count.first_key.as_ref()?.parse().ok()?;
    Some((orien_crypto::signin::SignInKey::from_secret(secret), binding, first_key))
}

fn keep(dir: &std::path::Path, address: &str, kept: &Kept) -> Result<(), String> {
    let mut saved = crate::settings(dir);
    let count = saved.sign_ins.entry(address.to_string()).or_default();
    count.key = Some(hex::encode(kept.0.secret()));
    count.binding = Some(hex::encode(borsh::to_vec(&kept.1).map_err(|error| error.to_string())?));
    count.first_key = Some(kept.2.to_string());
    crate::save_settings(dir, &saved)
}

/// What the window shows.
#[derive(Clone, Serialize)]
struct Ask {
    id: u64,
    site: String,
    origin: String,
    wallet: String,
    address: String,
    /// The wallet has a password and Touch ID doesn't hold it: ask for it.
    password: bool,
}

/// The website's name as people know it: `example.com`, `localhost:3000`.
fn site(origin: &str) -> String {
    origin.split("://").nth(1).unwrap_or(origin).to_string()
}

/// A website's request, from the extension: waits for the person's answer
/// and gives the website the signed proof, or why not.
/// Whether a website's request can be signed at all.
fn check(origin: &str, nonce: &str) -> Result<(), String> {
    let secure = origin.starts_with("https://") || origin.starts_with("http://localhost") || origin.starts_with("http://127.0.0.1");
    if !secure || origin.len() > 200 || origin[origin.find("://").unwrap_or(0) + 3..].contains('/') {
        return Err("Sign-in works on secure (https) websites only.".into());
    }
    if nonce.is_empty() || nonce.len() > 200 || nonce.chars().any(|c| c.is_control()) {
        return Err("The website's sign-in request isn't valid.".into());
    }
    Ok(())
}

/// A request the person already approved in the extension's window, as
/// Phantom asks in the browser: signed at once, with no window here. A
/// password only the first time on this device ("password: needed").
pub fn approved(app: &AppHandle, origin: String, nonce: String, password: Option<String>) -> Result<serde_json::Value, String> {
    check(&origin, &nonce)?;
    sign(app, &Asked { origin, nonce }, password)
}

pub fn request(app: &AppHandle, origin: String, nonce: String) -> Result<serde_json::Value, String> {
    check(&origin, &nonce)?;
    let dir = crate::folder(app)?;
    let file = crate::wallet_file(&dir).ok_or("Make a wallet in Orien Wallet first.")?;
    let address = orien_net::keys::Wallet::peek(&file).map_err(|error| error.to_string())?.0.to_string();
    let wallet = crate::known_wallets(&dir).into_iter().find(|w| w.path == file).map(|w| w.name).unwrap_or_else(|| "Wallet".into());
    // The password only the first time on this device, and only when Touch
    // ID doesn't already hold it.
    let password = crate::has_password(&file) && !crate::biometric::enabled(&file) && kept(&dir, &address).is_none();
    let id = {
        let mut next = NEXT.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        *next += 1;
        *next
    };
    let (tx, rx) = channel();
    WAITING
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .get_or_insert_with(HashMap::new)
        .insert(id, (tx, Asked { origin: origin.clone(), nonce }));
    #[cfg(desktop)]
    crate::background::show(app);
    let _ = app.emit("site-signin", Ask { id, site: site(&origin), origin, wallet, address, password });
    let answer = rx.recv_timeout(WAIT).unwrap_or_else(|_| Err("There was no answer in Orien Wallet.".into()));
    if let Some(waiting) = WAITING.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).as_mut()
        && waiting.remove(&id).is_some()
    {
        // Timed out: the window's question is withdrawn.
        let _ = app.emit("site-signin-gone", id);
    }
    answer
}

/// The person's answer, from the window. Sign in signs and sends the proof
/// to the website; the click is the approval, as no ORN moves. A wrong
/// password (first sign-in only) stays here, to try again.
#[tauri::command]
pub async fn site_signin_answer(app: AppHandle, id: u64, approve: bool, password: Option<String>) -> Result<(), String> {
    let asked = {
        let waiting = WAITING.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        waiting.as_ref().and_then(|w| w.get(&id)).map(|(_, asked)| asked.clone()).ok_or("The website stopped waiting.")?
    };
    let answer = if approve {
        let signed = tauri::async_runtime::spawn_blocking(move || sign(&app, &asked, password))
            .await
            .map_err(|error| error.to_string())?;
        // Keep the question open for another try on a password problem.
        if let Err(problem) = &signed
            && problem.starts_with("password:")
        {
            return Err(problem.clone());
        }
        signed
    } else {
        Err("You chose not to sign in.".into())
    };
    let sender = WAITING.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).as_mut().and_then(|w| w.remove(&id));
    if let Some((tx, _)) = sender {
        let failed = answer.as_ref().err().cloned();
        let _ = tx.send(answer);
        if let Some(problem) = failed
            && approve
        {
            return Err(problem);
        }
    }
    Ok(())
}

fn sign(app: &AppHandle, asked: &Asked, password: Option<String>) -> Result<serde_json::Value, String> {
    use base64::Engine;
    let dir = crate::folder(app)?;
    let file = crate::wallet_file(&dir).ok_or("no wallet yet")?;
    let address = orien_net::keys::Wallet::peek(&file).map_err(|error| error.to_string())?.0;
    let (key, binding, first_key) = match kept(&dir, &address.to_string()) {
        Some(kept) => kept,
        // The first sign-in on this device: the wallet makes its sign-in
        // key, once, which needs its seed.
        None => {
            let password = password
                .filter(|p| !p.is_empty())
                .or_else(|| crate::biometric::enabled(&file).then(|| crate::biometric::kept_password(&file)).flatten());
            let mut wallet = match crate::open_signing(&file, password.as_deref())? {
                orien_net::unlock::Signing::Here(wallet) => wallet,
                _ => return Err("This wallet can't sign here.".into()),
            };
            let made = wallet.sign_in_key().map_err(|error| error.to_string())?;
            keep(&dir, &address.to_string(), &made)?;
            made
        }
    };
    // The next one-time key, counted before it signs so it is never used twice.
    let (slot, leaf) = crate::next_sign_in_key(&dir, &address.to_string())?;
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let message = orien_crypto::signin::Message {
        origin: asked.origin.clone(),
        address,
        nonce: asked.nonce.clone(),
        issued: now,
        expires: now + GOOD_FOR,
    };
    let proof = key.sign(slot, leaf, message, binding, first_key);
    let bytes = borsh::to_vec(&proof).map_err(|error| error.to_string())?;
    Ok(serde_json::json!({
        "address": address.to_string(),
        "proof": base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes),
    }))
}
