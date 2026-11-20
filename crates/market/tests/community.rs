//! The community feed, its leaderboards and teams.

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use http_body_util::BodyExt;
use orien_market::api::{App, COOKIE, router, sign_in_for_tests, sign_in_without_wallet_for_tests, store_for_tests};
use orien_market::community::{Miner, board, happenings};
use orien_market::explorer::Mined;
use orien_market::images::Folder;
use orien_market::mail::Mailer;
use orien_market::screen::Screener;
use orien_market::store::Store;
use orien_market::wallets::Fake;
use serde_json::{Value, json};
use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;
use tower::ServiceExt;

const ORN: u64 = 1_000_000_000;

fn address(n: u8) -> String {
    format!("{:064x}", n)
}

/// Checkpoints 10 to 13: a and b mine every hour, c joins at 13, d once at 11.
fn mined() -> Mined {
    let mut by_epoch = BTreeMap::new();
    by_epoch.insert(10, vec![(address(1), 600 * ORN), (address(2), 50 * ORN)]);
    by_epoch.insert(11, vec![(address(1), 300 * ORN), (address(2), 50 * ORN), (address(4), 5 * ORN)]);
    by_epoch.insert(12, vec![(address(1), 200 * ORN), (address(2), 50 * ORN)]);
    by_epoch.insert(13, vec![(address(1), 100 * ORN), (address(2), 50 * ORN), (address(3), 70 * ORN), (address(5), 1 * ORN)]);
    Mined { since: 10, upto: 13, by_epoch }
}

#[test]
fn the_board_ranks_verified_miners_by_what_they_mined_with_their_streaks() {
    // Alice (account 7) verified addresses 1 and 4; Bob (8) address 2.
    let owners = HashMap::from([(address(1), 7), (address(4), 7), (address(2), 8)]);
    let names = HashMap::from([(7, ("Alice".to_string(), None)), (8, ("Bob".to_string(), None))]);
    let teams = vec![(1, "Diggers".to_string(), vec![7, 8])];
    let board = board(&mined(), &owners, &names, &teams);
    assert_eq!(board.epoch, 13);
    assert_eq!(board.miners[0].name.as_deref(), Some("Alice"));
    assert_eq!(board.miners[0].mined, (1_205 * ORN).to_string(), "both her wallets count");
    assert_eq!(board.miners[0].streak, 4);
    assert_eq!(board.miners[0].team.as_deref(), Some("Diggers"));
    assert_eq!(board.miners[1].name.as_deref(), Some("Bob"));
    // Addresses nobody verified are not ranked.
    assert_eq!(board.miners.len(), 2);
    assert_eq!(board.accounts[&7].rank, 1);
    assert_eq!(board.teams[0].mined, (1_405 * ORN).to_string());
    assert_eq!(board.teams[0].members, 2);
}

#[test]
fn the_network_posts_records_new_miners_and_milestones() {
    let owners = HashMap::from([(address(1), 7), (address(3), 9)]);
    // Nothing to compare the first hour with.
    assert!(happenings(&mined(), &owners, 10).is_empty());
    let said = happenings(&mined(), &owners, 13);
    let kinds: Vec<&str> = said.iter().map(|(kind, _, _)| kind.as_str()).collect();
    // Four addresses paid, beating three: a record.
    assert!(kinds.contains(&"record"));
    // Carol (9) is new and named; address 5 is new and unnamed.
    assert!(said.iter().any(|(kind, subject, _)| kind == "new_miner" && *subject == Some(9)));
    assert!(said.iter().any(|(kind, _, data)| kind == "new_miners" && data["count"] == 1));
    // Alice passed 1,000 ORN at 12 (600+300+200), not at 13.
    assert!(!kinds.contains(&"milestone"));
    let at_12 = happenings(&mined(), &owners, 12);
    assert!(at_12.iter().any(|(kind, subject, data)| kind == "milestone" && *subject == Some(7) && data["orn"] == 1000));
    let _ = Miner::Account(7);
}

fn market() -> Arc<App> {
    let mut random = [0u8; 8];
    getrandom::fill(&mut random).unwrap();
    let folder = Folder::open(&std::env::temp_dir().join(format!("orien-community-test-{}", hex::encode(random)))).unwrap();
    let app = App::new(
        Store::in_memory().unwrap(),
        folder,
        "https://orien.network",
        true,
        Mailer::log_only(),
        Screener::refusing_for_tests("scam"),
        Arc::new(Fake::default()),
        "0000000000000000000000000000000000000000000000000000000000000abc",
    )
    .unwrap();
    Arc::new(app)
}

async fn ask(app: &Arc<App>, method: &str, path: &str, cookie: Option<&str>, body: Option<Value>) -> (StatusCode, Value) {
    let mut request = Request::builder().method(method).uri(path);
    if let Some(token) = cookie {
        request = request.header(header::COOKIE, format!("{COOKIE}={token}"));
    }
    let body = match body {
        Some(body) => {
            request = request.header(header::CONTENT_TYPE, "application/json");
            Body::from(body.to_string())
        }
        None => Body::empty(),
    };
    let response = router(app.clone()).oneshot(request.body(body).unwrap()).await.unwrap();
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    (status, serde_json::from_slice(&bytes).unwrap_or(Value::Null))
}

