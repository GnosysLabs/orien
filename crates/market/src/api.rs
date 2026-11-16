//! What the marketplace pages ask the server, under /api/market.
//!
//! Signing in uses passkeys: the browser proves it holds an account's
//! passkey, and the server answers with a cookie that keeps it signed in.
//! Changes need that cookie; reading listings does not.

use crate::images::{self, Folder};
use crate::mail::Mailer;
use crate::screen::{Screener, Verdict};
use crate::store::{self, COOKIE_SECONDS, Draft, Store};
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
    /// What the network says about addresses.
    payments: Arc<dyn Payments>,
    /// Bitcoin's price in dollars.
    prices: Arc<crate::prices::Prices>,
    /// The marketplace's own Orien address. Sellers once proved their
    /// wallets by paying it; the dashboard names it among the holders.
    address: String,
    /// Whether the cookie is only sent over https: everywhere but tests.
    secure: bool,
    /// The site's address, for links in emails.
    origin: String,
    /// When contact-form messages were sent, and from which address, to
    /// keep anyone from flooding the inbox.
    contacts: Mutex<Vec<(Instant, String)>>,
    /// The website's traffic counts.
    traffic: Arc<crate::traffic::Traffic>,
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
        let site = origin.trim_end_matches('/').to_string();
        let origin = Url::parse(origin)?;
        let id = origin.host_str().ok_or_else(|| anyhow::anyhow!("the origin has no host"))?.to_string();
        let webauthn = WebauthnBuilder::new(&id, &origin)?.rp_name("Orien Market").build()?;
        Ok(App {
            store: Mutex::new(store),
            webauthn,
            ceremonies: Mutex::new(HashMap::new()),
            mailer: Arc::new(mailer),
            screener: Arc::new(screener),
            images,
            payments,
            prices: Arc::new(crate::prices::Prices::live()),
            address,
            secure,
            origin: site,
            contacts: Mutex::new(Vec::new()),
            traffic: Arc::new(crate::traffic::Traffic::in_memory()),
        })
    }

    /// Keeps the website's traffic counts in this folder.
    pub fn with_traffic(mut self, traffic: crate::traffic::Traffic) -> App {
        self.traffic = Arc::new(traffic);
        self
    }

    pub(crate) fn traffic(&self) -> &crate::traffic::Traffic {
        &self.traffic
    }

    /// Uses these prices instead of fetching them: for tests.
    pub fn with_prices(mut self, prices: crate::prices::Prices) -> App {
        self.prices = Arc::new(prices);
        self
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

    pub(crate) fn store(&self) -> std::sync::MutexGuard<'_, Store> {
        self.store.lock().unwrap()
    }

    pub(crate) fn own_address(&self) -> &str {
        &self.address
    }

    pub(crate) fn payments(&self) -> Arc<dyn Payments> {
        self.payments.clone()
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
    /// Not allowed here at all, such as the dashboard off the tailnet.
    Forbidden(String),
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
            Failure::Forbidden(message) => (StatusCode::FORBIDDEN, message),
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
        .route(
            "/api/market/me/banner",
            post(set_banner).delete(remove_banner).layer(axum::extract::DefaultBodyLimit::max(images::MAX_UPLOAD)),
        )
        .route("/api/market/me/wallet", get(wallet).post(link_wallet))
        .route("/api/market/me/wallets", get(my_wallets).put(report_wallets))
        .route("/api/market/me/delete", post(delete_me))
        .route("/api/market/profiles/{id}", get(profile))
        .route("/api/market/listings/{id}/messages", post(message_seller))
        .route(
            "/api/market/listings/{id}/pictures",
            post(add_listing_picture).layer(axum::extract::DefaultBodyLimit::max(images::MAX_UPLOAD)),
        )
        .route("/api/market/listings/{id}/pictures/{name}", axum::routing::delete(remove_listing_picture).post(make_cover))
        .route("/api/market/swaps", get(open_swaps).post(post_swap))
        .route("/api/market/swaps/mine", get(my_swaps))
        .route("/api/market/swaps/{id}/take", post(take_swap))
        .route("/api/market/swaps/{id}/close", post(close_swap))
        .route("/api/market/swaps/{id}/offer", post(swap_offer_code))
        .route("/api/market/swaps/{id}/run", post(run_swap))
        .route("/api/market/swaps/{id}/reply", post(swap_reply_code))
        .route("/api/market/swaps/{id}/finish", post(finish_swap))
        .route("/api/market/reports", post(report))
        .route("/api/market/contact", post(contact))
        .route("/api/market/t", post(ping))
        .route("/api/market/push/register", post(crate::push::register))
        .route("/api/market/push/unregister", post(crate::push::unregister))
        .route("/api/market/conversations", get(conversations))
        .route("/api/market/conversations/{id}", get(conversation))
        .route("/api/market/conversations/{id}/messages", post(reply))
        .route("/api/market/conversations/{id}/block", post(block))
        .route("/api/market/conversations/{id}/request", post(request_payment))
        .route("/api/market/conversations/{id}/paid", post(mark_paid))
        .route("/api/market/images/{name}", get(picture))
        .route("/api/market/vault/{id}", get(read_vault).put(write_vault))
        .route("/api/market/listings", get(browse).post(post_listing))
        .route("/api/market/listings/{id}", get(listing).put(edit_listing))
        .route("/api/market/listings/{id}/open", post(open_listing))
        .with_state(app)
}

