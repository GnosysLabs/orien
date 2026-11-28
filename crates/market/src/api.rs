//! What the marketplace pages ask the server, under /api/market.
//!
//! Signing in uses a code sent by email; the server answers with a cookie
//! that keeps the browser signed in. Changes need that cookie; reading
//! listings does not.

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
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::time::Instant;

/// The cookie that keeps a browser signed in.
pub const COOKIE: &str = "orien_market";

pub struct App {
    store: Mutex<Store>,
    mailer: Arc<Mailer>,
    screener: Arc<Screener>,
    pub(crate) images: Folder,
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
    /// When problem reports came from Orien Wallet, and from which
    /// internet address, to keep anyone from flooding them.
    pub(crate) app_reports_sent: Mutex<Vec<(Instant, String)>>,
    /// The website's traffic counts.
    traffic: Arc<crate::traffic::Traffic>,
    /// The oldest swap rules a buyer's wallet may take offers with.
    min_swap_version: u32,
    /// Market events waiting for the push watcher to tell phones about.
    events: Mutex<Vec<crate::push::Event>>,
    /// The last notice phones were told of; below zero until the first look.
    pub(crate) pushed_notice: std::sync::atomic::AtomicI64,
    /// Pairing mailboxes, for moving a wallet between someone's devices.
    pairs: crate::pair::Mailboxes,
    /// The App Store reviewers' email and its fixed code, if set.
    review: Option<Review>,
    /// What the explorer pages have already asked the network.
    explorer: crate::explorer::Explorer,
    /// Who may register websites for Sign in with Orien.
    sign_in: crate::oidc::Gate,
    /// The community's leaderboards, kept current by its watcher.
    community: crate::community::Community,
    /// What sellers' wallets could spend, and when that was asked.
    spendable: Mutex<HashMap<String, (std::time::Instant, u64)>>,
    /// Wallets being asked about in the background now.
    asking: Mutex<HashSet<String>>,
    /// The KLIPY key the apps search GIFs with, from the user's own
    /// device as KLIPY requires; empty when GIFs are off.
    klipy_key: String,
    /// Where the wallet's Solana requests go, with its key: never shown.
    solana_relay: Option<String>,
    /// Problems told to the founder in Discord's #network-alerts.
    alerts: crate::alerts::Alerts,
    /// Where Orien AI Support is answered: the Discord bot's expert, such
    /// as http://127.0.0.1:7300/support. None: it isn't available.
    support: Option<String>,
    /// Solana `orien-order` reader & relayer for funded buy orders.
    order_chain: Option<Arc<dyn crate::orders::OrderChain>>,
    /// Solana `orien-book` reader & filler for buy orders that fill in parts.
    book_chain: Option<Arc<dyn crate::books::BookChain>>,
    /// Per-order SOL limit (in lamports) for funded buy orders.
    max_order_lamports: u64,
}

/// One email, for App Store review, whose sign-in code never changes and is
/// never emailed. Wrong codes for it are counted, so it can't be guessed.
struct Review {
    email: String,
    code: String,
    misses: Mutex<Vec<Instant>>,
}

