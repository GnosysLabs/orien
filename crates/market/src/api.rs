//! What the marketplace pages ask the server, under /api/market.
//!
//! Signing in uses passkeys: the browser proves it holds an account's
//! passkey, and the server answers with a cookie that keeps it signed in.
//! Changes need that cookie; reading listings does not.

use crate::images::{self, Folder};
use crate::mail::Mailer;
use crate::screen::{Screener, Verdict};
use crate::store::{self, Draft, SESSION_SECONDS, Store};
use crate::wallets::Payments;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::json;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use webauthn_rs::prelude::*;

/// The cookie that keeps a browser signed in.
pub const COOKIE: &str = "orien_market";

/// How long a sign-up or sign-in may take between its two steps.
const CEREMONY: Duration = Duration::from_secs(300);
/// Sign-ups and sign-ins under way at once, at most. Beyond it the oldest
/// are dropped, so nobody can fill the server's memory with them.
const MAX_CEREMONIES: usize = 10_000;

enum Ceremony {
    SignUp { name: String, handle: Uuid, state: PasskeyRegistration },
    SignIn { state: DiscoverableAuthentication },
}

pub struct App {
    store: Mutex<Store>,
    webauthn: Webauthn,
    ceremonies: Mutex<HashMap<String, (Ceremony, Instant)>>,
    mailer: Arc<Mailer>,
    screener: Arc<Screener>,
    images: Folder,
    /// What the marketplace's own address has received, for checking wallets.
    payments: Arc<dyn Payments>,
    /// The marketplace's own Orien address, which sellers prove their
    /// wallets by paying.
    address: String,
    /// Whether the cookie is only sent over https: everywhere but tests.
    secure: bool,
}

impl App {
    /// `origin` is the site's address, such as https://orien.network.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        store: Store,
        images: Folder,
        origin: &str,
        secure: bool,
        mailer: Mailer,
        screener: Screener,
        payments: Arc<dyn Payments>,
        address: &str,
    ) -> anyhow::Result<App> {
        let address = store::orien_address(address)?;
        let origin = Url::parse(origin)?;
        let id = origin.host_str().ok_or_else(|| anyhow::anyhow!("the origin has no host"))?.to_string();
        let webauthn = WebauthnBuilder::new(&id, &origin)?.rp_name("Orien marketplace").build()?;
        Ok(App {
            store: Mutex::new(store),
            webauthn,
            ceremonies: Mutex::new(HashMap::new()),
            mailer: Arc::new(mailer),
            screener: Arc::new(screener),
            images,
            payments,
            address,
            secure,
        })
    }

    fn begin(&self, ceremony: Ceremony) -> String {
        let mut ceremonies = self.ceremonies.lock().unwrap();
        ceremonies.retain(|_, (_, at)| at.elapsed() < CEREMONY);
        if ceremonies.len() >= MAX_CEREMONIES
            && let Some(oldest) = ceremonies.iter().min_by_key(|(_, (_, at))| *at).map(|(id, _)| id.clone())
        {
            ceremonies.remove(&oldest);
        }
        let id = Uuid::new_v4().to_string();
        ceremonies.insert(id.clone(), (ceremony, Instant::now()));
        id
    }

    fn finish(&self, id: &str) -> Option<Ceremony> {
        let (ceremony, at) = self.ceremonies.lock().unwrap().remove(id)?;
        (at.elapsed() < CEREMONY).then_some(ceremony)
    }

    fn signed_in(&self, headers: &HeaderMap) -> Result<i64, Failure> {
        let token = cookie(headers).ok_or(Failure::SignIn)?;
        self.store.lock().unwrap().session(&token)?.ok_or(Failure::SignIn)
    }

    fn session_cookie(&self, token: &str, max_age: u64) -> HeaderValue {
        let secure = if self.secure { "; Secure" } else { "" };
        let value = format!("{COOKIE}={token}; Path=/api/market; HttpOnly; SameSite=Strict; Max-Age={max_age}{secure}");
        HeaderValue::from_str(&value).expect("a cookie of plain characters")
    }
}