#[derive(Deserialize)]
struct PingBody {
    #[serde(default)]
    t: String,
    #[serde(default)]
    p: String,
    #[serde(default)]
    r: String,
}

/// A page's traffic ping. Always answers 204; anything odd is dropped.
async fn ping(State(app): State<Arc<App>>, headers: HeaderMap, body: axum::body::Bytes) -> StatusCode {
    let header = |name: &str| headers.get(name).and_then(|v| v.to_str().ok()).unwrap_or("").trim().to_string();
    if let Ok(body) = serde_json::from_slice::<PingBody>(&body) {
        let address = [header("cf-connecting-ip"), header("x-forwarded-for").split(',').next().unwrap_or("").trim().to_string()]
            .into_iter()
            .find(|a| !a.is_empty())
            .unwrap_or_else(|| "unknown".into());
        let country = Some(header("cf-ipcountry")).filter(|c| !c.is_empty());
        app.traffic.record(crate::traffic::Ping {
            kind: body.t,
            path: body.p,
            referrer: body.r,
            address,
            browser: header("user-agent"),
            country,
            host: header("host"),
        });
    }
    StatusCode::NO_CONTENT
}

/// Contact-form messages one address may send in an hour.
const CONTACTS_PER_ADDRESS: usize = 3;
/// Contact-form messages everyone together may send in an hour.
const CONTACTS_PER_HOUR: usize = 40;
/// The longest contact-form message, in characters.
const MAX_CONTACT: usize = 5000;

#[derive(Deserialize)]
struct Contact {
    name: String,
    email: String,
    message: String,
    /// A field people never see: only programs fill it in.
    #[serde(default)]
    website: String,
}

