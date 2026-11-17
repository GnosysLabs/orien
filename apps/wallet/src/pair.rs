//! Moving a wallet to another of the person's own devices without typing its
//! 24 words. The sending device shows a short code (and a QR code holding
//! it); the receiving one types or scans it. The two devices meet in a
//! short-lived mailbox on the Market server and agree on a key from the code
//! (SPAKE2), so whoever doesn't know the code gets one wrong guess and
//! nothing else, and the server only ever passes along sealed bytes. Both
//! screens then show the same three emoji; once the person says they match,
//! the sending device seals the recovery phrase, the wallet's name and its
//! icon with that key, and the receiving device rebuilds the wallet from
//! them exactly as if the words had been typed.

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use chacha20poly1305::aead::{Aead, KeyInit};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use spake2::{Ed25519Group, Identity, Password, Spake2};
use std::sync::Mutex;
use std::time::{Duration, Instant};

const SENDER: &[u8] = b"orien wallet pair: sender";
const RECEIVER: &[u8] = b"orien wallet pair: receiver";

/// What travels, sealed.
#[derive(Serialize, Deserialize)]
pub struct Parcel {
    pub words: String,
    pub account: u32,
    pub name: String,
    pub icon: Option<String>,
}

/// Every wallet on the sending device, sent together when the new device
/// made the code ("Link a new device"). Older apps send one bare Parcel.
#[derive(Serialize, Deserialize)]
pub struct Bundle {
    pub wallets: Vec<Parcel>,
}

/// A pairing under way on this device: the mailbox, the code, and, once the
/// other device has answered, the agreed key.
struct Pairing {
    number: u32,
    spake: Option<Spake2<Ed25519Group>>,
    key: Option<[u8; 32]>,
    confirmed: bool,
}

static SENDING: Mutex<Option<Pairing>> = Mutex::new(None);
static RECEIVING: Mutex<Option<Pairing>> = Mutex::new(None);

fn url(path: &str) -> String {
    format!("{}/api/market/pair{path}", crate::board::base())
}

fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new().timeout(Duration::from_secs(15)).user_agent("orien-wallet").build()
}

const GONE: &str = "The pairing ran out of time. Start again.";
const OFFLINE: &str = "orien.network can't be reached right now. Check your connection and try again.";

fn put(number: u32, slot: &str, data: &[u8]) -> Result<(), String> {
    match agent().put(&url(&format!("/{number}/{slot}"))).send_json(json!({ "data": STANDARD.encode(data) })) {
        Ok(_) => Ok(()),
        Err(ureq::Error::Status(404, _)) => Err(GONE.into()),
        Err(ureq::Error::Status(409, _)) => Err("That code is already in use by another device. Start again.".into()),
        Err(_) => Err(OFFLINE.into()),
    }
}

/// The slot's bytes once written; None while it's still empty.
fn get(number: u32, slot: &str) -> Result<Option<Vec<u8>>, String> {
    match agent().get(&url(&format!("/{number}/{slot}"))).call() {
        Ok(response) if response.status() == 200 => {
            let body: Value = response.into_json().map_err(|_| OFFLINE.to_string())?;
            STANDARD.decode(body["data"].as_str().unwrap_or_default()).map(Some).map_err(|_| "The other device sent something damaged.".into())
        }
        Ok(_) => Ok(None),
        Err(ureq::Error::Status(404, _)) => Err(GONE.into()),
        Err(_) => Err(OFFLINE.into()),
    }
}

fn close(number: u32) {
    let _ = agent().delete(&url(&format!("/{number}"))).call();
}

/// Waits a little for a slot: up to `wait`, asking about once a second.
fn wait_for(number: u32, slot: &str, wait: Duration) -> Result<Option<Vec<u8>>, String> {
    let until = Instant::now() + wait;
    loop {
        if let Some(data) = get(number, slot)? {
            return Ok(Some(data));
        }
        if Instant::now() >= until {
            return Ok(None);
        }
        std::thread::sleep(Duration::from_millis(900));
    }
}

