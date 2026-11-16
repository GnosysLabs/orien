//! Notifications on iPhone when a wallet receives ORN.
//!
//! Orien Wallet sends its Apple push token with the addresses of the wallets
//! in the app (POST /api/market/push/register). Nothing is signed: what an
//! address receives is public on the network anyway. A watcher asks the
//! network every few seconds what each registered address has received,
//! and for each new payment, or locked payment claimed for it, tells
//! Apple's push service to show "You received 25 ORN" on every phone that
//! registered the address. Mining payouts, fees, refunds and unstaking are
//! left out: mining pays every hour.
//!
//! Apple's push service is reached with curl, which speaks HTTP/2 as Apple
//! requires, signed with a short-lived token made from the founder's Apple
//! key (a .p8 file). The token goes to curl on its standard input, never on
//! its command line, where other programs on the server could read it.

use crate::api::{App, Failure};
use crate::store::{Store, now, orien_address, orn};
use anyhow::{Context, Result, bail};
use axum::extract::State;
use axum::http::HeaderMap;
use axum::response::{IntoResponse, Response};
use axum::Json;
use rusqlite::params;
use serde::Deserialize;
use serde_json::json;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Wallets one phone may ask about.
pub const MAX_ADDRESSES: usize = 50;
/// Registrations one phone, or one internet address, may make in an hour.
const REGISTRATIONS_PER_HOUR: usize = 30;
/// How often the watcher asks the network.
const EVERY: Duration = Duration::from_secs(4);
/// The app's bundle id, which Apple calls the topic.
pub const TOPIC: &str = "network.orien.wallet";

type Answer = Result<Response, Failure>;

/// A device token tidied, or why it is not one.
fn device_token(text: &str) -> Result<String> {
    let token = text.trim().to_lowercase();
    if !(64..=400).contains(&token.len()) || !token.len().is_multiple_of(2) || !token.bytes().all(|b| b.is_ascii_hexdigit()) {
        bail!("a push token is 32 to 200 bytes in hexadecimal");
    }
    Ok(token)
}

