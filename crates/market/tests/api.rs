//! The marketplace server, asked the way its pages ask it.

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use http_body_util::BodyExt;
use orien_market::api::{App, COOKIE, router, sign_in_for_tests};
use orien_market::images::Folder;
use orien_market::mail::Mailer;
use orien_market::prices::Prices;
use orien_market::screen::Screener;
use orien_market::store::Store;
use orien_market::wallets::Fake;
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
        Market { app: Arc::new(app.unwrap().with_prices(Prices::fixed_for_tests(65_000.0))) }
    }

    fn new() -> Market {
        Market { app: Arc::new(App::new(Store::in_memory().unwrap(), folder(), "https://orien.network", true, Mailer::log_only(), Screener::off(), Arc::new(Fake::default()), MARKET).unwrap().with_prices(Prices::fixed_for_tests(65_000.0))) }
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
async fn signing_out_ends_the_session() {
    let market = Market::new();
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
        market.ask("PUT", "/api/market/me", Some(&maya), Some(json!({ "name": "Maya_Lin", "bio": "Designer in Lisbon.", "contact": { "telegram": "@maya_designs" } }))).await;
    assert_eq!((status, body["account"]["bio"].as_str()), (StatusCode::OK, Some("Designer in Lisbon.")));
    let id = body["account"]["id"].as_i64().unwrap();
    // Names are usernames, unique whatever the case.
    let sam = sign_in_for_tests(&market.app, "Sam");
    for (bad, why) in [("", "empty"), ("Maya Lin", "a space"), ("Mа", "too short"), ("Mаya", "a Cyrillic а"), ("maya_lin", "taken")] {
        let (status, body) = market.ask("PUT", "/api/market/me", Some(&sam), Some(json!({ "name": bad }))).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{why}: {body}");
    }
    let (status, _) = market.ask("PUT", "/api/market/me", Some(&maya), Some(json!({ "name": "maya_LIN", "bio": "Designer in Lisbon.", "contact": { "telegram": "@maya_designs" } }))).await;
    assert_eq!(status, StatusCode::OK, "changing the case of your own name is fine");
    let (status, body) = market.ask("GET", "/api/market/users/MAYA_lin", None, None).await;
    assert_eq!((status, body["account"]["id"].as_i64()), (StatusCode::OK, Some(id)), "{body}");
    let (status, _) = market.ask("GET", "/api/market/users/nobody", None, None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);

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
    assert_eq!((status, body["account"]["name"].as_str()), (StatusCode::OK, Some("maya_LIN")));
    assert_eq!(body["listings"].as_array().unwrap().len(), 1);
    assert_eq!(body["listings"][0]["id"], posted["listing"]["id"]);
    assert!(body["account"].get("email").is_none(), "a profile never shows the email used to sign in");
    assert_eq!(body["account"]["contact"]["telegram"], "maya_designs");
    let (_, body) = market.ask("GET", &format!("/api/market/listings/{}", posted["listing"]["id"]), None, None).await;
    assert_eq!(body["seller"]["contact"]["telegram"], "maya_designs", "a listing shows how to reach its seller");
    assert_eq!(market.ask("GET", "/api/market/images/..%2Fmarket.db", None, None).await.0, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn a_seller_links_their_wallet_before_listing() {
    let payments = Arc::new(Fake::default());
    let app = App::new(Store::in_memory().unwrap(), folder(), "https://orien.network", true, Mailer::log_only(), Screener::off(), payments.clone(), MARKET);
    let market = Market { app: Arc::new(app.unwrap().with_prices(Prices::fixed_for_tests(65_000.0))) };
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

    // No listing before a wallet is linked.
    let (status, body) = market.ask("POST", "/api/market/listings", Some(&maya), Some(logo())).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(body["error"].as_str().unwrap().contains("link your Orien wallet"));

    // Typed on the website: an address that never held ORN is likely a typo.
    let wallet = "be5b7cc81cbe475fe002bee2597c30b0ae4f030434e93f12ade7f1a266c6258b";
    let (status, body) = market.ask("POST", "/api/market/me/wallet", Some(&maya), Some(json!({ "address": wallet }))).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(body["error"].as_str().unwrap().contains("never held any ORN"), "{body}");

    // Orien Wallet names its own address, which may be brand new: linked at once.
    let (status, body) = market.ask("POST", "/api/market/me/wallet", Some(&maya), Some(json!({ "address": wallet, "app": true }))).await;
    assert_eq!((status, body["wallet"].as_str()), (StatusCode::OK, Some(wallet)));

    // A typed address that has held ORN is linked at once too.
    let other = "d".repeat(64);
    payments.held.lock().unwrap().push(other.parse().unwrap());
    let (status, body) = market.ask("POST", "/api/market/me/wallet", Some(&maya), Some(json!({ "address": other }))).await;
    assert_eq!((status, body["wallet"].as_str()), (StatusCode::OK, Some(other.as_str())));
    market.ask("POST", "/api/market/me/wallet", Some(&maya), Some(json!({ "address": wallet, "app": true }))).await;
    let (_, body) = market.ask("GET", "/api/market/me/wallet", Some(&maya), None).await;
    assert_eq!(body["wallet"], wallet);

    // A way to be reached, then listings pay to the linked wallet, whatever the form says.
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

#[tokio::test]
async fn a_seller_adds_pictures_to_a_listing() {
    let market = Market::screened("never");
    let maya = sign_in_for_tests(&market.app, "Maya");
    let sam = sign_in_for_tests(&market.app, "Sam");
    let (_, posted) = market.ask("POST", "/api/market/listings", Some(&maya), Some(logo())).await;
    let id = posted["listing"]["id"].as_i64().unwrap();
    let mut png = Vec::new();
    image::DynamicImage::new_rgb8(800, 800).write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).unwrap();
    let upload = |token: &str| {
        let request = Request::builder()
            .method("POST")
            .uri(format!("/api/market/listings/{id}/pictures"))
            .header(header::COOKIE, format!("{COOKIE}={token}"))
            .body(Body::from(png.clone()))
            .unwrap();
        router(market.app.clone()).oneshot(request)
    };
    assert_eq!(upload(&sam).await.unwrap().status(), StatusCode::NOT_FOUND, "only the seller");
    let response = upload(&maya).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body: Value = serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes()).unwrap();
    let picture = body["listing"]["pictures"][0].as_str().unwrap().to_string();
    let (_, shown) = market.ask("GET", "/api/market/listings", None, None).await;
    assert_eq!(shown["listings"][0]["pictures"][0], picture.as_str());
    let response = router(market.app.clone())
        .oneshot(Request::builder().uri(format!("/api/market/images/{picture}")).body(Body::empty()).unwrap())
        .await
        .unwrap();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let back = image::load_from_memory(&bytes).unwrap();
    assert_eq!((back.width(), back.height()), (orien_market::images::LISTING_WIDTH, orien_market::images::LISTING_HEIGHT));
    let (status, body) = market.ask("DELETE", &format!("/api/market/listings/{id}/pictures/{picture}"), Some(&maya), None).await;
    assert_eq!((status, body["listing"]["pictures"].as_array().unwrap().len()), (StatusCode::OK, 0));
}

#[tokio::test]
async fn swap_offers_go_on_the_board_and_are_taken_through_requests() {
    let market = Market::new();
    let maya = sign_in_for_tests(&market.app, "Maya");
    let sam = sign_in_for_tests(&market.app, "Sam");
    let (status, _) = market.ask("POST", "/api/market/swaps", None, Some(json!({ "orn": 1, "sats": 20000 }))).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let (status, body) = market.ask("POST", "/api/market/swaps", Some(&maya), Some(json!({ "orn": 500_000_000_000u64, "sats": 1_000_000 }))).await;
    assert_eq!(status, StatusCode::CREATED);
    let id = body["offer"]["id"].as_i64().unwrap();
    let (_, board) = market.ask("GET", "/api/market/swaps", None, None).await;
    assert_eq!(board["offers"][0]["seller_name"], "Maya");
    assert_eq!(board["bitcoin"]["usd"], 65_000.0, "with Bitcoin's dollar price");

    let mayas_phone = "ab".repeat(32);
    market.ask("POST", "/api/market/push/register", Some(&maya), Some(json!({ "token": mayas_phone, "addresses": [] }))).await;
    let (status, body) = market.ask("POST", &format!("/api/market/swaps/{id}/take"), Some(&sam), None).await;
    assert_eq!((status, body["swap"]["offer"]["state"].as_str()), (StatusCode::OK, Some("held")));
    let notebook = Notebook::default();
    assert_eq!(orien_market::push::check_once(&market.app, &notebook).unwrap(), 1, "the seller's phone hears");
    let told: Value = serde_json::from_str(&notebook.0.lock().unwrap()[0].2).unwrap();
    assert_eq!((told["aps"]["alert"]["body"].as_str(), told["swap"].as_i64()), (Some("Sam took your offer of 500 ORN"), Some(id)));
    let (status, _) = market.ask("POST", &format!("/api/market/swaps/{id}/take"), Some(&sam), None).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "already held");
    let (_, mine) = market.ask("GET", "/api/market/swaps/mine", Some(&maya), None).await;
    assert_eq!((mine["swaps"][0]["selling"].as_bool(), mine["swaps"][0]["taker_name"].as_str()), (Some(true), Some("Sam")));
    let (status, _) = market.ask("POST", &format!("/api/market/swaps/{id}/offer"), Some(&maya), Some(json!({ "code": "orien-offer:junk" }))).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (status, _) = market.ask("POST", &format!("/api/market/swaps/{id}/close"), Some(&maya), None).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "a held offer cannot be withdrawn");
}