/// A message from the site's contact form, emailed to hello@orien.network.
async fn contact(State(app): State<Arc<App>>, Json(request): Json<Contact>) -> Answer {
    let name = request.name.trim().to_string();
    let message = request.message.trim().to_string();
    if name.is_empty() || name.chars().count() > 100 {
        return Err(Failure::Refused("a name is 1 to 100 characters".into()));
    }
    let email = store::email(&request.email).map_err(refused)?;
    if message.chars().count() < 10 {
        return Err(Failure::Refused("write a little more, at least 10 characters".into()));
    }
    if message.chars().count() > MAX_CONTACT {
        return Err(Failure::Refused(format!("a message is at most {MAX_CONTACT} characters")));
    }
    // A program filled in the hidden field: it is told it worked.
    if !request.website.trim().is_empty() {
        return Ok(Json(json!({ "sent": true })).into_response());
    }
    {
        let mut sent = app.contacts.lock().unwrap();
        let hour = std::time::Duration::from_secs(3600);
        sent.retain(|(at, _)| at.elapsed() < hour);
        if sent.iter().filter(|(_, from)| *from == email).count() >= CONTACTS_PER_ADDRESS || sent.len() >= CONTACTS_PER_HOUR {
            return Err(Failure::Refused("too many messages for now; try again in an hour".into()));
        }
        sent.push((Instant::now(), email.clone()));
    }
    let mailer = app.mailer.clone();
    let sent = tokio::task::spawn_blocking(move || mailer.send_contact(&name, &email, &message))
        .await
        .map_err(|e| Failure::Broken(e.into()))?;
    if let Err(error) = sent {
        eprintln!("cannot send a contact message: {error:#}");
        return Err(Failure::Unavailable("the message could not be sent; try again in a few minutes".into()));
    }
    Ok(Json(json!({ "sent": true })).into_response())
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
    let sent = tokio::task::spawn_blocking(move || mailer.send_code(&to, &code)).await.map_err(|e| Failure::Broken(e.into()))?;
    if let Err(error) = sent {
        eprintln!("cannot send a sign-in code: {error:#}");
        // The code was not sent, so asking again may send one at once.
        app.store.lock().unwrap().forget_code(&email)?;
        return Err(Failure::Refused(
            "the code could not be sent to that address; check it, or try again in a few minutes".into(),
        ));
    }
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
        if let Some(id) = existing
            && store.is_suspended(id)?
        {
            return Err(Failure::Refused("this account is suspended".into()));
        }
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
    response.headers_mut().insert(header::SET_COOKIE, app.session_cookie(&token, COOKIE_SECONDS));
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
    response.headers_mut().insert(header::SET_COOKIE, app.session_cookie(&token, COOKIE_SECONDS));
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
    let unknown = || Failure::Refused("that passkey does not belong to a Market account".into());
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
    response.headers_mut().insert(header::SET_COOKIE, app.session_cookie(&token, COOKIE_SECONDS));
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
    let unread = store.unread(id)?;
    Ok(Json(json!({ "account": account, "listings": listings, "unread": unread })).into_response())
}

#[derive(Deserialize)]
struct Write {
    body: String,
}

/// Sends a message once it may be sent and the model has read it, and
/// emails the other side if they should be told.
async fn deliver(app: &App, conversation: i64, sender: i64, body: &str) -> Answer {
    let body = Store::message_text(body).map_err(refused)?;
    app.store.lock().unwrap().may_send(conversation, sender).map_err(refused)?;
    let checked = body.clone();
    screened(app, "message", move |s| s.message(&checked)).await?;
    let (message, notice) = app.store.lock().unwrap().send(conversation, sender, &body).map_err(refused)?;
    Ok(sent(app, conversation, message, notice))
}

/// Answers with a sent message, and emails the other side if they should be told.
fn sent(app: &App, conversation: i64, message: crate::messages::Message, notice: Option<crate::messages::Notice>) -> Response {
    if let Some(notice) = notice {
        let (mailer, origin) = (app.mailer.clone(), app.origin.clone());
        // An email that fails does not unsend the message.
        tokio::task::spawn_blocking(move || {
            if let Err(error) = mailer.send_notice(&notice, &origin) {
                eprintln!("cannot email a message notice: {error:#}");
            }
        });
    }
    Json(json!({ "conversation": conversation, "message": message })).into_response()
}

#[derive(Deserialize)]
struct Ask {
    amount: u64,
}

async fn request_payment(State(app): State<Arc<App>>, headers: HeaderMap, Path(id): Path<i64>, Json(request): Json<Ask>) -> Answer {
    let seller = app.signed_in(&headers)?;
    let (message, notice) = app.store.lock().unwrap().request_payment(id, seller, request.amount).map_err(refused)?;
    Ok(sent(&app, id, message, notice))
}

#[derive(Deserialize)]
struct Paid {
    request: i64,
}

async fn mark_paid(State(app): State<Arc<App>>, headers: HeaderMap, Path(id): Path<i64>, Json(request): Json<Paid>) -> Answer {
    let buyer = app.signed_in(&headers)?;
    let (message, notice) = app.store.lock().unwrap().mark_paid(id, buyer, request.request).map_err(refused)?;
    Ok(sent(&app, id, message, notice))
}

