//! The bell: what happened that involves a signed-in person. Replies and
//! reactions to their posts, replies after theirs on a post, the network's
//! posts about them, and Market messages they haven't read.
//!
//! Community notices are written when the thing happens and taken back when
//! it is undone (a reaction removed). Messages are not copied here: they are
//! read from the conversations, so reading one clears it in both places.
//! Opening the bell marks everything up to then as seen.

use crate::api::{App, Failure};
use crate::store::{Store, now};
use anyhow::Result;
use axum::extract::State;
use axum::http::HeaderMap;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use rusqlite::{OptionalExtension, params};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::sync::Arc;

/// How far back the bell looks.
const KEEP: u64 = 60 * 24 * 3600;
/// Lines in the bell.
const LINES: usize = 40;
/// Faces named on one line; the rest are counted.
const FACES: usize = 3;

impl Store {
    pub(crate) fn create_notice_tables(&self) -> Result<()> {
        let new = !self.has_table("notices")?;
        self.db.execute_batch(
            "CREATE TABLE IF NOT EXISTS notices (
                 id INTEGER PRIMARY KEY,
                 account INTEGER NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
                 -- reply, also (replied after you), react, network.
                 kind TEXT NOT NULL,
                 post INTEGER NOT NULL REFERENCES posts(id) ON DELETE CASCADE,
                 actor INTEGER REFERENCES accounts(id) ON DELETE CASCADE,
                 emoji TEXT NOT NULL DEFAULT '',
                 created INTEGER NOT NULL
             );
             CREATE INDEX IF NOT EXISTS notices_for ON notices(account, id);
             -- What each account has seen: notices up to an id, messages up to a time.
             CREATE TABLE IF NOT EXISTS notices_seen (
                 account INTEGER PRIMARY KEY REFERENCES accounts(id) ON DELETE CASCADE,
                 upto INTEGER NOT NULL,
                 at INTEGER NOT NULL
             );",
        )?;
        // Added for tips: how much a tip was, in ORN's smallest units.
        let has_amount: bool = self.db.query_row(
            "SELECT COUNT(*) > 0 FROM pragma_table_info('notices') WHERE name = 'amount'",
            [],
            |row| row.get(0),
        )?;
        if !has_amount {
            self.db.execute("ALTER TABLE notices ADD COLUMN amount INTEGER", [])?;
        }
        if new {
            self.fill_notices()?;
        }
        Ok(())
    }

    fn has_table(&self, name: &str) -> Result<bool> {
        Ok(self.db.query_row(
            "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type = 'table' AND name = ?1",
            [name],
            |row| row.get(0),
        )?)
    }

    /// The first time: notices for what happened before the bell existed,
    /// all already seen, so the bell has a history but starts quiet.
    fn fill_notices(&self) -> Result<()> {
        let mut replies = self.db.prepare("SELECT id, author, parent FROM posts WHERE parent IS NOT NULL AND author IS NOT NULL ORDER BY id")?;
        let found: Vec<(i64, i64, i64)> =
            replies.query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))?.collect::<rusqlite::Result<_>>()?;
        for (reply, author, parent) in found {
            self.notice_reply(reply, author, parent, None)?;
        }
        let mut reactions = self.db.prepare("SELECT post, account, emoji FROM reactions")?;
        let found: Vec<(i64, i64, String)> =
            reactions.query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))?.collect::<rusqlite::Result<_>>()?;
        for (post, account, emoji) in found {
            self.notice_reaction(post, account, &emoji, true, None)?;
        }
        let mut network = self.db.prepare("SELECT id FROM posts WHERE author IS NULL")?;
        let found: Vec<i64> = network.query_map([], |row| row.get(0))?.collect::<rusqlite::Result<_>>()?;
        for post in found {
            self.notice_happening(post)?;
        }
        self.db.execute(
            "INSERT OR REPLACE INTO notices_seen (account, upto, at)
             SELECT account, MAX(id), ?1 FROM notices GROUP BY account",
            [now() as i64],
        )?;
        Ok(())
    }

    /// Whose post this is: its author, or for the network's posts the one
    /// person it is about.
    fn owner(&self, post: i64) -> Result<Option<i64>> {
        Ok(self
            .db
            .query_row(
                "SELECT COALESCE(author, subject, json_extract(data, '$.miner.account')) FROM posts WHERE id = ?1",
                [post],
                |row| row.get::<_, Option<i64>>(0),
            )
            .optional()?
            .flatten())
    }

    fn tell_one(&self, account: i64, kind: &str, post: i64, actor: Option<i64>, emoji: &str, at: u64) -> Result<()> {
        if Some(account) == actor {
            return Ok(());
        }
        if let Some(actor) = actor
            && self.blocked_by(account, actor)?
        {
            return Ok(());
        }
        self.db.execute(
            "INSERT INTO notices (account, kind, post, actor, emoji, created) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![account, kind, post, actor, emoji, at as i64],
        )?;
        Ok(())
    }

    /// `tipper` tipped `author` `orn` for `post`: only the author hears who.
    pub(crate) fn notice_tip(&self, author: i64, post: i64, tipper: i64, orn: u64, at: u64) -> Result<()> {
        if author == tipper || self.blocked_by(author, tipper)? {
            return Ok(());
        }
        self.db.execute(
            "INSERT INTO notices (account, kind, post, actor, emoji, created, amount) VALUES (?1, 'tip', ?2, ?3, '', ?4, ?5)",
            params![author, post, tipper, at as i64, orn as i64],
        )?;
        Ok(())
    }

    /// `author` replied to `parent` with `reply`: its owner hears, and so does
    /// everyone who replied before.
    pub(crate) fn notice_reply(&self, reply: i64, author: i64, parent: i64, reply_to: Option<i64>) -> Result<()> {
        let at: i64 = self.db.query_row("SELECT created FROM posts WHERE id = ?1", [reply], |row| row.get(0))?;
        let owner = self.owner(parent)?;
        if let Some(owner) = owner {
            self.tell_one(owner, "reply", parent, Some(author), "", at as u64)?;
        }
        // The author of the reply answered hears "replied to your reply",
        // once, unless they own the post (told above) or answered themself.
        let answered = match reply_to {
            Some(target) => self.owner(target)?.filter(|a| Some(*a) != owner && *a != author),
            None => None,
        };
        if let (Some(person), Some(target)) = (answered, reply_to) {
            self.tell_one(person, "reply", target, Some(author), "", at as u64)?;
        }
        let mut earlier = self.db.prepare(
            "SELECT DISTINCT author FROM posts WHERE parent = ?1 AND id < ?2 AND author IS NOT NULL AND removed = 0",
        )?;
        let earlier: Vec<i64> = earlier.query_map(params![parent, reply], |row| row.get(0))?.collect::<rusqlite::Result<_>>()?;
        for person in earlier {
            if Some(person) != owner && Some(person) != answered {
                self.tell_one(person, "also", parent, Some(author), "", at as u64)?;
            }
        }
        Ok(())
    }

    /// `account` put `emoji` on `post`, or took it back.
    /// Reactions have no time of their own, so the first fill dates them
    /// at their post.
    pub(crate) fn notice_reaction(&self, post: i64, account: i64, emoji: &str, on: bool, at: Option<u64>) -> Result<()> {
        let Some(owner) = self.owner(post)? else { return Ok(()) };
        if on {
            let at = match at {
                Some(at) => at,
                None => self.db.query_row("SELECT created FROM posts WHERE id = ?1", [post], |row| row.get::<_, i64>(0))? as u64,
            };
            self.tell_one(owner, "react", post, Some(account), emoji, at)
        } else {
            self.db.execute(
                "DELETE FROM notices WHERE kind = 'react' AND post = ?1 AND actor = ?2 AND emoji = ?3",
                params![post, account, emoji],
            )?;
            Ok(())
        }
    }

    /// The network posted `post`: everyone it names hears.
    pub(crate) fn notice_happening(&self, post: i64) -> Result<()> {
        let (subject, data, at): (Option<i64>, String, i64) =
            self.db.query_row("SELECT subject, data, created FROM posts WHERE id = ?1", [post], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?))
            })?;
        let data: Value = serde_json::from_str(&data).unwrap_or(json!({}));
        let mut named: Vec<i64> = subject.into_iter().collect();
        named.extend(data["miner"]["account"].as_i64());
        if let Some(list) = data["miners"].as_array() {
            named.extend(list.iter().filter_map(|m| m["account"].as_i64()));
        }
        named.sort_unstable();
        named.dedup();
        for account in named {
            self.tell_one(account, "network", post, None, "", at as u64)?;
        }
        Ok(())
    }

    fn seen(&self, account: i64) -> Result<Option<(i64, u64)>> {
        Ok(self
            .db
            .query_row("SELECT upto, at FROM notices_seen WHERE account = ?1", [account], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)? as u64))
            })
            .optional()?)
    }

    /// Everything up to now is seen.
    pub fn see_notices(&self, account: i64) -> Result<()> {
        let upto: i64 =
            self.db.query_row("SELECT COALESCE(MAX(id), 0) FROM notices WHERE account = ?1", [account], |row| row.get(0))?;
        self.db.execute(
            "INSERT OR REPLACE INTO notices_seen (account, upto, at) VALUES (?1, ?2, ?3)",
            params![account, upto, now() as i64],
        )?;
        Ok(())
    }

    /// The bell's lines, newest first: one line per post and kind, with
    /// everyone who did it; and one per unread conversation.
    pub fn notices(&self, account: i64) -> Result<Vec<Value>> {
        let (upto, at) = self.seen(account)?.unwrap_or((0, 0));
        let mut query = self.db.prepare(
            "SELECT n.id, n.kind, n.post, n.emoji, n.created, a.name, a.picture,
                    p.kind, p.body, p.parent, COALESCE(p.parent, p.id), p.data, n.amount
             FROM notices n
             JOIN posts p ON p.id = n.post
             LEFT JOIN accounts a ON a.id = n.actor
             WHERE n.account = ?1 AND n.created > ?2 AND p.removed = 0
               AND (n.actor IS NULL OR n.actor NOT IN (SELECT account FROM suspended))
               AND (p.parent IS NULL OR (SELECT removed FROM posts WHERE id = p.parent) = 0)
             ORDER BY n.created DESC, n.id DESC",
        )?;
        struct Row {
            id: i64,
            kind: String,
            post: i64,
            emoji: String,
            created: i64,
            name: Option<String>,
            picture: Option<String>,
            post_kind: String,
            body: String,
            parent: Option<i64>,
            root: i64,
            data: String,
            amount: Option<i64>,
        }
        let rows: Vec<Row> = query
            .query_map(params![account, now().saturating_sub(KEEP) as i64], |row| {
                Ok(Row {
                    id: row.get(0)?,
                    kind: row.get(1)?,
                    post: row.get(2)?,
                    emoji: row.get(3)?,
                    created: row.get(4)?,
                    name: row.get(5)?,
                    picture: row.get(6)?,
                    post_kind: row.get(7)?,
                    body: row.get(8)?,
                    parent: row.get(9)?,
                    root: row.get(10)?,
                    data: row.get(11)?,
                    amount: row.get(12)?,
                })
            })?
            .collect::<rusqlite::Result<_>>()?;

        let mut lines: Vec<Value> = Vec::new();
        let mut at_line: HashMap<(String, i64), usize> = HashMap::new();
        for row in rows {
            // Each tip is its own line, with who sent it and how much.
            let key = if row.kind == "tip" { (format!("tip:{}", row.id), row.post) } else { (row.kind.clone(), row.post) };
            let index = match at_line.get(&key) {
                Some(&index) => index,
                None => {
                    let data: Value = serde_json::from_str(&row.data).unwrap_or(json!({}));
                    lines.push(json!({
                        "kind": row.kind,
                        "post": row.root,
                        "reply": row.parent.is_some(),
                        "post_kind": row.post_kind,
                        "text": snippet(&row.body),
                        "day": data["day"],
                        "time": row.created,
                        "unseen": false,
                        "people": [],
                        "others": 0,
                        "emojis": [],
                    }));
                    if row.kind == "tip" {
                        let line = lines.last_mut().expect("just pushed");
                        line["orn"] = json!(row.amount.unwrap_or(0));
                        line["name"] = json!(row.name);
                    }
                    at_line.insert(key, lines.len() - 1);
                    lines.len() - 1
                }
            };
            let line = &mut lines[index];
            if row.id > upto {
                line["unseen"] = json!(true);
            }
            if let Some(name) = row.name {
                let people = line["people"].as_array_mut().expect("a list");
                if !people.iter().any(|p| p["name"] == name) {
                    if people.len() < FACES {
                        people.push(json!({ "name": name, "picture": row.picture }));
                    } else {
                        line["others"] = json!(line["others"].as_i64().unwrap_or(0) + 1);
                    }
                }
            }
            if !row.emoji.is_empty() {
                let emojis = line["emojis"].as_array_mut().expect("a list");
                if !emojis.iter().any(|e| e == &row.emoji) {
                    emojis.push(json!(row.emoji));
                }
            }
        }

        for conversation in self.conversations(account)?.into_iter().filter(|c| c.unread) {
            let Some(last) = conversation.last else { continue };
            let text = match (last.kind.as_str(), last.amount) {
                ("request", Some(amount)) => format!("asks you to pay {} ORN", crate::push::grouped(amount)),
                ("paid", Some(amount)) => format!("says they paid {} ORN", crate::push::grouped(amount)),
                _ => snippet(&last.body),
            };
            lines.push(json!({
                "kind": "message",
                "conversation": conversation.id,
                "listing": conversation.listing_title,
                "text": text,
                "time": last.created,
                "unseen": last.created > at,
                "people": [{ "name": conversation.with_name, "picture": conversation.with_picture }],
                "others": 0,
                "emojis": [],
            }));
        }
        lines.sort_by(|a, b| b["time"].as_i64().cmp(&a["time"].as_i64()));
        lines.truncate(LINES);
        Ok(lines)
    }

    /// How many of the bell's lines are new.
    pub fn unseen_notices(&self, account: i64) -> Result<usize> {
        Ok(self.notices(account)?.iter().filter(|line| line["unseen"] == true).count())
    }
}