#[tokio::test]
async fn the_contact_form_sends_and_keeps_floods_and_bad_messages_out() {
    let market = Market::new();
    let message = |email: &str, text: &str| json!({ "name": "Ada", "email": email, "message": text });
    let (status, answer) = market.ask("POST", "/api/market/contact", None, Some(message("ada@example.com", "Hello, I have a question about mining."))).await;
    assert_eq!(status, StatusCode::OK, "{answer}");
    assert_eq!(answer["sent"], true);

    let (status, _) = market.ask("POST", "/api/market/contact", None, Some(message("ada@example.com", "hi"))).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "too short");
    let (status, _) = market.ask("POST", "/api/market/contact", None, Some(message("not an email", "Hello there, a question."))).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "not an email");

    // A program filling in the hidden field is told it worked, and nothing counts.
    let mut trap = message("bot@example.com", "Buy cheap things here now!");
    trap["website"] = json!("https://spam.example");
    let (status, _) = market.ask("POST", "/api/market/contact", None, Some(trap)).await;
    assert_eq!(status, StatusCode::OK);

    for _ in 0..2 {
        let (status, _) = market.ask("POST", "/api/market/contact", None, Some(message("ada@example.com", "Another question for you."))).await;
        assert_eq!(status, StatusCode::OK);
    }
    let (status, answer) = market.ask("POST", "/api/market/contact", None, Some(message("ada@example.com", "And yet another question."))).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(answer["error"].as_str().unwrap().contains("too many"), "{answer}");
}

