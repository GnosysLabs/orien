//! The marketplace server, asked the way its pages ask it.

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use http_body_util::BodyExt;
use orien_market::api::{App, COOKIE, router, sign_in_for_tests};
use orien_market::images::Folder;
use orien_market::mail::Mailer;
use orien_market::screen::Screener;
use orien_market::store::Store;
use orien_market::wallets::{Fake, Incoming};
use serde_json::{Value, json};
use std::sync::Arc;
use tower::ServiceExt;

/// The marketplace's own address in tests.
const MARKET: &str = "0000000000000000000000000000000000000000000000000000000000000abc";

/// A fresh folder for a test's pictures, which the system clears in time.
fn folder() -> Folder {
    let mut random = [0u8; 8];
    getrandom::fill(&mut random).unwrap();
    Folder::open(&std::env::temp_dir().join(format!("orien-market-test-{}", hex::encode(random)))).unwrap()
}

struct Market {
    app: Arc<App>,
}

impl Market {
    fn screened(word: &str) -> Market {
        let app = App::new(Store::in_memory().unwrap(), folder(), "https://orien.network", true, Mailer::log_only(), Screener::refusing_for_tests(word), Arc::new(Fake::default()), MARKET);
        Market { app: Arc::new(app.unwrap()) }
    }

    fn new() -> Market {
        Market { app: Arc::new(App::new(Store::in_memory().unwrap(), folder(), "https://orien.network", true, Mailer::log_only(), Screener::off(), Arc::new(Fake::default()), MARKET).unwrap()) }
    }

    async fn ask(&self, method: &str, path: &str, token: Option<&str>, body: Option<Value>) -> (StatusCode, Value) {
        let mut request = Request::builder().method(method).uri(path);
        if let Some(token) = token {
            request = request.header(header::COOKIE, format!("theme=dark; {COOKIE}={token}"));
        }
        let body = match body {
            Some(body) => {
                request = request.header(header::CONTENT_TYPE, "application/json");
                Body::from(body.to_string())
            }
            None => Body::empty(),
        };
        let response = router(self.app.clone()).oneshot(request.body(body).unwrap()).await.unwrap();
        let status = response.status();
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        (status, serde_json::from_slice(&bytes).unwrap_or(Value::Null))
    }
}

fn logo() -> Value {
    json!({
        "title": "Logo design",
        "description": "Clean, simple logos.",
        "category": "design",
        "price": 200_000_000_000u64,
        "pay_to": "be5b7cc81cbe475fe002bee2597c30b0ae4f030434e93f12ade7f1a266c6258b"
    })
}

