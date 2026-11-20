//! Sign in with Orien, used the way a website uses it.

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use http_body_util::BodyExt;
use orien_market::api::{App, COOKIE, router, sign_in_for_tests, sign_in_without_wallet_for_tests};
use orien_market::images::Folder;
use orien_market::mail::Mailer;
use orien_market::screen::Screener;
use orien_market::store::Store;
use orien_market::wallets::Fake;
use serde_json::{Value, json};
use std::sync::Arc;
use tower::ServiceExt;

const MARKET: &str = "0000000000000000000000000000000000000000000000000000000000000abc";
const BACK: &str = "https://shop.example/signed-in";

fn market(open: bool, developers: &[&str]) -> Arc<App> {
    let mut random = [0u8; 8];
    getrandom::fill(&mut random).unwrap();
    let folder = Folder::open(&std::env::temp_dir().join(format!("orien-oidc-test-{}", hex::encode(random)))).unwrap();
    let app = App::new(
        Store::in_memory().unwrap(),
        folder,
        "https://orien.network",
        true,
        Mailer::log_only(),
        Screener::off(),
        Arc::new(Fake::default()),
        MARKET,
    )
    .unwrap()
    .with_sign_in(open, developers.iter().map(|d| d.to_string()).collect());
    Arc::new(app)
}

/// Asks the server. `body` is JSON, or a form when `form` is set.
async fn ask(
    app: &Arc<App>,
    method: &str,
    path: &str,
    cookie: Option<&str>,
    headers: &[(&str, String)],
    body: Option<Value>,
    form: Option<&[(&str, &str)]>,
) -> (StatusCode, Value, axum::http::HeaderMap) {
    let mut request = Request::builder().method(method).uri(path);
    if let Some(token) = cookie {
        request = request.header(header::COOKIE, format!("{COOKIE}={token}"));
    }
    for (name, value) in headers {
        request = request.header(*name, value);
    }
    let body = if let Some(form) = form {
        request = request.header(header::CONTENT_TYPE, "application/x-www-form-urlencoded");
        Body::from(url::form_urlencoded::Serializer::new(String::new()).extend_pairs(form).finish())
    } else if let Some(body) = body {
        request = request.header(header::CONTENT_TYPE, "application/json");
        Body::from(body.to_string())
    } else {
        Body::empty()
    };
    let response = router(app.clone()).oneshot(request.body(body).unwrap()).await.unwrap();
    let (status, headers) = (response.status(), response.headers().clone());
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    (status, serde_json::from_slice(&bytes).unwrap_or(Value::Null), headers)
}

fn query(url: &str) -> std::collections::HashMap<String, String> {
    url::Url::parse(url).unwrap().query_pairs().into_owned().collect()
}

/// A PKCE verifier and its challenge.
fn pkce() -> (String, String) {
    let verifier = "a-verifier-of-at-least-forty-three-characters-long-0123456789".to_string();
    let challenge = URL_SAFE_NO_PAD.encode(openssl::sha::sha256(verifier.as_bytes()));
    (verifier, challenge)
}

/// Checks an ID token's signature against the published keys and gives its claims.
async fn verified(app: &Arc<App>, id_token: &str) -> Value {
    let (_, jwks, _) = ask(app, "GET", "/api/market/oidc/jwks", None, &[], None, None).await;
    let key = &jwks["keys"][0];
    let n = openssl::bn::BigNum::from_slice(&URL_SAFE_NO_PAD.decode(key["n"].as_str().unwrap()).unwrap()).unwrap();
    let e = openssl::bn::BigNum::from_slice(&URL_SAFE_NO_PAD.decode(key["e"].as_str().unwrap()).unwrap()).unwrap();
    let public = openssl::pkey::PKey::from_rsa(openssl::rsa::Rsa::from_public_components(n, e).unwrap()).unwrap();
    let parts: Vec<&str> = id_token.split('.').collect();
    let head: Value = serde_json::from_slice(&URL_SAFE_NO_PAD.decode(parts[0]).unwrap()).unwrap();
    assert_eq!((head["alg"].as_str(), head["kid"].as_str()), (Some("RS256"), key["kid"].as_str()));
    let mut verifier = openssl::sign::Verifier::new(openssl::hash::MessageDigest::sha256(), &public).unwrap();
    verifier.update(format!("{}.{}", parts[0], parts[1]).as_bytes()).unwrap();
    assert!(verifier.verify(&URL_SAFE_NO_PAD.decode(parts[2]).unwrap()).unwrap(), "the signature checks out");
    serde_json::from_slice(&URL_SAFE_NO_PAD.decode(parts[1]).unwrap()).unwrap()
}