/// Notifications written down instead of sent to Apple.
#[derive(Default)]
struct Notebook(std::sync::Mutex<Vec<(String, bool, String)>>, std::sync::atomic::AtomicBool);

impl orien_market::push::Notify for Notebook {
    fn notify(&self, token: &str, sandbox: bool, payload: &str) -> orien_market::push::Sent {
        self.0.lock().unwrap().push((token.into(), sandbox, payload.into()));
        if self.1.load(std::sync::atomic::Ordering::Relaxed) { orien_market::push::Sent::Gone } else { orien_market::push::Sent::Delivered }
    }
}

#[tokio::test]
async fn phones_hear_about_payments_to_their_wallets_once() {
    use orien_market::push::check_once;
    use orien_market::wallets::Incoming;
    let payments = Arc::new(Fake::default());
    *payments.epoch.lock().unwrap() = 30;
    let app = App::new(Store::in_memory().unwrap(), folder(), "https://orien.network", true, Mailer::log_only(), Screener::off(), payments.clone(), MARKET);
    let market = Market { app: Arc::new(app.unwrap().with_prices(Prices::fixed_for_tests(65_000.0))) };
    let phone = "ab".repeat(32);
    let wallet = "cd".repeat(32);
    let other = "ef".repeat(32);
    let id = |hex: &str| orien_crypto::Hash32(hex::decode(hex).unwrap().try_into().unwrap());
    let pay = |to: &str, amount: u64| payments.payments.lock().unwrap().push((id(to), Incoming { from: id(&other), amount, epoch: 30 }));

    // Already there when the phone registers: not told about.
    pay(&wallet, 5_000_000_000);
    let (status, body) = market.ask("POST", "/api/market/push/register", None, Some(json!({ "token": phone, "sandbox": true, "addresses": [wallet, wallet.to_uppercase()] }))).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["addresses"], 1, "the same address twice is one");
    let notebook = Notebook::default();
    assert_eq!(check_once(&market.app, &notebook).unwrap(), 0);

    // A new payment: told once, to the phone, as a sandbox build.
    pay(&wallet, 25_000_000_000);
    pay(&other, 1_000_000_000);
    assert_eq!(check_once(&market.app, &notebook).unwrap(), 1);
    assert_eq!(check_once(&market.app, &notebook).unwrap(), 0, "not twice");
    let sent = notebook.0.lock().unwrap().clone();
    assert_eq!(sent.len(), 1);
    assert_eq!((sent[0].0.as_str(), sent[0].1), (phone.as_str(), true));
    let payload: Value = serde_json::from_str(&sent[0].2).unwrap();
    assert_eq!(payload["aps"]["alert"]["body"], "You received 25 ORN");
    assert_eq!(payload["address"], wallet);

    // Bad requests are refused in words.
    let too_many: Vec<String> = (0..51).map(|i| format!("{i:064x}")).collect();
    for bad in [json!({ "token": "nothex", "addresses": [] }), json!({ "token": phone, "addresses": ["123"] }), json!({ "token": phone, "addresses": too_many })] {
        let (status, body) = market.ask("POST", "/api/market/push/register", None, Some(bad)).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    }

    // Apple says the phone is gone: it is forgotten.
    notebook.1.store(true, std::sync::atomic::Ordering::Relaxed);
    pay(&wallet, 2_000_000_000);
    check_once(&market.app, &notebook).unwrap();
    notebook.1.store(false, std::sync::atomic::Ordering::Relaxed);
    pay(&wallet, 3_000_000_000);
    let before = notebook.0.lock().unwrap().len();
    check_once(&market.app, &notebook).unwrap();
    assert_eq!(notebook.0.lock().unwrap().len(), before, "a forgotten phone hears nothing");

    // Registering again, then unregistering, ends it too.
    market.ask("POST", "/api/market/push/register", None, Some(json!({ "token": phone, "sandbox": false, "addresses": [wallet] }))).await;
    check_once(&market.app, &notebook).unwrap();
    let (status, _) = market.ask("POST", "/api/market/push/unregister", None, Some(json!({ "token": phone }))).await;
    assert_eq!(status, StatusCode::OK);
    pay(&wallet, 4_000_000_000);
    let before = notebook.0.lock().unwrap().len();
    check_once(&market.app, &notebook).unwrap();
    assert_eq!(notebook.0.lock().unwrap().len(), before);
}