/// The first message to a listing's seller, or the next one.
async fn message_seller(State(app): State<Arc<App>>, headers: HeaderMap, Path(listing): Path<i64>, Json(request): Json<Write>) -> Answer {
    let buyer = app.signed_in(&headers)?;
    Store::message_text(&request.body).map_err(refused)?;
    let conversation = app.store.lock().unwrap().conversation_about(buyer, listing).map_err(refused)?;
    deliver(&app, conversation, buyer, &request.body).await
}

async fn reply(State(app): State<Arc<App>>, headers: HeaderMap, Path(id): Path<i64>, Json(request): Json<Write>) -> Answer {
    let sender = app.signed_in(&headers)?;
    deliver(&app, id, sender, &request.body).await
}

async fn conversations(State(app): State<Arc<App>>, headers: HeaderMap) -> Answer {
    let id = app.signed_in(&headers)?;
    let conversations = app.store.lock().unwrap().conversations(id)?;
    Ok(Json(json!({ "conversations": conversations })).into_response())
}

async fn conversation(State(app): State<Arc<App>>, headers: HeaderMap, Path(id): Path<i64>) -> Answer {
    let account = app.signed_in(&headers)?;
    let (conversation, messages) = app.store.lock().unwrap().read(id, account).map_err(|_| Failure::NotFound)?;
    Ok(Json(json!({ "conversation": conversation, "messages": messages, "me": account })).into_response())
}

#[derive(Deserialize)]
struct Block {
    block: bool,
}

async fn block(State(app): State<Arc<App>>, headers: HeaderMap, Path(id): Path<i64>, Json(request): Json<Block>) -> Answer {
    let account = app.signed_in(&headers)?;
    app.store.lock().unwrap().block(id, account, request.block).map_err(refused)?;
    Ok(Json(json!({})).into_response())
}

#[derive(Deserialize)]
struct DeleteMe {
    /// The account's name, typed again, so nobody deletes by accident.
    confirm: String,
}

/// Deletes the signed-in account and everything it made.
async fn delete_me(State(app): State<Arc<App>>, headers: HeaderMap, Json(request): Json<DeleteMe>) -> Answer {
    let id = app.signed_in(&headers)?;
    let files = {
        let store = app.store.lock().unwrap();
        let account = store.account(id)?.ok_or(Failure::SignIn)?;
        if request.confirm.trim() != account.name {
            return Err(Failure::Refused("type your name exactly as it shows, to confirm".into()));
        }
        store.delete_account(id).map_err(refused)?
    };
    for file in files {
        app.images.remove(&file);
    }
    let mut response = Json(json!({})).into_response();
    response.headers_mut().insert(header::SET_COOKIE, app.session_cookie("", 0));
    Ok(response)
}

#[derive(Deserialize)]
struct ReportRequest {
    kind: String,
    id: i64,
    #[serde(default)]
    reason: String,
}

/// Someone flags something for the founder to look at.
async fn report(State(app): State<Arc<App>>, headers: HeaderMap, Json(request): Json<ReportRequest>) -> Answer {
    let reporter = app.signed_in(&headers)?;
    app.store.lock().unwrap().report(reporter, &request.kind, request.id, &request.reason).map_err(refused)?;
    Ok(Json(json!({})).into_response())
}

#[derive(Deserialize)]
struct NewSwap {
    orn: u64,
    sats: u64,
    /// The Bitcoin address to be paid at, and the wallet the ORN come from,
    /// so any of the seller's wallets can run the swap.
    #[serde(default)]
    payout: Option<String>,
    #[serde(default)]
    wallet: Option<String>,
}

#[derive(Deserialize)]
struct OfferCode {
    code: String,
    #[serde(default)]
    device: Option<String>,
}

#[derive(Deserialize)]
struct Run {
    device: String,
}

#[derive(Deserialize)]
struct Code {
    code: String,
}

#[derive(Deserialize)]
struct Finish2 {
    done: bool,
}

async fn open_swaps(State(app): State<Arc<App>>) -> Answer {
    let offers = app.store.lock().unwrap().open_swaps()?;
    let prices = app.prices.clone();
    let price = tokio::task::spawn_blocking(move || prices.bitcoin()).await.map_err(|e| Failure::Broken(e.into()))?;
    let usd = price.map(|p| json!({ "usd": p.usd, "at": p.at }));
    Ok(Json(json!({ "offers": offers, "bitcoin": usd })).into_response())
}