fn cookie(headers: &HeaderMap) -> Option<String> {
    headers.get_all(header::COOKIE).iter().filter_map(|value| value.to_str().ok()).find_map(|cookies| {
        cookies.split(';').find_map(|pair| {
            let (name, value) = pair.trim().split_once('=')?;
            (name == COOKIE).then(|| value.to_string())
        })
    })
}

/// Why a request failed, as the pages are told.
pub enum Failure {
    /// Something the person can fix, said in plain words.
    Refused(String),
    /// Something that cannot be done right now, said in plain words.
    Unavailable(String),
    SignIn,
    NotFound,
    Broken(anyhow::Error),
}

impl From<anyhow::Error> for Failure {
    fn from(error: anyhow::Error) -> Failure {
        Failure::Broken(error)
    }
}

impl IntoResponse for Failure {
    fn into_response(self) -> Response {
        let (status, message) = match self {
            Failure::Refused(message) => (StatusCode::BAD_REQUEST, message),
            Failure::Unavailable(message) => (StatusCode::SERVICE_UNAVAILABLE, message),
            Failure::SignIn => (StatusCode::UNAUTHORIZED, "sign in first".to_string()),
            Failure::NotFound => (StatusCode::NOT_FOUND, "not found".to_string()),
            Failure::Broken(error) => {
                eprintln!("marketplace error: {error:#}");
                (StatusCode::INTERNAL_SERVER_ERROR, "something went wrong on the server".to_string())
            }
        };
        (status, Json(json!({ "error": message }))).into_response()
    }
}

/// A store refusal the person can act on, such as a title too long.
fn refused(error: anyhow::Error) -> Failure {
    Failure::Refused(error.to_string())
}

type Answer = Result<Response, Failure>;

/// Runs a check by the screening model, off the async threads. Anything it
/// refuses, or cannot check, is not saved.
async fn screened(app: &App, what: &str, check: impl FnOnce(&Screener) -> anyhow::Result<Verdict> + Send + 'static) -> Result<(), Failure> {
    let screener = app.screener.clone();
    let verdict = tokio::task::spawn_blocking(move || check(&screener)).await.map_err(|e| Failure::Broken(e.into()))?;
    match verdict {
        Ok(Verdict::Allowed) => Ok(()),
        Ok(Verdict::Refused(reasons)) => Err(Failure::Refused(format!(
            "this {what} cannot be posted, because it looks like it contains {}. If that is a mistake, change the wording and try again",
            reasons.join(", and ")
        ))),
        Err(error) => {
            eprintln!("screening failed, so nothing was saved: {error:#}");
            Err(Failure::Unavailable(format!(
                "new {what}s cannot be checked right now, so they cannot be saved. Try again in a few minutes"
            )))
        }
    }
}

async fn screened_listing(app: &App, draft: Draft) -> Result<Draft, Failure> {
    let draft = draft.checked().map_err(refused)?;
    let d = draft.clone();
    screened(app, "listing", move |s| s.listing(&d.title, &d.description, &d.category, &d.unit)).await?;
    Ok(draft)
}

async fn screened_name(app: &App, name: &str) -> Result<(), Failure> {
    let name = name.trim().to_string();
    screened(app, "name", move |s| s.profile(&name, "")).await
}

pub fn router(app: Arc<App>) -> Router {
    Router::new()
        .route("/api/market/email/start", post(email_start))
        .route("/api/market/email/finish", post(email_finish))
        .route("/api/market/signup/start", post(signup_start))
        .route("/api/market/signup/finish", post(signup_finish))
        .route("/api/market/signin/start", post(signin_start))
        .route("/api/market/signin/finish", post(signin_finish))
        .route("/api/market/signout", post(signout))
        .route("/api/market/me", get(me).put(edit_profile))
        .route(
            "/api/market/me/picture",
            post(set_picture).layer(axum::extract::DefaultBodyLimit::max(images::MAX_UPLOAD)),
        )
        .route("/api/market/me/wallet", get(check_wallet).post(start_wallet))
        .route("/api/market/profiles/{id}", get(profile))
        .route("/api/market/images/{name}", get(picture))
        .route("/api/market/listings", get(browse).post(post_listing))
        .route("/api/market/listings/{id}", get(listing).put(edit_listing))
        .route("/api/market/listings/{id}/open", post(open_listing))
        .with_state(app)
}