#[tokio::test]
async fn sellers_post_and_anyone_browses() {
    let market = Market::new();
    let (status, body) = market.ask("GET", "/api/market/listings", None, None).await;
    assert_eq!((status, body["listings"].as_array().unwrap().len()), (StatusCode::OK, 0));

    // Posting needs signing in.
    let (status, _) = market.ask("POST", "/api/market/listings", None, Some(logo())).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let (status, _) = market.ask("POST", "/api/market/listings", Some("forged"), Some(logo())).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    let maya = sign_in_for_tests(&market.app, "Maya");
    let (status, body) = market.ask("POST", "/api/market/listings", Some(&maya), Some(logo())).await;
    assert_eq!(status, StatusCode::CREATED);
    let id = body["listing"]["id"].as_i64().unwrap();
    assert_eq!(body["listing"]["seller_name"], "Maya");

    let (_, body) = market.ask("GET", "/api/market/listings?category=design&q=logo", None, None).await;
    assert_eq!(body["listings"][0]["id"], id);
    let (_, body) = market.ask("GET", &format!("/api/market/listings/{id}"), None, None).await;
    assert_eq!((body["listing"]["title"].as_str(), body["mine"].as_bool()), (Some("Logo design"), Some(false)));

    let (status, body) = market.ask("GET", "/api/market/me", Some(&maya), None).await;
    assert_eq!((status, body["account"]["name"].as_str()), (StatusCode::OK, Some("Maya")));
    assert_eq!(body["listings"].as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn only_the_seller_changes_a_listing_and_a_closed_one_is_hidden() {
    let market = Market::new();
    let maya = sign_in_for_tests(&market.app, "Maya");
    let sam = sign_in_for_tests(&market.app, "Sam");
    let (_, body) = market.ask("POST", "/api/market/listings", Some(&maya), Some(logo())).await;
    let id = body["listing"]["id"].as_i64().unwrap();
    let path = format!("/api/market/listings/{id}");

    let mut changed = logo();
    changed["title"] = json!("Mine now");
    let (status, _) = market.ask("PUT", &path, Some(&sam), Some(changed.clone())).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (status, body) = market.ask("PUT", &path, Some(&maya), Some(changed)).await;
    assert_eq!((status, body["listing"]["title"].as_str()), (StatusCode::OK, Some("Mine now")));

    let close = json!({ "open": false });
    let (status, _) = market.ask("POST", &format!("{path}/open"), Some(&sam), Some(close.clone())).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (status, _) = market.ask("POST", &format!("{path}/open"), Some(&maya), Some(close)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(market.ask("GET", &path, None, None).await.0, StatusCode::NOT_FOUND);
    assert_eq!(market.ask("GET", &path, Some(&sam), None).await.0, StatusCode::NOT_FOUND);
    let (status, body) = market.ask("GET", &path, Some(&maya), None).await;
    assert_eq!((status, body["mine"].as_bool()), (StatusCode::OK, Some(true)));
}

#[tokio::test]
async fn a_bad_listing_is_refused_in_plain_words() {
    let market = Market::new();
    let maya = sign_in_for_tests(&market.app, "Maya");
    let mut bad = logo();
    bad["category"] = json!("weapons");
    let (status, body) = market.ask("POST", "/api/market/listings", Some(&maya), Some(bad)).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(body["error"].as_str().unwrap().contains("categories"));
}

#[tokio::test]
async fn passkey_ceremonies_start_and_signing_out_ends_the_session() {
    let market = Market::new();
    let (status, body) = market.ask("POST", "/api/market/signup/start", None, Some(json!({ "name": "Maya" }))).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["options"]["publicKey"]["rp"]["id"], "orien.network");
    assert!(body["options"]["publicKey"]["challenge"].is_string());
    let (status, _) = market.ask("POST", "/api/market/signup/start", None, Some(json!({ "name": " " }))).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (status, body) = market.ask("POST", "/api/market/signin/start", None, None).await;
    assert_eq!((status, body["id"].is_string()), (StatusCode::OK, true));

    // A ceremony nobody started cannot be finished.
    let fake = json!({ "id": "made-up", "credential": {} });
    assert_ne!(market.ask("POST", "/api/market/signin/finish", None, Some(fake)).await.0, StatusCode::OK);

    let maya = sign_in_for_tests(&market.app, "Maya");
    assert_eq!(market.ask("GET", "/api/market/me", Some(&maya), None).await.0, StatusCode::OK);
    assert_eq!(market.ask("POST", "/api/market/signout", Some(&maya), None).await.0, StatusCode::OK);
    assert_eq!(market.ask("GET", "/api/market/me", Some(&maya), None).await.0, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn an_email_code_signs_in_and_a_new_email_is_asked_for_a_name() {
    let market = Market::new();
    let start = json!({ "email": "Maya@Example.com" });
    let (status, body) = market.ask("POST", "/api/market/email/start", None, Some(start.clone())).await;
    assert_eq!((status, body["sent"].as_str()), (StatusCode::OK, Some("maya@example.com")));
    let (status, _) = market.ask("POST", "/api/market/email/start", None, Some(start)).await;
    assert_eq!(status, StatusCode::OK, "asking again within a minute sends nothing new, and the first code still works");
    let (status, _) =
        market.ask("POST", "/api/market/email/start", None, Some(json!({ "email": "nonsense" }))).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    // The code was printed, not sent; the store makes another to test with.
    let code = market.app.code_for_tests("maya@example.com");
    let wrong = if code == "000000" { "111111" } else { "000000" };
    let finish = |code: &str, name: &str| json!({ "email": "maya@example.com", "code": code, "name": name });
    let (status, _) = market.ask("POST", "/api/market/email/finish", None, Some(finish(wrong, ""))).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (status, body) = market.ask("POST", "/api/market/email/finish", None, Some(finish(&code, ""))).await;
    assert_eq!((status, body["needs_name"].as_bool()), (StatusCode::OK, Some(true)));
    let request = Request::builder()
        .method("POST")
        .uri("/api/market/email/finish")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(finish(&code, "Maya").to_string()))
        .unwrap();
    let response = router(market.app.clone()).oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let cookie = response.headers()[header::SET_COOKIE].to_str().unwrap().to_string();
    assert!(cookie.contains("HttpOnly") && cookie.contains("Secure") && cookie.contains("SameSite=Strict"));
    let token = cookie.split(';').next().unwrap().split_once('=').unwrap().1.to_string();
    let (_, body) = market.ask("GET", "/api/market/me", Some(&token), None).await;
    assert_eq!(body["account"]["name"], "Maya");

    // The code is used up; a fresh one signs the same account in, no name asked.
    let (status, _) = market.ask("POST", "/api/market/email/finish", None, Some(finish(&code, ""))).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let code = market.app.code_for_tests("maya@example.com");
    let (status, body) = market.ask("POST", "/api/market/email/finish", None, Some(finish(&code, ""))).await;
    assert_eq!((status, body["account"]["name"].as_str()), (StatusCode::OK, Some("Maya")));
}

#[tokio::test]
async fn the_screening_model_keeps_bad_listings_and_names_out_and_nothing_unchecked_gets_in() {
    let market = Market::screened("stolen");
    let maya = sign_in_for_tests(&market.app, "Maya");
    let mut bad = logo();
    bad["description"] = json!("Selling stolen accounts");
    let (status, body) = market.ask("POST", "/api/market/listings", Some(&maya), Some(bad)).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(body["error"].as_str().unwrap().contains("illegal goods or services"), "{body}");

    // When the model cannot be asked, nothing is saved.
    let mut unchecked = logo();
    unchecked["description"] = json!("this will fail to be checked");
    let (status, body) = market.ask("POST", "/api/market/listings", Some(&maya), Some(unchecked)).await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    assert!(body["error"].as_str().unwrap().contains("cannot be checked right now"));
    let (_, body) = market.ask("GET", "/api/market/listings", None, None).await;
    assert_eq!(body["listings"].as_array().unwrap().len(), 0);

    // A good listing goes through, and an edit is checked too.
    let (status, body) = market.ask("POST", "/api/market/listings", Some(&maya), Some(logo())).await;
    assert_eq!(status, StatusCode::CREATED);
    let id = body["listing"]["id"].as_i64().unwrap();
    let mut edit = logo();
    edit["title"] = json!("stolen logos");
    let (status, _) = market.ask("PUT", &format!("/api/market/listings/{id}"), Some(&maya), Some(edit)).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    // A new account's name is checked before the account is made.
    let email = "sam@example.com";
    market.ask("POST", "/api/market/email/start", None, Some(json!({ "email": email }))).await;
    let code = market.app.code_for_tests(email);
    let finish = json!({ "email": email, "code": code, "name": "stolen goods" });
    let (status, _) = market.ask("POST", "/api/market/email/finish", None, Some(finish)).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let finish = json!({ "email": email, "code": code, "name": "Sam" });
    let (status, body) = market.ask("POST", "/api/market/email/finish", None, Some(finish)).await;
    assert_eq!((status, body["account"]["name"].as_str()), (StatusCode::OK, Some("Sam")), "the code still works after a refused name");
}

#[tokio::test]
async fn a_seller_sets_up_a_profile_with_a_picture() {
    let market = Market::new();
    let maya = sign_in_for_tests(&market.app, "Maya");
    let (status, body) =
        market.ask("PUT", "/api/market/me", Some(&maya), Some(json!({ "name": "Maya Lin", "bio": "Designer in Lisbon.", "contact": { "telegram": "@maya_designs" } }))).await;
    assert_eq!((status, body["account"]["bio"].as_str()), (StatusCode::OK, Some("Designer in Lisbon.")));
    let id = body["account"]["id"].as_i64().unwrap();
    let (status, _) = market.ask("PUT", "/api/market/me", Some(&maya), Some(json!({ "name": "" }))).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    // A picture is sent as the body, and comes back as a square JPEG.
    let mut png = Vec::new();
    image::DynamicImage::new_rgb8(900, 600).write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).unwrap();
    let send = |bytes: Vec<u8>, token: Option<String>| {
        let mut request = Request::builder().method("POST").uri("/api/market/me/picture");
        if let Some(token) = token {
            request = request.header(header::COOKIE, format!("{COOKIE}={token}"));
        }
        router(market.app.clone()).oneshot(request.body(Body::from(bytes)).unwrap())
    };
    assert_eq!(send(png.clone(), None).await.unwrap().status(), StatusCode::UNAUTHORIZED);
    assert_eq!(send(b"not a picture".to_vec(), Some(maya.clone())).await.unwrap().status(), StatusCode::BAD_REQUEST);
    let response = send(png, Some(maya.clone())).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body: Value = serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes()).unwrap();
    let picture = body["account"]["picture"].as_str().unwrap().to_string();

    let response = router(market.app.clone())
        .oneshot(Request::builder().uri(format!("/api/market/images/{picture}")).body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.headers()[header::CONTENT_TYPE], "image/jpeg");
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    assert_eq!(image::load_from_memory(&bytes).unwrap().width(), orien_market::images::PICTURE);

    // The public profile shows the open listings only.
    let (_, posted) = market.ask("POST", "/api/market/listings", Some(&maya), Some(logo())).await;
    let mut other = logo();
    other["title"] = json!("Icons");
    let (_, closed) = market.ask("POST", "/api/market/listings", Some(&maya), Some(other)).await;
    let closed = closed["listing"]["id"].as_i64().unwrap();
    market.ask("POST", &format!("/api/market/listings/{closed}/open"), Some(&maya), Some(json!({ "open": false }))).await;
    let (status, body) = market.ask("GET", &format!("/api/market/profiles/{id}"), None, None).await;
    assert_eq!((status, body["account"]["name"].as_str()), (StatusCode::OK, Some("Maya Lin")));
    assert_eq!(body["listings"].as_array().unwrap().len(), 1);
    assert_eq!(body["listings"][0]["id"], posted["listing"]["id"]);
    assert!(body["account"].get("email").is_none(), "a profile never shows the email used to sign in");
    assert_eq!(body["account"]["contact"]["telegram"], "maya_designs");
    let (_, body) = market.ask("GET", &format!("/api/market/listings/{}", posted["listing"]["id"]), None, None).await;
    assert_eq!(body["seller"]["contact"]["telegram"], "maya_designs", "a listing shows how to reach its seller");
    assert_eq!(market.ask("GET", "/api/market/images/..%2Fmarket.db", None, None).await.0, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn a_seller_proves_their_wallet_with_a_small_payment_before_listing() {
    let payments = Arc::new(Fake::default());
    *payments.epoch.lock().unwrap() = 20;
    let app = App::new(Store::in_memory().unwrap(), folder(), "https://orien.network", true, Mailer::log_only(), Screener::off(), payments.clone(), MARKET);
    let market = Market { app: Arc::new(app.unwrap()) };
    let email = "maya@example.com";
    market.ask("POST", "/api/market/email/start", None, Some(json!({ "email": email }))).await;
    let code = market.app.code_for_tests(email);
    let finish = json!({ "email": email, "code": code, "name": "Maya" });
    let request = Request::builder()
        .method("POST")
        .uri("/api/market/email/finish")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(finish.to_string()))
        .unwrap();
    let response = router(market.app.clone()).oneshot(request).await.unwrap();
    let cookie = response.headers()[header::SET_COOKIE].to_str().unwrap().to_string();
    let maya = cookie.split(';').next().unwrap().split_once('=').unwrap().1.to_string();

    // No listing before the wallet is proven.
    let (status, body) = market.ask("POST", "/api/market/listings", Some(&maya), Some(logo())).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(body["error"].as_str().unwrap().contains("verify your Orien wallet"));

    let wallet = "be5b7cc81cbe475fe002bee2597c30b0ae4f030434e93f12ade7f1a266c6258b";
    let (status, body) = market.ask("POST", "/api/market/me/wallet", Some(&maya), Some(json!({ "address": wallet }))).await;
    assert_eq!((status, body["waiting"]["pay_to"].as_str()), (StatusCode::OK, Some(MARKET)));
    let amount = body["waiting"]["amount"].as_u64().unwrap();
    let pay = |from: &str, amount: u64, epoch: u64| {
        let to = MARKET.parse().unwrap();
        payments.payments.lock().unwrap().push((to, Incoming { from: from.parse().unwrap(), amount, epoch }));
    };

    // The wrong amount, the wrong sender, or a payment from long before prove nothing.
    pay(wallet, amount + 1, 20);
    pay(&"d".repeat(64), amount, 20);
    pay(wallet, amount, 3);
    let (_, body) = market.ask("GET", "/api/market/me/wallet", Some(&maya), None).await;
    assert!(body["waiting"].is_object(), "{body}");

    pay(wallet, amount, 21);
    let (_, body) = market.ask("GET", "/api/market/me/wallet", Some(&maya), None).await;
    assert_eq!(body["wallet"], wallet);

    // A way to be reached, then listings pay to the proven wallet, whatever the form says.
    let reach = json!({ "name": "Maya", "contact": { "x": "maya" } });
    assert_eq!(market.ask("PUT", "/api/market/me", Some(&maya), Some(reach)).await.0, StatusCode::OK);
    let mut listing = logo();
    listing.as_object_mut().unwrap().remove("pay_to");
    let (status, body) = market.ask("POST", "/api/market/listings", Some(&maya), Some(listing)).await;
    assert_eq!((status, body["listing"]["pay_to"].as_str()), (StatusCode::CREATED, Some(wallet)));
    let (_, body) = market.ask("GET", "/api/market/me", Some(&maya), None).await;
    assert_eq!(body["account"]["wallet"], wallet);
}

#[tokio::test]
async fn buyers_message_sellers_and_bad_messages_are_kept_out() {
    let market = Market::screened("stolen");
    let maya = sign_in_for_tests(&market.app, "Maya");
    let sam = sign_in_for_tests(&market.app, "Sam");
    let (_, posted) = market.ask("POST", "/api/market/listings", Some(&maya), Some(logo())).await;
    let listing = posted["listing"]["id"].as_i64().unwrap();
    let path = format!("/api/market/listings/{listing}/messages");

    assert_eq!(market.ask("POST", &path, None, Some(json!({ "body": "Hi" }))).await.0, StatusCode::UNAUTHORIZED);
    assert_eq!(market.ask("POST", &path, Some(&maya), Some(json!({ "body": "Hi" }))).await.0, StatusCode::BAD_REQUEST);
    let (status, body) = market.ask("POST", &path, Some(&sam), Some(json!({ "body": "Want some stolen accounts?" }))).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(body["error"].as_str().unwrap().contains("message cannot be posted"), "{body}");

    let (status, body) = market.ask("POST", &path, Some(&sam), Some(json!({ "body": "Hi! By Friday?" }))).await;
    assert_eq!(status, StatusCode::OK);
    let conversation = body["conversation"].as_i64().unwrap();

    let (_, me) = market.ask("GET", "/api/market/me", Some(&maya), None).await;
    assert_eq!(me["unread"], 1);
    let (_, list) = market.ask("GET", "/api/market/conversations", Some(&maya), None).await;
    assert_eq!(list["conversations"][0]["with_name"], "Sam");
    assert_eq!(list["conversations"][0]["listing_title"], "Logo design");

    let thread = format!("/api/market/conversations/{conversation}");
    let (_, read) = market.ask("GET", &thread, Some(&maya), None).await;
    assert_eq!(read["messages"][0]["body"], "Hi! By Friday?");
    let (_, me) = market.ask("GET", "/api/market/me", Some(&maya), None).await;
    assert_eq!(me["unread"], 0);
    let (status, _) = market.ask("POST", &format!("{thread}/messages"), Some(&maya), Some(json!({ "body": "Yes!" }))).await;
    assert_eq!(status, StatusCode::OK);

    let eve = sign_in_for_tests(&market.app, "Eve");
    assert_eq!(market.ask("GET", &thread, Some(&eve), None).await.0, StatusCode::NOT_FOUND);

    market.ask("POST", &format!("{thread}/block"), Some(&maya), Some(json!({ "block": true }))).await;
    let (status, body) = market.ask("POST", &format!("{thread}/messages"), Some(&sam), Some(json!({ "body": "Hello?" }))).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(body["error"].as_str().unwrap().contains("not taking messages"));
}