#[tokio::test]
async fn verified_accounts_post_reply_and_react_and_screening_holds() {
    let app = market();
    let alice = sign_in_for_tests(&app, "Alice");
    let nobody = sign_in_without_wallet_for_tests(&app, "Nobody");

    // Signed out, or without a verified wallet: no posting.
    assert_eq!(ask(&app, "POST", "/api/market/community/posts", None, Some(json!({ "body": "hi" }))).await.0, StatusCode::UNAUTHORIZED);
    let (status, body) = ask(&app, "POST", "/api/market/community/posts", Some(&nobody), Some(json!({ "body": "hi" }))).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    // Screening turns away what it should.
    let (status, _) = ask(&app, "POST", "/api/market/community/posts", Some(&alice), Some(json!({ "body": "a scam" }))).await;
    assert_ne!(status, StatusCode::OK);
    // Too long.
    let long = "x".repeat(501);
    assert_eq!(ask(&app, "POST", "/api/market/community/posts", Some(&alice), Some(json!({ "body": long }))).await.0, StatusCode::BAD_REQUEST);

    let (status, posted) = ask(&app, "POST", "/api/market/community/posts", Some(&alice), Some(json!({ "body": "First block-free hour!" }))).await;
    assert_eq!(status, StatusCode::OK, "{posted}");
    let id = posted["id"].as_i64().unwrap();
    let (_, reply) = ask(&app, "POST", "/api/market/community/posts", Some(&alice), Some(json!({ "body": "and a reply", "parent": id }))).await;
    // No replies to replies.
    let (status, _) =
        ask(&app, "POST", "/api/market/community/posts", Some(&alice), Some(json!({ "body": "deeper", "parent": reply["id"] }))).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    // Anyone signed in reacts; again takes it back.
    let path = format!("/api/market/community/posts/{id}/react");
    assert_eq!(ask(&app, "POST", &path, Some(&nobody), Some(json!({ "emoji": "🔥" }))).await.1["on"], true);
    assert_eq!(ask(&app, "POST", &path, Some(&alice), Some(json!({ "emoji": "🔥" }))).await.1["on"], true);
    assert_eq!(ask(&app, "POST", &path, Some(&alice), Some(json!({ "emoji": "🙃" }))).await.0, StatusCode::BAD_REQUEST);

    let (_, feed) = ask(&app, "GET", "/api/market/community/feed", Some(&nobody), None).await;
    let posts = feed["posts"].as_array().unwrap();
    assert_eq!(posts.len(), 1, "replies stay under their post");
    assert_eq!(posts[0]["author"]["name"], "Alice");
    assert_eq!(posts[0]["replies"], 1);
    assert_eq!(posts[0]["reactions"][0]["count"], 2);
    assert_eq!(posts[0]["reactions"][0]["me"], true);
    let (_, replies) = ask(&app, "GET", &format!("/api/market/community/posts/{id}"), None, None).await;
    assert_eq!(replies["replies"][0]["body"], "and a reply");

    assert_eq!(ask(&app, "POST", &path, Some(&nobody), Some(json!({ "emoji": "🔥" }))).await.1["on"], false);

    // Reported, then taken down: it leaves the feed.
    let (status, _) =
        ask(&app, "POST", "/api/market/reports", Some(&nobody), Some(json!({ "kind": "post", "id": id, "reason": "rude" }))).await;
    assert_eq!(status, StatusCode::OK);
    assert!(store_for_tests(&app, |s| s.remove_post(id).unwrap()));
    let (_, feed) = ask(&app, "GET", "/api/market/community/feed", None, None).await;
    assert!(feed["posts"].as_array().unwrap().is_empty());
}

#[tokio::test]
async fn teams_are_started_joined_left_and_handed_on() {
    let app = market();
    let alice = sign_in_for_tests(&app, "Alice");
    let bob = sign_in_for_tests(&app, "Bob");
    let nobody = sign_in_without_wallet_for_tests(&app, "Nobody");

    let (status, made) = ask(&app, "POST", "/api/market/community/teams", Some(&alice), Some(json!({ "name": "Diggers" }))).await;
    assert_eq!(status, StatusCode::OK, "{made}");
    let team = made["id"].as_i64().unwrap();
    // Names are unique, whatever their case.
    let (status, _) = ask(&app, "POST", "/api/market/community/teams", Some(&bob), Some(json!({ "name": "diggers" }))).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    // Joining needs a verified wallet, and one team at a time.
    let join = format!("/api/market/community/teams/{team}/join");
    assert_eq!(ask(&app, "POST", &join, Some(&nobody), None).await.0, StatusCode::BAD_REQUEST);
    assert_eq!(ask(&app, "POST", &join, Some(&bob), None).await.0, StatusCode::OK);
    assert_eq!(ask(&app, "POST", &join, Some(&bob), None).await.0, StatusCode::BAD_REQUEST);
    let (_, members) = ask(&app, "GET", &format!("/api/market/community/teams/{team}"), None, None).await;
    assert_eq!(members["members"].as_array().unwrap().len(), 2);

    // The founder leaves: Bob carries on with it. Bob leaves: it ends.
    assert_eq!(ask(&app, "POST", "/api/market/community/teams/leave", Some(&alice), None).await.0, StatusCode::OK);
    let (_, members) = ask(&app, "GET", &format!("/api/market/community/teams/{team}"), None, None).await;
    assert_eq!(members["members"][0]["name"], "Bob");
    assert_eq!(ask(&app, "POST", "/api/market/community/teams/leave", Some(&bob), None).await.0, StatusCode::OK);
    assert_eq!(ask(&app, "GET", &format!("/api/market/community/teams/{team}"), None, None).await.0, StatusCode::NOT_FOUND);
}