/// The start of a post, on one line.
fn snippet(body: &str) -> String {
    let text: String = body.split_whitespace().collect::<Vec<_>>().join(" ");
    let cut: String = text.chars().take(90).collect();
    if cut.chars().count() < text.chars().count() { format!("{cut}…") } else { cut }
}

pub fn routes() -> Router<Arc<App>> {
    Router::new()
        .route("/api/market/notices", get(list))
        .route("/api/market/notices/seen", post(see))
}

async fn list(State(app): State<Arc<App>>, headers: HeaderMap) -> Result<Response, Failure> {
    let account = app.signed_in(&headers)?;
    let lines = app.store().notices(account)?;
    Ok(Json(json!({ "notices": lines })).into_response())
}

async fn see(State(app): State<Arc<App>>, headers: HeaderMap) -> Result<Response, Failure> {
    let account = app.signed_in(&headers)?;
    app.store().see_notices(account)?;
    Ok(Json(json!({})).into_response())
}

#[cfg(test)]
mod tests {
    use crate::store::Store;
    use serde_json::json;

    fn people() -> (Store, i64, i64, i64) {
        let store = Store::in_memory().unwrap();
        let mut ids = Vec::new();
        for (i, name) in ["Maya", "Sam", "Ana"].iter().enumerate() {
            let id = store.create_account_with_email(name, &format!("{name}@example.com")).unwrap().id;
            store.verified_wallet_for_tests(id, &format!("{i}").repeat(64)).unwrap();
            ids.push(id);
        }
        (store, ids[0], ids[1], ids[2])
    }