#[tokio::test]
async fn a_website_signs_someone_in_and_learns_their_verified_wallets() {
    let app = market(true, &[]);
    let developer = sign_in_without_wallet_for_tests(&app, "Builder");
    let alice = sign_in_for_tests(&app, "Alice");

    // The discovery document points at the endpoints.
    let (status, discovery, headers) = ask(&app, "GET", "/.well-known/openid-configuration", None, &[], None, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(discovery["issuer"], "https://orien.network");
    assert_eq!(discovery["authorization_endpoint"], "https://orien.network/oauth/authorize/");
    assert_eq!(headers[header::ACCESS_CONTROL_ALLOW_ORIGIN], "*");

    // A page-only website registers, with no secret.
    let (status, added, _) = ask(
        &app,
        "POST",
        "/api/market/me/sites",
        Some(&developer),
        &[],
        Some(json!({ "name": "Example Shop", "returns": [BACK] })),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{added}");
    let client_id = added["site"]["client_id"].as_str().unwrap().to_string();
    assert!(added["secret"].is_null());

    // The authorize page asks what to show: the website, and Alice.
    let path = format!("/api/market/oidc/request?client_id={client_id}&redirect_uri={}", urlencode(BACK));
    let (status, shown, _) = ask(&app, "GET", &path, Some(&alice), &[], None, None).await;
    assert_eq!(status, StatusCode::OK, "{shown}");
    assert_eq!((shown["site"]["name"].as_str(), shown["site"]["host"].as_str()), (Some("Example Shop"), Some("shop.example")));
    assert_eq!(shown["you"]["name"], "Alice");
    assert_eq!(shown["you"]["wallets"].as_array().unwrap().len(), 1);
    assert_eq!(shown["you"]["allowed"], false);
    // Signed out, it says so.
    let (_, shown, _) = ask(&app, "GET", &path, None, &[], None, None).await;
    assert!(shown["you"].is_null());
    // An address the website never registered is refused.
    let bad = format!("/api/market/oidc/request?client_id={client_id}&redirect_uri={}", urlencode("https://evil.example/"));
    assert_eq!(ask(&app, "GET", &bad, Some(&alice), &[], None, None).await.0, StatusCode::BAD_REQUEST);

    // Alice allows it.
    let (verifier, challenge) = pkce();
    let allow = json!({
        "client_id": client_id, "redirect_uri": BACK, "scope": "openid profile wallets",
        "state": "s123", "nonce": "n456", "code_challenge": challenge, "code_challenge_method": "S256",
    });
    let (status, allowed, _) = ask(&app, "POST", "/api/market/oidc/allow", Some(&alice), &[], Some(allow.clone()), None).await;
    assert_eq!(status, StatusCode::OK, "{allowed}");
    let back = allowed["redirect"].as_str().unwrap();
    assert!(back.starts_with(BACK));
    let back = query(back);
    assert_eq!((back["state"].as_str(), back["iss"].as_str()), ("s123", "https://orien.network"));
    let code = back["code"].clone();

    // A wrong verifier gets nothing, and spends the code.
    let form = [
        ("grant_type", "authorization_code"),
        ("code", code.as_str()),
        ("redirect_uri", BACK),
        ("client_id", client_id.as_str()),
        ("code_verifier", "not-the-verifier-not-the-verifier-not-the-verifier"),
    ];
    let (status, refused, _) = ask(&app, "POST", "/api/market/oidc/token", None, &[], None, Some(&form)).await;
    assert_eq!((status, refused["error"].as_str()), (StatusCode::BAD_REQUEST, Some("invalid_grant")));

    // Again, properly.
    let (_, allowed, _) = ask(&app, "POST", "/api/market/oidc/allow", Some(&alice), &[], Some(allow), None).await;
    let code = query(allowed["redirect"].as_str().unwrap())["code"].clone();
    let form = [
        ("grant_type", "authorization_code"),
        ("code", code.as_str()),
        ("redirect_uri", BACK),
        ("client_id", client_id.as_str()),
        ("code_verifier", verifier.as_str()),
    ];
    let (status, tokens, headers) = ask(&app, "POST", "/api/market/oidc/token", None, &[], None, Some(&form)).await;
    assert_eq!(status, StatusCode::OK, "{tokens}");
    assert_eq!(headers[header::CACHE_CONTROL], "no-store");
    let claims = verified(&app, tokens["id_token"].as_str().unwrap()).await;
    assert_eq!(claims["iss"], "https://orien.network");
    assert_eq!(claims["aud"], client_id.as_str());
    assert_eq!(claims["nonce"], "n456");
    assert_eq!(claims["name"], "Alice");
    assert!(claims["exp"].as_u64().unwrap() > claims["iat"].as_u64().unwrap());
    assert!(claims.get("email").is_none(), "the email address is never given");
    assert!(claims.get("picture").is_none() && claims.get("orien_banner").is_none(), "none set yet");
    let wallets = claims["orien_wallets"].as_array().unwrap();
    assert_eq!(wallets.len(), 1);
    assert_eq!(wallets[0]["address"].as_str().unwrap().len(), 64);

    // A code works once.
    let (status, again, _) = ask(&app, "POST", "/api/market/oidc/token", None, &[], None, Some(&form)).await;
    assert_eq!((status, again["error"].as_str()), (StatusCode::BAD_REQUEST, Some("invalid_grant")));

    // The access token reads the same, from userinfo.
    let bearer = [("authorization", format!("Bearer {}", tokens["access_token"].as_str().unwrap()))];
    let (status, info, _) = ask(&app, "GET", "/api/market/oidc/userinfo", None, &bearer, None, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!((info["sub"].clone(), info["orien_wallets"].clone()), (claims["sub"].clone(), claims["orien_wallets"].clone()));
    let forged = [("authorization", "Bearer orna_forged".to_string())];
    assert_eq!(ask(&app, "GET", "/api/market/oidc/userinfo", None, &forged, None, None).await.0, StatusCode::UNAUTHORIZED);

    // A picture and banner, once set, come as full addresses.
    let id = orien_market::api::store_for_tests(&app, |store| store.account_id_for_tests("Alice"));
    orien_market::api::store_for_tests(&app, |store| {
        store.set_picture(id, "face.webp").unwrap();
        store.set_banner(id, Some("wide.webp")).unwrap();
    });
    let (_, info, _) = ask(&app, "GET", "/api/market/oidc/userinfo", None, &bearer, None, None).await;
    assert_eq!(info["picture"], "https://orien.network/api/market/images/face.webp");
    assert_eq!(info["orien_banner"], "https://orien.network/api/market/images/wide.webp");

    // Next time the page knows Alice allowed it.
    let (_, shown, _) = ask(&app, "GET", &path, Some(&alice), &[], None, None).await;
    assert_eq!(shown["you"]["allowed"], true);

    // Switched off from the dashboard, the website signs nobody in, and its tokens stop.
    let id = app_site_id(&app);
    assert!(app_store_off(&app, id));
    assert_eq!(ask(&app, "GET", &path, Some(&alice), &[], None, None).await.0, StatusCode::BAD_REQUEST);
    assert_eq!(ask(&app, "GET", "/api/market/oidc/userinfo", None, &bearer, None, None).await.0, StatusCode::UNAUTHORIZED);
}

fn app_site_id(app: &Arc<App>) -> i64 {
    orien_market::api::store_for_tests(app, |store| store.admin_sites().unwrap()[0].id)
}

fn app_store_off(app: &Arc<App>, id: i64) -> bool {
    orien_market::api::store_for_tests(app, |store| store.set_site_off(id, true).unwrap())
}

fn urlencode(text: &str) -> String {
    url::form_urlencoded::byte_serialize(text.as_bytes()).collect()
}

#[tokio::test]
async fn a_website_with_a_server_proves_itself_with_its_secret() {
    let app = market(true, &[]);
    let developer = sign_in_without_wallet_for_tests(&app, "Builder");
    let alice = sign_in_for_tests(&app, "Alice");
    let (_, added, _) = ask(
        &app,
        "POST",
        "/api/market/me/sites",
        Some(&developer),
        &[],
        Some(json!({ "name": "Server Shop", "returns": [BACK], "confidential": true })),
        None,
    )
    .await;
    let client_id = added["site"]["client_id"].as_str().unwrap().to_string();
    let secret = added["secret"].as_str().unwrap().to_string();
    assert!(secret.starts_with("orns_"));
    // The secret is shown once: the list never has it.
    let (_, listed, _) = ask(&app, "GET", "/api/market/me/sites", Some(&developer), &[], None, None).await;
    assert!(!listed.to_string().contains(&secret));
    assert_eq!(listed["sites"][0]["has_secret"], true);

    let allow = json!({ "client_id": client_id, "redirect_uri": BACK, "scope": "openid" });
    let code_for = |allowed: Value| query(allowed["redirect"].as_str().unwrap())["code"].clone();
    let (_, allowed, _) = ask(&app, "POST", "/api/market/oidc/allow", Some(&alice), &[], Some(allow.clone()), None).await;
    let code = code_for(allowed);
    let form = [("grant_type", "authorization_code"), ("code", code.as_str()), ("redirect_uri", BACK)];
    // Without its secret, or with a wrong one: refused.
    let wrong = [("authorization", format!("Basic {}", base64::engine::general_purpose::STANDARD.encode(format!("{client_id}:orns_wrong"))))];
    let (status, refused, _) = ask(&app, "POST", "/api/market/oidc/token", None, &wrong, None, Some(&form)).await;
    assert_eq!((status, refused["error"].as_str()), (StatusCode::UNAUTHORIZED, Some("invalid_client")));
    // With it, in the Basic header.
    let (_, allowed, _) = ask(&app, "POST", "/api/market/oidc/allow", Some(&alice), &[], Some(allow), None).await;
    let code = code_for(allowed);
    let form = [("grant_type", "authorization_code"), ("code", code.as_str()), ("redirect_uri", BACK)];
    let right = [("authorization", format!("Basic {}", base64::engine::general_purpose::STANDARD.encode(format!("{client_id}:{secret}"))))];
    let (status, tokens, _) = ask(&app, "POST", "/api/market/oidc/token", None, &right, None, Some(&form)).await;
    assert_eq!(status, StatusCode::OK, "{tokens}");
    assert_eq!(verified(&app, tokens["id_token"].as_str().unwrap()).await["name"], "Alice");
}

#[tokio::test]
async fn a_page_without_a_secret_must_use_pkce_and_ask_for_openid() {
    let app = market(true, &[]);
    let developer = sign_in_without_wallet_for_tests(&app, "Builder");
    let alice = sign_in_for_tests(&app, "Alice");
    let (_, added, _) = ask(&app, "POST", "/api/market/me/sites", Some(&developer), &[], Some(json!({ "name": "Page", "returns": [BACK] })), None).await;
    let client_id = added["site"]["client_id"].as_str().unwrap();
    for allow in [
        json!({ "client_id": client_id, "redirect_uri": BACK, "scope": "openid", "state": "x" }),
        json!({ "client_id": client_id, "redirect_uri": BACK, "scope": "profile", "code_challenge": pkce().1, "code_challenge_method": "S256" }),
        json!({ "client_id": client_id, "redirect_uri": BACK, "scope": "openid", "code_challenge": pkce().1, "code_challenge_method": "plain" }),
    ] {
        let (status, allowed, _) = ask(&app, "POST", "/api/market/oidc/allow", Some(&alice), &[], Some(allow), None).await;
        assert_eq!(status, StatusCode::OK);
        let back = query(allowed["redirect"].as_str().unwrap());
        assert_eq!(back["error"], "invalid_request");
        assert!(!back.contains_key("code"));
    }
}

#[tokio::test]
async fn return_addresses_are_https_or_this_computer() {
    let app = market(true, &[]);
    let developer = sign_in_without_wallet_for_tests(&app, "Builder");
    for bad in ["http://shop.example/", "javascript:alert(1)", "https://shop.example/#x", "not an address"] {
        let (status, _, _) =
            ask(&app, "POST", "/api/market/me/sites", Some(&developer), &[], Some(json!({ "name": "Shop", "returns": [bad] })), None).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{bad}");
    }
    for good in ["http://localhost:3000/callback", "https://shop.example/cb?x=1"] {
        let (status, _, _) =
            ask(&app, "POST", "/api/market/me/sites", Some(&developer), &[], Some(json!({ "name": "Shop", "returns": [good] })), None).await;
        assert_eq!(status, StatusCode::OK, "{good}");
    }
}

#[tokio::test]
async fn until_it_opens_only_the_named_accounts_register_websites() {
    let app = market(false, &["Founder"]);
    let founder = sign_in_without_wallet_for_tests(&app, "Founder");
    let other = sign_in_without_wallet_for_tests(&app, "Someone");
    let site = json!({ "name": "Shop", "returns": [BACK] });
    let (_, mine, _) = ask(&app, "GET", "/api/market/me/sites", Some(&other), &[], None, None).await;
    assert_eq!(mine["may_register"], false);
    assert_eq!(ask(&app, "POST", "/api/market/me/sites", Some(&other), &[], Some(site.clone()), None).await.0, StatusCode::FORBIDDEN);
    let (_, mine, _) = ask(&app, "GET", "/api/market/me/sites", Some(&founder), &[], None, None).await;
    assert_eq!(mine["may_register"], true);
    assert_eq!(ask(&app, "POST", "/api/market/me/sites", Some(&founder), &[], Some(site), None).await.0, StatusCode::OK);
}

#[tokio::test]
async fn orien_wallet_signs_a_browser_in_with_a_pass_that_works_once() {
    let app = market(true, &[]);
    let alice = sign_in_for_tests(&app, "Alice");
    // The wallet asks with its own session.
    assert_eq!(ask(&app, "POST", "/api/market/oidc/pass", None, &[], None, None).await.0, StatusCode::UNAUTHORIZED);
    let (status, pass, _) = ask(&app, "POST", "/api/market/oidc/pass", Some(&alice), &[], None, None).await;
    assert_eq!((status, pass["expires_in"].as_u64()), (StatusCode::OK, Some(60)));
    let pass = pass["pass"].as_str().unwrap().to_string();
    // The browser trades it for a session of its own.
    let (status, used, headers) = ask(&app, "POST", "/api/market/oidc/pass/use", None, &[], Some(json!({ "pass": pass })), None).await;
    assert_eq!((status, used["name"].as_str()), (StatusCode::OK, Some("Alice")));
    let cookie = headers[header::SET_COOKIE].to_str().unwrap();
    let token = cookie.split(';').next().unwrap().split_once('=').unwrap().1.to_string();
    let (status, me, _) = ask(&app, "GET", "/api/market/me", Some(&token), &[], None, None).await;
    assert_eq!((status, me["account"]["name"].as_str()), (StatusCode::OK, Some("Alice")));
    // Once only.
    let (status, _, _) = ask(&app, "POST", "/api/market/oidc/pass/use", None, &[], Some(json!({ "pass": pass })), None).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}