fn derive(context: &str, key: &[u8; 32]) -> [u8; 32] {
    blake3::derive_key(context, key)
}

/// The proof, from the receiving device, that it has the same key.
fn confirmation(key: &[u8; 32]) -> [u8; 32] {
    *blake3::keyed_hash(&derive("orien wallet pair 2026 confirm", key), b"receiver").as_bytes()
}

const EMOJI: [&str; 64] = [
    "🦊", "🐼", "🐙", "🦋", "🐢", "🦉", "🐳", "🦄", "🐝", "🦀", "🐬", "🦜", "🐧", "🦒", "🐘", "🦔",
    "🌵", "🌻", "🍄", "🌈", "🌙", "⭐", "🔥", "❄️", "🌊", "⚡", "🍀", "🌸", "🍁", "🌍", "☀️", "🌋",
    "🍎", "🍋", "🍉", "🍇", "🍒", "🥑", "🍩", "🍪", "🧁", "🍕", "🌮", "🍿", "🥨", "🧀", "🍓", "🥥",
    "🎈", "🎸", "🎲", "🚀", "⚓", "🔔", "💎", "🎯", "🧭", "🔑", "🎁", "🏆", "🪁", "🛸", "🎨", "🧩",
];

/// Three emoji from the key: the same on both devices only if they agreed.
fn emoji(key: &[u8; 32]) -> String {
    let bytes = derive("orien wallet pair 2026 emoji", key);
    bytes[..3].iter().map(|b| EMOJI[(*b as usize) % EMOJI.len()]).collect::<Vec<_>>().join(" ")
}

fn seal(key: &[u8; 32], plain: &[u8]) -> Result<Vec<u8>, String> {
    let cipher = XChaCha20Poly1305::new(&derive("orien wallet pair 2026 seal", key).into());
    let mut nonce = [0u8; 24];
    getrandom::fill(&mut nonce).map_err(|error| error.to_string())?;
    let mut out = nonce.to_vec();
    out.extend(cipher.encrypt(XNonce::from_slice(&nonce), plain).map_err(|_| "could not seal the wallet")?);
    Ok(out)
}

fn open(key: &[u8; 32], sealed: &[u8]) -> Result<Vec<u8>, String> {
    if sealed.len() < 24 {
        return Err("The other device sent something damaged.".into());
    }
    let cipher = XChaCha20Poly1305::new(&derive("orien wallet pair 2026 seal", key).into());
    cipher.decrypt(XNonce::from_slice(&sealed[..24]), &sealed[24..]).map_err(|_| "The other device sent something this one can't open.".into())
}

fn key_of(bytes: Vec<u8>) -> Result<[u8; 32], String> {
    bytes.try_into().map_err(|_| "The pairing failed. Start again.".to_string())
}

/// Two random words, from the recovery phrase's word list.
fn two_words() -> Result<String, String> {
    let mut seed = [0u8; 32];
    getrandom::fill(&mut seed).map_err(|error| error.to_string())?;
    let words = orien_net::phrase::to_words(&seed);
    Ok(format!("{}-{}", words[0], words[1]))
}

/// "7-orbit-lantern", typed any which way: its number and the whole code.
fn read_code(typed: &str) -> Result<(u32, String), String> {
    let parts: Vec<String> = typed
        .split(|c: char| c == '-' || c.is_whitespace())
        .filter(|part| !part.is_empty())
        .map(|part| part.to_lowercase())
        .collect();
    let bad = || "That isn't a pairing code. It looks like 7-orbit-lantern.".to_string();
    if parts.len() != 3 {
        return Err(bad());
    }
    let number: u32 = parts[0].parse().map_err(|_| bad())?;
    if !(1..=99_999).contains(&number) || !parts[1..].iter().all(|w| w.chars().all(|c| c.is_ascii_lowercase())) {
        return Err(bad());
    }
    Ok((number, parts.join("-")))
}

