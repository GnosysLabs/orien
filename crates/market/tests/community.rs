//! The community feed and its leaderboard.

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
    let board = board(&mined(), &owners, &names);
    assert_eq!(board.epoch, 14);
    assert_eq!(board.miners[0].name.as_deref(), Some("Alice"));
    assert_eq!(board.miners[0].mined, (1_215 * ORN).to_string(), "both her wallets count");
    assert_eq!(board.miners[0].streak, 5);
    assert_eq!(board.miners[1].name.as_deref(), Some("Bob"));
    // Addresses nobody verified are not ranked.
    assert_eq!(board.miners.len(), 2);
    assert_eq!(board.accounts[&7].rank, 1);
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
    // Alice passed 1,000 ORN at 12 (600+300+200), not at 13; at 13 Carol mined her first ORN.
    assert!(said.iter().any(|(kind, subject, data)| kind == "milestone" && *subject == Some(9) && data["orn"] == 1));
    assert!(!said.iter().any(|(kind, subject, _)| kind == "milestone" && *subject == Some(7)));
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
    // About one person: their own posts, and the network's posts that name them.
    let (_, about) = ask(&app, "GET", "/api/market/community/feed?about=Alice", None, None).await;
    let about = about["posts"].as_array().unwrap();
    assert!(about.iter().any(|p| p["body"] == "hello"));
    assert!(about.iter().any(|p| p["kind"] == "milestone" && p["subject"]["name"] == "Alice"));
    assert!(about.iter().all(|p| p["kind"] != "welcome"), "Alice was mining before the records began");
    let (_, about) = ask(&app, "GET", "/api/market/community/feed?about=Carol", None, None).await;
    let about = about["posts"].as_array().unwrap();
    assert!(about.iter().any(|p| p["kind"] == "welcome"), "Carol's face is in a welcome");
    assert!(about.iter().all(|p| p["body"] != "hello"));
    assert_eq!(ask(&app, "GET", "/api/market/community/feed?about=Nobody", None, None).await.0, StatusCode::NOT_FOUND);
    // Members are found by part of their name, the closest first.
    let (_, found) = ask(&app, "GET", "/api/market/community/members?q=aro", None, None).await;
    assert_eq!(found["members"][0]["name"], "Carol");
    let (_, found) = ask(&app, "GET", "/api/market/community/members?q=a", None, None).await;
    assert!(found["members"].as_array().unwrap().is_empty(), "two letters at least");
    // Told once, nothing is told twice.
    let before = network["posts"].as_array().unwrap().len();
    store_for_tests(&app, |s| s.tell(&mined(), &owners).unwrap());
    let (_, again) = ask(&app, "GET", "/api/market/community/feed?only=network", None, None).await;
    assert_eq!(again["posts"].as_array().unwrap().len(), before);
}

#[tokio::test]
async fn past_days_top_miners_are_filled_in_once_the_rule_knows_them() {
    let app = market();
    sign_in_for_tests(&app, "Alice");
    let alice_id = store_for_tests(&app, |s| s.account_id_for_tests("Alice"));
    let mut by_epoch = BTreeMap::new();
    for e in 10..=34u64 {
        by_epoch.insert(e, vec![(address(1), 10 * ORN), (address(2), 1 * ORN)]);
    }
    let base = 1_795_000_000 - (1_795_000_000 % 86_400);
    let times: BTreeMap<u64, u64> = (10..=34).map(|e| (e, base + (e - 10) * 3600 + 4 * 3600)).collect();
    let closed_the_day = times[&30];
    let mined = Mined { since: 10, upto: 34, by_epoch, times };
    // Told with nobody verified: no top miner of the day, since only members count.
    store_for_tests(&app, |s| s.tell(&mined, &HashMap::new()).unwrap());
    let (_, feed) = ask(&app, "GET", "/api/market/community/feed?only=network", None, None).await;
    assert!(feed["posts"].as_array().unwrap().iter().all(|p| p["kind"] != "top_day"));
    // Alice verifies address 1 later: the day is filled in, dated at the
    // checkpoint that closed it, with her milestones and streak, each where
    // it happened. Once: told again, nothing is added.
    let owners = HashMap::from([(address(1), alice_id)]);
    assert_eq!(store_for_tests(&app, |s| s.tell_late(&mined, &owners).unwrap()), 4);
    assert_eq!(store_for_tests(&app, |s| s.tell_late(&mined, &owners).unwrap()), 0);
    let (_, feed) = ask(&app, "GET", "/api/market/community/feed?only=network", None, None).await;
    let posts = feed["posts"].as_array().unwrap();
    let tops: Vec<&Value> = posts.iter().filter(|p| p["kind"] == "top_day").collect();
    assert_eq!(tops.len(), 1);
    assert_eq!(tops[0]["created"], closed_the_day);
    assert_eq!(tops[0]["subject"]["name"], "Alice");
    let hers = |kind: &str| -> Vec<(Value, Value)> {
        posts
            .iter()
            .filter(|p| p["kind"] == kind && p["subject"]["name"] == "Alice")
            .map(|p| (p["data"]["epoch"].clone(), p["created"].clone()))
            .collect()
    };
    let mut milestones = hers("milestone");
    milestones.sort_by_key(|(e, _)| e.as_u64());
    assert_eq!(milestones, vec![(json!(19), json!(times_at(&mined, 19))), (json!(34), json!(times_at(&mined, 34)))], "100 and 250 ORN");
    assert_eq!(hers("streak"), vec![(json!(33), json!(times_at(&mined, 33)))], "24 hours in a row");
}

