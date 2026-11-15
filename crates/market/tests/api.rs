//! The marketplace server, asked the way its pages ask it.

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use http_body_util::BodyExt;
use orien_market::api::{App, COOKIE, router, sign_in_for_tests};
use orien_market::mail::Mailer;
use orien_market::screen::Screener;
use orien_market::store::Store;
use serde_json::{Value, json};
use std::sync::Arc;
use tower::ServiceExt;

struct Market {
    app: Arc<App>,
}

impl Market {
    fn screened(word: &str) -> Market {
        let app = App::new(Store::in_memory().unwrap(), "https://orien.network", true, Mailer::log_only(), Screener::refusing_for_tests(word));
        Market { app: Arc::new(app.unwrap()) }
    }

    fn new() -> Market {
        Market { app: Arc::new(App::new(Store::in_memory().unwrap(), "https://orien.network", true, Mailer::log_only(), Screener::off()).unwrap()) }
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
        "pay_to": "be5b7cc81cbe475fe002bee2597c30b0ae4f030434e93f12ade7f1a266c6258b",
        "contact": "maya@example.com"
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
    bad["pay_to"] = json!("my wallet");
    let (status, body) = market.ask("POST", "/api/market/listings", Some(&maya), Some(bad)).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(body["error"].as_str().unwrap().contains("Orien address"));
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
    assert_eq!(status, StatusCode::BAD_REQUEST, "one code a minute");
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