#[tokio::test]
async fn phones_hear_about_mining_rewards_merged() {
    use orien_market::push::check_once;
    let payments = Arc::new(Fake::default());
    *payments.epoch.lock().unwrap() = 30;
    let app = App::new(Store::in_memory().unwrap(), folder(), "https://orien.network", true, Mailer::log_only(), Screener::off(), payments.clone(), MARKET);
    let market = Market { app: Arc::new(app.unwrap().with_prices(Prices::fixed_for_tests(65_000.0))) };
    let (phone, miner) = ("ab".repeat(32), "cd".repeat(32));
    let id = orien_crypto::Hash32(hex::decode(&miner).unwrap().try_into().unwrap());
    let mine = |amount: u64, epoch: u64| payments.mined.lock().unwrap().push((id, amount, epoch));

    mine(9_000_000_000, 30);
    market.ask("POST", "/api/market/push/register", None, Some(json!({ "token": phone, "sandbox": true, "addresses": [miner] }))).await;
    let notebook = Notebook::default();
    assert_eq!(check_once(&market.app, &notebook).unwrap(), 0, "rewards from before registering are not told");

    // Muting "received" leaves mining alone; muting "mining" stops it.
    market.ask("POST", "/api/market/push/register", None, Some(json!({ "token": phone, "sandbox": true, "addresses": [miner], "mute": ["received", "mining"] }))).await;
    mine(1_000_000_000, 30);
    assert_eq!(check_once(&market.app, &notebook).unwrap(), 0, "mining muted");
    market.ask("POST", "/api/market/push/register", None, Some(json!({ "token": phone, "sandbox": true, "addresses": [miner], "mute": ["received"] }))).await;

    // Two rewards seen at once: one notification with their sum.
    mine(4_000_000_000, 30);
    mine(8_004_000_000, 31);
    *payments.epoch.lock().unwrap() = 31;
    assert_eq!(check_once(&market.app, &notebook).unwrap(), 1);
    assert_eq!(check_once(&market.app, &notebook).unwrap(), 0, "not twice");
    let sent = notebook.0.lock().unwrap().clone();
    let payload: Value = serde_json::from_str(&sent[0].2).unwrap();
    assert_eq!(payload["aps"]["alert"]["title"], "Orien Wallet");
    assert_eq!(payload["aps"]["alert"]["body"], "You mined 12 ORN");
    assert_eq!(payload["address"], miner);
}

#[tokio::test]
async fn wallets_on_older_swap_rules_cannot_take_once_the_gate_is_on() {
    let app = App::new(Store::in_memory().unwrap(), folder(), "https://orien.network", true, Mailer::log_only(), Screener::off(), Arc::new(Fake::default()), MARKET)
        .unwrap()
        .with_prices(Prices::fixed_for_tests(65_000.0))
        .with_min_swap_version(2);
    let market = Market { app: Arc::new(app) };
    let maya = sign_in_for_tests(&market.app, "Maya");
    let sam = sign_in_for_tests(&market.app, "Sam");
    let (_, body) = market.ask("POST", "/api/market/swaps", Some(&maya), Some(json!({ "orn": 5_000_000_000u64, "sats": 15_000 }))).await;
    let id = body["offer"]["id"].as_i64().unwrap();
    let buyer = "bb".repeat(32);
    let path = format!("/api/market/swaps/{id}/take");
    let (status, body) = market.ask("POST", &path, Some(&sam), Some(json!({ "address": buyer }))).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(body["error"].as_str().unwrap().contains("update Orien Wallet"), "{body}");
    let (status, body) = market.ask("POST", &path, Some(&sam), Some(json!({ "address": buyer, "swap_version": 2 }))).await;
    assert_eq!(status, StatusCode::OK, "{body}");
}