async fn post_swap(State(app): State<Arc<App>>, headers: HeaderMap, Json(request): Json<NewSwap>) -> Answer {
    let seller = app.signed_in(&headers)?;
    let offer = app
        .store
        .lock()
        .unwrap()
        .post_swap_for(seller, request.orn, request.sats, request.payout.as_deref(), request.wallet.as_deref())
        .map_err(refused)?;
    Ok((StatusCode::CREATED, Json(json!({ "offer": offer }))).into_response())
}

async fn my_swaps(State(app): State<Arc<App>>, headers: HeaderMap) -> Answer {
    let account = app.signed_in(&headers)?;
    let swaps = app.store.lock().unwrap().my_swaps(account)?;
    Ok(Json(json!({ "swaps": swaps })).into_response())
}

#[derive(Deserialize, Default)]
struct Take {
    /// The Orien address to lock the ORN for: the buyer's wallet sends its
    /// own. Without it, the account's verified wallet, if it has one.
    #[serde(default)]
    address: Option<String>,
}

async fn take_swap(State(app): State<Arc<App>>, headers: HeaderMap, Path(id): Path<i64>, body: Option<Json<Take>>) -> Answer {
    let buyer = app.signed_in(&headers)?;
    let request = body.map(|Json(b)| b).unwrap_or_default();
    let (swap, seller_email, taker) = {
        let store = app.store.lock().unwrap();
        let address = match request.address {
            Some(address) => address,
            None => store.account(buyer)?.and_then(|a| a.wallet).ok_or_else(|| {
                Failure::Refused("take this offer in Orien Wallet, which sends where the ORN should go".into())
            })?,
        };
        let swap = store.take_swap(buyer, id, &address).map_err(refused)?;
        let email = store.email_of(swap.offer.seller)?;
        let taker = store.account(buyer)?.map(|a| a.name).unwrap_or_default();
        (swap, email, taker)
    };
    if let Some(to) = seller_email {
        let (mailer, origin) = (app.mailer.clone(), app.origin.clone());
        tokio::task::spawn_blocking(move || {
            if let Err(error) = mailer.send_swap_taken(&to, &taker, &origin) {
                eprintln!("cannot email a swap notice: {error:#}");
            }
        });
    }
    Ok(Json(json!({ "swap": swap })).into_response())
}

async fn close_swap(State(app): State<Arc<App>>, headers: HeaderMap, Path(id): Path<i64>) -> Answer {
    let seller = app.signed_in(&headers)?;
    app.store.lock().unwrap().close_swap(seller, id).map_err(refused)?;
    Ok(Json(json!({})).into_response())
}

async fn swap_offer_code(State(app): State<Arc<App>>, headers: HeaderMap, Path(id): Path<i64>, Json(request): Json<OfferCode>) -> Answer {
    let seller = app.signed_in(&headers)?;
    let swap = app.store.lock().unwrap().post_offer_code_from(seller, id, &request.code, request.device.as_deref()).map_err(refused)?;
    Ok(Json(json!({ "swap": swap })).into_response())
}

/// One of the seller's devices asks to run a taken offer: whether it may.
async fn run_swap(State(app): State<Arc<App>>, headers: HeaderMap, Path(id): Path<i64>, Json(request): Json<Run>) -> Answer {
    let seller = app.signed_in(&headers)?;
    let run = app.store.lock().unwrap().run_swap(seller, id, &request.device).map_err(refused)?;
    Ok(Json(json!({ "run": run })).into_response())
}

async fn swap_reply_code(State(app): State<Arc<App>>, headers: HeaderMap, Path(id): Path<i64>, Json(request): Json<Code>) -> Answer {
    let buyer = app.signed_in(&headers)?;
    let swap = app.store.lock().unwrap().post_reply_code(buyer, id, &request.code).map_err(refused)?;
    Ok(Json(json!({ "swap": swap })).into_response())
}

async fn finish_swap(State(app): State<Arc<App>>, headers: HeaderMap, Path(id): Path<i64>, Json(request): Json<Finish2>) -> Answer {
    let account = app.signed_in(&headers)?;
    app.store.lock().unwrap().finish_swap(account, id, request.done).map_err(refused)?;
    Ok(Json(json!({})).into_response())
}