// ---- The sending device ----

/// Opens a mailbox and starts: the code to show, typed or scanned.
pub fn offer() -> Result<String, String> {
    cancel_sending();
    let number = open_mailbox()?;
    let code = format!("{number}-{}", two_words()?);
    let (spake, message) = Spake2::<Ed25519Group>::start_a(&Password::new(code.as_bytes()), &Identity::new(SENDER), &Identity::new(RECEIVER));
    put(number, "a", &message)?;
    *SENDING.lock().unwrap_or_else(|p| p.into_inner()) = Some(Pairing { number, spake: Some(spake), key: None, confirmed: false });
    Ok(code)
}

/// A new mailbox's number.
fn open_mailbox() -> Result<u32, String> {
    let answer: Value = agent()
        .post(&url(""))
        .send_json(json!({}))
        .map_err(|error| match error {
            ureq::Error::Status(429, _) => "Too many pairings just now. Wait a few minutes and try again.".to_string(),
            _ => OFFLINE.to_string(),
        })?
        .into_json()
        .map_err(|_| OFFLINE.to_string())?;
    Ok(answer["code"].as_u64().filter(|n| *n > 0 && *n < 100_000).ok_or(OFFLINE)? as u32)
}

/// How the other device stands: None while it hasn't answered yet; the
/// emoji to compare once it has, and has proven it knows the code.
pub fn offer_wait() -> Result<Option<String>, String> {
    let (number, have_key) = {
        let guard = SENDING.lock().unwrap_or_else(|p| p.into_inner());
        let pairing = guard.as_ref().ok_or(GONE)?;
        if pairing.confirmed {
            return Ok(pairing.key.as_ref().map(emoji));
        }
        (pairing.number, pairing.key.is_some())
    };
    if !have_key {
        let Some(theirs) = wait_for(number, "b", Duration::from_millis(2500))? else { return Ok(None) };
        let mut guard = SENDING.lock().unwrap_or_else(|p| p.into_inner());
        let pairing = guard.as_mut().ok_or(GONE)?;
        let spake = pairing.spake.take().ok_or(GONE)?;
        match spake.finish(&theirs) {
            Ok(key) => pairing.key = Some(key_of(key)?),
            Err(_) => {
                drop(guard);
                cancel_sending();
                return Err("The other device's code didn't match. Start again.".into());
            }
        }
    }
    let Some(proof) = wait_for(number, "c", Duration::from_millis(2500))? else { return Ok(None) };
    let mut guard = SENDING.lock().unwrap_or_else(|p| p.into_inner());
    let pairing = guard.as_mut().ok_or(GONE)?;
    let key = pairing.key.ok_or(GONE)?;
    if proof != confirmation(&key) {
        drop(guard);
        cancel_sending();
        return Err("The code typed on the other device didn't match. Start again.".into());
    }
    pairing.confirmed = true;
    Ok(Some(emoji(&key)))
}

/// Sends the wallet, sealed, once the person has said the emoji match.
pub fn offer_send(parcel: &Parcel) -> Result<(), String> {
    let (number, key) = {
        let guard = SENDING.lock().unwrap_or_else(|p| p.into_inner());
        let pairing = guard.as_ref().ok_or(GONE)?;
        if !pairing.confirmed {
            return Err("The other device hasn't joined yet.".into());
        }
        (pairing.number, pairing.key.ok_or(GONE)?)
    };
    let plain = serde_json::to_vec(parcel).map_err(|error| error.to_string())?;
    put(number, "d", &seal(&key, &plain)?)?;
    // The receiving device closes the mailbox once it has the wallet.
    *SENDING.lock().unwrap_or_else(|p| p.into_inner()) = None;
    Ok(())
}