impl Store {
    pub(crate) fn create_push_tables(&self) -> Result<()> {
        self.db.execute_batch(
            "CREATE TABLE IF NOT EXISTS push_tokens (
                 token TEXT PRIMARY KEY,
                 sandbox INTEGER NOT NULL,
                 updated INTEGER NOT NULL
             );
             CREATE TABLE IF NOT EXISTS push_addresses (
                 token TEXT NOT NULL REFERENCES push_tokens(token) ON DELETE CASCADE,
                 address TEXT NOT NULL,
                 PRIMARY KEY (token, address)
             );
             CREATE INDEX IF NOT EXISTS push_addresses_by_address ON push_addresses(address);
             -- Addresses being watched. Until `primed`, what an address
             -- already had is noted without notifying.
             CREATE TABLE IF NOT EXISTS push_watch (
                 address TEXT PRIMARY KEY,
                 primed INTEGER NOT NULL DEFAULT 0
             );
             -- Payments already told about, by address and a key naming each.
             CREATE TABLE IF NOT EXISTS push_seen (
                 address TEXT NOT NULL,
                 key TEXT NOT NULL,
                 epoch INTEGER NOT NULL,
                 PRIMARY KEY (address, key)
             );",
        )?;
        Ok(())
    }

    /// The whole set of addresses `token` asks about, replacing any before.
    /// No addresses is the same as `unregister_push`.
    pub fn register_push(&self, token: &str, sandbox: bool, addresses: &[String]) -> Result<usize> {
        let token = device_token(token)?;
        let mut tidy = Vec::new();
        for address in addresses {
            let address = orien_address(address)?;
            if !tidy.contains(&address) {
                tidy.push(address);
            }
        }
        if tidy.len() > MAX_ADDRESSES {
            bail!("at most {MAX_ADDRESSES} wallets per phone");
        }
        if tidy.is_empty() {
            self.unregister_push(&token)?;
            return Ok(0);
        }
        let tx = self.db.unchecked_transaction()?;
        tx.execute(
            "INSERT INTO push_tokens (token, sandbox, updated) VALUES (?1, ?2, ?3)
             ON CONFLICT(token) DO UPDATE SET sandbox = ?2, updated = ?3",
            params![token, sandbox, now() as i64],
        )?;
        tx.execute("DELETE FROM push_addresses WHERE token = ?1", [&token])?;
        for address in &tidy {
            tx.execute("INSERT INTO push_addresses (token, address) VALUES (?1, ?2)", [&token, address])?;
            tx.execute("INSERT OR IGNORE INTO push_watch (address) VALUES (?1)", [address])?;
        }
        tx.commit()?;
        self.forget_unwatched()?;
        Ok(tidy.len())
    }

    pub fn unregister_push(&self, token: &str) -> Result<()> {
        let token = token.trim().to_lowercase();
        self.db.execute("DELETE FROM push_addresses WHERE token = ?1", [&token])?;
        self.db.execute("DELETE FROM push_tokens WHERE token = ?1", [&token])?;
        self.forget_unwatched()
    }

    /// Addresses no phone asks about any more are no longer watched.
    fn forget_unwatched(&self) -> Result<()> {
        self.db.execute_batch(
            "DELETE FROM push_watch WHERE address NOT IN (SELECT address FROM push_addresses);
             DELETE FROM push_seen WHERE address NOT IN (SELECT address FROM push_addresses);",
        )?;
        Ok(())
    }

    /// Every watched address, and whether it is primed.
    fn watched(&self) -> Result<Vec<(String, bool)>> {
        let mut statement = self.db.prepare("SELECT address, primed FROM push_watch ORDER BY address")?;
        let rows = statement.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    fn prime(&self, address: &str) -> Result<()> {
        self.db.execute("UPDATE push_watch SET primed = 1 WHERE address = ?1", [address])?;
        Ok(())
    }

    /// Notes a payment as told about; true if it was new.
    fn first_sight(&self, address: &str, key: &str, epoch: u64) -> Result<bool> {
        Ok(self.db.execute(
            "INSERT OR IGNORE INTO push_seen (address, key, epoch) VALUES (?1, ?2, ?3)",
            params![address, key, epoch as i64],
        )? == 1)
    }

    fn forget_seen_before(&self, epoch: u64) -> Result<()> {
        self.db.execute("DELETE FROM push_seen WHERE epoch < ?1", [epoch as i64])?;
        Ok(())
    }

    /// The phones that ask about `address`: their tokens, and whether each
    /// is a development build's.
    fn phones_for(&self, address: &str) -> Result<Vec<(String, bool)>> {
        let mut statement = self.db.prepare(
            "SELECT t.token, t.sandbox FROM push_addresses a JOIN push_tokens t ON t.token = a.token WHERE a.address = ?1",
        )?;
        let rows = statement.query_map([address], |row| Ok((row.get(0)?, row.get(1)?)))?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// How many phones and addresses are registered, for the dashboard.
    pub fn push_counts(&self) -> Result<(i64, i64)> {
        Ok((
            self.db.query_row("SELECT COUNT(*) FROM push_tokens", [], |row| row.get(0))?,
            self.db.query_row("SELECT COUNT(*) FROM push_watch", [], |row| row.get(0))?,
        ))
    }
}

/// "You received 1,234.5 ORN".
pub fn received_text(amount: u64) -> String {
    let written = orn(amount);
    let (whole, fraction) = written.split_once('.').map_or((written.as_str(), None), |(w, f)| (w, Some(f)));
    let mut grouped = String::new();
    for (i, digit) in whole.chars().enumerate() {
        if i > 0 && (whole.len() - i) % 3 == 0 {
            grouped.push(',');
        }
        grouped.push(digit);
    }
    match fraction {
        Some(fraction) => format!("You received {grouped}.{fraction} ORN"),
        None => format!("You received {grouped} ORN"),
    }
}

/// What Apple shows on the phone.
pub fn payload(amount: u64, address: &str) -> String {
    json!({
        "aps": { "alert": { "title": "Orien Wallet", "body": received_text(amount) }, "sound": "default" },
        "address": address,
    })
    .to_string()
}

/// What happened to one notification.
#[derive(Debug, PartialEq)]
pub enum Sent {
    Delivered,
    /// The phone's token no longer works: forget it.
    Gone,
    Failed(String),
}

/// Something that delivers notifications: Apple, or a test's notebook.
pub trait Notify: Send + Sync {
    fn notify(&self, token: &str, sandbox: bool, payload: &str) -> Sent;
}

/// The founder's Apple key, for signing requests to Apple's push service.
struct Key {
    key: openssl::pkey::PKey<openssl::pkey::Private>,
    id: String,
    team: String,
}

/// Apple's push service.
pub struct Apns {
    key: Option<Key>,
    /// The signed token, and when it was made: Apple takes one for an hour.
    signed: Mutex<Option<(String, u64)>>,
}

fn base64url(bytes: &[u8]) -> String {
    use base64::Engine;
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

impl Apns {
    /// With the key in `p8`, its id as Apple shows it, and the team's id.
    pub fn load(p8: &Path, id: &str, team: &str) -> Result<Apns> {
        let pem = std::fs::read(p8).with_context(|| format!("cannot read the Apple key {}", p8.display()))?;
        let key = openssl::pkey::PKey::private_key_from_pem(&pem).context("the Apple key is not a .p8 key")?;
        Ok(Apns { key: Some(Key { key, id: id.trim().into(), team: team.trim().into() }), signed: Mutex::new(None) })
    }

    /// No key yet: notifications are written to the log instead.
    pub fn off() -> Apns {
        Apns { key: None, signed: Mutex::new(None) }
    }

    /// The token that proves requests come from the founder's team (ES256),
    /// made again after 40 minutes.
    fn token(&self, key: &Key) -> Result<String> {
        let mut signed = self.signed.lock().unwrap();
        if let Some((token, made)) = &*signed
            && now().saturating_sub(*made) < 40 * 60
        {
            return Ok(token.clone());
        }
        let made = now();
        let head = base64url(json!({ "alg": "ES256", "kid": key.id }).to_string().as_bytes());
        let claims = base64url(json!({ "iss": key.team, "iat": made }).to_string().as_bytes());
        let message = format!("{head}.{claims}");
        let mut signer = openssl::sign::Signer::new(openssl::hash::MessageDigest::sha256(), &key.key)?;
        signer.update(message.as_bytes())?;
        // OpenSSL signs in DER; the token wants r and s, 32 bytes each.
        let der = signer.sign_to_vec()?;
        let signature = openssl::ecdsa::EcdsaSig::from_der(&der)?;
        let mut raw = signature.r().to_vec_padded(32)?;
        raw.extend(signature.s().to_vec_padded(32)?);
        let token = format!("{message}.{}", base64url(&raw));
        *signed = Some((token.clone(), made));
        Ok(token)
    }
}

/// A value for curl's configuration, quoted.
fn quoted(text: &str) -> String {
    format!("\"{}\"", text.replace('\\', "\\\\").replace('"', "\\\""))
}

impl Notify for Apns {
    fn notify(&self, phone: &str, sandbox: bool, payload: &str) -> Sent {
        let Some(key) = &self.key else {
            println!("would notify {}…: {payload}", &phone[..phone.len().min(8)]);
            return Sent::Delivered;
        };
        let token = match self.token(key) {
            Ok(token) => token,
            Err(error) => return Sent::Failed(format!("cannot sign for Apple: {error:#}")),
        };
        let host = if sandbox { "api.sandbox.push.apple.com" } else { "api.push.apple.com" };
        let config = [
            format!("url = {}", quoted(&format!("https://{host}/3/device/{phone}"))),
            "http2".into(),
            "silent".into(),
            "max-time = 15".into(),
            format!("header = {}", quoted(&format!("authorization: bearer {token}"))),
            format!("header = {}", quoted(&format!("apns-topic: {TOPIC}"))),
            format!("header = {}", quoted("apns-push-type: alert")),
            format!("header = {}", quoted("apns-priority: 10")),
            format!("data-binary = {}", quoted(payload)),
            format!("write-out = {}", quoted("\\n%{http_code}")),
        ]
        .join("\n");
        let child = Command::new("curl").args(["--config", "-"]).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn();
        let mut child = match child {
            Ok(child) => child,
            Err(error) => return Sent::Failed(format!("cannot run curl: {error}")),
        };
        if let Some(mut stdin) = child.stdin.take() {
            let _ = stdin.write_all(config.as_bytes());
        }
        let output = match child.wait_with_output() {
            Ok(output) => String::from_utf8_lossy(&output.stdout).to_string(),
            Err(error) => return Sent::Failed(format!("curl failed: {error}")),
        };
        let (body, status) = output.rsplit_once('\n').unwrap_or(("", output.as_str()));
        match status.trim() {
            "200" => Sent::Delivered,
            "410" => Sent::Gone,
            "400" if body.contains("BadDeviceToken") => Sent::Gone,
            other => Sent::Failed(format!("Apple answered {other}: {body}")),
        }
    }
}

/// One look at the network: notifies each phone of what its addresses
/// received since the last look.
pub fn check_once(app: &App, notify: &dyn Notify) -> Result<usize> {
    let payments = app.payments();
    let epoch = payments.epoch()?;
    let from = epoch.saturating_sub(1);
    let watched = app.store().watched()?;
    let mut sent = 0;
    for (address, primed) in watched {
        let Ok(bytes) = hex::decode(&address) else { continue };
        let Ok(id) = <[u8; 32]>::try_from(bytes) else { continue };
        let credits = match payments.credits(&orien_crypto::Hash32(id), from) {
            Ok(credits) => credits,
            Err(error) => {
                eprintln!("push: cannot read what {}… received: {error:#}", &address[..8]);
                continue;
            }
        };
        for credit in credits {
            if !app.store().first_sight(&address, &credit.key, credit.epoch)? || !primed {
                continue;
            }
            let payload = payload(credit.amount, &address);
            // Read first, so the store is not held while forgetting a phone.
            let phones = app.store().phones_for(&address)?;
            for (phone, sandbox) in phones {
                match notify.notify(&phone, sandbox, &payload) {
                    Sent::Delivered => sent += 1,
                    Sent::Gone => app.store().unregister_push(&phone)?,
                    Sent::Failed(why) => eprintln!("push: {why}"),
                }
            }
        }
        if !primed {
            app.store().prime(&address)?;
        }
    }
    app.store().forget_seen_before(epoch.saturating_sub(3))?;
    Ok(sent)
}

/// Watches the network for as long as the server runs.
pub fn watch(app: Arc<App>, notify: Apns) {
    std::thread::spawn(move || {
        loop {
            if let Err(error) = check_once(&app, &notify) {
                eprintln!("push: {error:#}");
            }
            std::thread::sleep(EVERY);
        }
    });
}

/// Recent registrations, by phone and by internet address, to keep anyone
/// from churning the table.
static RECENT: Mutex<Vec<(Instant, String)>> = Mutex::new(Vec::new());

fn allowed(keys: &[String]) -> bool {
    let mut recent = RECENT.lock().unwrap();
    recent.retain(|(at, _)| at.elapsed() < Duration::from_secs(3600));
    if keys.iter().any(|key| recent.iter().filter(|(_, k)| k == key).count() >= REGISTRATIONS_PER_HOUR) {
        return false;
    }
    recent.extend(keys.iter().map(|key| (Instant::now(), key.clone())));
    true
}

fn sender(headers: &HeaderMap) -> String {
    let header = |name: &str| headers.get(name).and_then(|v| v.to_str().ok()).unwrap_or("").trim().to_string();
    let forwarded = header("x-forwarded-for");
    [header("cf-connecting-ip"), forwarded.split(',').next().unwrap_or("").trim().to_string()]
        .into_iter()
        .find(|a| !a.is_empty())
        .unwrap_or_else(|| "unknown".into())
}

#[derive(Deserialize)]
pub struct Register {
    token: String,
    #[serde(default)]
    sandbox: bool,
    #[serde(default)]
    addresses: Vec<String>,
}

pub async fn register(State(app): State<Arc<App>>, headers: HeaderMap, Json(body): Json<Register>) -> Answer {
    let token = device_token(&body.token).map_err(|e| Failure::Refused(e.to_string()))?;
    if !allowed(&[format!("token {token}"), format!("from {}", sender(&headers))]) {
        return Err(Failure::Refused("too many registrations for now; try again in a few minutes".into()));
    }
    let kept = app.store().register_push(&token, body.sandbox, &body.addresses).map_err(|e| Failure::Refused(e.to_string()))?;
    Ok(Json(json!({ "addresses": kept })).into_response())
}

#[derive(Deserialize)]
pub struct Unregister {
    token: String,
}

pub async fn unregister(State(app): State<Arc<App>>, Json(body): Json<Unregister>) -> Answer {
    app.store().unregister_push(&body.token)?;
    Ok(Json(json!({ "ok": true })).into_response())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn amounts_read_as_people_write_them() {
        assert_eq!(received_text(25_000_000_000), "You received 25 ORN");
        assert_eq!(received_text(1_234_500_000_000), "You received 1,234.5 ORN");
        assert_eq!(received_text(1_000_000), "You received 0.001 ORN");
        assert_eq!(received_text(12_345_678_000_000_000), "You received 12,345,678 ORN");
    }

    #[test]
    fn the_token_for_apple_is_signed_with_the_key() {
        let ec = openssl::ec::EcKey::generate(&openssl::ec::EcGroup::from_curve_name(openssl::nid::Nid::X9_62_PRIME256V1).unwrap()).unwrap();
        let public = openssl::pkey::PKey::from_ec_key(openssl::ec::EcKey::from_public_key(ec.group(), ec.public_key()).unwrap()).unwrap();
        let key = Key { key: openssl::pkey::PKey::from_ec_key(ec).unwrap(), id: "ABC123DEFG".into(), team: "4PDUNTF69S".into() };
        let apns = Apns { key: None, signed: Mutex::new(None) };
        let token = apns.token(&key).unwrap();
        assert_eq!(apns.token(&key).unwrap(), token, "made once, then reused");
        let parts: Vec<&str> = token.split('.').collect();
        assert_eq!(parts.len(), 3);
        use base64::Engine;
        let decode = |part: &str| base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(part).unwrap();
        let head: serde_json::Value = serde_json::from_slice(&decode(parts[0])).unwrap();
        assert_eq!(head["kid"], "ABC123DEFG");
        let claims: serde_json::Value = serde_json::from_slice(&decode(parts[1])).unwrap();
        assert_eq!(claims["iss"], "4PDUNTF69S");
        let raw = decode(parts[2]);
        assert_eq!(raw.len(), 64);
        let signature = openssl::ecdsa::EcdsaSig::from_private_components(
            openssl::bn::BigNum::from_slice(&raw[..32]).unwrap(),
            openssl::bn::BigNum::from_slice(&raw[32..]).unwrap(),
        )
        .unwrap();
        let digest = openssl::sha::sha256(format!("{}.{}", parts[0], parts[1]).as_bytes());
        assert!(signature.verify(&digest, &public.ec_key().unwrap()).unwrap());
    }

    /// Asks Apple's sandbox with a made-up key and phone, to check the
    /// request reaches Apple and is read: `cargo test -p orien-market
    /// apple_answers -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn apple_answers() {
        let ec = openssl::ec::EcKey::generate(&openssl::ec::EcGroup::from_curve_name(openssl::nid::Nid::X9_62_PRIME256V1).unwrap()).unwrap();
        let apns = Apns {
            key: Some(Key { key: openssl::pkey::PKey::from_ec_key(ec).unwrap(), id: "ABC123DEFG".into(), team: "4PDUNTF69S".into() }),
            signed: Mutex::new(None),
        };
        let answer = apns.notify(&"ab".repeat(32), true, &payload(1, &"cd".repeat(32)));
        println!("{answer:?}");
        assert!(matches!(&answer, Sent::Gone) || matches!(&answer, Sent::Failed(why) if why.contains("Apple answered 403")), "{answer:?}");
    }

    #[test]
    fn curl_configuration_quotes_safely() {
        assert_eq!(quoted(r#"{"a":"b\c"}"#), r#""{\"a\":\"b\\c\"}""#);
    }
}
