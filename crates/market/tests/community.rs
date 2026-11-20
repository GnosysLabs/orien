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

/// Checkpoints 10 to 14: a and b mine every hour, c joins at 13, d once at
/// 11, e once at 13. Six-hour blocks: 10 and 11 in the first, 12 and 13 in
/// the second, 14 starts a third.
fn mined() -> Mined {
    let mut by_epoch = BTreeMap::new();
    by_epoch.insert(10, vec![(address(1), 600 * ORN), (address(2), 50 * ORN)]);
    by_epoch.insert(11, vec![(address(1), 300 * ORN), (address(2), 50 * ORN), (address(4), 5 * ORN)]);
    by_epoch.insert(12, vec![(address(1), 200 * ORN), (address(2), 50 * ORN)]);
    by_epoch.insert(13, vec![(address(1), 100 * ORN), (address(2), 50 * ORN), (address(3), 70 * ORN), (address(5), 1 * ORN)]);
    by_epoch.insert(14, vec![(address(1), 10 * ORN), (address(2), 50 * ORN)]);
    let midnight = 1_795_000_000 - (1_795_000_000 % 86_400);
    let hour = |h: u64| midnight + h * 3600;
    let times = BTreeMap::from([(10, hour(0)), (11, hour(1)), (12, hour(6)), (13, hour(7)), (14, hour(12))]);
    Mined { since: 10, upto: 14, by_epoch, times }
}