#[derive(Deserialize)]
struct EmailStart {
    email: String,
}

/// Sends a sign-in code. Says the same whether or not the email has an
/// account, so nobody can use it to find out who is a member.
async fn email_start(State(app): State<Arc<App>>, Json(request): Json<EmailStart>) -> Answer {
    let email = store::email(&request.email).map_err(refused)?;
    let Some(code) = app.store.lock().unwrap().issue_code(&email).map_err(refused)? else {
        // One was sent a moment ago, and still works.
        return Ok(Json(json!({ "sent": email })).into_response());
    };
    let mailer = app.mailer.clone();
    let to = email.clone();
    tokio::task::spawn_blocking(move || mailer.send_code(&to, &code))
        .await
        .map_err(|e| Failure::Broken(e.into()))?
        .map_err(Failure::Broken)?;
    Ok(Json(json!({ "sent": email })).into_response())
}

#[derive(Deserialize)]
struct EmailFinish {
    email: String,
    code: String,
    /// The new account's name: asked only when the email has none.
    #[serde(default)]
    name: String,
}

/// Signs in with a code. For an email with no account yet, it answers
/// `needs_name` and keeps the code good, until the page sends a name too.
async fn email_finish(State(app): State<Arc<App>>, Json(request): Json<EmailFinish>) -> Answer {
    let email = store::email(&request.email).map_err(refused)?;
    let existing = {
        let store = app.store.lock().unwrap();
        let existing = store.account_by_email(&email)?;
        if existing.is_none() {
            store.check_code(&email, &request.code, false).map_err(refused)?;
            if request.name.trim().is_empty() {
                return Ok(Json(json!({ "needs_name": true })).into_response());
            }
        }
        existing
    };
    if existing.is_none() {
        screened_name(&app, &request.name).await?;
    }
    let store = app.store.lock().unwrap();
    store.check_code(&email, &request.code, true).map_err(refused)?;
    let account = match existing {
        Some(id) => store.account(id)?.ok_or(Failure::NotFound)?,
        None => store.create_account_with_email(&request.name, &email).map_err(refused)?,
    };
    let token = store.start_session(account.id)?;
    let mut response = Json(json!({ "account": account })).into_response();
    response.headers_mut().insert(header::SET_COOKIE, app.session_cookie(&token, SESSION_SECONDS));
    Ok(response)
}

#[derive(Deserialize)]
struct SignUp {
    name: String,
}

async fn signup_start(State(app): State<Arc<App>>, Json(request): Json<SignUp>) -> Answer {
    let name = request.name.trim().to_string();
    if name.is_empty() || name.chars().count() > crate::store::MAX_NAME {
        return Err(Failure::Refused(format!("a name is 1 to {} characters", crate::store::MAX_NAME)));
    }
    screened_name(&app, &name).await?;
    let handle = Uuid::new_v4();
    let (options, state) = app
        .webauthn
        .start_passkey_registration(handle, &name, &name, None)
        .map_err(|e| Failure::Broken(anyhow::anyhow!("passkey registration: {e}")))?;
    let id = app.begin(Ceremony::SignUp { name, handle, state });
    Ok(Json(json!({ "id": id, "options": options })).into_response())
}

#[derive(Deserialize)]
struct Finish<T> {
    id: String,
    credential: T,
}