fn times_at(mined: &Mined, epoch: u64) -> u64 {
    mined.times[&epoch]
}

#[tokio::test]
async fn a_welcome_card_shows_who_verified_its_wallet_since() {
    let app = market();
    sign_in_for_tests(&app, "Cara");
    let cara = store_for_tests(&app, |s| s.account_id_for_tests("Cara"));
    // Told while nobody had verified address 3: its face is just the address.
    store_for_tests(&app, |s| s.tell(&mined(), &HashMap::new()).unwrap());
    let face = |feed: &Value| {
        let welcome = feed["posts"].as_array().unwrap().iter().find(|p| p["kind"] == "welcome").expect("a welcome card").clone();
        welcome["data"]["miners"].as_array().unwrap().iter().find(|m| m["address"] == address(3)).expect("c on it").clone()
    };
    let (_, feed) = ask(&app, "GET", "/api/market/community/feed?only=network", None, None).await;
    assert!(face(&feed)["name"].is_null());
    store_for_tests(&app, |s| s.verified_wallet_for_tests(cara, &address(3)).unwrap());
    let (_, feed) = ask(&app, "GET", "/api/market/community/feed?only=network", None, None).await;
    assert_eq!(face(&feed)["name"], "Cara");
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
    // Only members count for the day's top miner, however much an address mined;
    // with no member paid that day, nothing is posted.
    let bob_only = HashMap::from([(address(2), 8)]);
    let top = happenings(&mined, &bob_only, 30).into_iter().find(|(kind, _, _)| kind == "top_day").unwrap();
    assert_eq!(top.1, Some(8));
    assert_eq!(top.2["miners"], 2, "the day's totals still count everyone");
    assert!(!happenings(&mined, &HashMap::new(), 30).iter().any(|(kind, _, _)| kind == "top_day"));
}

