//! Sign in with Orien: an OpenID Connect provider, so any website can let
//! people sign in with their Orien Network account and learn the wallets
//! that account proved are its own.
//!
//! The flow is the standard authorization code flow, with PKCE. A website
//! sends the person to /oauth/authorize/ on the site, a page that asks this
//! server about the request, signs the person in with the usual email code
//! if need be, and shows what the website will see, with Allow and Cancel.
//! Allow gives the website a code, which its server, or its page with the
//! PKCE verifier, trades at the token endpoint for an ID token (signed
//! RS256) and an access token for the userinfo endpoint.
//!
//! What a website learns: a stable id for the account (its random handle),
//! its name and picture, and its verified wallets: only those proven by the
//! small payment. Never the email address. No wallet key is ever involved.
//!
//! Anyone signed in may register websites, once the gate is open; until
//! then only the named accounts may. The dashboard can switch a website off.
//!
//! Orien Wallet can also ask for a sign-in pass with its own session: a
//! single-use secret, good for a minute, that signs a browser in. The
//! wallet's Safari extension opens the authorize page with it in the
//! address's fragment, so people with the wallet sign in with Touch ID
//! instead of an email code.

use crate::api::{App, Failure, refused, screened};
use crate::store::{Store, now, random, token_hash};
use anyhow::{Result, bail};
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Form, Json, Router};
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use openssl::hash::MessageDigest;
use openssl::pkey::{PKey, Private};
use openssl::rsa::Rsa;
use openssl::sign::Signer;
use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::sync::Arc;

/// Websites one account may register.
pub const MAX_SITES: usize = 20;
/// Return addresses one website may have.
pub const MAX_RETURNS: usize = 10;
const MAX_SITE_NAME: usize = 40;
const MAX_RETURN: usize = 500;
/// How long a code from Allow may wait to be traded at the token endpoint.
const CODE_SECONDS: u64 = 120;
/// How long an access token and an ID token last.
const TOKEN_SECONDS: u64 = 3600;
/// How long a sign-in pass from Orien Wallet lasts.
pub const PASS_SECONDS: u64 = 60;
const MAX_STATE: usize = 1024;
const MAX_NONCE: usize = 512;

/// Who may register websites.
#[derive(Default)]
pub struct Gate {
    pub open: bool,
    /// Account names, lowercase, allowed while it is not open.
    pub developers: Vec<String>,
}

/// A website registered for Sign in with Orien, as its owner sees it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Site {
    pub client_id: String,
    pub name: String,
    pub returns: Vec<String>,
    /// Whether it has a secret: websites with a server do; pages that sign
    /// in on their own use PKCE alone.
    pub has_secret: bool,
    pub created: u64,
    /// Switched off from the dashboard.
    pub off: bool,
}

/// One website, as the dashboard sees it.
#[derive(Clone, Debug, Serialize)]
pub struct AdminSite {
    pub id: i64,
    pub owner: i64,
    pub owner_name: String,
    #[serde(flatten)]
    pub site: Site,
    pub sign_ins: u64,
}

struct Grant {
    site: i64,
    account: i64,
    redirect: String,
    challenge: Option<String>,
    nonce: Option<String>,
    scope: String,
}

/// A return address tidied, or why it cannot be one: https, or http only
/// for this computer while building a website.
pub fn return_address(text: &str) -> Result<String> {
    let text = text.trim();
    if text.is_empty() || text.len() > MAX_RETURN {
        bail!("a return address is a web address of at most {MAX_RETURN} characters");
    }
    let Ok(url) = url::Url::parse(text) else { bail!("{text} is not a web address") };
    let local = matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "[::1]"));
    if !(url.scheme() == "https" || (url.scheme() == "http" && local)) {
        bail!("a return address starts with https:// (or http://localhost while you build)");
    }
    if url.fragment().is_some() || url.host_str().is_none() || !url.username().is_empty() || url.password().is_some() {
        bail!("a return address has no # part and no name or password in it");
    }
    Ok(text.to_string())
}

fn site_name(text: &str) -> Result<String> {
    let name: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if name.is_empty() || name.chars().count() > MAX_SITE_NAME || name.chars().any(char::is_control) {
        bail!("a website's name is 1 to {MAX_SITE_NAME} characters");
    }
    Ok(name)
}

fn secret_hash(secret: &str) -> String {
    token_hash(&format!("site secret\n{secret}"))
}