async fn signup_finish(State(app): State<Arc<App>>, Json(request): Json<Finish<RegisterPublicKeyCredential>>) -> Answer {
    let Some(Ceremony::SignUp { name, handle, state }) = app.finish(&request.id) else {
        return Err(Failure::Refused("that sign-up took too long; start again".into()));
    };
    let passkey = app
        .webauthn
        .finish_passkey_registration(&request.credential, &state)
        .map_err(|_| Failure::Refused("the passkey could not be checked; try again".into()))?;
    let store = app.store.lock().unwrap();
    let account = store.create_account(&name, &handle.to_string()).map_err(refused)?;
    let credential = hex::encode(passkey.cred_id());
    store.add_passkey(account.id, &credential, &serde_json::to_string(&passkey).map_err(anyhow::Error::from)?)?;
    let token = store.start_session(account.id)?;
    let mut response = Json(json!({ "account": account })).into_response();
    response.headers_mut().insert(header::SET_COOKIE, app.session_cookie(&token, SESSION_SECONDS));
    Ok(response)
}

async fn signin_start(State(app): State<Arc<App>>) -> Answer {
    let (options, state) = app
        .webauthn
        .start_discoverable_authentication()
        .map_err(|e| Failure::Broken(anyhow::anyhow!("passkey sign-in: {e}")))?;
    let id = app.begin(Ceremony::SignIn { state });
    Ok(Json(json!({ "id": id, "options": options })).into_response())
}

async fn signin_finish(State(app): State<Arc<App>>, Json(request): Json<Finish<PublicKeyCredential>>) -> Answer {
    let Some(Ceremony::SignIn { state }) = app.finish(&request.id) else {
        return Err(Failure::Refused("that sign-in took too long; start again".into()));
    };
    let unknown = || Failure::Refused("that passkey does not belong to a marketplace account".into());
    let (_, credential_id) = app.webauthn.identify_discoverable_authentication(&request.credential).map_err(|_| unknown())?;
    let credential = hex::encode(credential_id);
    let store = app.store.lock().unwrap();
    let (account, stored) = store.passkey(&credential)?.ok_or_else(unknown)?;
    let mut passkey: Passkey = serde_json::from_str(&stored).map_err(anyhow::Error::from)?;
    let result = app
        .webauthn
        .finish_discoverable_authentication_v2(&request.credential, state, &passkey)
        .map_err(|_| Failure::Refused("the passkey could not be checked; try again".into()))?;
    if passkey.update_credential(&result) == Some(true) {
        store.update_passkey(&credential, &serde_json::to_string(&passkey).map_err(anyhow::Error::from)?)?;
    }
    let token = store.start_session(account)?;
    let account = store.account(account)?.ok_or(Failure::NotFound)?;
    let mut response = Json(json!({ "account": account })).into_response();
    response.headers_mut().insert(header::SET_COOKIE, app.session_cookie(&token, SESSION_SECONDS));
    Ok(response)
}

async fn signout(State(app): State<Arc<App>>, headers: HeaderMap) -> Answer {
    if let Some(token) = cookie(&headers) {
        app.store.lock().unwrap().end_session(&token)?;
    }
    let mut response = Json(json!({})).into_response();
    response.headers_mut().insert(header::SET_COOKIE, app.session_cookie("", 0));
    Ok(response)
}

/// The signed-in account and all its listings.
async fn me(State(app): State<Arc<App>>, headers: HeaderMap) -> Answer {
    let id = app.signed_in(&headers)?;
    let store = app.store.lock().unwrap();
    let account = store.account(id)?.ok_or(Failure::SignIn)?;
    let listings = store.listings_of(id)?;
    Ok(Json(json!({ "account": account, "listings": listings })).into_response())
}

#[derive(Deserialize)]
struct WalletStart {
    address: String,
}

/// Asks the network something, off the async threads.
async fn network<T: Send + 'static>(
    app: &App,
    ask: impl FnOnce(&dyn Payments) -> anyhow::Result<T> + Send + 'static,
) -> Result<T, Failure> {
    let payments = app.payments.clone();
    tokio::task::spawn_blocking(move || ask(payments.as_ref()))
        .await
        .map_err(|e| Failure::Broken(e.into()))?
        .map_err(|error| {
            eprintln!("cannot reach the Orien network: {error:#}");
            Failure::Unavailable("the Orien network is not answering right now; try again in a minute".into())
        })
}