/// "Link a new device": the code the new device shows, typed or scanned
/// here, on the device that has the wallets. Returns the emoji to compare,
/// once the new device has proven it made that code.
pub fn accept(typed: &str) -> Result<String, String> {
    cancel_sending();
    let (number, code) = read_code(typed)?;
    let (spake, message) = Spake2::<Ed25519Group>::start_a(&Password::new(code.as_bytes()), &Identity::new(SENDER), &Identity::new(RECEIVER));
    let theirs = wait_for(number, "b", Duration::from_secs(5))?.ok_or(GONE)?;
    put(number, "a", &message)?;
    let key = key_of(spake.finish(&theirs).map_err(|_| "That code didn't work. Check it and try again.".to_string())?)?;
    let Some(proof) = wait_for(number, "c", Duration::from_secs(10))? else {
        close(number);
        return Err("Your new device didn't answer. Check it's still showing the code, and try again.".into());
    };
    if proof != confirmation(&key) {
        close(number);
        return Err("That code didn't work. Check it and try again.".into());
    }
    *SENDING.lock().unwrap_or_else(|p| p.into_inner()) = Some(Pairing { number, spake: None, key: Some(key), confirmed: true });
    Ok(emoji(&key))
}

/// Sends every wallet, sealed, once the person has said the emoji match.
pub fn accept_send(bundle: &Bundle) -> Result<(), String> {
    let (number, key) = {
        let guard = SENDING.lock().unwrap_or_else(|p| p.into_inner());
        let pairing = guard.as_ref().ok_or(GONE)?;
        (pairing.number, pairing.key.ok_or(GONE)?)
    };
    let plain = serde_json::to_vec(bundle).map_err(|error| error.to_string())?;
    put(number, "d", &seal(&key, &plain)?)?;
    *SENDING.lock().unwrap_or_else(|p| p.into_inner()) = None;
    Ok(())
}

pub fn cancel_sending() {
    if let Some(pairing) = SENDING.lock().unwrap_or_else(|p| p.into_inner()).take() {
        close(pairing.number);
    }
}

// ---- The receiving device ----

/// "Use my other device": opens a mailbox and makes the code this new
/// device shows, for the device that has the wallets to type or scan.
pub fn invite() -> Result<String, String> {
    cancel_receiving();
    let number = open_mailbox()?;
    let code = format!("{number}-{}", two_words()?);
    let (spake, message) = Spake2::<Ed25519Group>::start_b(&Password::new(code.as_bytes()), &Identity::new(SENDER), &Identity::new(RECEIVER));
    put(number, "b", &message)?;
    *RECEIVING.lock().unwrap_or_else(|p| p.into_inner()) = Some(Pairing { number, spake: Some(spake), key: None, confirmed: false });
    Ok(code)
}

/// None while the other device hasn't typed the code yet; then the emoji
/// to compare. The wallets follow through `join_wait`.
pub fn invite_wait() -> Result<Option<String>, String> {
    let number = {
        let guard = RECEIVING.lock().unwrap_or_else(|p| p.into_inner());
        let pairing = guard.as_ref().ok_or(GONE)?;
        if let Some(key) = pairing.key.as_ref() {
            return Ok(Some(emoji(key)));
        }
        pairing.number
    };
    let Some(theirs) = wait_for(number, "a", Duration::from_millis(2500))? else { return Ok(None) };
    let mut guard = RECEIVING.lock().unwrap_or_else(|p| p.into_inner());
    let pairing = guard.as_mut().ok_or(GONE)?;
    let spake = pairing.spake.take().ok_or(GONE)?;
    let key = key_of(spake.finish(&theirs).map_err(|_| "The pairing failed. Start again.".to_string())?)?;
    put(number, "c", &confirmation(&key))?;
    pairing.key = Some(key);
    pairing.confirmed = true;
    Ok(Some(emoji(&key)))
}