    #[test]
    fn replies_and_reactions_ring_the_bell_once_per_post_until_seen() {
        let (store, maya, sam, ana) = people();
        let post = store.add_post(maya, "First block mined", None).unwrap();
        store.add_post(sam, "Nice one", Some(post)).unwrap();
        store.add_post(ana, "Congrats", Some(post)).unwrap();
        store.react(sam, post, "🔥").unwrap();
        store.react(ana, post, "❤️").unwrap();

        let lines = store.notices(maya).unwrap();
        assert_eq!(lines.len(), 2, "one line for the replies, one for the reactions: {lines:#?}");
        let replies = lines.iter().find(|l| l["kind"] == "reply").unwrap();
        assert_eq!(replies["people"].as_array().unwrap().len(), 2);
        assert_eq!(store.unseen_notices(maya).unwrap(), 2);
        // Sam replied first, so Ana's reply reaches Sam too; Ana hears nothing.
        assert_eq!(store.notices(sam).unwrap()[0]["kind"], "also");
        assert!(store.notices(ana).unwrap().is_empty());

        store.see_notices(maya).unwrap();
        assert_eq!(store.unseen_notices(maya).unwrap(), 0);
        assert_eq!(store.notices(maya).unwrap().len(), 2, "seen lines stay in the bell");

        // Taking a reaction back takes its notice back.
        store.react(ana, post, "❤️").unwrap();
        let reactions = store.notices(maya).unwrap().into_iter().find(|l| l["kind"] == "react").unwrap();
        assert_eq!(reactions["emojis"], json!(["🔥"]));
        // Your own reaction says nothing.
        store.react(maya, post, "👏").unwrap();
        assert_eq!(store.unseen_notices(maya).unwrap(), 0);
    }

    #[test]
    fn the_network_tells_whom_it_posts_about() {
        let (store, maya, sam, _) = people();
        store.add_happening("top_day", None, &json!({ "miner": { "account": maya }, "day": "2026-11-20" }), 1).unwrap();
        store.add_happening("welcome", None, &json!({ "miners": [{ "account": maya }, { "account": sam }] }), crate::store::now()).unwrap();
        assert_eq!(store.unseen_notices(sam).unwrap(), 1);
        assert_eq!(store.unseen_notices(maya).unwrap(), 1, "the old one is past what the bell keeps");
    }
}