#[tokio::test]
async fn a_block_makes_two_people_nobody_to_each_other() {
    let app = market();
    let maya = sign_in_for_tests(&app, "Maya");
    let sam = sign_in_for_tests(&app, "Sam");
    let maya_id = store_for_tests(&app, |s| s.account_id_for_tests("Maya"));
    let sam_id = store_for_tests(&app, |s| s.account_id_for_tests("Sam"));
    let (status, _) = ask(&app, "POST", "/api/market/community/posts", Some(&maya), Some(json!({ "body": "Hello, miners" }))).await;
    assert_eq!(status, StatusCode::OK);
    let listing = json!({ "title": "Logo design", "description": "Clean logos.", "category": "design", "price": ORN, "unit": "" });
    let (_, posted) = ask(&app, "POST", "/api/market/listings", Some(&maya), Some(listing)).await;
    let listing = posted["listing"]["id"].as_i64().unwrap();
    let (_, me) = ask(&app, "GET", "/api/market/me/wallet", Some(&maya), None).await;
    let wallet = me["wallet"].as_str().unwrap().to_string();

    // Everything Sam can see of Maya before.
    let sees = |sam: String, wallet: String| {
        let app = app.clone();
        async move {
            let ok = |s: StatusCode| s == StatusCode::OK;
            let profile = ok(ask(&app, "GET", "/api/market/users/maya", Some(&sam), None).await.0);
            let by_id = ok(ask(&app, "GET", &format!("/api/market/profiles/{maya_id}"), Some(&sam), None).await.0);
            let (_, feed) = ask(&app, "GET", "/api/market/community/feed", Some(&sam), None).await;
            let post = feed["posts"].as_array().unwrap().iter().any(|p| p["author"]["name"] == "Maya");
            let (_, members) = ask(&app, "GET", "/api/market/community/members?q=may", Some(&sam), None).await;
            let member = !members["members"].as_array().unwrap().is_empty();
            let (_, payees) = ask(&app, "GET", "/api/market/profiles?q=may", Some(&sam), None).await;
            let payee = !payees["profiles"].as_array().unwrap().is_empty();
            let (_, lookup) = ask(&app, "POST", "/api/market/wallets/lookup", Some(&sam), Some(json!({ "addresses": [wallet] }))).await;
            let looked_up = !lookup["profiles"].as_object().unwrap().is_empty();
            let one = ok(ask(&app, "GET", &format!("/api/market/listings/{listing}"), Some(&sam), None).await.0);
            let (_, browse) = ask(&app, "GET", "/api/market/listings", Some(&sam), None).await;
            let listed = !browse["listings"].as_array().unwrap().is_empty();
            [profile, by_id, post, member, payee, looked_up, one, listed]
        }
    };
    assert_eq!(sees(sam.clone(), wallet.clone()).await, [true; 8]);

    // Maya blocks Sam: to him, she is gone everywhere.
    let (status, _) = ask(&app, "POST", &format!("/api/market/accounts/{sam_id}/block"), Some(&maya), Some(json!({ "block": true }))).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(sees(sam.clone(), wallet.clone()).await, [false; 8]);
    let (status, _) = ask(&app, "GET", "/api/market/community/feed?about=Maya", Some(&sam), None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    // Everyone else still sees her, signed out included.
    assert_eq!(ask(&app, "GET", "/api/market/users/maya", None, None).await.0, StatusCode::OK);
    let eve = sign_in_for_tests(&app, "Eve");
    assert_eq!(sees(eve, wallet.clone()).await, [true; 8]);

    // Both ways: Maya no longer sees Sam either, and he is on her list.
    assert_eq!(ask(&app, "GET", "/api/market/users/sam", Some(&maya), None).await.0, StatusCode::NOT_FOUND);
    assert_eq!(ask(&app, "GET", &format!("/api/market/profiles/{sam_id}"), Some(&maya), None).await.0, StatusCode::NOT_FOUND);
    let (_, members) = ask(&app, "GET", "/api/market/community/members?q=sam", Some(&maya), None).await;
    assert!(members["members"].as_array().unwrap().is_empty());
    let (_, payees) = ask(&app, "GET", "/api/market/profiles?q=sam", Some(&maya), None).await;
    assert!(payees["profiles"].as_array().unwrap().is_empty());
    let (_, list) = ask(&app, "GET", "/api/market/me/blocked", Some(&maya), None).await;
    let list = list["blocked"].as_array().unwrap().clone();
    assert_eq!((list.len(), list[0]["id"].as_i64(), list[0]["name"].as_str()), (1, Some(sam_id), Some("Sam")));
    assert!(list[0]["blocked_at"].as_u64().unwrap() > 0 && list[0]["picture"].is_null());
    let (_, his) = ask(&app, "GET", "/api/market/me/blocked", Some(&sam), None).await;
    assert!(his["blocked"].as_array().unwrap().is_empty(), "only the blocker's list");
    assert_eq!(ask(&app, "GET", "/api/market/me/blocked", None, None).await.0, StatusCode::UNAUTHORIZED);

    // Unblocked, each is back for the other.
    ask(&app, "POST", &format!("/api/market/accounts/{sam_id}/block"), Some(&maya), Some(json!({ "block": false }))).await;
    assert_eq!(sees(sam, wallet).await, [true; 8]);
    assert_eq!(ask(&app, "GET", "/api/market/users/sam", Some(&maya), None).await.0, StatusCode::OK);
    let (_, list) = ask(&app, "GET", "/api/market/me/blocked", Some(&maya), None).await;
    assert!(list["blocked"].as_array().unwrap().is_empty());
}

#[tokio::test]
async fn the_network_posts_orn_mined_new_validators_and_swaps_to_apps_that_know_them() {
    use orien_market::community::next_supply_mark;
    let app = market();
    let at = 1_795_000_000;
    // ORN mined: the first look notes where things stand, then each round
    // number passed is posted once.
    assert_eq!((next_supply_mark(424_238), next_supply_mark(900_000), next_supply_mark(1_000_000)), (500_000, 1_000_000, 2_000_000));
    assert!(!store_for_tests(&app, |s| s.tell_supply(424_238, 100_000_000, at).unwrap()), "nothing on the first look");
    assert!(!store_for_tests(&app, |s| s.tell_supply(499_999, 100_000_000, at).unwrap()));
    assert!(store_for_tests(&app, |s| s.tell_supply(500_100, 100_000_000, at).unwrap()), "500,000 passed");
    assert!(!store_for_tests(&app, |s| s.tell_supply(500_200, 100_000_000, at).unwrap()), "once");
    // Validators: the ones there are noted; a new one is posted once.
    let four: Vec<(String, usize)> = (0..4).map(|i| (format!("{i:064x}"), i)).collect();
    assert_eq!(store_for_tests(&app, |s| s.tell_validators(&four, at).unwrap()), 0);
    let mut five = four.clone();
    five.push((format!("{:064x}", 9), 4));
    assert_eq!(store_for_tests(&app, |s| s.tell_validators(&five, at).unwrap()), 1);
    assert_eq!(store_for_tests(&app, |s| s.tell_validators(&five, at).unwrap()), 0);
    // A swap, posted when it's priced: no names.
    sign_in_for_tests(&app, "Maya");
    sign_in_for_tests(&app, "Sam");
    let (maya, sam) = (store_for_tests(&app, |s| s.account_id_for_tests("Maya")), store_for_tests(&app, |s| s.account_id_for_tests("Sam")));
    let id = store_for_tests(&app, |s| s.swap_done_for_tests(maya, sam, 500_000_000_000, "sol", 500_000_000));
    store_for_tests(&app, |s| s.price_swap(id, 36.5).unwrap());
    store_for_tests(&app, |s| s.price_swap(id, 99.0).unwrap());

    // Apps that don't ask for them never see them; those that ask do.
    let (_, old) = ask(&app, "GET", "/api/market/community/feed?only=network", None, None).await;
    assert!(old["posts"].as_array().unwrap().is_empty(), "{old}");
    let (_, new) = ask(&app, "GET", "/api/market/community/feed?only=network&kinds=swap,supply,validator", None, None).await;
    let posts = new["posts"].as_array().unwrap();
    let kinds: Vec<&str> = posts.iter().map(|p| p["kind"].as_str().unwrap()).collect();
    assert_eq!(kinds.iter().filter(|k| **k == "swap").count(), 1, "one post per swap: {kinds:?}");
    assert!(kinds.contains(&"supply") && kinds.contains(&"validator"));
    let swap = posts.iter().find(|p| p["kind"] == "swap").unwrap();
    assert_eq!((swap["data"]["orn"].as_u64(), swap["data"]["lamports"].as_u64(), swap["data"]["usd"].as_f64()), (Some(500_000_000_000), Some(500_000_000), Some(36.5)));
    assert!(swap["subject"].is_null() && swap["author"].is_null(), "no names");
    let supply = posts.iter().find(|p| p["kind"] == "supply").unwrap();
    assert_eq!((supply["data"]["orn"].as_u64(), supply["data"]["share"].as_f64()), (Some(500_000), Some(0.5)));
    let validator = posts.iter().find(|p| p["kind"] == "validator").unwrap();
    assert_eq!((validator["data"]["seat"].as_u64(), validator["data"]["seats"].as_u64()), (Some(5), Some(5)));
    let (_, some) = ask(&app, "GET", "/api/market/community/feed?only=network&kinds=swap", None, None).await;
    assert!(some["posts"].as_array().unwrap().iter().all(|p| p["kind"] == "swap"), "only the kinds asked for");
}

#[tokio::test]
async fn tips_are_counted_once_the_network_shows_them_and_only_the_author_hears_who() {
    let payments = Arc::new(Fake::default());
    let mut random = [0u8; 8];
    getrandom::fill(&mut random).unwrap();
    let folder = Folder::open(&std::env::temp_dir().join(format!("orien-community-test-{}", hex::encode(random)))).unwrap();
    let app = Arc::new(
        App::new(Store::in_memory().unwrap(), folder, "https://orien.network", true, Mailer::log_only(), Screener::refusing_for_tests("scam"), payments.clone(), "0000000000000000000000000000000000000000000000000000000000000abc")
            .unwrap(),
    );
    let alice = sign_in_for_tests(&app, "Alice");
    let sam = sign_in_for_tests(&app, "Sam");
    let (_, posted) = ask(&app, "POST", "/api/market/community/posts", Some(&alice), Some(json!({ "body": "Hello, miners" }))).await;
    let post = posted["id"].as_i64().unwrap_or_else(|| posted["post"]["id"].as_i64().unwrap());
    let alice_id = store_for_tests(&app, |s| s.account_id_for_tests("Alice"));
    let sam_id = store_for_tests(&app, |s| s.account_id_for_tests("Sam"));
    let wallet_of = |id: i64| store_for_tests(&app, |s| s.verified_wallets(id).unwrap()[0].address.clone());
    let (alices, sams) = (wallet_of(alice_id), wallet_of(sam_id));
    let account = |hex: &str| hex.parse::<orien_ledger::AccountId>().unwrap();

    // The post says where a tip goes, and has none yet.
    let (_, feed) = ask(&app, "GET", "/api/market/community/feed", None, None).await;
    let shown = feed["posts"].as_array().unwrap().iter().find(|p| p["id"] == post).unwrap().clone();
    assert_eq!(shown["author"]["tip_to"], alices);
    assert_eq!(shown["tips"], json!({ "count": 0, "orn": 0 }));

    let path = format!("/api/market/community/posts/{post}/tips");
    let tip = json!({ "from": sams, "seq": 7 });
    assert_eq!(ask(&app, "POST", &path, None, Some(tip.clone())).await.0, StatusCode::UNAUTHORIZED);
    let (status, body) = ask(&app, "POST", &path, Some(&sam), Some(tip.clone())).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "not on the network yet: {body}");
    // Paid to someone else: not a tip for her.
    payments.orders.lock().unwrap().push((account(&"ee".repeat(32)), account(&sams), 7, 5 * ORN));
    assert_eq!(ask(&app, "POST", &path, Some(&sam), Some(tip.clone())).await.0, StatusCode::BAD_REQUEST);
    // Paid to her verified wallet: counted, once.
    payments.orders.lock().unwrap().push((account(&alices), account(&sams), 8, 5 * ORN));
    let tip = json!({ "from": sams, "seq": 8 });
    let (status, totals) = ask(&app, "POST", &path, Some(&sam), Some(tip.clone())).await;
    assert_eq!((status, totals.clone()), (StatusCode::OK, json!({ "count": 1, "orn": 5 * ORN })));
    let (status, again) = ask(&app, "POST", &path, Some(&sam), Some(tip)).await;
    assert_eq!((status, again), (StatusCode::OK, totals), "the same payment counts once");
    assert_eq!(ask(&app, "POST", &path, Some(&alice), Some(json!({ "from": alices, "seq": 1 }))).await.0, StatusCode::BAD_REQUEST, "not your own");

    // Everyone sees the totals; only Alice hears who.
    let (_, feed) = ask(&app, "GET", "/api/market/community/feed", None, None).await;
    let shown = feed["posts"].as_array().unwrap().iter().find(|p| p["id"] == post).unwrap().clone();
    assert_eq!(shown["tips"], json!({ "count": 1, "orn": 5 * ORN }));
    let (_, notices) = ask(&app, "GET", "/api/market/notices", Some(&alice), None).await;
    let line = notices["notices"].as_array().unwrap().iter().find(|l| l["kind"] == "tip").expect("a tip line").clone();
    assert_eq!((line["name"].as_str(), line["orn"].as_u64(), line["post"].as_i64()), (Some("Sam"), Some(5 * ORN), Some(post)));
    let (_, theirs) = ask(&app, "GET", "/api/market/notices", Some(&sam), None).await;
    assert!(theirs["notices"].as_array().unwrap().iter().all(|l| l["kind"] != "tip"));
}