#[tokio::test]
async fn sol_offers_are_shown_and_taken_only_by_wallets_that_know_them() {
    use orien_swap::sol::{SolOffer, SolReply};
    use orien_swap::swap::LockRef;
    let market = Market::new();
    let maya = sign_in_for_tests(&market.app, "Maya");
    let sam = sign_in_for_tests(&market.app, "Sam");
    let payout = "99zcMPAgZwXxoLDwor424zro9BGMATTsMk8djqov2qKf";
    // Too small, or with no payout address: refused in words.
    let (status, _) = market.ask("POST", "/api/market/swaps", Some(&maya), Some(json!({ "coin": "sol", "orn": 5_000_000_000u64, "lamports": 1000, "payout": payout }))).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (status, _) = market.ask("POST", "/api/market/swaps", Some(&maya), Some(json!({ "coin": "sol", "orn": 5_000_000_000u64, "lamports": 200_000_000u64 }))).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (status, body) = market.ask("POST", "/api/market/swaps", Some(&maya), Some(json!({ "coin": "sol", "orn": 5_000_000_000u64, "lamports": 200_000_000u64, "payout": payout }))).await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    assert_eq!((body["offer"]["coin"].as_str(), body["offer"]["lamports"].as_u64(), body["offer"]["sats"].as_u64()), (Some("sol"), Some(200_000_000), Some(0)));
    let id = body["offer"]["id"].as_i64().unwrap();
    market.ask("POST", "/api/market/swaps", Some(&maya), Some(json!({ "orn": 1_000_000_000u64, "sats": 20_000 }))).await;

    // Wallets from before SOL swaps see Bitcoin only; newer ones ask for both, SOL first.
    let (_, board) = market.ask("GET", "/api/market/swaps", None, None).await;
    assert!(board["offers"].as_array().unwrap().iter().all(|o| o["coin"] == "btc"));
    let (_, board) = market.ask("GET", "/api/market/swaps?coins=sol,btc", None, None).await;
    let coins: Vec<&str> = board["offers"].as_array().unwrap().iter().map(|o| o["coin"].as_str().unwrap()).collect();
    assert_eq!(coins, ["sol", "btc"]);
    assert_eq!(board["solana"]["usd"], 70.0);

    let path = format!("/api/market/swaps/{id}/take");
    let buyer = "bb".repeat(32);
    let (status, body) = market.ask("POST", &path, Some(&sam), Some(json!({ "address": buyer, "swap_version": 2 }))).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(body["error"].as_str().unwrap().contains("SOL"), "{body}");
    let (status, body) = market.ask("POST", &path, Some(&sam), Some(json!({ "address": buyer, "swap_version": 3 }))).await;
    assert_eq!(status, StatusCode::OK, "{body}");

    // The seller's wallet posts a SOL offer code, checked against the offer.
    let offer = |lamports: u64, payout: &str| SolOffer {
        network: "orien".into(),
        orn: 5_000_000_000,
        lamports,
        hash: [7; 32],
        lock: LockRef { from: MARKET.parse().unwrap(), seq: 1 },
        buyer: buyer.parse().unwrap(),
        payout: payout.into(),
        fee: payout.into(),
    };
    let code_path = format!("/api/market/swaps/{id}/offer");
    let (status, _) = market.ask("POST", &code_path, Some(&maya), Some(json!({ "code": offer(100_000_000, payout).code() }))).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "another price");
    let (status, _) = market.ask("POST", &code_path, Some(&maya), Some(json!({ "code": offer(200_000_000, "11111111111111111111111111111111").code() }))).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "another payout");
    let (status, body) = market.ask("POST", &code_path, Some(&maya), Some(json!({ "code": offer(200_000_000, payout).code() }))).await;
    assert_eq!(status, StatusCode::OK, "{body}");

    let reply = |hash: [u8; 32]| SolReply { hash, lock: payout.into(), refund: payout.into() }.code();
    let good_reply = SolReply { hash: [7; 32], lock: payout.into(), refund: "11111111111111111111111111111111".into() }.code();
    let reply_path = format!("/api/market/swaps/{id}/reply");
    let (status, _) = market.ask("POST", &reply_path, Some(&sam), Some(json!({ "code": reply([8; 32]) }))).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "another swap's reply");
    // The reply helper pays refunds to the payout itself: refused.
    let (status, body) = market.ask("POST", &reply_path, Some(&sam), Some(json!({ "code": reply([7; 32]) }))).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "refund = payout: {body}");
    let (status, body) = market.ask("POST", &reply_path, Some(&sam), Some(json!({ "code": good_reply }))).await;
    assert_eq!(status, StatusCode::OK, "{body}");
}

