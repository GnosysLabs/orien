//! Orien AI Support: a private chat with an AI helper, pinned in Orien
//! Wallet's Messages for anyone signed in to Orien Network. Answers come
//! from the Discord bot's Orien expert, which knows the website, the paper,
//! the changelog and the wallet's own User Guide; this server keeps each
//! person's conversation (the last 50 messages, theirs alone, never shown
//! anywhere else) and the limits. Their words are never written to the log,
//! and recovery words pasted by mistake are neither kept nor sent on.

use std::collections::HashSet;
use std::sync::{Arc, LazyLock};

use anyhow::Result;
use axum::extract::State;
use axum::http::HeaderMap;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use rusqlite::params;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::api::{App, Failure};
use crate::store::Store;

/// Answers one account may have in a day, and everyone together.
pub const PER_DAY: i64 = 30;
const ALL_PER_DAY: i64 = 600;
/// The most messages kept per person, and the most the helper reads.
const KEPT: i64 = 50;
const READ: usize = 12;
/// A question's length, in characters.
const MAX_BODY: usize = 1000;
/// How long the helper may take, in seconds.
const WAIT: u64 = 55;

/// What a pasted recovery phrase is answered with, without asking the helper.
const PHRASE_WARNING: &str = "Please don't share your recovery words with anyone, here included: I've not kept or passed on what you sent. Anyone who sees them can spend your ORN. If you think someone may have seen them, make a new wallet in Orien Wallet and send your ORN to it. Orien will never ask for your recovery words.";
/// What's kept in place of a message that held recovery words.
const PHRASE_REMOVED: &str = "(Recovery words removed: they were not kept or sent anywhere.)";

static WORDLIST: LazyLock<HashSet<&'static str>> = LazyLock::new(|| include_str!("../../net/src/words.txt").lines().collect());

/// Whether `text` holds what looks like a recovery phrase: twelve or more
/// words in a row from the 2,048 phrase words. Everyday sentences can't,
/// since words like "the", "is" and "to" aren't among them.
pub fn looks_like_phrase(text: &str) -> bool {
    let mut run = 0;
    for word in text.split(|c: char| !c.is_ascii_alphabetic()).filter(|w| !w.is_empty()) {
        if WORDLIST.contains(word.to_lowercase().as_str()) {
            run += 1;
            if run >= 12 {
                return true;
            }
        } else {
            run = 0;
        }
    }
    false
}

fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

fn today() -> i64 {
    (now() / 86_400) as i64
}