#[test]
fn the_board_ranks_verified_miners_by_what_they_mined_with_their_streaks() {
    // Alice (account 7) verified addresses 1 and 4; Bob (8) address 2.
    let owners = HashMap::from([(address(1), 7), (address(4), 7), (address(2), 8)]);
    let names = HashMap::from([(7, ("Alice".to_string(), None)), (8, ("Bob".to_string(), None))]);
    let teams = vec![(1, "Diggers".to_string(), vec![7, 8])];
    let board = board(&mined(), &owners, &names, &teams);
    assert_eq!(board.epoch, 14);
    assert_eq!(board.miners[0].name.as_deref(), Some("Alice"));
    assert_eq!(board.miners[0].mined, (1_215 * ORN).to_string(), "both her wallets count");
    assert_eq!(board.miners[0].streak, 5);
    assert_eq!(board.miners[0].team.as_deref(), Some("Diggers"));
    assert_eq!(board.miners[1].name.as_deref(), Some("Bob"));
    // Addresses nobody verified are not ranked.
    assert_eq!(board.miners.len(), 2);
    assert_eq!(board.accounts[&7].rank, 1);
    assert_eq!(board.teams[0].mined, (1_465 * ORN).to_string());
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
    // Newcomers are welcomed once their six-hour block ends: Carol (9) and
    // address 5 joined in the block of 12 and 13, so 14 posts the welcome,
    // Carol first. Nobody is welcomed for the block the records begin in.
    assert!(!kinds.contains(&"welcome"));
    let at_14 = happenings(&mined(), &owners, 14);
    let welcome = at_14.iter().find(|(kind, _, _)| kind == "welcome").expect("a welcome at 14");
    assert_eq!(welcome.1, Some(9));
    assert_eq!((welcome.2["count"].as_u64(), welcome.2["from"].as_u64(), welcome.2["to"].as_u64()), (Some(2), Some(12), Some(13)));
    assert_eq!(welcome.2["miners"][0]["account"], 9);
    assert_eq!(welcome.2["miners"][1]["address"], address(5));
    assert!(!happenings(&mined(), &owners, 12).iter().any(|(kind, _, _)| kind == "welcome"), "the first block has no welcome");
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

#[tokio::test]
async fn the_networks_story_is_backfilled_and_dated_at_its_checkpoints() {
    let app = market();
    let alice = sign_in_for_tests(&app, "Alice");
    // Alice posts first, now; then the network's story since the records began is filled in.
    ask(&app, "POST", "/api/market/community/posts", Some(&alice), Some(json!({ "body": "hello" }))).await;
    sign_in_for_tests(&app, "Carol");
    let (alice_id, carol_id) = store_for_tests(&app, |s| (s.account_id_for_tests("Alice"), s.account_id_for_tests("Carol")));
    let owners = HashMap::from([(address(1), alice_id), (address(3), carol_id)]);
    store_for_tests(&app, |s| s.tell(&mined(), &owners).unwrap());
    let (_, feed) = ask(&app, "GET", "/api/market/community/feed", None, None).await;
    let posts = feed["posts"].as_array().unwrap();
    // Newest first by time: Alice's post today, then the news in checkpoint order.
    assert_eq!(posts[0]["body"], "hello");
    let record = posts.iter().find(|p| p["kind"] == "record" && p["data"]["epoch"] == 13).expect("the record at 13");
    let midnight = 1_795_000_000 - (1_795_000_000 % 86_400);
    assert_eq!(record["created"], midnight + 7 * 3600, "dated when checkpoint 13 was made");
    assert!(posts.iter().any(|p| p["kind"] == "milestone" && p["data"]["epoch"] == 12));
    // Faces are named at read time.
    let welcome = posts.iter().find(|p| p["kind"] == "welcome").expect("a welcome");
    assert_eq!(welcome["data"]["miners"][0]["name"], "Carol");
    // Only the network, only people.
    let (_, network) = ask(&app, "GET", "/api/market/community/feed?only=network", None, None).await;
    assert!(network["posts"].as_array().unwrap().iter().all(|p| p["kind"] != "post"));
    let (_, people) = ask(&app, "GET", "/api/market/community/feed?only=people", None, None).await;
    assert_eq!(people["posts"].as_array().unwrap().len(), 1);
    // Told once, nothing is told twice.
    let before = network["posts"].as_array().unwrap().len();
    store_for_tests(&app, |s| s.tell(&mined(), &owners).unwrap());
    let (_, again) = ask(&app, "GET", "/api/market/community/feed?only=network", None, None).await;
    assert_eq!(again["posts"].as_array().unwrap().len(), before);
}

#[test]
fn streaks_and_the_days_top_miner_are_posted() {
    // Alice mines every hour from 10 to 34: a 24-hour streak at 33 (10..=33).
    let mut by_epoch = BTreeMap::new();
    for e in 10..=34u64 {
        by_epoch.insert(e, vec![(address(1), 10 * ORN), (address(2), 1 * ORN)]);
    }
    // Checkpoints at 23:00 and 00:00 across a day boundary: 12 → day n, 13 → day n+1 ... make 20 the first of a new day.
    let base = 1_795_000_000 - (1_795_000_000 % 86_400) + 20 * 3600 - 20 * 3600; // midnight
    let times = (10..=34).map(|e| (e, base + (e - 10) * 3600 + 4 * 3600)).collect(); // epoch 30 is the first past midnight
    let mined = Mined { since: 10, upto: 34, by_epoch, times };
    let owners = HashMap::from([(address(1), 7)]);
    let at_33 = happenings(&mined, &owners, 33);
    assert!(at_33.iter().any(|(kind, subject, data)| kind == "streak" && *subject == Some(7) && data["hours"] == 24), "{at_33:?}");
    assert!(!happenings(&mined, &owners, 32).iter().any(|(kind, _, _)| kind == "streak"));
    // Epoch 30 starts a new day: yesterday's top miner is posted then, and only then.
    let at_30 = happenings(&mined, &owners, 30);
    let top = at_30.iter().find(|(kind, _, _)| kind == "top_day").expect("a top miner for the day before");
    assert_eq!(top.1, Some(7));
    assert_eq!(top.2["miner"]["account"], 7);
    assert_eq!(top.2["hours"], 20);
    assert!(!happenings(&mined, &owners, 31).iter().any(|(kind, _, _)| kind == "top_day"));
}