/// Wrong codes allowed for the review email in an hour.
const REVIEW_MISSES: usize = 10;

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
        anyhow::ensure!(url::Url::parse(origin)?.host_str().is_some(), "the origin has no host");
        Ok(App {
            store: Mutex::new(store),
            mailer: Arc::new(mailer),
            screener: Arc::new(screener),
            images,
            payments,
            prices: Arc::new(crate::prices::Prices::live()),
            address,
            secure,
            origin: site,
            contacts: Mutex::new(Vec::new()),
            app_reports_sent: Mutex::new(Vec::new()),
            traffic: Arc::new(crate::traffic::Traffic::in_memory()),
            min_swap_version: 0,
            events: Mutex::new(Vec::new()),
            pushed_notice: std::sync::atomic::AtomicI64::new(-1),
            pairs: crate::pair::Mailboxes::default(),
            review: None,
            explorer: crate::explorer::Explorer::default(),
            sign_in: crate::oidc::Gate::default(),
            community: crate::community::Community::default(),
            spendable: Mutex::new(HashMap::new()),
            asking: Mutex::new(HashSet::new()),
            klipy_key: String::new(),
            solana_relay: None,
            alerts: crate::alerts::Alerts::default(),
            support: None,
            order_chain: None,
            book_chain: None,
            max_order_lamports: crate::orders::DEFAULT_MAX_ORDER_LAMPORTS,
        })
    }

    /// Lets `email` sign in with the fixed `code`, for App Store review.
    pub fn with_review(mut self, email: &str, code: &str) -> anyhow::Result<App> {
        let code = code.trim().to_string();
        anyhow::ensure!(code.len() == 6 && code.bytes().all(|b| b.is_ascii_digit()), "the review code is 6 digits");
        self.review = Some(Review { email: store::email(email)?, code, misses: Mutex::new(Vec::new()) });
        Ok(self)
    }

    fn is_review(&self, email: &str) -> bool {
        self.review.as_ref().is_some_and(|r| r.email == email)
    }

    /// Checks a sign-in code: the review email's fixed one, or one emailed.
    fn check_code(&self, store: &Store, email: &str, code: &str, consume: bool) -> Result<(), Failure> {
        let Some(review) = self.review.as_ref().filter(|r| r.email == email) else {
            return store.check_code(email, code, consume).map_err(refused);
        };
        let mut misses = review.misses.lock().unwrap();
        misses.retain(|at| at.elapsed() < std::time::Duration::from_secs(3600));
        if misses.len() >= REVIEW_MISSES {
            return Err(Failure::Refused("too many wrong codes; try again in an hour".into()));
        }
        let given = code.trim().as_bytes();
        let same = given.len() == review.code.len() && given.iter().zip(review.code.as_bytes()).fold(0, |d, (a, b)| d | (a ^ b)) == 0;
        if !same {
            misses.push(Instant::now());
            return Err(Failure::Refused("that code is not right; check it and try again".into()));
        }
        Ok(())
    }

    /// Turns away takes from wallets on swap rules older than `version`.
    /// GIFs in the community composer, searched with this KLIPY key.
    /// Where to send alerts: the Discord bot's inbox, such as
    /// http://127.0.0.1:7301/alert.
    pub fn with_alerts(mut self, url: Option<String>) -> App {
        self.alerts = crate::alerts::Alerts::to(url);
        self
    }

    /// Where Orien AI Support is answered (see `support`).
    pub fn with_support(mut self, url: Option<String>) -> App {
        self.support = url.filter(|u| !u.trim().is_empty());
        self
    }

    pub(crate) fn support_url(&self) -> Option<String> {
        self.support.clone()
    }

    pub fn alerts(&self) -> &crate::alerts::Alerts {
        &self.alerts
    }

    pub fn with_klipy_key(mut self, key: String) -> App {
        self.klipy_key = key;
        self
    }

    pub fn with_solana_relay(mut self, upstream: Option<String>) -> App {
        self.solana_relay = upstream.filter(|url| !url.is_empty());
        self
    }

    pub(crate) fn solana_relay(&self) -> Option<&str> {
        self.solana_relay.as_deref()
    }

    pub(crate) fn klipy_key(&self) -> &str {
        &self.klipy_key
    }

    pub fn with_min_swap_version(mut self, version: u32) -> App {
        self.min_swap_version = version;
        self
    }

    pub fn with_order_chain(mut self, chain: Arc<dyn crate::orders::OrderChain>) -> App {
        self.order_chain = Some(chain);
        self
    }

    pub fn with_max_order_lamports(mut self, lamports: u64) -> App {
        self.max_order_lamports = lamports;
        self
    }

    pub fn with_book_chain(mut self, chain: Arc<dyn crate::books::BookChain>) -> App {
        self.book_chain = Some(chain);
        self
    }

    pub(crate) fn book_chain(&self) -> Option<Arc<dyn crate::books::BookChain>> {
        self.book_chain.clone()
    }

    pub(crate) fn order_chain(&self) -> Option<Arc<dyn crate::orders::OrderChain>> {
        self.order_chain.clone()
    }

    pub(crate) fn store_lock(&self) -> std::sync::MutexGuard<'_, Store> {
        self.store.lock().unwrap()
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

    /// Who may register websites for Sign in with Orien: everyone once it
    /// is open, else only these account names.
    pub fn with_sign_in(mut self, open: bool, developers: Vec<String>) -> App {
        self.sign_in = crate::oidc::Gate { open, developers: developers.into_iter().map(|n| n.to_lowercase()).collect() };
        self
    }

    pub(crate) fn sign_in_gate(&self) -> &crate::oidc::Gate {
        &self.sign_in
    }

    pub(crate) fn community(&self) -> &crate::community::Community {
        &self.community
    }

    pub(crate) fn origin(&self) -> &str {
        &self.origin
    }

    /// A path on this site as a whole address, such as for pictures.
    pub(crate) fn absolute(&self, path: &str) -> String {
        format!("{}{path}", self.origin.trim_end_matches('/'))
    }

    pub(crate) fn store(&self) -> std::sync::MutexGuard<'_, Store> {
        self.store.lock().unwrap()
    }

    pub(crate) fn own_address(&self) -> &str {
        &self.address
    }

    /// The Market's own Orien address.
    pub(crate) fn address(&self) -> &str {
        &self.address
    }

    pub(crate) fn screener(&self) -> Arc<Screener> {
        self.screener.clone()
    }

    pub(crate) fn images(&self) -> &Folder {
        &self.images
    }

    pub(crate) fn mailer(&self) -> &Mailer {
        &self.mailer
    }

    pub(crate) fn explorer(&self) -> &crate::explorer::Explorer {
        &self.explorer
    }

    pub(crate) fn payments(&self) -> Arc<dyn Payments> {
        self.payments.clone()
    }

    #[doc(hidden)]
    pub fn pairs(&self) -> &crate::pair::Mailboxes {
        &self.pairs
    }

    /// Queues a notification for the phones of `event.account`.
    pub(crate) fn tell(&self, event: crate::push::Event) {
        let mut events = self.events.lock().unwrap();
        // Bounded, so a stalled watcher can't fill the memory.
        if events.len() < 10_000 {
            events.push(event);
        }
    }

    pub(crate) fn take_events(&self) -> Vec<crate::push::Event> {
        std::mem::take(&mut *self.events.lock().unwrap())
    }

    pub(crate) fn signed_in(&self, headers: &HeaderMap) -> Result<i64, Failure> {
        let token = cookie(headers).ok_or(Failure::SignIn)?;
        self.store.lock().unwrap().session(&token)?.ok_or(Failure::SignIn)
    }

    pub(crate) fn session_cookie(&self, token: &str, max_age: u64) -> HeaderValue {
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
pub(crate) fn refused(error: anyhow::Error) -> Failure {
    Failure::Refused(error.to_string())
}

type Answer = Result<Response, Failure>;

/// Runs a check by the screening model, off the async threads. Anything it
/// refuses, or cannot check, is not saved.
pub(crate) async fn screened(app: &App, what: &str, check: impl FnOnce(&Screener) -> anyhow::Result<Verdict> + Send + 'static) -> Result<(), Failure> {
    let screener = app.screener.clone();
    let verdict = tokio::task::spawn_blocking(move || check(&screener)).await.map_err(|e| Failure::Broken(e.into()))?;
    match &verdict {
        Ok(_) => app.alerts().fine("screening", "Content checks are working again: posts, photos, profiles and listings can be saved."),
        Err(error) => app.alerts().trouble(
            "screening",
            format!("Content checks are failing, so new posts, photos, profiles and listings can't be saved.\nReason: {error:#}"),
        ),
    }
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

/// Every 15 minutes, checks a harmless line, so a broken screening service
/// is noticed (and told in #network-alerts) before someone runs into it.
pub fn watch_screening(app: Arc<App>) {
    tokio::spawn(async move {
        let mut every = tokio::time::interval(std::time::Duration::from_secs(15 * 60));
        loop {
            every.tick().await;
            let _ = screened(&app, "self-check", |s| s.message("Good morning, miners. The network is running well today.")).await;
        }
    });
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
        .route("/api/market/me/wallet/sol-devnet", axum::routing::delete(crate::sol_profile::unlink))
        .route("/api/market/me/wallet/sol-devnet/challenge", post(crate::sol_profile::challenge))
        .route("/api/market/me/wallet/sol-devnet/confirm", post(crate::sol_profile::confirm))
        .route("/api/market/me/wallet/verify", post(verify_wallet))
        .route("/api/market/me/referrals", get(crate::referrals::status).post(crate::referrals::claim))
        .route("/api/market/me/wallet/remove", post(remove_verified_wallet))
        .route("/api/market/wallets/lookup", post(lookup_wallets).options(open_preflight))
        .route("/api/market/wallets/{address}", get(wallet_profile).options(open_preflight))
        .route("/api/market/me/wallets", get(my_wallets).put(report_wallets))
        .route("/api/market/me/delete", post(delete_me))
        .route("/api/market/profiles", get(search_profiles))
        .route("/api/market/profiles/{id}", get(profile))
        .route("/api/market/accounts/{id}/block", post(block_account))
        .route("/api/market/me/blocked", get(my_blocks))
        .route("/api/market/users/{name}", get(user))
        .route("/api/market/users/{name}/messages", post(message_person))
        .route("/api/market/listings/{id}/messages", post(message_seller))
        .route(
            "/api/market/listings/{id}/pictures",
            post(add_listing_picture).layer(axum::extract::DefaultBodyLimit::max(images::MAX_UPLOAD)),
        )
        .route("/api/market/listings/{id}/pictures/{name}", axum::routing::delete(remove_listing_picture).post(make_cover))
        .route("/api/market/swaps", get(open_swaps).post(post_swap))
        .route("/api/market/swaps/history", get(swap_history))
        .route("/api/market/swaps/mine", get(my_swaps))
        .route("/api/market/swaps/{id}/take", post(take_swap))
        .route("/api/market/swaps/{id}/close", post(close_swap))
        .route("/api/market/swaps/{id}/offer", post(swap_offer_code))
        .route("/api/market/swaps/{id}/run", post(run_swap))
        .route("/api/market/swaps/{id}/reply", post(swap_reply_code))
        .route("/api/market/swaps/{id}/finish", post(finish_swap))
        .route("/api/market/orders", post(post_funded_order))
        .route("/api/market/orders/mine", get(my_funded_orders))
        .route("/api/market/orders/{id}/reserve", post(reserve_funded_order))
        .route("/api/market/orders/{id}/fill", post(fill_funded_order))
        .route("/api/market/orders/{id}/sync", post(sync_funded_order))
        .route("/api/market/books", post(post_book_order))
        .route("/api/market/books/mine", get(my_book_orders))
        .route("/api/market/books/{id}/reserve", post(reserve_book_part))
        .route("/api/market/books/parts/{id}/fill", post(fill_book_part))
        .route("/api/market/reports", post(report))
        .route("/api/market/contact", post(contact))
        .route("/api/market/t", post(ping))
        .route("/api/market/push/register", post(crate::push::register))
        .route("/api/market/pair", post(crate::pair::open))
        .route("/api/market/pair/{code}", axum::routing::delete(crate::pair::close))
        .route("/api/market/pair/{code}/{slot}", get(crate::pair::read).put(crate::pair::write))
        .route("/api/market/push/unregister", post(crate::push::unregister))
        .route("/api/market/conversations", get(conversations))
        .route("/api/market/conversations/{id}", get(conversation))
        .route("/api/market/conversations/{id}/messages", post(reply))
        .route("/api/market/conversations/{id}/block", post(block))
        .route("/api/market/conversations/{id}/delete", post(delete_conversation))
        .route("/api/market/conversations/{id}/request", post(request_payment))
        .route("/api/market/conversations/{id}/paid", post(mark_paid))
        .route("/api/market/images/{name}", get(picture))
        .route("/api/market/vault/{id}", get(read_vault).put(write_vault))
        .route(
            "/api/market/vault/{id}/pictures/{picture}",
            get(read_vault_picture)
                .put(write_vault_picture)
                .delete(delete_vault_picture)
                .layer(axum::extract::DefaultBodyLimit::max(3 * 1024 * 1024)),
        )
        .route("/api/market/listings", get(browse).post(post_listing))
        .route("/api/market/listings/{id}", get(listing).put(edit_listing))
        .route("/api/market/listings/{id}/open", post(open_listing))
        .merge(crate::oidc::routes())
        .merge(crate::community::routes())
        .merge(crate::notices::routes())
        .merge(crate::room::routes())
        .merge(crate::app_reports::routes())
        .merge(crate::themes::routes())
        .merge(crate::support::routes())
        .merge(crate::solana_relay::routes())
        .route("/api/explorer/overview", get(crate::explorer::overview))
        .route("/api/explorer/checkpoints", get(crate::explorer::checkpoints))
        .route("/api/explorer/checkpoints/{epoch}", get(crate::explorer::checkpoint))
        .route("/api/explorer/address/{address}", get(crate::explorer::address))
        .route("/api/explorer/top", get(crate::explorer::top))
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
    /// The sender's business, if they gave one.
    #[serde(default)]
    business: String,
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
    let business = request.business.trim().to_string();
    if business.chars().count() > 150 {
        return Err(Failure::Refused("a business name is at most 150 characters".into()));
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
    let sent = tokio::task::spawn_blocking(move || mailer.send_contact(&name, &business, &email, &message))
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
    // The review email's code is fixed and never sent.
    if app.is_review(&email) {
        return Ok(Json(json!({ "sent": email })).into_response());
    }
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
            app.check_code(&store, &email, &request.code, false)?;
            if request.name.trim().is_empty() {
                return Ok(Json(json!({ "needs_name": true })).into_response());
            }
            // Before the code is used up, so a taken name can be changed.
            store.new_name(&request.name, None).map_err(refused)?;
        }
        existing
    };
    if existing.is_none() {
        screened_name(&app, &request.name).await?;
    }
    let store = app.store.lock().unwrap();
    app.check_code(&store, &email, &request.code, true)?;
    let account = match existing {
        Some(id) => store.account(id)?.ok_or(Failure::NotFound)?,
        None => store.create_account_with_email(&request.name, &email).map_err(refused)?,
    };
    let token = store.start_session(account.id)?;
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
    let notices = store.unseen_notices(id)?;
    let room_read = store.room_read(id)?;
    Ok(Json(json!({ "account": account, "listings": listings, "unread": unread, "notices": notices, "room_read": room_read })).into_response())
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
    // The other side's phones hear about every message.
    if let Ok(Some((to, from))) = app.store.lock().unwrap().other_side(conversation, message.sender) {
        app.tell(crate::push::Event {
            account: to,
            address: None,
            body: crate::push::message_text(&from, &message.kind, &message.body, message.amount),
            kind: "message",
            open: json!({ "conversation": conversation }),
        });
    }
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
    let conversation = {
        let store = app.store.lock().unwrap();
        // A block stops a conversation before it is made.
        if let Some(seller) = store.listing(listing)?.map(|l| l.seller)
            && let Some(why) = store.block_between(buyer, seller)?
        {
            return Err(Failure::Refused(why.into()));
        }
        store.conversation_about(buyer, listing).map_err(refused)?
    };
    deliver(&app, conversation, buyer, &request.body).await
}

/// The first message to anyone, by name, or the next one in the direct
/// conversation the two already have. Answers as `message_seller` does.
async fn message_person(State(app): State<Arc<App>>, headers: HeaderMap, Path(name): Path<String>, Json(request): Json<Write>) -> Answer {
    let sender = app.signed_in(&headers)?;
    Store::message_text(&request.body).map_err(refused)?;
    let conversation = app.store.lock().unwrap().direct_conversation(sender, &name).map_err(refused)?;
    deliver(&app, conversation, sender, &request.body).await
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

#[derive(Deserialize, Default)]
struct Peek {
    /// Set by an app loading a conversation ahead: it stays unread.
    peek: Option<u8>,
}

async fn conversation(State(app): State<Arc<App>>, headers: HeaderMap, Path(id): Path<i64>, Query(ask): Query<Peek>) -> Answer {
    let account = app.signed_in(&headers)?;
    let store = app.store.lock().unwrap();
    let opened = if ask.peek == Some(1) { store.peek(id, account) } else { store.read(id, account) };
    let (conversation, messages) = opened.map_err(|_| Failure::NotFound)?;
    Ok(Json(json!({ "conversation": conversation, "messages": messages, "me": account })).into_response())
}

/// Deletes a conversation for the signed-in side only.
async fn delete_conversation(State(app): State<Arc<App>>, headers: HeaderMap, Path(id): Path<i64>) -> Answer {
    let account = app.signed_in(&headers)?;
    app.store.lock().unwrap().delete_conversation(id, account).map_err(|_| Failure::NotFound)?;
    Ok(Json(json!({})).into_response())
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
    let reason: String = request.reason.trim().chars().take(300).collect();
    let why = if reason.is_empty() { String::new() } else { format!(": “{reason}”") };
    let what = if request.kind == "room" { "group chat message" } else { request.kind.as_str() };
    app.alerts().event(format!("🚩 Someone reported a {what} (#{}){why}. It's on the dashboard under Reports.", request.id));
    Ok(Json(json!({})).into_response())
}

#[derive(Deserialize)]
struct NewSwap {
    orn: u64,
    /// "sell" (default) or "buy".
    #[serde(default)]
    side: Option<String>,
    /// "sol" or "btc" (the default, for wallets from before SOL swaps).
    #[serde(default)]
    coin: Option<String>,
    /// The price of a Bitcoin offer.
    #[serde(default)]
    sats: u64,
    /// The price of a SOL offer.
    #[serde(default)]
    lamports: u64,
    /// For sell offers: the SOL or Bitcoin address to be paid at, and the
    /// wallet the ORN come from, so any of the seller's wallets can run the swap.
    #[serde(default)]
    payout: Option<String>,
    #[serde(default)]
    wallet: Option<String>,
    /// The swap rules the poster's wallet follows: 5 from the wallet that
    /// sells part of an order when someone buys part.
    #[serde(default)]
    swap_version: u32,
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

#[derive(Deserialize, Default)]
struct Board {
    /// Which coins' offers, comma-separated: "sol,btc". Bitcoin only when
    /// left out, so wallets from before SOL swaps never see SOL offers.
    #[serde(default)]
    coins: Option<String>,
    /// Which sides' offers, comma-separated: "sell,buy". Sell only when left
    /// out, so wallets from before buy offers never mistake a buy offer for a
    /// sell offer.
    #[serde(default)]
    sides: Option<String>,
}

/// What each of these wallets can spend. A wallet asked about in the last
/// minute answers from memory; an older answer is used while a fresh one is
/// fetched in the background. Wallets never asked are asked all at once
/// (a lookup takes seconds); with `wait` false they are asked in the
/// background instead and left out this time, so the swap board never
/// waits. One the network doesn't answer for is left out, so it counts as
/// able, as before this check.
async fn spendable_of(app: &Arc<App>, wallets: Vec<String>, wait: bool) -> HashMap<String, u64> {
    let app = app.clone();
    tokio::task::spawn_blocking(move || {
        let ask = |app: &App, wallet: &str| -> Option<u64> {
            let n = app.payments.spendable(&wallet.parse::<orien_ledger::AccountId>().ok()?).ok()?;
            app.spendable.lock().unwrap().insert(wallet.to_string(), (std::time::Instant::now(), n));
            Some(n)
        };
        let mut found = HashMap::new();
        let mut missing = Vec::new();
        let mut stale = Vec::new();
        {
            let cache = app.spendable.lock().unwrap();
            for wallet in wallets {
                match cache.get(&wallet) {
                    Some((at, n)) => {
                        found.insert(wallet.clone(), *n);
                        if at.elapsed() >= std::time::Duration::from_secs(60) {
                            stale.push(wallet);
                        }
                    }
                    None => missing.push(wallet),
                }
            }
        }
        if !wait {
            stale.append(&mut missing);
        }
        if !stale.is_empty() {
            // Marked fresh now, so a burst of requests refreshes each once.
            let mut asking = app.asking.lock().unwrap();
            stale.retain(|wallet| asking.insert(wallet.clone()));
            drop(asking);
            let app = app.clone();
            std::thread::spawn(move || {
                std::thread::scope(|scope| {
                    for wallet in &stale {
                        let app = &app;
                        scope.spawn(move || {
                            ask(app, wallet);
                            app.asking.lock().unwrap().remove(wallet);
                        });
                    }
                });
            });
        }
        let fetched: Vec<(String, Option<u64>)> = std::thread::scope(|scope| {
            let handles: Vec<_> = missing
                .into_iter()
                .map(|wallet| {
                    let app = &app;
                    scope.spawn(move || {
                        let n = ask(app, &wallet);
                        (wallet, n)
                    })
                })
                .collect();
            handles.into_iter().filter_map(|h| h.join().ok()).collect()
        });
        for (wallet, n) in fetched {
            if let Some(n) = n {
                found.insert(wallet, n);
            }
        }
        found
    })
    .await
    .unwrap_or_default()
}

/// Whether a wallet is known to hold less than `orn`.
fn short_of(spendable: &HashMap<String, u64>, wallet: &Option<String>, orn: u64) -> bool {
    wallet.as_ref().and_then(|w| spendable.get(w)).is_some_and(|n| *n < orn)
}

async fn open_swaps(State(app): State<Arc<App>>, Query(board): Query<Board>) -> Answer {
    let wanted = board.coins.unwrap_or_else(|| "btc".into());
    let coins: Vec<&str> = wanted.split(',').map(str::trim).filter(|c| matches!(*c, "sol" | "btc")).collect();
    let wanted_sides = board.sides.unwrap_or_else(|| "sell".into());
    let sides: Vec<&str> = wanted_sides.split(',').map(str::trim).filter(|s| matches!(*s, "sell" | "buy")).collect();
    // Sell offers come from swap_offers; buy offers come ONLY from on-chain
    // funded Solana orders (never unfunded buy offers).
    let want_sells = sides.contains(&"sell");
    let want_buys = sides.contains(&"buy") && coins.contains(&"sol");
    let (sell_offers, wallets, funded_buys) = {
        let store = app.store.lock().unwrap();
        let sell_offers = if want_sells { store.open_swaps_in_sides(&coins, &["sell"])? } else { Vec::new() };
        let wallets: Vec<Option<String>> = sell_offers.iter().map(|o| store.offer_wallet(o.id)).collect::<anyhow::Result<_>>()?;
        let funded_buys = if want_buys { store.open_funded_orders()? } else { Vec::new() };
        (sell_offers, wallets, funded_buys)
    };
    // Sell offers whose seller's wallet no longer holds the ORN would only hold
    // a buyer up: they leave the board until it does again.
    let mut asked: Vec<String> = wallets.iter().flatten().cloned().collect();
    asked.sort();
    asked.dedup();
    let spendable = spendable_of(&app, asked, false).await;
    let mut offers: Vec<serde_json::Value> = sell_offers
        .into_iter()
        .zip(wallets)
        .filter(|(offer, wallet)| !short_of(&spendable, wallet, offer.orn))
        .map(|(offer, _)| serde_json::to_value(offer).expect("serialize offer"))
        .collect();
    for o in funded_buys {
        // Funded buy orders never require the buyer's app to be online.
        offers.push(json!({
            "id": o.id,
            "seller": o.buyer,
            "seller_name": o.buyer_name,
            "seller_picture": o.buyer_picture,
            "seller_done": o.buyer_done,
            "orn": o.orn,
            "coin": "sol",
            "sats": 0,
            "lamports": o.lamports,
            "state": o.state,
            "created": o.created,
            "seller_online": true,
            "side": "buy",
            "funded": true,
            "order_pubkey": o.order_pubkey,
            "hash": o.hash,
            "buyer_orien": o.buyer_orien,
            "expiry": o.expiry,
            "min_bond": o.min_bond,
            "max_fill_window": o.max_fill_window,
        }));
    }
    let prices = app.prices.clone();
    let (bitcoin, solana, rates) = tokio::task::spawn_blocking(move || {
        let (bitcoin, solana) = (prices.bitcoin(), prices.solana());
        (bitcoin, solana, prices.solana_rates())
    })
        .await
        .map_err(|e| Failure::Broken(e.into()))?;
    let usd = |price: Option<crate::prices::Price>| price.map(|p| json!({ "usd": p.usd, "at": p.at }));
    // SOL also in other currencies, for prices in people's own money.
    let mut solana = usd(solana);
    if let Some(sol) = solana.as_mut() {
        for (code, rate) in rates {
            sol[code] = json!(rate);
        }
    }
    if want_buys && let Some(chain) = app.book_chain.as_ref() {
        let books = app.store.lock().unwrap().open_book_orders()?;
        let filler = chain.filler().map(|f| orien_swap::solana::text(&f));
        for o in books {
            offers.push(json!({
                "id": o.id,
                "seller": o.buyer,
                "seller_name": o.buyer_name,
                "seller_picture": o.buyer_picture,
                "seller_done": o.buyer_done,
                // What's still to fill: any part of it may be sold.
                "orn": o.orn_left,
                "lamports": o.lamports_left,
                "orn_total": o.orn_total,
                "lamports_total": o.lamports_total,
                "coin": "sol",
                "sats": 0,
                "state": o.state,
                "created": o.created,
                "seller_online": true,
                "side": "buy",
                "funded": true,
                "book": true,
                "parts": true,
                "order_pubkey": o.order_pubkey,
                "buyer_orien": o.buyer_orien,
                "expiry": o.expiry,
                "filler": filler,
            }));
        }
    }
    let order_program = app.order_chain.as_ref().map(|c| c.program_id());
    let book_program = app.book_chain.as_ref().map(|c| orien_swap::solana::text(&c.program()));
    let book_filler = app.book_chain.as_ref().and_then(|c| c.filler()).map(|f| orien_swap::solana::text(&f));
    Ok(Json(json!({
        "offers": offers,
        "bitcoin": usd(bitcoin),
        "solana": solana,
        "order_program": order_program,
        "book_program": book_program,
        "book_filler": book_filler,
        "max_order_lamports": app.max_order_lamports,
    }))
    .into_response())
}

async fn post_swap(State(app): State<Arc<App>>, headers: HeaderMap, Json(request): Json<NewSwap>) -> Answer {
    let account = app.signed_in(&headers)?;
    let side = request.side.as_deref().unwrap_or("sell");
    let coin = request.coin.as_deref().unwrap_or("btc");
    let price = if coin == "sol" { request.lamports } else { request.sats };
    if side == "buy" {
        return Err(Failure::Refused("Buy orders must be funded on Solana through /api/market/orders.".into()));
    }
    if side != "sell" {
        return Err(Failure::Refused("side must be sell or buy".into()));
    }
    // Anyone can buy part of a sell order: only a wallet that sells parts
    // may post one.
    if request.swap_version < crate::swaps::PART_SWAP_VERSION {
        return Err(Failure::Refused("Update Orien Wallet to post a sell order.".into()));
    }
    // New sell offers come from one of the seller's verified wallets, so buyers
    // know who they deal with. Offers already up stay as they are.
    let verified = match request.wallet.as_deref() {
        Some(wallet) => {
            let wallet = store::orien_address(wallet).unwrap_or_default();
            app.store.lock().unwrap().is_verified_wallet(account, &wallet)?
        }
        None => false,
    };
    if !verified {
        return Err(Failure::Refused("Verify this wallet on your Orien Network profile before offering a swap from it.".into()));
    }
    let offer = app
        .store
        .lock()
        .unwrap()
        .post_offer(account, coin, request.orn, price, request.payout.as_deref(), request.wallet.as_deref())
        .map_err(refused)?;
    Ok((StatusCode::CREATED, Json(json!({ "offer": offer }))).into_response())
}

async fn my_swaps(State(app): State<Arc<App>>, headers: HeaderMap) -> Answer {
    let account = app.signed_in(&headers)?;
    // Orien Wallet asks this every few seconds while open: the seller is
    // there to lock the ORN of an offer someone takes.
    app.store.lock().unwrap().saw_app(account)?;
    let mut swaps = app.store.lock().unwrap().my_swaps(account)?;
    // The seller's open offers their wallet can no longer cover: off the
    // board, so they can withdraw it or top the wallet up.
    let asked: Vec<String> =
        swaps.iter().filter(|s| s.selling && s.offer.state == "open").filter_map(|s| s.wallet.clone()).collect();
    if !asked.is_empty() {
        // Waits on a first lookup: the seller's own list should be right.
        let spendable = spendable_of(&app, asked, true).await;
        for swap in swaps.iter_mut().filter(|s| s.selling && s.offer.state == "open") {
            swap.short_of_orn = short_of(&spendable, &swap.wallet, swap.offer.orn);
        }
    }
    Ok(Json(json!({ "swaps": swaps })).into_response())
}

#[derive(Deserialize, Default)]
struct Take {
    /// For sell offers: the Orien address to lock the ORN for. Without it,
    /// the account's verified wallet, if it has one.
    #[serde(default)]
    address: Option<String>,
    /// For buy offers: the seller's SOL or Bitcoin payout address.
    #[serde(default)]
    payout: Option<String>,
    /// For buy offers: the seller's Orien wallet address holding the ORN.
    #[serde(default)]
    wallet: Option<String>,
    /// The swap rules the taker's wallet follows: 2 from Orien Wallet 1.8.0,
    /// 3 for SOL swaps, 4 for buy offers.
    #[serde(default)]
    swap_version: u32,
    /// For sell offers: how much of it to take, in ORN units; all of it
    /// without. The rest stays on the board at the same price.
    #[serde(default)]
    orn: Option<u64>,
}

async fn take_swap(State(app): State<Arc<App>>, headers: HeaderMap, Path(id): Path<i64>, body: Option<Json<Take>>) -> Answer {
    let account = app.signed_in(&headers)?;
    let request = body.map(|Json(b)| b).unwrap_or_default();
    // A wallet on older swap rules would refuse a newer seller's shorter
    // lock only after the ORN were locked: turn it away before anything is.
    if request.swap_version < app.min_swap_version {
        return Err(Failure::Refused("update Orien Wallet to take this offer".into()));
    }
    let (offer, wallet) = {
        let store = app.store.lock().unwrap();
        (store.swap_offer(id)?, store.offer_wallet(id)?)
    };
    let coin = offer.as_ref().map(|o| o.coin.as_str()).unwrap_or_default();
    if coin == "sol" && request.swap_version < 3 {
        return Err(Failure::Refused("update Orien Wallet to take SOL offers".into()));
    }
    if offer.as_ref().is_some_and(|o| o.side == "buy") {
        if request.swap_version < 4 {
            return Err(Failure::Refused("update Orien Wallet to sell into buy offers".into()));
        }
        let offer = offer.unwrap();
        if !offer.seller_online {
            return Err(Failure::Refused("The buyer's Orien Wallet isn't open right now, so this swap can't start. Try again later.".into()));
        }
        let payout = request.payout.as_deref().ok_or_else(|| Failure::Refused("say the address you're paid at".into()))?;
        let seller_wallet = request
            .wallet
            .as_deref()
            .or(request.address.as_deref())
            .ok_or_else(|| Failure::Refused("give the Orien wallet you're selling from".into()))?;
        let seller_wallet = store::orien_address(seller_wallet).map_err(refused)?;
        let spendable = spendable_of(&app, vec![seller_wallet.clone()], true).await;
        if short_of(&spendable, &Some(seller_wallet.clone()), offer.orn) {
            return Err(Failure::Refused("Your wallet doesn't hold enough ORN for this swap.".into()));
        }
        let (swap, buyer_email, taker_name) = {
            let store = app.store.lock().unwrap();
            let swap = store.take_buy_swap(account, id, payout, &seller_wallet).map_err(refused)?;
            let email = store.email_of(swap.offer.seller)?;
            let taker_name = store.account(account)?.map(|a| a.name).unwrap_or_default();
            (swap, email, taker_name)
        };
        app.tell(crate::push::Event {
            account: swap.offer.seller,
            address: swap.taker_wallet.clone(),
            body: format!("{taker_name} accepted your buy offer of {} ORN", crate::store::orn(swap.offer.orn)),
            kind: "swap",
            open: json!({ "swap": id }),
        });
        if let Some(to) = buyer_email {
            let (mailer, origin) = (app.mailer.clone(), app.origin.clone());
            tokio::task::spawn_blocking(move || {
                if let Err(error) = mailer.send_swap_taken(&to, &taker_name, &origin) {
                    eprintln!("cannot email a swap notice: {error:#}");
                }
            });
        }
        return Ok(Json(json!({ "swap": swap })).into_response());
    }
    let orn = offer.as_ref().map_or(0, |o| o.orn);
    let online = offer.is_none_or(|o| o.seller_online);
    // Only the seller's wallet can lock the ORN: without it open, the buyer
    // would wait for nothing.
    if !online {
        return Err(Failure::Refused("The seller's Orien Wallet isn't open right now, so this swap can't start. Try again later.".into()));
    }
    if let Some(w) = &wallet {
        let spendable = spendable_of(&app, vec![w.clone()], true).await;
        if short_of(&spendable, &wallet, orn) {
            return Err(Failure::Refused("That offer isn't available any more: the seller's wallet no longer holds enough ORN.".into()));
        }
    }
    let (swap, seller_email, taker) = {
        let store = app.store.lock().unwrap();
        let address = match request.address {
            Some(address) => address,
            None => store.account(account)?.and_then(|a| a.wallet).ok_or_else(|| {
                Failure::Refused("take this offer in Orien Wallet, which sends where the ORN should go".into())
            })?,
        };
        let swap = store.take_swap_part(account, id, &address, request.orn).map_err(refused)?;
        let email = store.email_of(swap.offer.seller)?;
        let taker = store.account(account)?.map(|a| a.name).unwrap_or_default();
        (swap, email, taker)
    };
    app.tell(crate::push::Event {
        account: swap.offer.seller,
        address: swap.wallet.clone(),
        body: if swap.offer.id == id {
            format!("{taker} took your offer of {} ORN", crate::store::orn(swap.offer.orn))
        } else {
            format!("{taker} took {} ORN of your offer", crate::store::orn(swap.offer.orn))
        },
        kind: "swap",
        open: json!({ "swap": swap.offer.id }),
    });
    // A SOL swap runs by itself in the seller's wallet, which has just been
    // told: no email on top. Bitcoin's seller has to act, so theirs stays.
    if let Some(to) = seller_email.filter(|_| swap.offer.coin != "sol") {
        let (mailer, origin) = (app.mailer.clone(), app.origin.clone());
        tokio::task::spawn_blocking(move || {
            if let Err(error) = mailer.send_swap_taken(&to, &taker, &origin) {
                eprintln!("cannot email a swap notice: {error:#}");
            }
        });
    }
    Ok(Json(json!({ "swap": swap })).into_response())
}

#[derive(Deserialize, Default)]
struct Close {
    /// The seller's wallet could not lock a taken offer, being short of ORN.
    #[serde(default)]
    short: bool,
}

async fn close_swap(State(app): State<Arc<App>>, headers: HeaderMap, Path(id): Path<i64>, body: Option<Json<Close>>) -> Answer {
    let account = app.signed_in(&headers)?;
    let short = body.is_some_and(|Json(b)| b.short);
    let (side, buyer) = {
        let store = app.store.lock().unwrap();
        let side = store.swap_offer(id)?.map(|o| o.side).unwrap_or_else(|| "sell".into());
        let buyer = store.close_swap_short(account, id, short).map_err(refused)?;
        (side, buyer)
    };
    if let Some(buyer) = buyer {
        app.tell(crate::push::Event {
            account: buyer,
            address: None,
            body: if side == "buy" {
                "The seller couldn't lock the ORN, so your buy offer is back on the board. Nothing was paid.".into()
            } else {
                "The seller withdrew this offer before anything was locked. Nothing was paid.".into()
            },
            kind: "swap",
            open: json!({ "swap": id }),
        });
    }
    Ok(Json(json!({})).into_response())
}

async fn swap_offer_code(State(app): State<Arc<App>>, headers: HeaderMap, Path(id): Path<i64>, Json(request): Json<OfferCode>) -> Answer {
    let seller = app.signed_in(&headers)?;
    let (swap, orn_buyer, orn_seller_name, buyer_email) = {
        let store = app.store.lock().unwrap();
        let swap = store.post_offer_code_from(seller, id, &request.code, request.device.as_deref()).map_err(refused)?;
        let (buyer_id, seller_name) = if swap.offer.side == "buy" {
            (Some(swap.offer.seller), swap.taker_name.clone().unwrap_or_else(|| "The seller".into()))
        } else {
            (swap.taker, swap.offer.seller_name.clone())
        };
        let email = match buyer_id {
            Some(buyer) => store.email_of(buyer)?,
            None => None,
        };
        (swap, buyer_id, seller_name, email)
    };
    let price = if swap.offer.coin == "sol" {
        format!("{} SOL", trimmed(swap.offer.lamports, 9))
    } else {
        format!("{} BTC", bitcoin_amount(swap.offer.sats))
    };
    // A SOL swap is paid by the buyer's wallet itself, from its own Solana
    // balance: nothing to tell them to do, by notification or by email.
    let buyer_acts = swap.offer.coin != "sol";
    if let Some(buyer_id) = orn_buyer.filter(|_| buyer_acts) {
        app.tell(crate::push::Event {
            account: buyer_id,
            address: swap.taker_wallet.clone(),
            body: format!("{orn_seller_name} accepted your swap: pay {price} now"),
            kind: "swap",
            open: json!({ "swap": id }),
        });
    }
    // The buyer's turn: tell them, so it can't be missed.
    if let Some(to) = buyer_email.filter(|_| buyer_acts) {
        let (mailer, origin) = (app.mailer.clone(), app.origin.clone());
        let btc = if swap.offer.coin == "sol" {
            format!("{} SOL", trimmed(swap.offer.lamports, 9))
        } else {
            format!("{} BTC", bitcoin_amount(swap.offer.sats))
        };
        tokio::task::spawn_blocking(move || {
            if let Err(error) = mailer.send_swap_accepted(&to, &orn_seller_name, &btc, &origin) {
                eprintln!("cannot email a swap notice: {error:#}");
            }
        });
    }
    Ok(Json(json!({ "swap": swap })).into_response())
}

/// An amount with `places` decimals, as people write it: 0.15.
fn trimmed(units: u64, places: u32) -> String {
    let unit = 10u64.pow(places);
    let (whole, fraction) = (units / unit, units % unit);
    if fraction == 0 { whole.to_string() } else { format!("{whole}.{}", format!("{fraction:0width$}", width = places as usize).trim_end_matches('0')) }
}

/// Satoshis as BTC, as people write it: 0.00005.
fn bitcoin_amount(sats: u64) -> String {
    let (whole, fraction) = (sats / 100_000_000, sats % 100_000_000);
    if fraction == 0 { whole.to_string() } else { format!("{whole}.{}", format!("{fraction:08}").trim_end_matches('0')) }
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

/// Keeps the dollar value of swaps that just finished, at the coin's price
/// now. Called when a swap finishes and when the history is read, so swaps
/// settled by waiting are priced too.
async fn price_finished_swaps(app: &Arc<App>) {
    let Ok(unpriced) = app.store.lock().unwrap().unpriced_swaps() else { return };
    if unpriced.is_empty() {
        return;
    }
    let app = app.clone();
    let _ = tokio::task::spawn_blocking(move || {
        for (id, coin, amount) in unpriced {
            let (price, units) = if coin == "sol" { (app.prices.solana(), 1e9) } else { (app.prices.bitcoin(), 1e8) };
            if let Some(price) = price {
                let _ = app.store.lock().unwrap().price_swap(id, amount as f64 / units * price.usd);
            }
        }
    })
    .await;
}

/// Keeps the dollar value of finished funded buy orders, at SOL's price now.
pub(crate) async fn price_finished_orders(app: &Arc<App>) {
    let Ok(unpriced) = app.store.lock().unwrap().unpriced_funded_orders() else { return };
    let parts = app.store.lock().unwrap().unpriced_book_parts().unwrap_or_default();
    if unpriced.is_empty() && parts.is_empty() {
        return;
    }
    let app = app.clone();
    let _ = tokio::task::spawn_blocking(move || {
        let Some(price) = app.prices.solana() else { return };
        for (id, lamports) in unpriced {
            let _ = app.store.lock().unwrap().price_funded_order(id, lamports as f64 / 1e9 * price.usd);
        }
        let parts = app.store.lock().unwrap().unpriced_book_parts().unwrap_or_default();
        for (id, lamports) in parts {
            let _ = app.store.lock().unwrap().price_book_part(id, lamports as f64 / 1e9 * price.usd);
        }
    })
    .await;
}

#[derive(Deserialize)]
struct NewFundedOrder {
    order_pubkey: String,
    /// 64-character hex of the buyer's 32-byte secret S. Stored only in SQLite,
    /// never returned or logged, and only released after this server directly
    /// verifies the seller's ORN lock on Orien.
    secret: String,
}

async fn post_funded_order(State(app): State<Arc<App>>, headers: HeaderMap, Json(request): Json<NewFundedOrder>) -> Answer {
    let buyer = app.signed_in(&headers)?;
    let Some(chain) = app.order_chain.clone() else {
        return Err(Failure::Refused("Funded buy orders are not enabled on this server yet.".into()));
    };
    let order_pk = orien_swap::solana::address(&request.order_pubkey).map_err(refused)?;
    let order_pubkey = orien_swap::solana::text(&order_pk);
    let secret_bytes = hex::decode(request.secret.trim()).map_err(|_| Failure::Refused("secret must be 64 hex characters".into()))?;
    let secret: [u8; 32] = secret_bytes.try_into().map_err(|_| Failure::Refused("secret must be 32 bytes".into()))?;
    let (on_chain, chain_now) = tokio::task::spawn_blocking(move || -> anyhow::Result<_> {
        let acct = chain.read_order(&order_pk)?;
        let t = chain.now()?;
        Ok((acct, t))
    })
    .await
    .map_err(|e| Failure::Broken(e.into()))?
    .map_err(refused)?;
    let Some(on_chain) = on_chain else {
        return Err(Failure::Refused("That Solana order account isn't finalized on-chain yet. Try again in a moment.".into()));
    };
    if on_chain.terms.expiry <= chain_now + crate::orders::MIN_ORDER_LIFE_SECONDS {
        return Err(Failure::Refused("That order expires too soon.".into()));
    }
    let order = app
        .store
        .lock()
        .unwrap()
        .post_funded_order(buyer, &order_pubkey, &on_chain, &secret, app.max_order_lamports)
        .map_err(refused)?;
    Ok((StatusCode::CREATED, Json(json!({ "order": order }))).into_response())
}

async fn my_funded_orders(State(app): State<Arc<App>>, headers: HeaderMap) -> Answer {
    let account = app.signed_in(&headers)?;
    let orders = app.store.lock().unwrap().my_funded_orders(account)?;
    Ok(Json(json!({ "orders": orders })).into_response())
}

#[derive(Deserialize)]
struct ReserveFundedOrder {
    /// Seller's SOL payout address.
    seller_sol: String,
    /// Seller's Orien wallet address holding the ORN (checked to prevent spam).
    wallet: String,
}

async fn reserve_funded_order(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Path(id): Path<i64>,
    Json(request): Json<ReserveFundedOrder>,
) -> Answer {
    let seller = app.signed_in(&headers)?;
    let Some(chain) = app.order_chain.clone() else {
        return Err(Failure::Refused("Funded buy orders are not enabled on this server yet.".into()));
    };
    let order = app
        .store
        .lock()
        .unwrap()
        .funded_order(id)?
        .ok_or_else(|| Failure::Refused("there is no such order".into()))?;
    if order.state != "open" {
        return Err(Failure::Refused("that order is no longer open".into()));
    }
    if order.buyer == seller {
        return Err(Failure::Refused("this is your own order".into()));
    }
    let seller_sol_pk = orien_swap::solana::address(request.seller_sol.trim()).map_err(|_| Failure::Refused("That is not a SOL address.".into()))?;
    let buyer_sol_pk = orien_swap::solana::address(&order.buyer_sol).map_err(refused)?;
    if seller_sol_pk == buyer_sol_pk {
        return Err(Failure::Refused("That SOL address is the buyer's own. Give your own SOL wallet address.".into()));
    }
    let seller_wallet = store::orien_address(&request.wallet).map_err(refused)?;
    let spendable = spendable_of(&app, vec![seller_wallet.clone()], true).await;
    if short_of(&spendable, &Some(seller_wallet), order.orn) {
        return Err(Failure::Refused("Your wallet doesn't hold enough ORN for this order.".into()));
    }
    let order_pk = orien_swap::solana::address(&order.order_pubkey).map_err(refused)?;
    let sig = tokio::task::spawn_blocking(move || -> anyhow::Result<Option<String>> {
        let Some(acct) = chain.read_order(&order_pk)? else {
            anyhow::bail!("that order is no longer on Solana");
        };
        if acct.state == orien_swap::order::STATE_FILLED && acct.seller == seller_sol_pk {
            return Ok(None);
        }
        if acct.state != orien_swap::order::STATE_OPEN {
            anyhow::bail!("someone else is already filling that order");
        }
        if acct.terms.min_bond > 0 {
            anyhow::bail!("this order requires a SOL bond to fill");
        }
        let s = chain.relay_fill(&order_pk, &seller_sol_pk, acct.terms.max_fill_window)?;
        Ok(Some(s))
    })
    .await
    .map_err(|e| Failure::Broken(e.into()))?
    .map_err(refused)?;
    Ok(Json(json!({ "sig": sig })).into_response())
}

#[derive(Deserialize)]
struct FillFundedOrder {
    /// Seller's Orien account ID (64 hex) that locked the ORN.
    lock_from: String,
    /// Sequence number of the Orien lock order.
    lock_seq: u64,
}

async fn fill_funded_order(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Path(id): Path<i64>,
    Json(request): Json<FillFundedOrder>,
) -> Answer {
    let seller = app.signed_in(&headers)?;
    let Some(chain) = app.order_chain.clone() else {
        return Err(Failure::Refused("Funded buy orders are not enabled on this server yet.".into()));
    };
    let order = app
        .store
        .lock()
        .unwrap()
        .funded_order(id)?
        .ok_or_else(|| Failure::Refused("there is no such order".into()))?;
    if order.state != "open" {
        return Err(Failure::Refused("that order is no longer open".into()));
    }
    if order.buyer == seller {
        return Err(Failure::Refused("this is your own order".into()));
    }
    let lock_from_clean = store::orien_address(&request.lock_from).map_err(refused)?;
    let lock_from_id: orien_ledger::AccountId = lock_from_clean.parse().map_err(|_| Failure::Refused("invalid Orien address".into()))?;
    let buyer_orien_id: orien_ledger::AccountId = order.buyer_orien.parse().map_err(|_| Failure::Broken(anyhow::anyhow!("bad buyer_orien")))?;
    let expected_hash: [u8; 32] = hex::decode(&order.hash)
        .ok()
        .and_then(|b| b.try_into().ok())
        .ok_or_else(|| Failure::Broken(anyhow::anyhow!("bad hash")))?;
    let order_pk = orien_swap::solana::address(&order.order_pubkey).map_err(refused)?;
    let payments = app.payments.clone();
    let orn = order.orn;
    let lock_seq = request.lock_seq;

    // Verify BOTH the Solana `fill` AND the Orien ORN lock directly on-chain!
    let (seller_sol, fill_deadline) = tokio::task::spawn_blocking(move || -> anyhow::Result<(String, i64)> {
        let Some(acct) = chain.read_order(&order_pk)? else {
            anyhow::bail!("that order is no longer on Solana");
        };
        if acct.state != orien_swap::order::STATE_FILLED || acct.seller == [0u8; 32] {
            anyhow::bail!("Solana hasn't finalized your fill yet; try again in a few seconds");
        }
        let chain_now = chain.now()?;
        if chain_now >= acct.fill_deadline {
            anyhow::bail!("that fill window has already lapsed");
        }
        crate::orders::verify_orien_lock(
            payments.as_ref(),
            &lock_from_id,
            lock_seq,
            &buyer_orien_id,
            orn,
            &expected_hash,
            None,
            1,
        )?;
        Ok((orien_swap::solana::text(&acct.seller), acct.fill_deadline))
    })
    .await
    .map_err(|e| Failure::Broken(e.into()))?
    .map_err(refused)?;

    let filled = app
        .store
        .lock()
        .unwrap()
        .mark_funded_filled(id, seller, &seller_sol, &lock_from_clean, lock_seq, fill_deadline)
        .map_err(refused)?;

    // Kick off immediate auto-settlement in the background so seller is paid
    // in ~1 minute without waiting for the next tick.
    let app_bg = app.clone();
    tokio::spawn(async move {
        let app_inner = app_bg.clone();
        let _ = tokio::task::spawn_blocking(move || {
            let _ = crate::orders::settle_once(&app_inner);
        })
        .await;
        price_finished_orders(&app_bg).await;
    });

    Ok(Json(json!({ "order": filled })).into_response())
}

async fn sync_funded_order(State(app): State<Arc<App>>, headers: HeaderMap, Path(id): Path<i64>) -> Answer {
    let account = app.signed_in(&headers)?;
    let Some(chain) = app.order_chain.clone() else {
        return Err(Failure::Refused("Funded buy orders are not enabled on this server yet.".into()));
    };
    let order = app
        .store
        .lock()
        .unwrap()
        .funded_order(id)?
        .ok_or_else(|| Failure::Refused("there is no such order".into()))?;
    if order.buyer != account && order.seller != Some(account) {
        return Err(Failure::Refused("that is not your order".into()));
    }
    let order_pk = orien_swap::solana::address(&order.order_pubkey).map_err(refused)?;
    let (on_chain, chain_now) = tokio::task::spawn_blocking(move || -> anyhow::Result<_> {
        Ok((chain.read_order(&order_pk)?, chain.now()?))
    })
    .await
    .map_err(|e| Failure::Broken(e.into()))?
    .map_err(refused)?;
    if on_chain.is_none() && matches!(order.state.as_str(), "open" | "filled") {
        let next = if order.orn_claimed {
            app.store.lock().unwrap().mark_funded_done(id, None)?;
            None
        } else if chain_now >= order.expiry {
            Some("expired")
        } else {
            Some("cancelled")
        };
        if let Some(status) = next {
            app.store.lock().unwrap().close_funded_order(id, status)?;
        }
    }
    let updated = app.store.lock().unwrap().funded_order(id)?;
    Ok(Json(json!({ "order": updated })).into_response())
}

#[derive(Deserialize)]
struct NewBookOrder {
    order_pubkey: String,
    /// The order's 64 secrets, each 64 hex characters, in order. Kept only in
    /// SQLite, never returned or logged; each used only after its part's
    /// seller's ORN lock is checked on Orien.
    secrets: Vec<String>,
}

async fn post_book_order(State(app): State<Arc<App>>, headers: HeaderMap, Json(request): Json<NewBookOrder>) -> Answer {
    let buyer = app.signed_in(&headers)?;
    let Some(chain) = app.book_chain.clone() else {
        return Err(Failure::Refused("Buy orders that fill in parts are not enabled on this server yet.".into()));
    };
    let Some(filler) = chain.filler() else {
        return Err(Failure::Refused("Buy orders that fill in parts are not enabled on this server yet.".into()));
    };
    let order_pk = orien_swap::solana::address(&request.order_pubkey).map_err(refused)?;
    let order_pubkey = orien_swap::solana::text(&order_pk);
    let secrets: Vec<[u8; 32]> = request
        .secrets
        .iter()
        .map(|s| hex::decode(s.trim()).ok().and_then(|b| b.try_into().ok()))
        .collect::<Option<_>>()
        .ok_or_else(|| Failure::Refused("each secret is 64 hex characters".into()))?;
    let (on_chain, chain_now) = tokio::task::spawn_blocking(move || -> anyhow::Result<_> { Ok((chain.read(&order_pk)?, chain.now()?)) })
        .await
        .map_err(|e| Failure::Broken(e.into()))?
        .map_err(refused)?;
    let Some(on_chain) = on_chain else {
        return Err(Failure::Refused("That Solana order isn't finalized yet. Try again in a moment.".into()));
    };
    if on_chain.terms.expiry <= chain_now + crate::orders::MIN_ORDER_LIFE_SECONDS {
        return Err(Failure::Refused("That order expires too soon.".into()));
    }
    let order = app
        .store
        .lock()
        .unwrap()
        .post_book_order(buyer, &order_pubkey, &on_chain, &secrets, &filler, app.max_order_lamports)
        .map_err(refused)?;
    Ok((StatusCode::CREATED, Json(json!({ "order": order }))).into_response())
}

async fn my_book_orders(State(app): State<Arc<App>>, headers: HeaderMap) -> Answer {
    let account = app.signed_in(&headers)?;
    let (orders, parts) = app.store.lock().unwrap().my_books(account)?;
    Ok(Json(json!({ "orders": orders, "parts": parts })).into_response())
}

#[derive(Deserialize)]
struct ReserveBookPart {
    /// Where the seller's SOL goes.
    seller_sol: String,
    /// The seller's Orien wallet holding the ORN.
    wallet: String,
    /// How many ORN (in units) the seller sells into the order.
    orn: u64,
}

async fn reserve_book_part(State(app): State<Arc<App>>, headers: HeaderMap, Path(id): Path<i64>, Json(request): Json<ReserveBookPart>) -> Answer {
    let seller = app.signed_in(&headers)?;
    let Some(chain) = app.book_chain.clone() else {
        return Err(Failure::Refused("Buy orders that fill in parts are not enabled on this server yet.".into()));
    };
    let seller_sol = orien_swap::solana::address(request.seller_sol.trim()).map_err(|_| Failure::Refused("That is not a SOL address.".into()))?;
    let wallet = store::orien_address(&request.wallet).map_err(refused)?;
    let spendable = spendable_of(&app, vec![wallet.clone()], true).await;
    if short_of(&spendable, &Some(wallet), request.orn) {
        return Err(Failure::Refused("Your wallet doesn't hold that many ORN.".into()));
    }
    let worker = app.clone();
    let part = tokio::task::spawn_blocking(move || crate::books::reserve(&worker, chain.as_ref(), id, seller, &seller_sol, request.orn))
        .await
        .map_err(|e| Failure::Broken(e.into()))?
        .map_err(refused)?;
    let order = app.store.lock().unwrap().book_order(id)?;
    Ok(Json(json!({ "part": part, "order": order })).into_response())
}

#[derive(Deserialize)]
struct FillBookPart {
    /// The seller's Orien account that locked the ORN.
    lock_from: String,
    /// The lock's sequence number.
    lock_seq: u64,
}

async fn fill_book_part(State(app): State<Arc<App>>, headers: HeaderMap, Path(id): Path<i64>, Json(request): Json<FillBookPart>) -> Answer {
    let seller = app.signed_in(&headers)?;
    let Some(chain) = app.book_chain.clone() else {
        return Err(Failure::Refused("Buy orders that fill in parts are not enabled on this server yet.".into()));
    };
    let part = app.store.lock().unwrap().book_part(id)?.ok_or_else(|| Failure::Refused("there is no such part".into()))?;
    if part.seller != seller {
        return Err(Failure::Refused("that part is not yours".into()));
    }
    if !matches!(part.state.as_str(), "reserved" | "filled") {
        return Err(Failure::Refused("that part is no longer waiting for your ORN".into()));
    }
    let lock_from = store::orien_address(&request.lock_from).map_err(refused)?;
    let lock_id: orien_ledger::AccountId = lock_from.parse().map_err(|_| Failure::Refused("invalid Orien address".into()))?;
    let lock_seq = request.lock_seq;
    let worker = app.clone();
    let checked = part.clone();
    tokio::task::spawn_blocking(move || crate::books::check_lock(&worker, chain.as_ref(), &checked, &lock_id, lock_seq))
        .await
        .map_err(|e| Failure::Broken(e.into()))?
        .map_err(refused)?;
    let filled = app.store.lock().unwrap().book_part_locked(id, seller, &lock_from, lock_seq).map_err(refused)?;
    // Settle at once rather than at the next tick.
    let background = app.clone();
    tokio::spawn(async move {
        let inner = background.clone();
        let _ = tokio::task::spawn_blocking(move || crate::books::settle_once(&inner)).await;
        price_finished_orders(&background).await;
    });
    Ok(Json(json!({ "part": filled })).into_response())
}

#[derive(Deserialize)]
struct HistoryPage {
    before: Option<u64>,
}

/// What ORN actually traded for: finished swaps and funded buy orders,
/// newest first, 30 a page, with no names or addresses. `last` is the latest.
async fn swap_history(State(app): State<Arc<App>>, Query(page): Query<HistoryPage>) -> Answer {
    price_finished_swaps(&app).await;
    price_finished_orders(&app).await;
    let store = app.store.lock().unwrap();
    let mut combined = store.swap_history(page.before, 31)?;
    combined.extend(store.funded_order_history(page.before, 31)?);
    combined.extend(store.book_history(page.before, 31)?);
    combined.sort_by_key(|v| std::cmp::Reverse(v["at"].as_u64().unwrap_or(0)));
    let more = combined.len() > 30;
    let swaps: Vec<_> = combined.into_iter().take(30).collect();
    let mut latest = store.swap_history(None, 1)?;
    latest.extend(store.funded_order_history(None, 1)?);
    latest.extend(store.book_history(None, 1)?);
    latest.sort_by_key(|v| std::cmp::Reverse(v["at"].as_u64().unwrap_or(0)));
    let last = latest.into_iter().next();
    Ok(([(header::CACHE_CONTROL, "public, max-age=30")], Json(json!({ "last": last, "swaps": swaps, "more": more }))).into_response())
}

async fn finish_swap(State(app): State<Arc<App>>, headers: HeaderMap, Path(id): Path<i64>, Json(request): Json<Finish2>) -> Answer {
    let account = app.signed_in(&headers)?;
    let first_done = app.store.lock().unwrap().finish_swap(account, id, request.done).map_err(refused)?;
    price_finished_swaps(&app).await;
    // The first wallet to finish tells both people's phones, once.
    if let Some(swap) = first_done {
        let orn = crate::store::orn(swap.offer.orn);
        let (orn_buyer, orn_buyer_name, orn_seller, orn_seller_name) = if swap.offer.side == "buy" {
            (
                Some(swap.offer.seller),
                swap.offer.seller_name.clone(),
                swap.taker.unwrap_or(swap.offer.seller),
                swap.taker_name.clone().unwrap_or_else(|| "the seller".into()),
            )
        } else {
            (
                swap.taker,
                swap.taker_name.clone().unwrap_or_else(|| "the buyer".into()),
                swap.offer.seller,
                swap.offer.seller_name.clone(),
            )
        };
        if let Some(buyer_id) = orn_buyer {
            app.tell(crate::push::Event {
                account: buyer_id,
                address: swap.taker_wallet.clone(),
                body: format!("Swap done: your {orn} ORN from {orn_seller_name} are yours"),
                kind: "swap",
                open: json!({ "swap": id }),
            });
        }
        app.tell(crate::push::Event {
            account: orn_seller,
            address: swap.wallet.clone(),
            body: format!("Swap done: {orn_buyer_name} bought your {orn} ORN"),
            kind: "swap",
            open: json!({ "swap": id }),
        });
    }
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
pub(crate) async fn network<T: Send + 'static>(
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
    // Buyers pay only verified wallets: once one is, the wallet paid at can
    // only become another verified one.
    {
        let store = app.store.lock().unwrap();
        if store.wallet_verified(id)? && !store.is_verified_wallet(id, &address)? {
            return Err(Failure::Refused("verify that wallet first, then choose Get paid here".into()));
        }
    }
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

/// The linked wallet, whether it is verified, and what verifying it takes
/// if that was asked for.
async fn wallet(State(app): State<Arc<App>>, headers: HeaderMap) -> Answer {
    let id = app.signed_in(&headers)?;
    let store = app.store.lock().unwrap();
    let account = store.account(id)?.ok_or(Failure::SignIn)?;
    let wallets = store.proofs(id, &app.address)?;
    let proof = wallets.iter().find(|p| Some(&p.address) == account.wallet.as_ref()).cloned();
    Ok(Json(json!({ "wallet": account.wallet, "verified": account.wallet_verified, "proof": proof, "wallets": wallets })).into_response())
}

#[derive(Deserialize, Default)]
struct WalletChoice {
    #[serde(default)]
    address: Option<String>,
}

/// Starts verifying the linked wallet: what to send, from it, to the
/// Market's wallet. Asking again gives the same amount until it is paid.
async fn verify_wallet(State(app): State<Arc<App>>, headers: HeaderMap, body: axum::body::Bytes) -> Answer {
    let id = app.signed_in(&headers)?;
    // Which wallet: the one buyers pay when the body names none.
    let choice: WalletChoice = if body.is_empty() {
        WalletChoice::default()
    } else {
        serde_json::from_slice(&body).map_err(|_| Failure::Refused("that request is not understood".into()))?
    };
    let epoch = network(&app, |p| p.epoch()).await?;
    let proof = app.store.lock().unwrap().start_proof(id, choice.address.as_deref(), epoch, &app.address).map_err(refused)?;
    let proof = proof.ok_or_else(|| Failure::Refused("link your Orien wallet first, under Your profile".into()))?;
    Ok(Json(json!({ "proof": proof })).into_response())
}

/// Stops a wallet being named as this account's, verified or waiting.
async fn remove_verified_wallet(State(app): State<Arc<App>>, headers: HeaderMap, Json(choice): Json<WalletChoice>) -> Answer {
    let id = app.signed_in(&headers)?;
    let address = choice.address.ok_or_else(|| Failure::Refused("say which wallet".into()))?;
    app.store.lock().unwrap().remove_proof(id, &address).map_err(refused)?;
    Ok(Json(json!({ "ok": true })).into_response())
}

#[derive(Deserialize)]
struct ProfileSearch {
    #[serde(default)]
    q: String,
}

/// People to pay, found by name: profiles with verified wallets, at most
/// ten, and none for under two letters.
async fn search_profiles(State(app): State<Arc<App>>, headers: HeaderMap, Query(search): Query<ProfileSearch>) -> Answer {
    let viewer = app.signed_in(&headers).ok();
    let profiles = app.store.lock().unwrap().search_payable(&search.q, viewer)?;
    Ok(([(header::CACHE_CONTROL, cache_for(viewer, 30))], Json(json!({ "profiles": profiles }))).into_response())
}

/// The public profile of the account whose verified wallet this is. Any
/// website may read it from a visitor's browser, as for "Sign in with Orien
/// Wallet".
async fn wallet_profile(State(app): State<Arc<App>>, headers: HeaderMap, Path(address): Path<String>) -> Answer {
    let viewer = app.signed_in(&headers).ok();
    let found = app.store.lock().unwrap().wallet_profiles(&[address], viewer);
    let (_, profile) = found?.into_iter().next().ok_or(Failure::NotFound)?;
    let profile = public_wallet_profile(app.origin(), profile);
    Ok(([(header::CACHE_CONTROL, cache_for(viewer, 60)), (header::ACCESS_CONTROL_ALLOW_ORIGIN, "*".to_string())], Json(profile)).into_response())
}

/// How long an answer may be kept: by anyone, or only by the signed-in
/// viewer's own browser, whose answer leaves out who blocked them.
fn cache_for(viewer: Option<i64>, seconds: u32) -> String {
    match viewer {
        Some(_) => format!("private, max-age={seconds}"),
        None => format!("public, max-age={seconds}"),
    }
}

/// The browser's question before another website sends JSON to a public
/// endpoint: yes, from anywhere.
async fn open_preflight() -> Response {
    (
        StatusCode::NO_CONTENT,
        [
            (header::ACCESS_CONTROL_ALLOW_ORIGIN, "*"),
            (header::ACCESS_CONTROL_ALLOW_METHODS, "GET, POST"),
            (header::ACCESS_CONTROL_ALLOW_HEADERS, "content-type"),
            (header::ACCESS_CONTROL_MAX_AGE, "86400"),
        ],
    )
        .into_response()
}

/// A verified wallet's profile as websites see it: the fields the apps read
/// (`picture` is a file name under /api/market/images/), plus full addresses
/// for the picture, banner and profile page, and the usernames the person
/// shows on their profile.
fn public_wallet_profile(origin: &str, profile: crate::proofs::WalletProfile) -> serde_json::Value {
    let image = |name: &Option<String>| name.as_ref().map(|name| format!("{origin}/api/market/images/{name}"));
    let contact = &profile.contact;
    let mut links = Vec::new();
    if !contact.x.is_empty() {
        links.push(json!({ "kind": "x", "handle": contact.x, "url": format!("https://x.com/{}", contact.x) }));
    }
    if !contact.telegram.is_empty() {
        links.push(json!({ "kind": "telegram", "handle": contact.telegram, "url": format!("https://t.me/{}", contact.telegram) }));
    }
    if !contact.discord.is_empty() {
        links.push(json!({ "kind": "discord", "handle": contact.discord, "url": null }));
    }
    json!({
        "id": profile.id,
        "name": profile.name,
        "picture": profile.picture,
        "wallet_name": profile.wallet_name,
        "bio": profile.bio,
        "picture_url": image(&profile.picture),
        "banner_url": image(&profile.banner),
        "profile_url": format!("{origin}/u/{}/", profile.name),
        "links": links,
    })
}

#[derive(Deserialize)]
struct Lookup {
    addresses: Vec<String>,
}

/// Public profiles for the verified wallets among up to 100 addresses, by
/// address. Any website may ask, from a visitor's browser.
async fn lookup_wallets(State(app): State<Arc<App>>, headers: HeaderMap, Json(request): Json<Lookup>) -> Answer {
    let viewer = app.signed_in(&headers).ok();
    if request.addresses.len() > crate::proofs::MAX_LOOKUP {
        return Err(Failure::Refused(format!("at most {} addresses at a time", crate::proofs::MAX_LOOKUP)));
    }
    let found = app.store.lock().unwrap().wallet_profiles(&request.addresses, viewer)?;
    let profiles: serde_json::Map<String, serde_json::Value> =
        found.into_iter().map(|(address, profile)| (address, public_wallet_profile(app.origin(), profile))).collect();
    // The explorer's nicknames, for the wallets without a profile.
    let orien: Vec<String> = request
        .addresses
        .iter()
        .filter(|a| a.len() == 64 && a.bytes().all(|b| b.is_ascii_hexdigit()) && !profiles.contains_key(a.as_str()))
        .cloned()
        .collect();
    let nicknames = app.store.lock().unwrap().nicknames(&orien).unwrap_or_default();
    Ok(([(header::ACCESS_CONTROL_ALLOW_ORIGIN, "*")], Json(json!({ "profiles": profiles, "nicknames": nicknames }))).into_response())
}

/// A wallet's vault: sealed bytes, base64.
async fn read_vault(State(app): State<Arc<App>>, Path(id): Path<String>) -> Answer {
    let found = app.store().vault(&id).map_err(refused)?.ok_or(Failure::NotFound)?;
    // Started by a picture, not yet written: nothing to give.
    if found.0.is_empty() {
        return Err(Failure::NotFound);
    }
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
struct VaultToken {
    token: String,
}

/// A sealed picture beside a wallet's vault, base64: for its token's holder.
async fn read_vault_picture(
    State(app): State<Arc<App>>,
    Path((id, picture)): Path<(String, String)>,
    Query(given): Query<VaultToken>,
) -> Answer {
    let found = app.store().vault_picture(&id, &given.token, &picture).map_err(refused)?.ok_or(Failure::NotFound)?;
    use base64::Engine;
    Ok(Json(json!({ "blob": base64::engine::general_purpose::STANDARD.encode(&found) })).into_response())
}

async fn write_vault_picture(
    State(app): State<Arc<App>>,
    Path((id, picture)): Path<(String, String)>,
    Json(body): Json<VaultWrite>,
) -> Answer {
    use base64::Engine;
    let blob = base64::engine::general_purpose::STANDARD
        .decode(body.blob.as_bytes())
        .map_err(|_| Failure::Refused("the picture is not base64".into()))?;
    app.store().put_vault_picture(&id, &body.token, &picture, &blob).map_err(refused)?;
    Ok(Json(json!({ "ok": true })).into_response())
}

async fn delete_vault_picture(
    State(app): State<Arc<App>>,
    Path((id, picture)): Path<(String, String)>,
    Json(body): Json<VaultToken>,
) -> Answer {
    let gone = app.store().delete_vault_picture(&id, &body.token, &picture).map_err(refused)?;
    Ok(Json(json!({ "deleted": gone })).into_response())
}

#[derive(Deserialize)]
struct ProfileText {
    name: String,
    #[serde(default)]
    bio: String,
    #[serde(default)]
    contact: store::Contact,
    /// Who may message the account; left as it is when absent (older apps).
    #[serde(default)]
    messages_from: Option<String>,
}

async fn edit_profile(State(app): State<Arc<App>>, headers: HeaderMap, Json(request): Json<ProfileText>) -> Answer {
    let id = app.signed_in(&headers)?;
    let (name, bio) = store::profile_text(&request.name, &request.bio).map_err(refused)?;
    let contact = request.contact.checked().map_err(refused)?;
    if let Some(choice) = &request.messages_from
        && !store::MESSAGES_FROM.contains(&choice.as_str())
    {
        return Err(Failure::Refused("who can message you is everyone, talked or nobody".into()));
    }
    // The usernames are public too, so they are read with the bio.
    let handles = format!("{bio}\n\nTelegram: {}\nX: {}\nDiscord: {}", contact.telegram, contact.x, contact.discord);
    let n = name.clone();
    screened(&app, "profile", move |s| s.profile(&n, &handles)).await?;
    let account = {
        let store = app.store.lock().unwrap();
        if let Some(choice) = &request.messages_from {
            store.set_messages_from(id, choice).map_err(refused)?;
        }
        store.set_profile(id, &name, &bio, &contact).map_err(refused)?
    };
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
async fn profile(State(app): State<Arc<App>>, headers: HeaderMap, Path(id): Path<i64>) -> Answer {
    let viewer = app.signed_in(&headers).ok();
    let store = app.store.lock().unwrap();
    // A block either way makes them nobody to each other, as for an unknown name.
    if store.is_suspended(id)? || viewer.is_some_and(|v| store.blocked_either(id, v).unwrap_or(false)) {
        return Err(Failure::NotFound);
    }
    let account = store.account(id)?.ok_or(Failure::NotFound)?;
    let listings = store.open_listings_of(id)?;
    let swaps_done = store.swaps_done(id)?;
    let wallets = store.verified_wallets(id)?;
    let mining = app.community().stats_of(id);
    let mut answer = json!({ "account": account, "listings": listings, "swaps_done": swaps_done, "wallets": wallets, "mining": mining });
    // Signed in: whether you've blocked this person.
    if let Some(viewer) = viewer {
        answer["blocked"] = json!(store.blocked_by(viewer, id)?);
        // Whether a Message button would work: their choice, blocks, yourself.
        answer["takes_messages"] = json!(store.takes_messages(viewer, id)?);
    }
    Ok(Json(answer).into_response())
}

/// A profile by its name, as /u/NAME shows it.
async fn user(State(app): State<Arc<App>>, headers: HeaderMap, Path(name): Path<String>) -> Answer {
    let id = app.store.lock().unwrap().account_by_name(&name)?.ok_or(Failure::NotFound)?;
    profile(State(app), headers, Path(id)).await
}

/// The people the signed-in account has blocked, most recent first.
async fn my_blocks(State(app): State<Arc<App>>, headers: HeaderMap) -> Answer {
    let account = app.signed_in(&headers)?;
    let blocked = app.store.lock().unwrap().blocked_list(account)?;
    Ok(Json(json!({ "blocked": blocked })).into_response())
}

/// Blocks or unblocks a person, everywhere, for the signed-in account.
async fn block_account(State(app): State<Arc<App>>, headers: HeaderMap, Path(id): Path<i64>, Json(request): Json<Block>) -> Answer {
    let account = app.signed_in(&headers)?;
    app.store.lock().unwrap().block_account(account, id, request.block).map_err(refused)?;
    Ok(Json(json!({ "blocked": request.block })).into_response())
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

async fn browse(State(app): State<Arc<App>>, headers: HeaderMap, Query(query): Query<Browse>) -> Answer {
    let category = query.category.as_deref().filter(|c| !c.is_empty());
    let viewer = app.signed_in(&headers).ok();
    let listings = app.store.lock().unwrap().browse_for(viewer, category, query.q.as_deref(), PAGE, query.offset)?;
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
    if let Some(viewer) = viewer
        && store.blocked_either(listing.seller, viewer)?
    {
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
/// For tests: a signed-in account with no wallet yet.
#[doc(hidden)]
/// For tests: reads or changes the store directly.
#[doc(hidden)]
pub fn store_for_tests<T>(app: &App, work: impl FnOnce(&Store) -> T) -> T {
    work(&app.store.lock().unwrap())
}

pub fn sign_in_without_wallet_for_tests(app: &App, name: &str) -> String {
    let store = app.store.lock().unwrap();
    let account = store.create_account(name, &hex::encode(rand_bytes())).unwrap();
    store.start_session(account.id).unwrap()
}

pub fn sign_in_for_tests(app: &App, name: &str) -> String {
    let store = app.store.lock().unwrap();
    let account = store.create_account(name, &hex::encode(rand_bytes())).unwrap();
    // A verified wallet of its own and a way to be reached, so it can post listings.
    let wallet = hex::encode(rand_bytes()).repeat(2);
    store.set_wallet(account.id, &wallet).unwrap();
    store.start_proof(account.id, Some(&wallet), 0, "to").unwrap();
    store.mark_proven(account.id, &wallet).unwrap();
    let reach = store::Contact { discord: name.to_lowercase().replace(' ', "_"), ..Default::default() };
    store.set_profile(account.id, name, "", &reach).unwrap();
    store.start_session(account.id).unwrap()
}

/// Sixteen random bytes, for tests.
fn rand_bytes() -> [u8; 16] {
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes).expect("the system makes random numbers");
    bytes
}