/// Equal without saying how much of it matched, by its timing.
fn same(a: &str, b: &str) -> bool {
    a.len() == b.len() && a.bytes().zip(b.bytes()).fold(0u8, |d, (x, y)| d | (x ^ y)) == 0
}

impl Store {
    pub(crate) fn create_oidc_tables(&self) -> Result<()> {
        self.db.execute_batch(
            "CREATE TABLE IF NOT EXISTS oidc_sites (
                 id INTEGER PRIMARY KEY,
                 client_id TEXT NOT NULL UNIQUE,
                 owner INTEGER NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
                 name TEXT NOT NULL,
                 -- One return address a line.
                 returns TEXT NOT NULL,
                 -- Its hash; none for pages that sign in with PKCE alone.
                 secret TEXT,
                 created INTEGER NOT NULL,
                 off INTEGER NOT NULL DEFAULT 0
             );
             CREATE TABLE IF NOT EXISTS oidc_codes (
                 code TEXT PRIMARY KEY,
                 site INTEGER NOT NULL REFERENCES oidc_sites(id) ON DELETE CASCADE,
                 account INTEGER NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
                 redirect TEXT NOT NULL,
                 challenge TEXT,
                 nonce TEXT,
                 scope TEXT NOT NULL,
                 expires INTEGER NOT NULL
             );
             CREATE TABLE IF NOT EXISTS oidc_tokens (
                 token TEXT PRIMARY KEY,
                 site INTEGER NOT NULL REFERENCES oidc_sites(id) ON DELETE CASCADE,
                 account INTEGER NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
                 expires INTEGER NOT NULL
             );
             -- Websites each account has allowed, and how often it signed in.
             CREATE TABLE IF NOT EXISTS oidc_allowed (
                 account INTEGER NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
                 site INTEGER NOT NULL REFERENCES oidc_sites(id) ON DELETE CASCADE,
                 first INTEGER NOT NULL,
                 last INTEGER NOT NULL,
                 times INTEGER NOT NULL,
                 PRIMARY KEY (account, site)
             );
             CREATE TABLE IF NOT EXISTS oidc_passes (
                 pass TEXT PRIMARY KEY,
                 account INTEGER NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
                 expires INTEGER NOT NULL
             );
             -- The key ID tokens are signed with, made on first use.
             CREATE TABLE IF NOT EXISTS oidc_keys (
                 kid TEXT PRIMARY KEY,
                 pem TEXT NOT NULL,
                 created INTEGER NOT NULL
             );",
        )?;
        Ok(())
    }

    /// The websites an account registered, oldest first.
    pub fn sites(&self, owner: i64) -> Result<Vec<Site>> {
        let mut query = self.db.prepare(
            "SELECT client_id, name, returns, secret IS NOT NULL, created, off FROM oidc_sites WHERE owner = ?1 ORDER BY id",
        )?;
        let rows = query.query_map([owner], site_row)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// Registers a website. With `confidential`, it gets a secret, given
    /// back here once and never again.
    pub fn add_site(&self, owner: i64, name: &str, returns: &[String], confidential: bool) -> Result<(Site, Option<String>)> {
        let name = site_name(name)?;
        let returns = tidy_returns(returns)?;
        let count: i64 = self.db.query_row("SELECT COUNT(*) FROM oidc_sites WHERE owner = ?1", [owner], |r| r.get(0))?;
        if count as usize >= MAX_SITES {
            bail!("an account can register at most {MAX_SITES} websites");
        }
        let client_id = format!("orn_{}", hex::encode(random::<12>()?));
        let secret = confidential.then(|| random::<32>().map(|b| format!("orns_{}", hex::encode(b)))).transpose()?;
        self.db.execute(
            "INSERT INTO oidc_sites (client_id, owner, name, returns, secret, created) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![client_id, owner, name, returns.join("\n"), secret.as_deref().map(secret_hash), now() as i64],
        )?;
        let site = self.own_site(owner, &client_id)?.expect("just added");
        Ok((site, secret))
    }

    fn own_site(&self, owner: i64, client_id: &str) -> Result<Option<Site>> {
        Ok(self
            .db
            .query_row(
                "SELECT client_id, name, returns, secret IS NOT NULL, created, off FROM oidc_sites WHERE owner = ?1 AND client_id = ?2",
                params![owner, client_id],
                site_row,
            )
            .optional()?)
    }

    pub fn edit_site(&self, owner: i64, client_id: &str, name: &str, returns: &[String]) -> Result<Option<Site>> {
        let (name, returns) = (site_name(name)?, tidy_returns(returns)?);
        self.db.execute(
            "UPDATE oidc_sites SET name = ?3, returns = ?4 WHERE owner = ?1 AND client_id = ?2",
            params![owner, client_id, name, returns.join("\n")],
        )?;
        self.own_site(owner, client_id)
    }

    /// A new secret for a website, replacing the old one at once.
    pub fn new_site_secret(&self, owner: i64, client_id: &str) -> Result<Option<String>> {
        let secret = format!("orns_{}", hex::encode(random::<32>()?));
        let changed = self.db.execute(
            "UPDATE oidc_sites SET secret = ?3 WHERE owner = ?1 AND client_id = ?2",
            params![owner, client_id, secret_hash(&secret)],
        )?;
        Ok((changed == 1).then_some(secret))
    }

    pub fn remove_site(&self, owner: i64, client_id: &str) -> Result<bool> {
        Ok(self.db.execute("DELETE FROM oidc_sites WHERE owner = ?1 AND client_id = ?2", params![owner, client_id])? == 1)
    }

    /// A website by its client id, if it is registered and switched on:
    /// its row id, name, return addresses and secret hash.
    fn site_for(&self, client_id: &str) -> Result<Option<(i64, String, Vec<String>, Option<String>)>> {
        Ok(self
            .db
            .query_row(
                "SELECT id, name, returns, secret FROM oidc_sites WHERE client_id = ?1 AND off = 0",
                [client_id],
                |row| {
                    let returns: String = row.get(2)?;
                    Ok((row.get(0)?, row.get(1)?, returns.lines().map(str::to_string).collect(), row.get(3)?))
                },
            )
            .optional()?)
    }

    fn allowed(&self, account: i64, site: i64) -> Result<bool> {
        Ok(self
            .db
            .query_row("SELECT 1 FROM oidc_allowed WHERE account = ?1 AND site = ?2", params![account, site], |_| Ok(()))
            .optional()?
            .is_some())
    }

    fn grant(&self, grant: &Grant) -> Result<String> {
        let code = hex::encode(random::<32>()?);
        self.db.execute("DELETE FROM oidc_codes WHERE expires < ?1", [now() as i64])?;
        self.db.execute(
            "INSERT INTO oidc_codes (code, site, account, redirect, challenge, nonce, scope, expires)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                token_hash(&code),
                grant.site,
                grant.account,
                grant.redirect,
                grant.challenge,
                grant.nonce,
                grant.scope,
                (now() + CODE_SECONDS) as i64
            ],
        )?;
        self.db.execute(
            "INSERT INTO oidc_allowed (account, site, first, last, times) VALUES (?1, ?2, ?3, ?3, 1)
             ON CONFLICT (account, site) DO UPDATE SET last = ?3, times = times + 1",
            params![grant.account, grant.site, now() as i64],
        )?;
        Ok(code)
    }

    /// Takes a code, which works once.
    fn take_code(&self, code: &str) -> Result<Option<Grant>> {
        let hash = token_hash(code);
        let found = self
            .db
            .query_row(
                "SELECT site, account, redirect, challenge, nonce, scope FROM oidc_codes WHERE code = ?1 AND expires >= ?2",
                params![hash, now() as i64],
                |row| {
                    Ok(Grant {
                        site: row.get(0)?,
                        account: row.get(1)?,
                        redirect: row.get(2)?,
                        challenge: row.get(3)?,
                        nonce: row.get(4)?,
                        scope: row.get(5)?,
                    })
                },
            )
            .optional()?;
        self.db.execute("DELETE FROM oidc_codes WHERE code = ?1", [hash])?;
        Ok(found)
    }

    fn new_access_token(&self, site: i64, account: i64) -> Result<String> {
        let token = format!("orna_{}", hex::encode(random::<32>()?));
        self.db.execute("DELETE FROM oidc_tokens WHERE expires < ?1", [now() as i64])?;
        self.db.execute(
            "INSERT INTO oidc_tokens (token, site, account, expires) VALUES (?1, ?2, ?3, ?4)",
            params![token_hash(&token), site, account, (now() + TOKEN_SECONDS) as i64],
        )?;
        Ok(token)
    }

    /// The account an access token speaks for, while it and its website are good.
    fn token_account(&self, token: &str) -> Result<Option<i64>> {
        Ok(self
            .db
            .query_row(
                "SELECT t.account FROM oidc_tokens t JOIN oidc_sites s ON s.id = t.site
                 WHERE t.token = ?1 AND t.expires >= ?2 AND s.off = 0
                   AND t.account NOT IN (SELECT account FROM suspended)",
                params![token_hash(token), now() as i64],
                |row| row.get(0),
            )
            .optional()?)
    }

    /// A sign-in pass for an account, for Orien Wallet to hand a browser.
    pub fn new_pass(&self, account: i64) -> Result<String> {
        let pass = format!("ornp_{}", hex::encode(random::<32>()?));
        self.db.execute("DELETE FROM oidc_passes WHERE expires < ?1", [now() as i64])?;
        self.db.execute(
            "INSERT INTO oidc_passes (pass, account, expires) VALUES (?1, ?2, ?3)",
            params![token_hash(&pass), account, (now() + PASS_SECONDS) as i64],
        )?;
        Ok(pass)
    }

    /// Takes a pass, which works once.
    fn take_pass(&self, pass: &str) -> Result<Option<i64>> {
        let hash = token_hash(pass);
        let account = self
            .db
            .query_row(
                "SELECT account FROM oidc_passes WHERE pass = ?1 AND expires >= ?2
                   AND account NOT IN (SELECT account FROM suspended)",
                params![hash, now() as i64],
                |row| row.get(0),
            )
            .optional()?;
        self.db.execute("DELETE FROM oidc_passes WHERE pass = ?1", [hash])?;
        Ok(account)
    }

    /// The key ID tokens are signed with, made the first time it is needed.
    fn signing_key(&self) -> Result<(String, PKey<Private>)> {
        let found: Option<(String, String)> = self
            .db
            .query_row("SELECT kid, pem FROM oidc_keys ORDER BY created DESC LIMIT 1", [], |row| Ok((row.get(0)?, row.get(1)?)))
            .optional()?;
        if let Some((kid, pem)) = found {
            return Ok((kid, PKey::private_key_from_pem(pem.as_bytes())?));
        }
        let key = PKey::from_rsa(Rsa::generate(2048)?)?;
        let kid = hex::encode(&blake3::hash(&key.public_key_to_der()?).as_bytes()[..8]);
        let pem = String::from_utf8(key.private_key_to_pem_pkcs8()?)?;
        self.db.execute("INSERT INTO oidc_keys (kid, pem, created) VALUES (?1, ?2, ?3)", params![kid, pem, now() as i64])?;
        Ok((kid, key))
    }

    /// What a website learns about an account.
    fn claims(&self, account: i64, origin: &str) -> Result<Value> {
        let (handle, name, picture): (String, String, Option<String>) = self.db.query_row(
            "SELECT handle, name, picture FROM accounts WHERE id = ?1",
            [account],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )?;
        let wallets: Vec<Value> = self
            .verified_wallets(account)?
            .into_iter()
            .map(|w| json!({ "address": w.address, "name": w.name }))
            .collect();
        let mut claims = json!({
            "sub": handle,
            "name": name,
            "preferred_username": name,
            "profile": format!("{origin}/u/{}/", url::form_urlencoded::byte_serialize(name.as_bytes()).collect::<String>()),
            "orien_wallets": wallets,
        });
        if let Some(picture) = picture {
            claims["picture"] = json!(format!("{origin}/api/market/images/{picture}"));
        }
        Ok(claims)
    }

    /// Every website, for the dashboard, newest first.
    pub fn admin_sites(&self) -> Result<Vec<AdminSite>> {
        let mut query = self.db.prepare(
            "SELECT s.client_id, s.name, s.returns, s.secret IS NOT NULL, s.created, s.off, s.id, s.owner, a.name,
                    (SELECT COALESCE(SUM(times), 0) FROM oidc_allowed WHERE site = s.id)
             FROM oidc_sites s JOIN accounts a ON a.id = s.owner ORDER BY s.id DESC",
        )?;
        let rows = query.query_map([], |row| {
            Ok(AdminSite {
                site: site_row(row)?,
                id: row.get(6)?,
                owner: row.get(7)?,
                owner_name: row.get(8)?,
                sign_ins: row.get::<_, i64>(9)? as u64,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// Switches a website off, or on again. Off, it signs nobody in and its
    /// access tokens stop working.
    pub fn set_site_off(&self, id: i64, off: bool) -> Result<bool> {
        Ok(self.db.execute("UPDATE oidc_sites SET off = ?2 WHERE id = ?1", params![id, off])? == 1)
    }

    fn account_name(&self, account: i64) -> Result<String> {
        Ok(self.db.query_row("SELECT name FROM accounts WHERE id = ?1", [account], |row| row.get(0))?)
    }
}

fn site_row(row: &rusqlite::Row) -> rusqlite::Result<Site> {
    let returns: String = row.get(2)?;
    Ok(Site {
        client_id: row.get(0)?,
        name: row.get(1)?,
        returns: returns.lines().map(str::to_string).collect(),
        has_secret: row.get(3)?,
        created: row.get::<_, i64>(4)? as u64,
        off: row.get(5)?,
    })
}

fn tidy_returns(returns: &[String]) -> Result<Vec<String>> {
    let mut tidy = Vec::new();
    for text in returns.iter().filter(|t| !t.trim().is_empty()) {
        let address = return_address(text)?;
        if !tidy.contains(&address) {
            tidy.push(address);
        }
    }
    if tidy.is_empty() || tidy.len() > MAX_RETURNS {
        bail!("a website has 1 to {MAX_RETURNS} return addresses");
    }
    Ok(tidy)
}

fn b64(bytes: &[u8]) -> String {
    URL_SAFE_NO_PAD.encode(bytes)
}

/// A signed ID token.
fn id_token(key: &(String, PKey<Private>), claims: &Value) -> Result<String> {
    let head = b64(json!({ "alg": "RS256", "typ": "JWT", "kid": key.0 }).to_string().as_bytes());
    let body = b64(claims.to_string().as_bytes());
    let mut signer = Signer::new(MessageDigest::sha256(), &key.1)?;
    signer.update(format!("{head}.{body}").as_bytes())?;
    Ok(format!("{head}.{body}.{}", b64(&signer.sign_to_vec()?)))
}

pub fn routes() -> Router<Arc<App>> {
    Router::new()
        .route("/.well-known/openid-configuration", get(discovery).options(preflight))
        .route("/api/market/oidc/jwks", get(jwks).options(preflight))
        .route("/api/market/oidc/request", get(request))
        .route("/api/market/oidc/allow", post(allow))
        .route("/api/market/oidc/token", post(token).options(preflight))
        .route("/api/market/oidc/userinfo", get(userinfo).post(userinfo).options(preflight))
        .route("/api/market/oidc/pass", post(new_pass))
        .route("/api/market/oidc/pass/use", post(use_pass))
        .route("/api/market/me/sites", get(my_sites).post(add_site))
        .route("/api/market/me/sites/{client_id}", axum::routing::put(edit_site).delete(remove_site))
        .route("/api/market/me/sites/{client_id}/secret", post(new_secret))
}

/// Headers that let any website's pages read an answer.
fn open_to_pages(mut response: Response) -> Response {
    let headers = response.headers_mut();
    headers.insert(header::ACCESS_CONTROL_ALLOW_ORIGIN, HeaderValue::from_static("*"));
    headers.insert(header::ACCESS_CONTROL_ALLOW_HEADERS, HeaderValue::from_static("authorization, content-type"));
    headers.insert(header::ACCESS_CONTROL_ALLOW_METHODS, HeaderValue::from_static("GET, POST, OPTIONS"));
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

async fn preflight() -> Response {
    open_to_pages(StatusCode::NO_CONTENT.into_response())
}

async fn discovery(State(app): State<Arc<App>>) -> Response {
    let origin = app.origin();
    open_to_pages(
        Json(json!({
            "issuer": origin,
            "authorization_endpoint": format!("{origin}/oauth/authorize/"),
            "token_endpoint": format!("{origin}/api/market/oidc/token"),
            "userinfo_endpoint": format!("{origin}/api/market/oidc/userinfo"),
            "jwks_uri": format!("{origin}/api/market/oidc/jwks"),
            "response_types_supported": ["code"],
            "response_modes_supported": ["query"],
            "grant_types_supported": ["authorization_code"],
            "subject_types_supported": ["public"],
            "id_token_signing_alg_values_supported": ["RS256"],
            "scopes_supported": ["openid", "profile", "wallets"],
            "token_endpoint_auth_methods_supported": ["client_secret_basic", "client_secret_post", "none"],
            "code_challenge_methods_supported": ["S256"],
            "claims_supported": ["sub", "iss", "aud", "exp", "iat", "nonce", "name", "preferred_username", "picture", "profile", "orien_wallets"],
            "service_documentation": format!("{origin}/developers/sign-in/"),
        }))
        .into_response(),
    )
}

async fn jwks(State(app): State<Arc<App>>) -> Result<Response, Failure> {
    let (kid, key) = app.store().signing_key()?;
    let rsa = key.rsa().map_err(anyhow::Error::from)?;
    Ok(open_to_pages(
        Json(json!({ "keys": [{
            "kty": "RSA", "use": "sig", "alg": "RS256", "kid": kid,
            "n": b64(&rsa.n().to_vec()), "e": b64(&rsa.e().to_vec()),
        }]}))
        .into_response(),
    ))
}

#[derive(Deserialize)]
pub struct Asked {
    client_id: String,
    redirect_uri: String,
}

/// What the authorize page shows: which website asks, whether this
/// browser is signed in, and if so what the website would see.
async fn request(State(app): State<Arc<App>>, headers: HeaderMap, Query(asked): Query<Asked>) -> Result<Response, Failure> {
    let store = app.store();
    let Some((site, name, returns, _)) = store.site_for(&asked.client_id)? else {
        return Err(Failure::Refused("This website isn’t registered for Sign in with Orien, or it was switched off.".into()));
    };
    if !returns.contains(&asked.redirect_uri) {
        return Err(Failure::Refused("This website asked to send you back to an address it hasn’t registered.".into()));
    }
    let host = url::Url::parse(&asked.redirect_uri).ok().and_then(|u| u.host_str().map(str::to_string)).unwrap_or_default();
    drop(store);
    let account = app.signed_in(&headers).ok();
    let store = app.store();
    let you = match account {
        Some(account) => {
            let claims = store.claims(account, app.origin())?;
            Some(json!({ "name": claims["name"], "picture": claims["picture"], "wallets": claims["orien_wallets"], "allowed": store.allowed(account, site)? }))
        }
        None => None,
    };
    Ok(Json(json!({ "site": { "name": name, "host": host }, "you": you })).into_response())
}

#[derive(Deserialize)]
pub struct Allow {
    client_id: String,
    redirect_uri: String,
    #[serde(default)]
    scope: String,
    state: Option<String>,
    nonce: Option<String>,
    code_challenge: Option<String>,
    code_challenge_method: Option<String>,
}

/// Allow: a code for the website, and the address to send the browser to.
async fn allow(State(app): State<Arc<App>>, headers: HeaderMap, Json(asked): Json<Allow>) -> Result<Response, Failure> {
    let account = app.signed_in(&headers)?;
    let store = app.store();
    let Some((site, _, returns, secret)) = store.site_for(&asked.client_id)? else {
        return Err(Failure::Refused("This website isn’t registered for Sign in with Orien, or it was switched off.".into()));
    };
    if !returns.contains(&asked.redirect_uri) {
        return Err(Failure::Refused("This website asked to send you back to an address it hasn’t registered.".into()));
    }
    let mut back = url::Url::parse(&asked.redirect_uri).map_err(|e| Failure::Broken(e.into()))?;
    let problem = if !asked.scope.split_whitespace().any(|s| s == "openid") {
        Some("the request must ask for the openid scope")
    } else if asked.state.as_ref().is_some_and(|s| s.len() > MAX_STATE) || asked.nonce.as_ref().is_some_and(|n| n.len() > MAX_NONCE) {
        Some("state or nonce is too long")
    } else if asked.code_challenge.is_some() && asked.code_challenge_method.as_deref() != Some("S256") {
        Some("code_challenge_method must be S256")
    } else if asked.code_challenge.as_ref().is_some_and(|c| c.len() < 43 || c.len() > 128) {
        Some("code_challenge must be 43 to 128 characters")
    } else if secret.is_none() && asked.code_challenge.is_none() {
        Some("a website without a secret must use PKCE (code_challenge)")
    } else {
        None
    };
    {
        let mut pairs = back.query_pairs_mut();
        match problem {
            Some(problem) => {
                pairs.append_pair("error", "invalid_request").append_pair("error_description", problem);
            }
            None => {
                let code = store.grant(&Grant {
                    site,
                    account,
                    redirect: asked.redirect_uri.clone(),
                    challenge: asked.code_challenge.clone(),
                    nonce: asked.nonce.clone(),
                    scope: asked.scope.clone(),
                })?;
                pairs.append_pair("code", &code);
            }
        }
        if let Some(state) = &asked.state {
            pairs.append_pair("state", state);
        }
        pairs.append_pair("iss", app.origin());
    }
    Ok(Json(json!({ "redirect": back.to_string() })).into_response())
}

#[derive(Deserialize)]
pub struct TokenAsk {
    grant_type: String,
    code: Option<String>,
    redirect_uri: Option<String>,
    client_id: Option<String>,
    client_secret: Option<String>,
    code_verifier: Option<String>,
}

fn oauth_error(status: StatusCode, error: &str, description: &str) -> Response {
    open_to_pages((status, Json(json!({ "error": error, "error_description": description }))).into_response())
}

/// The client id and secret from an Authorization: Basic header.
fn basic(headers: &HeaderMap) -> Option<(String, String)> {
    let value = headers.get(header::AUTHORIZATION)?.to_str().ok()?.strip_prefix("Basic ")?;
    let decoded = String::from_utf8(base64::engine::general_purpose::STANDARD.decode(value.trim()).ok()?).ok()?;
    let (id, secret) = decoded.split_once(':')?;
    let unescape = |s: &str| url::form_urlencoded::parse(format!("x={s}").as_bytes()).next().map(|(_, v)| v.into_owned());
    Some((unescape(id)?, unescape(secret)?))
}

async fn token(State(app): State<Arc<App>>, headers: HeaderMap, Form(ask): Form<TokenAsk>) -> Result<Response, Failure> {
    if ask.grant_type != "authorization_code" {
        return Ok(oauth_error(StatusCode::BAD_REQUEST, "unsupported_grant_type", "only authorization_code"));
    }
    let (client_id, secret) = match (basic(&headers), ask.client_id.clone()) {
        (Some((id, secret)), _) => (id, Some(secret)),
        (None, Some(id)) => (id, ask.client_secret.clone()),
        (None, None) => return Ok(oauth_error(StatusCode::UNAUTHORIZED, "invalid_client", "client_id is missing")),
    };
    let store = app.store();
    let Some((site, _, _, secret_kept)) = store.site_for(&client_id)? else {
        return Ok(oauth_error(StatusCode::UNAUTHORIZED, "invalid_client", "unknown or switched-off website"));
    };
    if let Some(kept) = &secret_kept
        && !secret.as_deref().is_some_and(|given| same(&secret_hash(given), kept))
    {
        return Ok(oauth_error(StatusCode::UNAUTHORIZED, "invalid_client", "wrong client secret"));
    }
    let Some(code) = ask.code.as_deref() else {
        return Ok(oauth_error(StatusCode::BAD_REQUEST, "invalid_request", "code is missing"));
    };
    let Some(grant) = store.take_code(code)? else {
        return Ok(oauth_error(StatusCode::BAD_REQUEST, "invalid_grant", "the code is unknown, used or expired"));
    };
    if grant.site != site || ask.redirect_uri.as_deref() != Some(grant.redirect.as_str()) {
        return Ok(oauth_error(StatusCode::BAD_REQUEST, "invalid_grant", "the code was given to another website or address"));
    }
    if let Some(challenge) = &grant.challenge {
        let Some(verifier) = ask.code_verifier.as_deref() else {
            return Ok(oauth_error(StatusCode::BAD_REQUEST, "invalid_grant", "code_verifier is missing"));
        };
        let digest = openssl::sha::sha256(verifier.as_bytes());
        if !same(&b64(&digest), challenge) {
            return Ok(oauth_error(StatusCode::BAD_REQUEST, "invalid_grant", "code_verifier does not match"));
        }
    }
    let mut claims = store.claims(grant.account, app.origin())?;
    let issued = now();
    claims["iss"] = json!(app.origin());
    claims["aud"] = json!(client_id);
    claims["iat"] = json!(issued);
    claims["exp"] = json!(issued + TOKEN_SECONDS);
    if let Some(nonce) = &grant.nonce {
        claims["nonce"] = json!(nonce);
    }
    let key = store.signing_key()?;
    let id_token = id_token(&key, &claims)?;
    let access = store.new_access_token(site, grant.account)?;
    Ok(open_to_pages(
        Json(json!({
            "access_token": access,
            "token_type": "Bearer",
            "expires_in": TOKEN_SECONDS,
            "id_token": id_token,
            "scope": grant.scope,
        }))
        .into_response(),
    ))
}

async fn userinfo(State(app): State<Arc<App>>, headers: HeaderMap) -> Result<Response, Failure> {
    let token = headers.get(header::AUTHORIZATION).and_then(|v| v.to_str().ok()).and_then(|v| v.strip_prefix("Bearer "));
    let store = app.store();
    let Some(account) = token.map(|t| store.token_account(t.trim())).transpose()?.flatten() else {
        let mut response = oauth_error(StatusCode::UNAUTHORIZED, "invalid_token", "the access token is unknown or expired");
        response.headers_mut().insert(header::WWW_AUTHENTICATE, HeaderValue::from_static("Bearer error=\"invalid_token\""));
        return Ok(response);
    };
    Ok(open_to_pages(Json(store.claims(account, app.origin())?).into_response()))
}

/// A sign-in pass, for Orien Wallet to hand its Safari extension.
async fn new_pass(State(app): State<Arc<App>>, headers: HeaderMap) -> Result<Response, Failure> {
    let account = app.signed_in(&headers)?;
    let pass = app.store().new_pass(account)?;
    Ok(Json(json!({ "pass": pass, "expires_in": PASS_SECONDS })).into_response())
}

#[derive(Deserialize)]
pub struct UsePass {
    pass: String,
}

/// Signs this browser in with a pass from Orien Wallet.
async fn use_pass(State(app): State<Arc<App>>, Json(given): Json<UsePass>) -> Result<Response, Failure> {
    let store = app.store();
    let Some(account) = store.take_pass(given.pass.trim())? else {
        return Err(Failure::Refused("That sign-in from Orien Wallet has expired. Try again.".into()));
    };
    let token = store.start_session(account)?;
    let name = store.account_name(account)?;
    let mut response = Json(json!({ "name": name })).into_response();
    response.headers_mut().insert(header::SET_COOKIE, app.session_cookie(&token, crate::store::COOKIE_SECONDS));
    Ok(response)
}

async fn my_sites(State(app): State<Arc<App>>, headers: HeaderMap) -> Result<Response, Failure> {
    let account = app.signed_in(&headers)?;
    let store = app.store();
    let may = may_register(&app, &store, account)?;
    Ok(Json(json!({ "may_register": may, "sites": store.sites(account)? })).into_response())
}

fn may_register(app: &App, store: &Store, account: i64) -> Result<bool, Failure> {
    let gate = app.sign_in_gate();
    Ok(gate.open || gate.developers.contains(&store.account_name(account)?.to_lowercase()))
}

#[derive(Deserialize)]
pub struct SiteForm {
    name: String,
    returns: Vec<String>,
    #[serde(default)]
    confidential: bool,
}

async fn add_site(State(app): State<Arc<App>>, headers: HeaderMap, Json(form): Json<SiteForm>) -> Result<Response, Failure> {
    let account = app.signed_in(&headers)?;
    {
        let store = app.store();
        if !may_register(&app, &store, account)? {
            return Err(Failure::Forbidden("Registering websites isn’t open yet.".into()));
        }
        site_name(&form.name).map_err(refused)?;
        tidy_returns(&form.returns).map_err(refused)?;
    }
    let name = form.name.clone();
    screened(&app, "website name", move |s| s.profile(&name, "")).await?;
    let (site, secret) = app.store().add_site(account, &form.name, &form.returns, form.confidential).map_err(refused)?;
    Ok(Json(json!({ "site": site, "secret": secret })).into_response())
}

async fn edit_site(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Path(client_id): Path<String>,
    Json(form): Json<SiteForm>,
) -> Result<Response, Failure> {
    let account = app.signed_in(&headers)?;
    site_name(&form.name).map_err(refused)?;
    tidy_returns(&form.returns).map_err(refused)?;
    let name = form.name.clone();
    screened(&app, "website name", move |s| s.profile(&name, "")).await?;
    let site = app.store().edit_site(account, &client_id, &form.name, &form.returns).map_err(refused)?;
    Ok(Json(json!({ "site": site.ok_or(Failure::NotFound)? })).into_response())
}

async fn new_secret(State(app): State<Arc<App>>, headers: HeaderMap, Path(client_id): Path<String>) -> Result<Response, Failure> {
    let account = app.signed_in(&headers)?;
    let secret = app.store().new_site_secret(account, &client_id)?.ok_or(Failure::NotFound)?;
    Ok(Json(json!({ "secret": secret })).into_response())
}

async fn remove_site(State(app): State<Arc<App>>, headers: HeaderMap, Path(client_id): Path<String>) -> Result<Response, Failure> {
    let account = app.signed_in(&headers)?;
    if !app.store().remove_site(account, &client_id)? {
        return Err(Failure::NotFound);
    }
    Ok(Json(json!({ "removed": true })).into_response())
}