/// Joins with the code typed or scanned; returns the emoji to compare.
pub fn join(typed: &str) -> Result<String, String> {
    cancel_receiving();
    let (number, code) = read_code(typed)?;
    let (spake, message) = Spake2::<Ed25519Group>::start_b(&Password::new(code.as_bytes()), &Identity::new(SENDER), &Identity::new(RECEIVER));
    let theirs = wait_for(number, "a", Duration::from_secs(5))?.ok_or(GONE)?;
    put(number, "b", &message)?;
    let key = key_of(spake.finish(&theirs).map_err(|_| "That code didn't work. Check it and try again.".to_string())?)?;
    put(number, "c", &confirmation(&key))?;
    *RECEIVING.lock().unwrap_or_else(|p| p.into_inner()) = Some(Pairing { number, spake: None, key: Some(key), confirmed: true });
    Ok(emoji(&key))
}

/// The wallets, once the other device has sent them (one, from an older
/// app or "Add to another device"); None until then.
pub fn join_wait() -> Result<Option<Vec<Parcel>>, String> {
    let (number, key) = {
        let guard = RECEIVING.lock().unwrap_or_else(|p| p.into_inner());
        let pairing = guard.as_ref().ok_or(GONE)?;
        (pairing.number, pairing.key.ok_or(GONE)?)
    };
    let sealed = match wait_for(number, "d", Duration::from_millis(2500)) {
        Ok(Some(sealed)) => sealed,
        Ok(None) => return Ok(None),
        Err(error) => {
            *RECEIVING.lock().unwrap_or_else(|p| p.into_inner()) = None;
            return Err(error);
        }
    };
    cancel_receiving();
    let plain = open(&key, &sealed)?;
    let unreadable = || "The other device sent something this one can't read.".to_string();
    if let Ok(bundle) = serde_json::from_slice::<Bundle>(&plain) {
        return if bundle.wallets.is_empty() { Err(unreadable()) } else { Ok(Some(bundle.wallets)) };
    }
    serde_json::from_slice::<Parcel>(&plain).map(|parcel| Some(vec![parcel])).map_err(|_| unreadable())
}

