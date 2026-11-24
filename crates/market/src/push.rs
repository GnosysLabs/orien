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

/// The kinds of notification a phone can turn off.
pub const MUTABLE: [&str; 5] = ["received", "mining", "market", "swaps", "community"];

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
        // The Market account a phone's app is signed in to, for messages
        // and swaps, which belong to accounts rather than addresses.
        let has: bool =
            self.db.query_row("SELECT COUNT(*) > 0 FROM pragma_table_info('push_tokens') WHERE name = 'account'", [], |row| row.get(0))?;
        if !has {
            self.db.execute("ALTER TABLE push_tokens ADD COLUMN account INTEGER REFERENCES accounts(id) ON DELETE SET NULL", [])?;
        }
        // The kinds of notification the phone's owner turned off, comma
        // separated: received, mining, market, swaps.
        let has_mute: bool =
            self.db.query_row("SELECT COUNT(*) > 0 FROM pragma_table_info('push_tokens') WHERE name = 'mute'", [], |row| row.get(0))?;
        if !has_mute {
            self.db.execute("ALTER TABLE push_tokens ADD COLUMN mute TEXT NOT NULL DEFAULT ''", [])?;
        }
        Ok(())
    }

    /// The whole set of addresses `token` asks about, replacing any before.
    /// No addresses is the same as `unregister_push`.
    /// `account` is the Market account the app is signed in to, if any:
    /// registering again signed out stops its messages reaching the phone.
    /// `mute` lists the kinds of notification turned off: "received",
    /// "mining", "market" or "swaps"; anything else is ignored.
    pub fn register_push(&self, token: &str, sandbox: bool, addresses: &[String], account: Option<i64>, mute: &[String]) -> Result<usize> {
        let token = device_token(token)?;
        let mut muted: Vec<&str> = MUTABLE.iter().copied().filter(|kind| mute.iter().any(|m| m.trim() == *kind)).collect();
        muted.dedup();
        let muted = muted.join(",");
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
        if tidy.is_empty() && account.is_none() {
            self.unregister_push(&token)?;
            return Ok(0);
        }
        let tx = self.db.unchecked_transaction()?;
        tx.execute(
            "INSERT INTO push_tokens (token, sandbox, updated, account, mute) VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(token) DO UPDATE SET sandbox = ?2, updated = ?3, account = ?4, mute = ?5",
            params![token, sandbox, now() as i64, account, muted],
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
    /// The phones that ask about `address` and haven't muted `kind`.
    fn phones_for(&self, address: &str, kind: &str) -> Result<Vec<(String, bool)>> {
        let mut statement = self.db.prepare(
            "SELECT t.token, t.sandbox FROM push_addresses a JOIN push_tokens t ON t.token = a.token
             WHERE a.address = ?1 AND instr(',' || t.mute || ',', ',' || ?2 || ',') = 0",
        )?;
        let rows = statement.query_map(params![address, kind], |row| Ok((row.get(0)?, row.get(1)?)))?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// The phones signed in to `account`.
    fn phones_for_account(&self, account: i64, kind: &str) -> Result<Vec<(String, bool)>> {
        let mut statement = self.db.prepare(
            "SELECT token, sandbox FROM push_tokens WHERE account = ?1 AND instr(',' || mute || ',', ',' || ?2 || ',') = 0",
        )?;
        let rows = statement.query_map(params![account, kind], |row| Ok((row.get(0)?, row.get(1)?)))?;
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
    format!("You received {} ORN", grouped(amount))
}

/// "You mined 12.35 ORN": rewards rounded to hundredths, since a share of an
/// epoch's coins rarely comes out round.
pub fn mined_text(amount: u64) -> String {
    let hundredths = (amount + 5_000_000) / 10_000_000 * 10_000_000;
    format!("You mined {} ORN", grouped(if hundredths == 0 { amount } else { hundredths }))
}

/// ORN written as people write them: "1,234.5".
pub(crate) fn grouped(amount: u64) -> String {
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
        Some(fraction) => format!("{grouped}.{fraction}"),
        None => grouped,
    }
}

/// What Apple shows on the phone.
pub fn payload(body: &str, address: &str) -> String {
    json!({
        "aps": { "alert": { "title": "Orien Wallet", "body": body }, "sound": "default" },
        "address": address,
    })
    .to_string()
}

/// A Market event for phones: a message, or a step of a swap.
#[derive(Clone, Debug, PartialEq)]
pub struct Event {
    /// Phones signed in to this account.
    pub account: i64,
    /// And phones that registered this wallet, if one is named.
    pub address: Option<String>,
    pub body: String,
    /// "message" or "swap", and what the app opens: {"conversation": 7}
    /// or {"swap": 3}.
    pub kind: &'static str,
    pub open: serde_json::Value,
}

impl Event {
    pub fn payload(&self) -> String {
        let mut payload = json!({
            "aps": { "alert": { "title": "Orien Wallet", "body": self.body }, "sound": "default" },
            "kind": self.kind,
        });
        if let Some(address) = &self.address {
            payload["address"] = json!(address);
        }
        if let Some(open) = self.open.as_object() {
            for (key, value) in open {
                payload[key] = value.clone();
            }
        }
        payload.to_string()
    }
}

/// A message's text for a notification, cut to fit: "Maya: Hi, is the …".
pub fn message_text(sender: &str, kind: &str, body: &str, amount: Option<u64>) -> String {
    match (kind, amount) {
        ("request", Some(amount)) => format!("{sender} asks you to pay {} ORN", grouped(amount)),
        ("paid", Some(amount)) => format!("{sender} says they paid {} ORN", grouped(amount)),
        _ => {
            let text: String = body.split_whitespace().collect::<Vec<_>>().join(" ");
            let cut: String = text.chars().take(120).collect();
            let more = if cut.chars().count() < text.chars().count() { "…" } else { "" };
            format!("{sender}: {cut}{more}")
        }
    }
}

/// Sends what the Market has queued for phones.
fn deliver_events(app: &App, notify: &dyn Notify) -> Result<usize> {
    // The bell's new replies, answers, reactions and network posts, said as
    // the bell says them; none older than ten minutes, so a first look or a
    // filled-in history never floods a phone.
    use std::sync::atomic::Ordering;
    let since = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs()).saturating_sub(600);
    let (last, fresh) = app.store().fresh_for_phones(app.pushed_notice.load(Ordering::Relaxed), since)?;
    app.pushed_notice.store(last, Ordering::Relaxed);
    for (account, body, post) in fresh {
        app.tell(Event { account, address: None, body, kind: "community", open: json!({ "post": post }) });
    }
    let mut sent = 0;
    for event in app.take_events() {
        // Messages are "market", tips and the bell's community lines
        // "community", swap steps "swaps", as the phone mutes them.
        let kind = match event.kind {
            "message" => "market",
            "tip" | "community" => "community",
            _ => "swaps",
        };
        let mut phones = app.store().phones_for_account(event.account, kind)?;
        if let Some(address) = &event.address {
            for phone in app.store().phones_for(address, kind)? {
                if !phones.contains(&phone) {
                    phones.push(phone);
                }
            }
        }
        let payload = event.payload();
        for (phone, sandbox) in phones {
            match notify.notify(&phone, sandbox, &payload) {
                Sent::Delivered => sent += 1,
                Sent::Gone => app.store().unregister_push(&phone)?,
                Sent::Failed(why) => eprintln!("push: {why}"),
            }
        }
    }
    Ok(sent)
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
    // Market events first: they need no network.
    let told = deliver_events(app, notify)?;
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
        // Each payment on its own; mining rewards seen together, one
        // notification for all of them.
        let (mut bodies, mut mined) = (Vec::new(), 0u64);
        for credit in credits {
            if !app.store().first_sight(&address, &credit.key, credit.epoch)? || !primed {
                continue;
            }
            if credit.mined {
                mined = mined.saturating_add(credit.amount);
            } else {
                bodies.push(("received", received_text(credit.amount)));
            }
        }
        if mined > 0 {
            bodies.push(("mining", mined_text(mined)));
        }
        for (kind, body) in bodies {
            let payload = payload(&body, &address);
            // Read first, so the store is not held while forgetting a phone.
            let phones = app.store().phones_for(&address, kind)?;
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
    Ok(told + sent)
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

pub(crate) fn sender(headers: &HeaderMap) -> String {
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
    /// Kinds of notification turned off: "received", "mining", "market", "swaps".
    #[serde(default)]
    mute: Vec<String>,
}

pub async fn register(State(app): State<Arc<App>>, headers: HeaderMap, Json(body): Json<Register>) -> Answer {
    let token = device_token(&body.token).map_err(|e| Failure::Refused(e.to_string()))?;
    if !allowed(&[format!("token {token}"), format!("from {}", sender(&headers))]) {
        return Err(Failure::Refused("too many registrations for now; try again in a few minutes".into()));
    }
    // Signed in, the phone also hears about that account's messages and swaps.
    let account = app.signed_in(&headers).ok();
    let kept =
        app.store().register_push(&token, body.sandbox, &body.addresses, account, &body.mute).map_err(|e| Failure::Refused(e.to_string()))?;
    Ok(Json(json!({ "addresses": kept, "account": account.is_some() })).into_response())
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
    #[test]
    fn a_phone_that_mutes_the_community_hears_no_tips() {
        let store = crate::store::Store::in_memory().unwrap();
        let maya = store.create_account("Maya", "maya").unwrap().id;
        store.register_push(&"ab".repeat(32), false, &[], Some(maya), &["community".to_string()]).unwrap();
        assert!(store.phones_for_account(maya, "community").unwrap().is_empty());
        assert_eq!(store.phones_for_account(maya, "market").unwrap().len(), 1, "messages still come");
    }

    use super::*;

    #[test]
    fn amounts_read_as_people_write_them() {
        assert_eq!(received_text(25_000_000_000), "You received 25 ORN");
        assert_eq!(received_text(1_234_500_000_000), "You received 1,234.5 ORN");
        assert_eq!(received_text(1_000_000), "You received 0.001 ORN");
        assert_eq!(received_text(12_345_678_000_000_000), "You received 12,345,678 ORN");
        assert_eq!(mined_text(12_000_000_000), "You mined 12 ORN");
        assert_eq!(mined_text(4_123_456_789), "You mined 4.12 ORN");
        assert_eq!(mined_text(1_829_996_000_000), "You mined 1,830 ORN");
        assert_eq!(mined_text(3_000_000), "You mined 0.003 ORN", "too small to round");
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
        let answer = apns.notify(&"ab".repeat(32), true, &payload("You received 0.000000001 ORN", &"cd".repeat(32)));
        println!("{answer:?}");
        assert!(matches!(&answer, Sent::Gone) || matches!(&answer, Sent::Failed(why) if why.contains("Apple answered 403")), "{answer:?}");
    }

    #[test]
    fn curl_configuration_quotes_safely() {
        assert_eq!(quoted(r#"{"a":"b\c"}"#), r#""{\"a\":\"b\\c\"}""#);
    }
}