#[derive(Deserialize)]
struct WalletLink {
    address: String,
    /// Sent by Orien Wallet, which names its own address, so it cannot be
    /// mistyped and may be new.
    #[serde(default)]
    app: bool,
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

/// Links the wallet buyers pay. Nothing is proven: naming someone else's
/// wallet only pays them. An address typed on the website must have held
/// ORN before, which a mistyped one never has.
async fn link_wallet(State(app): State<Arc<App>>, headers: HeaderMap, Json(request): Json<WalletLink>) -> Answer {
    let id = app.signed_in(&headers)?;
    let address = store::orien_address(&request.address).map_err(refused)?;
    if !request.app {
        let account: orien_ledger::AccountId = address.parse().map_err(|_| refused(anyhow::anyhow!("that is not an Orien address")))?;
        if !network(&app, move |p| p.has_held(&account)).await? {
            return Err(refused(anyhow::anyhow!(
                "that address has never held any ORN, so it may have a typo; check it, or link your wallet by signing in from Orien Wallet"
            )));
        }
    }
    app.store.lock().unwrap().set_wallet(id, &address).map_err(refused)?;
    Ok(Json(json!({ "wallet": address })).into_response())
}

#[derive(Deserialize)]
struct ReportedWallet {
    address: String,
    #[serde(default)]
    name: String,
}

#[derive(Deserialize)]
struct ReportedWallets {
    device: String,
    #[serde(default)]
    wallets: Vec<ReportedWallet>,
}

/// Orien Wallet says which wallets this device holds, by name.
async fn report_wallets(State(app): State<Arc<App>>, headers: HeaderMap, Json(request): Json<ReportedWallets>) -> Answer {
    let id = app.signed_in(&headers)?;
    let wallets: Vec<(String, String)> = request.wallets.into_iter().map(|w| (w.address, w.name)).collect();
    let kept = app.store.lock().unwrap().report_wallets(id, &request.device, &wallets).map_err(refused)?;
    Ok(Json(json!({ "wallets": kept })).into_response())
}

/// The account's wallets, by name, for "Sell from".
async fn my_wallets(State(app): State<Arc<App>>, headers: HeaderMap) -> Answer {
    let id = app.signed_in(&headers)?;
    let (wallets, reported) = {
        let store = app.store.lock().unwrap();
        (store.account_wallets(id)?, store.reported_wallets(id)?)
    };
    // Only wallets new enough to report themselves can run an offer posted
    // here, so the website offers posting only when one has.
    Ok(Json(json!({ "wallets": wallets, "reported": reported })).into_response())
}

/// The linked wallet.
async fn wallet(State(app): State<Arc<App>>, headers: HeaderMap) -> Answer {
    let id = app.signed_in(&headers)?;
    let account = app.store.lock().unwrap().account(id)?.ok_or(Failure::SignIn)?;
    Ok(Json(json!({ "wallet": account.wallet })).into_response())
}

/// A wallet's vault: sealed bytes, base64.
async fn read_vault(State(app): State<Arc<App>>, Path(id): Path<String>) -> Answer {
    let found = app.store().vault(&id).map_err(refused)?.ok_or(Failure::NotFound)?;
    use base64::Engine;
    let blob = base64::engine::general_purpose::STANDARD.encode(&found.0);
    Ok(Json(json!({ "blob": blob, "updated": found.1 })).into_response())
}

#[derive(Deserialize)]
struct VaultWrite {
    token: String,
    blob: String,
}

async fn write_vault(State(app): State<Arc<App>>, Path(id): Path<String>, Json(body): Json<VaultWrite>) -> Answer {
    use base64::Engine;
    let blob = base64::engine::general_purpose::STANDARD
        .decode(body.blob.as_bytes())
        .map_err(|_| Failure::Refused("the vault's contents are not base64".into()))?;
    app.store().put_vault(&id, &body.token, &blob).map_err(refused)?;
    Ok(Json(json!({ "ok": true })).into_response())
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

/// A new banner for the profile, sent as the request's whole body. Checked
/// like a profile picture.
async fn set_banner(State(app): State<Arc<App>>, headers: HeaderMap, body: axum::body::Bytes) -> Answer {
    let id = app.signed_in(&headers)?;
    let jpeg = tokio::task::spawn_blocking(move || images::banner(&body))
        .await
        .map_err(|e| Failure::Broken(e.into()))?
        .map_err(refused)?;
    let checked = jpeg.clone();
    screened(&app, "picture", move |s| s.picture(&checked)).await?;
    let name = app.images.save(&jpeg)?;
    if let Some(old) = app.store.lock().unwrap().set_banner(id, Some(&name))? {
        app.images.remove(&old);
    }
    let account = app.store.lock().unwrap().account(id)?.ok_or(Failure::SignIn)?;
    Ok(Json(json!({ "account": account })).into_response())
}

async fn remove_banner(State(app): State<Arc<App>>, headers: HeaderMap) -> Answer {
    let id = app.signed_in(&headers)?;
    if let Some(old) = app.store.lock().unwrap().set_banner(id, None)? {
        app.images.remove(&old);
    }
    let account = app.store.lock().unwrap().account(id)?.ok_or(Failure::SignIn)?;
    Ok(Json(json!({ "account": account })).into_response())
}

/// A new picture for a listing, sent as the request's whole body.
async fn add_listing_picture(State(app): State<Arc<App>>, headers: HeaderMap, Path(id): Path<i64>, body: axum::body::Bytes) -> Answer {
    let seller = app.signed_in(&headers)?;
    let listing = app.store.lock().unwrap().listing(id)?.filter(|l| l.seller == seller).ok_or(Failure::NotFound)?;
    if listing.pictures.len() as i64 >= store::MAX_LISTING_PICTURES {
        return Err(Failure::Refused(format!("a listing has at most {} pictures", store::MAX_LISTING_PICTURES)));
    }
    let jpeg = tokio::task::spawn_blocking(move || images::listing_picture(&body))
        .await
        .map_err(|e| Failure::Broken(e.into()))?
        .map_err(refused)?;
    let (checked, title) = (jpeg.clone(), listing.title.clone());
    screened(&app, "picture", move |s| s.listing_picture(&checked, &title)).await?;
    let name = app.images.save(&jpeg)?;
    if let Err(error) = app.store.lock().unwrap().add_listing_picture(seller, id, &name) {
        app.images.remove(&name);
        return Err(refused(error));
    }
    let listing = app.store.lock().unwrap().listing(id)?.ok_or(Failure::NotFound)?;
    Ok(Json(json!({ "listing": listing })).into_response())
}

async fn remove_listing_picture(State(app): State<Arc<App>>, headers: HeaderMap, Path((id, name)): Path<(i64, String)>) -> Answer {
    let seller = app.signed_in(&headers)?;
    app.store.lock().unwrap().remove_listing_picture(seller, id, &name).map_err(refused)?;
    app.images.remove(&name);
    let listing = app.store.lock().unwrap().listing(id)?.ok_or(Failure::NotFound)?;
    Ok(Json(json!({ "listing": listing })).into_response())
}

async fn make_cover(State(app): State<Arc<App>>, headers: HeaderMap, Path((id, name)): Path<(i64, String)>) -> Answer {
    let seller = app.signed_in(&headers)?;
    app.store.lock().unwrap().make_cover(seller, id, &name).map_err(refused)?;
    let listing = app.store.lock().unwrap().listing(id)?.ok_or(Failure::NotFound)?;
    Ok(Json(json!({ "listing": listing })).into_response())
}

/// A seller's public profile: who they are and what they offer.
async fn profile(State(app): State<Arc<App>>, Path(id): Path<i64>) -> Answer {
    let store = app.store.lock().unwrap();
    if store.is_suspended(id)? {
        return Err(Failure::NotFound);
    }
    let account = store.account(id)?.ok_or(Failure::NotFound)?;
    let listings = store.open_listings_of(id)?;
    let swaps_done = store.swaps_done(id)?;
    Ok(Json(json!({ "account": account, "listings": listings, "swaps_done": swaps_done })).into_response())
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
    let mine = viewer == Some(listing.seller);
    Ok(Json(json!({ "listing": listing, "seller": seller, "mine": mine, "signed_in": viewer.is_some() })).into_response())
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