#[tokio::test]
async fn a_wallet_is_verified_by_a_small_payment_and_then_named() {
    use orien_market::wallets::Incoming;
    let payments = Arc::new(Fake::default());
    *payments.epoch.lock().unwrap() = 40;
    let app = App::new(Store::in_memory().unwrap(), folder(), "https://orien.network", true, Mailer::log_only(), Screener::off(), payments.clone(), MARKET);
    let market = Market { app: Arc::new(app.unwrap().with_prices(Prices::fixed_for_tests(65_000.0))) };
    let id = |hex: &str| orien_crypto::Hash32(hex::decode(hex).unwrap().try_into().unwrap());
    let (maya, eve) = (sign_in_for_tests(&market.app, "Maya"), sign_in_for_tests(&market.app, "Eve"));
    let wallet = "7a".repeat(32);
    market.ask("POST", "/api/market/me/wallet", Some(&maya), Some(json!({ "address": wallet, "app": true }))).await;
    // Eve names Maya's wallet as her own: allowed for payouts, never verified.
    market.ask("POST", "/api/market/me/wallet", Some(&eve), Some(json!({ "address": wallet, "app": true }))).await;

    let (status, _) = market.ask("GET", &format!("/api/market/wallets/{wallet}"), None, None).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "nobody has verified it yet");

    let (status, body) = market.ask("POST", "/api/market/me/wallet/verify", Some(&maya), None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let amount = body["proof"]["amount"].as_u64().unwrap();
    assert!((100_000..1_000_000).contains(&amount) && amount % 1_000 == 0, "{amount}");
    assert_eq!(body["proof"]["to"], MARKET);
    assert_eq!(body["proof"]["address"], wallet);
    assert_eq!(body["proof"]["verified"], false);
    let (_, again) = market.ask("POST", "/api/market/me/wallet/verify", Some(&maya), None).await;
    assert_eq!(again["proof"]["amount"], amount, "the same amount until it is paid");
    let (_, theirs) = market.ask("POST", "/api/market/me/wallet/verify", Some(&eve), None).await;
    assert_ne!(theirs["proof"]["amount"], amount, "each waiting proof has its own amount");

    // A wrong amount, or the right one from another wallet, proves nothing.
    let pay = |from: &str, amount: u64| payments.payments.lock().unwrap().push((id(MARKET), Incoming { from: id(from), amount, epoch: 40 }));
    pay(&wallet, amount + 1_000);
    pay(&"11".repeat(32), amount);
    orien_market::proofs::check_once(&market.app).unwrap();
    let (_, body) = market.ask("GET", "/api/market/me/wallet", Some(&maya), None).await;
    assert_eq!(body["verified"], false);

    pay(&wallet, amount);
    assert_eq!(orien_market::proofs::check_once(&market.app).unwrap(), 1);
    let (_, body) = market.ask("GET", "/api/market/me/wallet", Some(&maya), None).await;
    assert_eq!(body["verified"], true);
    let (_, body) = market.ask("GET", "/api/market/me/wallet", Some(&eve), None).await;
    assert_eq!(body["verified"], false, "Eve never paid from it");

    let (status, body) = market.ask("GET", &format!("/api/market/wallets/{}", wallet.to_uppercase()), None, None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["name"], "Maya");
    let (_, body) = market.ask("POST", "/api/market/wallets/lookup", None, Some(json!({ "addresses": [wallet, "22".repeat(32), "nonsense"] }))).await;
    assert_eq!(body["profiles"].as_object().unwrap().len(), 1);
    assert_eq!(body["profiles"][&wallet]["name"], "Maya");
    let too_many: Vec<String> = (0..101).map(|i| format!("{i:064x}")).collect();
    let (status, _) = market.ask("POST", "/api/market/wallets/lookup", None, Some(json!({ "addresses": too_many }))).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    // A second wallet, named in her app: verified by choice, by its own payment.
    let savings = "33".repeat(32);
    let (status, body) = market
        .ask("PUT", "/api/market/me/wallets", Some(&maya), Some(json!({ "device": "mac", "wallets": [{ "address": savings, "name": "Savings" }] })))
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let (status, body) = market.ask("POST", "/api/market/me/wallet/verify", Some(&maya), Some(json!({ "address": savings }))).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["proof"]["name"], "Savings");
    pay(&savings, body["proof"]["amount"].as_u64().unwrap());
    orien_market::proofs::check_once(&market.app).unwrap();
    let (_, body) = market.ask("POST", "/api/market/wallets/lookup", None, Some(json!({ "addresses": [wallet, savings] }))).await;
    assert_eq!(body["profiles"][&savings]["name"], "Maya");
    assert_eq!(body["profiles"][&savings]["wallet_name"], "Savings");
    let (_, mine) = market.ask("GET", "/api/market/me/wallet", Some(&maya), None).await;
    assert_eq!(mine["wallets"].as_array().unwrap().len(), 2);
    let maya_id = body["profiles"][&savings]["id"].as_i64().unwrap();
    let (_, profile) = market.ask("GET", &format!("/api/market/profiles/{maya_id}"), None, None).await;
    assert_eq!(profile["wallets"].as_array().unwrap().len(), 2, "{profile}");
    assert_eq!(profile["account"]["wallet_verified"], true);

    // Linking another wallet to be paid at keeps the verified ones.
    market.ask("POST", "/api/market/me/wallet", Some(&maya), Some(json!({ "address": savings, "app": true }))).await;
    let (status, _) = market.ask("GET", &format!("/api/market/wallets/{wallet}"), None, None).await;
    assert_eq!(status, StatusCode::OK);

    // Removing one stops it being named.
    let (status, _) = market.ask("POST", "/api/market/me/wallet/remove", Some(&maya), Some(json!({ "address": wallet }))).await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = market.ask("GET", &format!("/api/market/wallets/{wallet}"), None, None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // Found by name to pay: only people with a verified wallet.
    let (status, body) = market.ask("GET", "/api/market/profiles?q=MA", None, None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let found = body["profiles"].as_array().unwrap();
    assert_eq!(found.len(), 1, "{body}");
    assert_eq!(found[0]["name"], "Maya");
    assert_eq!(found[0]["wallets"][0]["name"], "Savings");
    let (_, body) = market.ask("GET", "/api/market/profiles?q=eve", None, None).await;
    assert_eq!(body["profiles"].as_array().unwrap().len(), 0, "Eve verified nothing");
    let (_, body) = market.ask("GET", "/api/market/profiles?q=m", None, None).await;
    assert_eq!(body["profiles"].as_array().unwrap().len(), 0, "one letter finds nobody");
    let (_, body) = market.ask("GET", "/api/market/profiles?q=%25", None, None).await;
    assert_eq!(body["profiles"].as_array().unwrap().len(), 0);
}

#[tokio::test]
async fn a_signed_in_phone_hears_about_messages_and_payment_requests() {
    use orien_market::push::check_once;
    let market = Market::new();
    let maya = sign_in_for_tests(&market.app, "Maya");
    let sam = sign_in_for_tests(&market.app, "Sam");
    let phone = "ab".repeat(32);
    // Maya's phone, signed in, with no wallets of its own yet.
    let (status, body) = market.ask("POST", "/api/market/push/register", Some(&maya), Some(json!({ "token": phone, "addresses": [] }))).await;
    assert_eq!((status, body["account"].as_bool()), (StatusCode::OK, Some(true)), "{body}");

    let (_, posted) = market.ask("POST", "/api/market/listings", Some(&maya), Some(logo())).await;
    let listing = posted["listing"]["id"].as_i64().unwrap();
    let long = format!("Hi! {}", "Can you do it by Friday? ".repeat(10));
    let (_, body) = market.ask("POST", &format!("/api/market/listings/{listing}/messages"), Some(&sam), Some(json!({ "body": long }))).await;
    let conversation = body["conversation"].as_i64().unwrap();
    let notebook = Notebook::default();
    assert_eq!(check_once(&market.app, &notebook).unwrap(), 1);
    let sent = notebook.0.lock().unwrap().clone();
    let payload: Value = serde_json::from_str(&sent[0].2).unwrap();
    let text = payload["aps"]["alert"]["body"].as_str().unwrap();
    assert!(text.starts_with("Sam: Hi! Can you do it by Friday?") && text.ends_with('…') && text.chars().count() < 140, "{text}");
    assert_eq!((payload["kind"].as_str(), payload["conversation"].as_i64()), (Some("message"), Some(conversation)));

    // Sam's own messages don't reach Sam; Maya's request reaches only Sam's phones (none).
    let (status, _) = market.ask("POST", &format!("/api/market/conversations/{conversation}/request"), Some(&maya), Some(json!({ "amount": 2_500_000_000u64 }))).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(check_once(&market.app, &notebook).unwrap(), 0);

    // Sam's phone signs in, and hears Maya's next request.
    let sams_phone = "cd".repeat(32);
    market.ask("POST", "/api/market/push/register", Some(&sam), Some(json!({ "token": sams_phone, "addresses": [] }))).await;
    market.ask("POST", &format!("/api/market/conversations/{conversation}/request"), Some(&maya), Some(json!({ "amount": 2_500_000_000u64 }))).await;
    assert_eq!(check_once(&market.app, &notebook).unwrap(), 1);
    let last: Value = serde_json::from_str(&notebook.0.lock().unwrap().last().unwrap().2).unwrap();
    assert_eq!(last["aps"]["alert"]["body"], "Maya asks you to pay 2.5 ORN");

    // Registered again signed out, with no wallets: the phone is forgotten.
    market.ask("POST", "/api/market/push/register", None, Some(json!({ "token": phone, "addresses": [] }))).await;
    market.ask("POST", &format!("/api/market/conversations/{conversation}/messages"), Some(&sam), Some(json!({ "body": "Ok" }))).await;
    assert_eq!(check_once(&market.app, &notebook).unwrap(), 0);
}

#[tokio::test]
async fn blocking_a_person_from_their_profile_works_everywhere() {
    let market = Market::new();
    let maya = sign_in_for_tests(&market.app, "Maya");
    let sam = sign_in_for_tests(&market.app, "Sam");
    let (_, posted) = market.ask("POST", "/api/market/listings", Some(&maya), Some(logo())).await;
    let listing = posted["listing"]["id"].as_i64().unwrap();
    let maya_id = posted["listing"]["seller"].as_i64().unwrap();
    let (_, me) = market.ask("GET", "/api/market/me", Some(&sam), None).await;
    let sam_id = me["account"]["id"].as_i64().unwrap();

    let (_, profile) = market.ask("GET", &format!("/api/market/profiles/{maya_id}"), Some(&sam), None).await;
    assert_eq!(profile["blocked"], false);
    let (_, public) = market.ask("GET", &format!("/api/market/profiles/{maya_id}"), None, None).await;
    assert!(public.get("blocked").is_none(), "only for someone signed in");

    // Maya blocks Sam from his profile: he can't write, and nothing is made.
    let (status, _) = market.ask("POST", &format!("/api/market/accounts/{sam_id}/block"), Some(&maya), Some(json!({ "block": true }))).await;
    assert_eq!(status, StatusCode::OK);
    let (_, profile) = market.ask("GET", "/api/market/users/sam", Some(&maya), None).await;
    assert_eq!(profile["blocked"], true);
    let (status, body) = market.ask("POST", &format!("/api/market/listings/{listing}/messages"), Some(&sam), Some(json!({ "body": "Hi" }))).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(body["error"].as_str().unwrap().contains("not taking messages"), "{body}");
    let (_, list) = market.ask("GET", "/api/market/conversations", Some(&sam), None).await;
    assert_eq!(list["conversations"].as_array().unwrap().len(), 0, "no empty conversation left behind");

    // Sam blocks Maya: her listings leave his browse, not anyone else's.
    market.ask("POST", &format!("/api/market/accounts/{maya_id}/block"), Some(&sam), Some(json!({ "block": true }))).await;
    let (_, browse) = market.ask("GET", "/api/market/listings", Some(&sam), None).await;
    assert_eq!(browse["listings"].as_array().unwrap().len(), 0);
    let (_, browse) = market.ask("GET", "/api/market/listings", None, None).await;
    assert_eq!(browse["listings"].as_array().unwrap().len(), 1);

    // Both unblock: messages flow again.
    market.ask("POST", &format!("/api/market/accounts/{sam_id}/block"), Some(&maya), Some(json!({ "block": false }))).await;
    market.ask("POST", &format!("/api/market/accounts/{maya_id}/block"), Some(&sam), Some(json!({ "block": false }))).await;
    let (status, _) = market.ask("POST", &format!("/api/market/listings/{listing}/messages"), Some(&sam), Some(json!({ "body": "Hi" }))).await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = market.ask("POST", &format!("/api/market/accounts/{sam_id}/block"), Some(&sam), Some(json!({ "block": true }))).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "not yourself");
}