impl Store {
    pub(crate) fn create_support_tables(&self) -> Result<()> {
        self.db.execute_batch(
            "CREATE TABLE IF NOT EXISTS support_messages (
                 id INTEGER PRIMARY KEY,
                 account INTEGER NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
                 role TEXT NOT NULL,
                 body TEXT NOT NULL,
                 created INTEGER NOT NULL
             );
             CREATE INDEX IF NOT EXISTS support_by_account ON support_messages(account, id);
             -- Answers given each day (UTC), per account and (account 0) in all.
             CREATE TABLE IF NOT EXISTS support_usage (
                 day INTEGER NOT NULL,
                 account INTEGER NOT NULL,
                 answers INTEGER NOT NULL,
                 PRIMARY KEY (day, account)
             );",
        )?;
        Ok(())
    }

    /// The person's conversation, oldest first.
    pub fn support_messages(&self, account: i64) -> Result<Vec<Value>> {
        let mut query = self.db.prepare(
            "SELECT id, role, body, created FROM (
                 SELECT id, role, body, created FROM support_messages WHERE account = ?1 ORDER BY id DESC LIMIT ?2
             ) ORDER BY id",
        )?;
        let rows = query.query_map(params![account, KEPT], |row| {
            Ok(json!({ "id": row.get::<_, i64>(0)?, "role": row.get::<_, String>(1)?, "body": row.get::<_, String>(2)?, "created": row.get::<_, i64>(3)? }))
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    fn support_used(&self, account: i64) -> Result<i64> {
        Ok(self
            .db
            .query_row("SELECT answers FROM support_usage WHERE day = ?1 AND account = ?2", params![today(), account], |r| r.get(0))
            .unwrap_or(0))
    }

    /// Answers left today for `account`.
    pub fn support_left(&self, account: i64) -> Result<i64> {
        Ok((PER_DAY - self.support_used(account)?).max(0))
    }

    fn count_support_answer(&self, account: i64) -> Result<()> {
        for who in [account, 0] {
            self.db.execute(
                "INSERT INTO support_usage (day, account, answers) VALUES (?1, ?2, 1)
                 ON CONFLICT (day, account) DO UPDATE SET answers = answers + 1",
                params![today(), who],
            )?;
        }
        // Days long gone are of no use.
        self.db.execute("DELETE FROM support_usage WHERE day < ?1", [today() - 7])?;
        Ok(())
    }

    /// Adds a question and its answer, and keeps only the last KEPT
    /// messages. Gives both as the app shows them.
    fn add_support_pair(&self, account: i64, question: &str, answer: &str) -> Result<Vec<Value>> {
        let at = now() as i64;
        let mut ids = Vec::new();
        for (role, body) in [("you", question), ("ai", answer)] {
            self.db.execute(
                "INSERT INTO support_messages (account, role, body, created) VALUES (?1, ?2, ?3, ?4)",
                params![account, role, body, at],
            )?;
            ids.push((self.db.last_insert_rowid(), role, body));
        }
        self.db.execute(
            "DELETE FROM support_messages WHERE account = ?1 AND id NOT IN
                 (SELECT id FROM support_messages WHERE account = ?1 ORDER BY id DESC LIMIT ?2)",
            params![account, KEPT],
        )?;
        Ok(ids.into_iter().map(|(id, role, body)| json!({ "id": id, "role": role, "body": body, "created": at })).collect())
    }

    pub fn forget_support(&self, account: i64) -> Result<()> {
        self.db.execute("DELETE FROM support_messages WHERE account = ?1", [account])?;
        Ok(())
    }
}

async fn messages(State(app): State<Arc<App>>, headers: HeaderMap) -> Result<Response, Failure> {
    let account = app.signed_in(&headers)?;
    let store = app.store();
    let messages = store.support_messages(account)?;
    let left = store.support_left(account)?;
    Ok(Json(json!({ "messages": messages, "left": left, "limit": PER_DAY })).into_response())
}

#[derive(Deserialize)]
struct Ask {
    body: String,
}

/// Asks the bot's expert, with the conversation so far.
fn ask_helper(url: &str, turns: Vec<Value>) -> Result<String> {
    let mut response = ureq::post(url)
        .config()
        .http_status_as_error(false)
        .timeout_global(Some(std::time::Duration::from_secs(WAIT)))
        .build()
        .send_json(json!({ "messages": turns }))?;
    let status = response.status();
    let answer: Value = response.body_mut().read_json()?;
    match answer["answer"].as_str() {
        Some(text) if status.is_success() && !text.trim().is_empty() => Ok(text.trim().to_string()),
        _ => anyhow::bail!("the helper answered {status}"),
    }
}

async fn ask(State(app): State<Arc<App>>, headers: HeaderMap, Json(given): Json<Ask>) -> Result<Response, Failure> {
    let account = app.signed_in(&headers)?;
    let question = given.body.trim().to_string();
    if question.is_empty() {
        return Err(Failure::Refused("write a question first".into()));
    }
    if question.chars().count() > MAX_BODY {
        return Err(Failure::Refused(format!("a question is at most {MAX_BODY} characters")));
    }
    // Recovery words go nowhere: not to the helper, not into the database.
    if looks_like_phrase(&question) {
        let store = app.store();
        let pair = store.add_support_pair(account, PHRASE_REMOVED, PHRASE_WARNING)?;
        let left = store.support_left(account)?;
        return Ok(Json(json!({ "messages": pair, "left": left })).into_response());
    }
    let turns = {
        let store = app.store();
        if store.support_used(account)? >= PER_DAY {
            return Err(Failure::Refused("you've asked a lot today; try again tomorrow".into()));
        }
        if store.support_used(0)? >= ALL_PER_DAY {
            return Err(Failure::Refused(
                "Orien AI Support has answered all it can for today; try again tomorrow, or ask in Orien's Discord".into(),
            ));
        }
        let earlier = store.support_messages(account)?;
        let mut turns: Vec<Value> = earlier
            .iter()
            .filter(|m| m["body"] != PHRASE_REMOVED && m["body"] != PHRASE_WARNING)
            .map(|m| json!({ "role": if m["role"] == "ai" { "assistant" } else { "user" }, "content": m["body"] }))
            .collect();
        turns.push(json!({ "role": "user", "content": question }));
        turns[turns.len().saturating_sub(READ)..].to_vec()
    };
    let url = app.support_url().ok_or_else(|| Failure::Unavailable("Orien AI Support isn't available right now".into()))?;
    let answer = tokio::task::spawn_blocking(move || ask_helper(&url, turns))
        .await
        .map_err(|e| Failure::Broken(e.into()))?
        .map_err(|error| {
            eprintln!("support: no answer: {error:#}");
            Failure::Unavailable("Orien AI Support couldn't answer just now; try again in a minute".into())
        })?;
    let store = app.store();
    store.count_support_answer(account)?;
    let pair = store.add_support_pair(account, &question, &answer)?;
    let left = store.support_left(account)?;
    Ok(Json(json!({ "messages": pair, "left": left })).into_response())
}

async fn forget(State(app): State<Arc<App>>, headers: HeaderMap) -> Result<Response, Failure> {
    let account = app.signed_in(&headers)?;
    app.store().forget_support(account)?;
    Ok(Json(json!({})).into_response())
}

pub fn routes() -> Router<Arc<App>> {
    Router::new().route("/api/market/support/messages", get(messages).post(ask).delete(forget))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pasted_phrase_is_spotted_and_sentences_are_not() {
        let phrase = "abandon ability able about above absent absorb abstract absurd abuse access accident account accuse";
        assert!(looks_like_phrase(phrase));
        assert!(looks_like_phrase(&format!("Is this right? {}", phrase.to_uppercase())));
        assert!(!looks_like_phrase("How do I back up my recovery phrase and keep my account safe when I travel abroad next month?"));
        assert!(!looks_like_phrase("abandon ability able about above absent absorb abstract absurd abuse access"));
    }

    #[test]
    fn a_conversation_keeps_its_last_fifty_and_counts_answers() {
        let store = Store::in_memory().unwrap();
        let maya = store.create_account_with_email("Maya", "maya@example.com").unwrap().id;
        let sam = store.create_account_with_email("Sam", "sam@example.com").unwrap().id;
        for i in 0..30 {
            store.add_support_pair(maya, &format!("question {i}"), &format!("answer {i}")).unwrap();
            store.count_support_answer(maya).unwrap();
        }
        let kept = store.support_messages(maya).unwrap();
        assert_eq!(kept.len(), 50);
        assert_eq!(kept[0]["body"], "question 5");
        assert_eq!(kept[49]["body"], "answer 29");
        assert_eq!(store.support_left(maya).unwrap(), 0);
        assert_eq!(store.support_left(sam).unwrap(), PER_DAY);
        assert!(store.support_messages(sam).unwrap().is_empty(), "each sees only their own");
        store.forget_support(maya).unwrap();
        assert!(store.support_messages(maya).unwrap().is_empty());
        assert_eq!(store.support_left(maya).unwrap(), 0, "starting over doesn't reset the day's answers");
    }
}