pub fn cancel_receiving() {
    if let Some(pairing) = RECEIVING.lock().unwrap_or_else(|p| p.into_inner()).take() {
        close(pairing.number);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The live tests share this device's one pairing at a time.
    static LIVE: Mutex<()> = Mutex::new(());

    #[test]
    fn codes_are_read_however_they_are_typed() {
        assert_eq!(read_code("7-orbit-lantern").unwrap(), (7, "7-orbit-lantern".into()));
        assert_eq!(read_code(" 7 Orbit  LANTERN ").unwrap(), (7, "7-orbit-lantern".into()));
        assert!(read_code("orbit-lantern").is_err());
        assert!(read_code("7-orbit").is_err());
        assert!(read_code("0-orbit-lantern").is_err());
    }

    /// Both sides agree on a key, and on the emoji, only with the same code;
    /// the confirmation tells them apart.
    #[test]
    fn the_same_code_gives_the_same_key_and_emoji() {
        let run = |a: &str, b: &str| {
            let (sa, ma) = Spake2::<Ed25519Group>::start_a(&Password::new(a.as_bytes()), &Identity::new(SENDER), &Identity::new(RECEIVER));
            let (sb, mb) = Spake2::<Ed25519Group>::start_b(&Password::new(b.as_bytes()), &Identity::new(SENDER), &Identity::new(RECEIVER));
            (key_of(sa.finish(&mb).unwrap()).unwrap(), key_of(sb.finish(&ma).unwrap()).unwrap())
        };
        let (ka, kb) = run("7-orbit-lantern", "7-orbit-lantern");
        assert_eq!(ka, kb);
        assert_eq!(emoji(&ka), emoji(&kb));
        assert_eq!(confirmation(&ka), confirmation(&kb));
        let parcel = seal(&ka, b"hello").unwrap();
        assert_eq!(open(&kb, &parcel).unwrap(), b"hello");

        let (ka, kb) = run("7-orbit-lantern", "7-orbit-lanterns");
        assert_ne!(ka, kb);
        assert_ne!(confirmation(&ka), confirmation(&kb));
        assert!(open(&kb, &seal(&ka, b"hello").unwrap()).is_err());
    }

    /// A whole pairing against the live mailbox: one side offers, the other
    /// joins with the code, both see the same emoji, and the parcel arrives
    /// intact. A wrong code fails and closes the mailbox.
    #[test]
    #[ignore = "reaches orien.network"]
    fn a_wallet_crosses_through_the_live_mailbox() {
        let _one = LIVE.lock().unwrap_or_else(|p| p.into_inner());
        let code = offer().unwrap();
        let joiner = std::thread::spawn({
            let code = code.clone();
            move || join(&code).unwrap()
        });
        let mut seen = None;
        for _ in 0..10 {
            if let Some(emoji) = offer_wait().unwrap() {
                seen = Some(emoji);
                break;
            }
        }
        let theirs = joiner.join().unwrap();
        assert_eq!(seen.as_deref(), Some(theirs.as_str()));
        offer_send(&Parcel { words: "test words".into(), account: 1, name: "Savings".into(), icon: Some("💎".into()) }).unwrap();
        let mut got = None;
        for _ in 0..10 {
            if let Some(parcel) = join_wait().unwrap() {
                got = Some(parcel);
                break;
            }
        }
        let got = got.unwrap().remove(0);
        assert_eq!((got.words.as_str(), got.name.as_str(), got.icon.as_deref()), ("test words", "Savings", Some("💎")));

        // A wrong code: the joiner fails, or the offer sees the mismatch.
        let code = offer().unwrap();
        let number = code.split('-').next().unwrap().to_string();
        let wrong = std::thread::spawn(move || join(&format!("{number}-wrong-words")));
        let mut failed = false;
        for _ in 0..6 {
            match offer_wait() {
                Err(_) => { failed = true; break; }
                Ok(Some(_)) => panic!("a wrong code matched"),
                Ok(None) => {}
            }
        }
        let _ = wrong.join();
        assert!(failed, "the offer noticed the wrong code");
    }

    /// "Link a new device": the new device makes the code, the old one types
    /// it, both see the same emoji, and every wallet arrives together.
    #[test]
    #[ignore = "reaches orien.network"]
    fn the_new_device_makes_the_code_and_all_wallets_arrive() {
        let _one = LIVE.lock().unwrap_or_else(|p| p.into_inner());
        let code = invite().unwrap();
        let sender = std::thread::spawn({
            let code = code.clone();
            move || accept(&code).unwrap()
        });
        let mut seen = None;
        for _ in 0..10 {
            if let Some(emoji) = invite_wait().unwrap() {
                seen = Some(emoji);
                break;
            }
        }
        let theirs = sender.join().unwrap();
        assert_eq!(seen.as_deref(), Some(theirs.as_str()));
        let wallets = vec![
            Parcel { words: "one".into(), account: 1, name: "Main".into(), icon: None },
            Parcel { words: "two".into(), account: 2, name: "Savings".into(), icon: Some("💎".into()) },
        ];
        accept_send(&Bundle { wallets }).unwrap();
        let mut got = None;
        for _ in 0..10 {
            if let Some(parcels) = join_wait().unwrap() {
                got = Some(parcels);
                break;
            }
        }
        let names: Vec<String> = got.unwrap().into_iter().map(|p| p.name).collect();
        assert_eq!(names, ["Main", "Savings"]);

        // A mistyped code on the old device fails there, and the new
        // device's mailbox closes, so it makes a fresh code.
        let code = invite().unwrap();
        let number = code.split('-').next().unwrap().to_string();
        let wrong = std::thread::spawn(move || accept(&format!("{number}-wrong-words")));
        let mut gone = false;
        for _ in 0..6 {
            match invite_wait().and_then(|_| join_wait()) {
                Err(_) => { gone = true; break; }
                Ok(_) => {}
            }
        }
        assert!(wrong.join().unwrap().is_err(), "a wrong code failed on the old device");
        assert!(gone, "the new device noticed the closed mailbox");
    }
}