#[tokio::test]
async fn two_devices_pass_sealed_bytes_through_a_pairing_mailbox() {
    use base64::Engine;
    let market = Market::new();
    let b64 = |bytes: &[u8]| base64::engine::general_purpose::STANDARD.encode(bytes);
    let (status, body) = market.ask("POST", "/api/market/pair", None, None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let code = body["code"].as_u64().unwrap();
    assert!((1..=9999).contains(&code));
    let slot = |s: &str| format!("/api/market/pair/{code}/{s}");

    // Nothing there yet: keep polling.
    assert_eq!(market.ask("GET", &slot("a"), None, None).await.0, StatusCode::ACCEPTED);
    let (status, _) = market.ask("PUT", &slot("a"), None, Some(json!({ "data": b64(b"spake a") }))).await;
    assert_eq!(status, StatusCode::OK);
    let (status, body) = market.ask("GET", &slot("a"), None, None).await;
    assert_eq!((status, body["data"].as_str()), (StatusCode::OK, Some(b64(b"spake a").as_str())));
    // Write-once.
    assert_eq!(market.ask("PUT", &slot("a"), None, Some(json!({ "data": b64(b"other") }))).await.0, StatusCode::CONFLICT);
    // Bad slots, bad data, too much data.
    assert_eq!(market.ask("PUT", &slot("e"), None, Some(json!({ "data": b64(b"x") }))).await.0, StatusCode::NOT_FOUND);
    assert_eq!(market.ask("PUT", &slot("b"), None, Some(json!({ "data": "not base64!" }))).await.0, StatusCode::BAD_REQUEST);
    let big = vec![7u8; 16 * 1024 + 1];
    assert_eq!(market.ask("PUT", &slot("d"), None, Some(json!({ "data": b64(&big) }))).await.0, StatusCode::PAYLOAD_TOO_LARGE);
    let full = vec![7u8; 16 * 1024];
    assert_eq!(market.ask("PUT", &slot("d"), None, Some(json!({ "data": b64(&full) }))).await.0, StatusCode::OK);
    // An unknown code.
    let other = if code == 9999 { 1 } else { code + 1 };
    assert_eq!(market.ask("GET", &format!("/api/market/pair/{other}/a"), None, None).await.0, StatusCode::NOT_FOUND);

    // Closed by a device: gone.
    assert_eq!(market.ask("DELETE", &format!("/api/market/pair/{code}"), None, None).await.0, StatusCode::OK);
    assert_eq!(market.ask("GET", &slot("a"), None, None).await.0, StatusCode::NOT_FOUND);
    assert_eq!(market.ask("DELETE", &format!("/api/market/pair/{code}"), None, None).await.0, StatusCode::NOT_FOUND);

    // Ten minutes on, a mailbox lapses by itself.
    let (_, body) = market.ask("POST", "/api/market/pair", None, None).await;
    let code = body["code"].as_u64().unwrap();
    market.app.pairs().age_for_tests(std::time::Duration::from_secs(601));
    assert_eq!(market.ask("GET", &format!("/api/market/pair/{code}/a"), None, None).await.0, StatusCode::NOT_FOUND);

    // At most 20 codes an hour from one address (two used above).
    for _ in 0..18 {
        assert_eq!(market.ask("POST", "/api/market/pair", None, None).await.0, StatusCode::OK);
    }
    assert_eq!(market.ask("POST", "/api/market/pair", None, None).await.0, StatusCode::TOO_MANY_REQUESTS);
}

#[tokio::test]
async fn a_phone_can_mute_kinds_of_notification() {
    use orien_market::push::check_once;
    use orien_market::wallets::Incoming;
    let payments = Arc::new(Fake::default());
    *payments.epoch.lock().unwrap() = 30;
    let app = App::new(Store::in_memory().unwrap(), folder(), "https://orien.network", true, Mailer::log_only(), Screener::off(), payments.clone(), MARKET);
    let market = Market { app: Arc::new(app.unwrap().with_prices(Prices::fixed_for_tests(65_000.0))) };
    let maya = sign_in_for_tests(&market.app, "Maya");
    let sam = sign_in_for_tests(&market.app, "Sam");
    let (phone, wallet) = ("ab".repeat(32), "cd".repeat(32));
    let id = |hex: &str| orien_crypto::Hash32(hex::decode(hex).unwrap().try_into().unwrap());
    let register = |mute: Option<Value>| {
        let mut body = json!({ "token": phone, "addresses": [wallet] });
        if let Some(mute) = mute {
            body["mute"] = mute;
        }
        body
    };
    let notebook = Notebook::default();
    market.ask("POST", "/api/market/push/register", Some(&maya), Some(register(Some(json!(["market", "received", "nonsense"]))))).await;
    check_once(&market.app, &notebook).unwrap(); // primes the wallet

    // Muted: a message and a payment are not told.
    let (_, posted) = market.ask("POST", "/api/market/listings", Some(&maya), Some(logo())).await;
    let listing = posted["listing"]["id"].as_i64().unwrap();
    market.ask("POST", &format!("/api/market/listings/{listing}/messages"), Some(&sam), Some(json!({ "body": "Hi" }))).await;
    payments.payments.lock().unwrap().push((id(&wallet), Incoming { from: id(&"ef".repeat(32)), amount: 5_000_000_000, epoch: 30 }));
    assert_eq!(check_once(&market.app, &notebook).unwrap(), 0);

    // Registered again without the field: nothing muted any more.
    market.ask("POST", "/api/market/push/register", Some(&maya), Some(register(None))).await;
    let (_, list) = market.ask("GET", "/api/market/conversations", Some(&sam), None).await;
    let conversation = list["conversations"][0]["id"].as_i64().unwrap();
    market.ask("POST", &format!("/api/market/conversations/{conversation}/messages"), Some(&sam), Some(json!({ "body": "Still there?" }))).await;
    payments.payments.lock().unwrap().push((id(&wallet), Incoming { from: id(&"ef".repeat(32)), amount: 7_000_000_000, epoch: 30 }));
    assert_eq!(check_once(&market.app, &notebook).unwrap(), 2, "the message and the payment");
}