/// Starts proving a wallet: says what to send, from where, to where.
async fn start_wallet(State(app): State<Arc<App>>, headers: HeaderMap, Json(request): Json<WalletStart>) -> Answer {
    let id = app.signed_in(&headers)?;
    store::orien_address(&request.address).map_err(refused)?;
    let epoch = network(&app, |p| p.epoch()).await?;
    let amount = app.store.lock().unwrap().start_verification(id, &request.address, epoch).map_err(refused)?;
    let from = store::orien_address(&request.address).map_err(refused)?;
    Ok(Json(json!({ "waiting": { "from": from, "pay_to": app.address, "amount": amount } })).into_response())
}

/// Looks for the payment that proves the wallet, and links it once found.
async fn check_wallet(State(app): State<Arc<App>>, headers: HeaderMap) -> Answer {
    let id = app.signed_in(&headers)?;
    let waiting = app.store.lock().unwrap().verification(id)?;
    let Some((from, amount, epoch)) = waiting else {
        let account = app.store.lock().unwrap().account(id)?.ok_or(Failure::SignIn)?;
        return Ok(Json(json!({ "wallet": account.wallet })).into_response());
    };
    let to: orien_ledger::AccountId = app.address.parse().map_err(|_| Failure::Broken(anyhow::anyhow!("bad marketplace address")))?;
    // From the epoch before, in case the check began as an epoch closed.
    let received = network(&app, move |p| p.received(&to, epoch.saturating_sub(1))).await?;
    let paid = received.iter().any(|payment| payment.from.to_string() == from && payment.amount == amount);
    if paid {
        app.store.lock().unwrap().set_wallet(id, &from).map_err(refused)?;
        return Ok(Json(json!({ "wallet": from })).into_response());
    }
    Ok(Json(json!({ "waiting": { "from": from, "pay_to": app.address, "amount": amount } })).into_response())
}

#[derive(Deserialize)]
struct ProfileText {
    name: String,
    #[serde(default)]
    bio: String,
    #[serde(default)]
    contact: store::Contact,
}

async fn edit_profile(State(app): State<Arc<App>>, headers: HeaderMap, Json(request): Json<ProfileText>) -> Answer {
    let id = app.signed_in(&headers)?;
    let (name, bio) = store::profile_text(&request.name, &request.bio).map_err(refused)?;
    let contact = request.contact.checked().map_err(refused)?;
    // The usernames are public too, so they are read with the bio.
    let handles = format!("{bio}\n\nTelegram: {}\nX: {}", contact.telegram, contact.x);
    let n = name.clone();
    screened(&app, "profile", move |s| s.profile(&n, &handles)).await?;
    let account = app.store.lock().unwrap().set_profile(id, &name, &bio, &contact).map_err(refused)?;
    Ok(Json(json!({ "account": account })).into_response())
}

/// A new profile picture, sent as the request's whole body.
async fn set_picture(State(app): State<Arc<App>>, headers: HeaderMap, body: axum::body::Bytes) -> Answer {
    let id = app.signed_in(&headers)?;
    let jpeg = tokio::task::spawn_blocking(move || images::profile_picture(&body))
        .await
        .map_err(|e| Failure::Broken(e.into()))?
        .map_err(refused)?;
    let checked = jpeg.clone();
    screened(&app, "picture", move |s| s.picture(&checked)).await?;
    let name = app.images.save(&jpeg)?;
    let old = app.store.lock().unwrap().set_picture(id, &name)?;
    if let Some(old) = old {
        app.images.remove(&old);
    }
    let account = app.store.lock().unwrap().account(id)?.ok_or(Failure::SignIn)?;
    Ok(Json(json!({ "account": account })).into_response())
}

/// A seller's public profile: who they are and what they offer.
async fn profile(State(app): State<Arc<App>>, Path(id): Path<i64>) -> Answer {
    let store = app.store.lock().unwrap();
    let account = store.account(id)?.ok_or(Failure::NotFound)?;
    let listings = store.open_listings_of(id)?;
    Ok(Json(json!({ "account": account, "listings": listings })).into_response())
}

