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
#[derive(Clone, Copy, Default, Serialize, Deserialize)]
pub struct Count {
    pub slot: u32,
    pub next: u32,
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
pub fn request(app: &AppHandle, origin: String, nonce: String) -> Result<serde_json::Value, String> {
    let secure = origin.starts_with("https://") || origin.starts_with("http://localhost") || origin.starts_with("http://127.0.0.1");
    if !secure || origin.len() > 200 || origin[origin.find("://").unwrap_or(0) + 3..].contains('/') {
        return Err("Sign-in works on secure (https) websites only.".into());
    }
    if nonce.is_empty() || nonce.len() > 200 || nonce.chars().any(|c| c.is_control()) {
        return Err("The website's sign-in request isn't valid.".into());
    }
    let dir = crate::folder(app)?;
    let file = crate::wallet_file(&dir).ok_or("Make a wallet in Orien Wallet first.")?;
    let address = orien_net::keys::Wallet::peek(&file).map_err(|error| error.to_string())?.0.to_string();
    let wallet = crate::known_wallets(&dir).into_iter().find(|w| w.path == file).map(|w| w.name).unwrap_or_else(|| "Wallet".into());
    let password = crate::has_password(&file) && !crate::biometric::enabled(&file);
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

/// The person's answer, from the window. Sign in: unlock (Touch ID on a
/// Mac when it holds the password, or the password typed), sign, and send
/// the proof to the website. A wrong password stays here, to try again.
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
    if password.as_deref().is_none_or(str::is_empty) && crate::has_password(&file) {
        if !crate::biometric::enabled(&file) {
            return Err("password: needed".into());
        }
        crate::biometric::unlock_for(&file, &format!("sign in to {}", site(&asked.origin)))?;
    }
    let mut wallet = match crate::open_signing(&file, password.as_deref())? {
        orien_net::unlock::Signing::Here(wallet) => wallet,
        _ => return Err("This wallet can't sign here.".into()),
    };
    let address = wallet.address().to_string();
    // The next one-time key, counted before it signs so it is never used twice.
    let (slot, leaf) = crate::next_sign_in_key(&dir, &address)?;
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let proof = wallet.sign_in(slot, leaf, &asked.origin, &asked.nonce, now, now + GOOD_FOR).map_err(|error| error.to_string())?;
    let bytes = borsh::to_vec(&proof).map_err(|error| error.to_string())?;
    Ok(serde_json::json!({
        "address": address,
        "proof": base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes),
    }))
}