async fn picture(State(app): State<Arc<App>>, Path(name): Path<String>) -> Answer {
    let bytes = app.images.read(&name).ok_or(Failure::NotFound)?;
    // Names are never reused, so a picture can be kept for good.
    Ok((
        [(header::CONTENT_TYPE, "image/jpeg"), (header::CACHE_CONTROL, "public, max-age=31536000, immutable")],
        bytes,
    )
        .into_response())
}

#[derive(Deserialize)]
struct Browse {
    category: Option<String>,
    q: Option<String>,
    #[serde(default)]
    offset: u32,
}

/// Listings a page shows at a time.
const PAGE: u32 = 30;

async fn browse(State(app): State<Arc<App>>, Query(query): Query<Browse>) -> Answer {
    let category = query.category.as_deref().filter(|c| !c.is_empty());
    let listings = app.store.lock().unwrap().browse(category, query.q.as_deref(), PAGE, query.offset)?;
    let more = listings.len() as u32 == PAGE;
    Ok(Json(json!({ "listings": listings, "more": more })).into_response())
}

/// One listing. A closed one is shown only to its seller.
async fn listing(State(app): State<Arc<App>>, headers: HeaderMap, Path(id): Path<i64>) -> Answer {
    let viewer = app.signed_in(&headers).ok();
    let store = app.store.lock().unwrap();
    let listing = store.listing(id)?.ok_or(Failure::NotFound)?;
    if !listing.open && viewer != Some(listing.seller) {
        return Err(Failure::NotFound);
    }
    let seller = store.account(listing.seller)?.ok_or(Failure::NotFound)?;
    Ok(Json(json!({ "listing": listing, "seller": seller, "mine": viewer == Some(listing.seller) })).into_response())
}

async fn post_listing(State(app): State<Arc<App>>, headers: HeaderMap, Json(draft): Json<Draft>) -> Answer {
    let seller = app.signed_in(&headers)?;
    let draft = screened_listing(&app, draft).await?;
    let listing = app.store.lock().unwrap().post(seller, &draft).map_err(refused)?;
    Ok((StatusCode::CREATED, Json(json!({ "listing": listing }))).into_response())
}

async fn edit_listing(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Path(id): Path<i64>,
    Json(draft): Json<Draft>,
) -> Answer {
    let seller = app.signed_in(&headers)?;
    let draft = screened_listing(&app, draft).await?;
    let listing = app.store.lock().unwrap().edit(seller, id, &draft).map_err(refused)?;
    Ok(Json(json!({ "listing": listing })).into_response())
}

#[derive(Deserialize)]
struct Open {
    open: bool,
}

async fn open_listing(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Path(id): Path<i64>,
    Json(request): Json<Open>,
) -> Answer {
    let seller = app.signed_in(&headers)?;
    app.store.lock().unwrap().set_open(seller, id, request.open).map_err(refused)?;
    Ok(Json(json!({})).into_response())
}

impl App {
    /// For tests: a fresh sign-in code for `email`, as if it had been sent.
    #[doc(hidden)]
    pub fn code_for_tests(&self, email: &str) -> String {
        let store = self.store.lock().unwrap();
        store.forget_codes_for_tests();
        store.issue_code(email).unwrap().unwrap()
    }
}

/// For tests: signs an account in without a passkey.
#[doc(hidden)]
pub fn sign_in_for_tests(app: &App, name: &str) -> String {
    let store = app.store.lock().unwrap();
    let account = store.create_account(name, &Uuid::new_v4().to_string()).unwrap();
    // A verified wallet of its own and a way to be reached, so it can post listings.
    store.set_wallet(account.id, &hex::encode(Uuid::new_v4().as_bytes()).repeat(2)).unwrap();
    let reach = store::Contact { email: format!("{}@example.com", name.to_lowercase().replace(' ', "")), ..Default::default() };
    store.set_profile(account.id, name, "", &reach).unwrap();
    store.start_session(account.id).unwrap()
}
