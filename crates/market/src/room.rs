//! The group chat: one public room, pinned at the top of Messages in Orien
//! Wallet. Anyone can read it, signed in or not; only accounts with a
//! verified wallet send, react or reply. Every message is screened like a
//! post before anyone sees it and can be reported to the dashboard. People
//! the reader blocked, or who blocked them, are left out, and so are
//! suspended accounts.
//!
//! The app asks every few seconds for what's new: messages after the last
//! one it has, and with `changes`, the reactions and removals since its last
//! look (each change bumps a counter, `version`). It keeps what's read
//! itself, so the server keeps no read state. Someone whose message is
//! answered hears it in the bell; ordinary messages ring nothing.

use crate::api::{App, Failure, refused, screened};
use crate::community::{Gif, MAX_PICTURES, REACTIONS};
use crate::store::{Store, now};
use anyhow::{Result, bail};
use axum::extract::{Path, Query, State};
use axum::http::HeaderMap;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use rusqlite::{OptionalExtension, params};
use serde::Deserialize;
use serde_json::{Value, json};
use std::sync::Arc;

pub const MAX_MESSAGE: usize = 500;
/// Messages one account may send in a minute, and in a day.
const A_MINUTE: i64 = 10;
const A_DAY: i64 = 500;
/// Messages in an answer unless the app asks for fewer, and at most.
const PAGE: usize = 50;
const MOST: usize = 100;
/// How much of an answered message is quoted.
const QUOTE: usize = 120;

/// The people a viewer doesn't see: anyone they blocked or who blocked
/// them (?2 is the viewer, or -1 for someone not signed in), and suspended
/// accounts. For an author column `who`.
fn hidden(who: &str) -> String {
    format!(
        "({who} IN (SELECT account FROM suspended)
          OR {who} IN (SELECT blocker FROM blocks WHERE blocked = ?2 UNION SELECT blocked FROM blocks WHERE blocker = ?2))"
    )
}

impl Store {
    pub(crate) fn create_room_tables(&self) -> Result<()> {
        self.db.execute_batch(
            "CREATE TABLE IF NOT EXISTS room_messages (
                 id INTEGER PRIMARY KEY,
                 author INTEGER NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
                 body TEXT NOT NULL DEFAULT '',
                 -- The message this one answers.
                 reply_to INTEGER REFERENCES room_messages(id) ON DELETE SET NULL,
                 created INTEGER NOT NULL,
                 removed INTEGER NOT NULL DEFAULT 0,
                 -- Bumped on every change (sent, reacted to, removed), so
                 -- an app can ask what changed since it last looked.
                 version INTEGER NOT NULL DEFAULT 0
             );
             CREATE INDEX IF NOT EXISTS room_messages_by_author ON room_messages(author, created);
             CREATE INDEX IF NOT EXISTS room_messages_by_version ON room_messages(version);
             -- Photos on messages: uploaded through the Community's photo
             -- upload, then moved here when the message is sent.
             CREATE TABLE IF NOT EXISTS room_pictures (
                 name TEXT PRIMARY KEY,
                 message INTEGER NOT NULL REFERENCES room_messages(id) ON DELETE CASCADE,
                 position INTEGER NOT NULL,
                 width INTEGER NOT NULL,
                 height INTEGER NOT NULL
             );
             CREATE INDEX IF NOT EXISTS room_pictures_by_message ON room_pictures(message, position);
             CREATE TABLE IF NOT EXISTS room_gifs (
                 message INTEGER PRIMARY KEY REFERENCES room_messages(id) ON DELETE CASCADE,
                 gif TEXT NOT NULL,
                 url TEXT NOT NULL,
                 preview TEXT NOT NULL,
                 width INTEGER NOT NULL,
                 height INTEGER NOT NULL
             );
             CREATE TABLE IF NOT EXISTS room_reactions (
                 message INTEGER NOT NULL REFERENCES room_messages(id) ON DELETE CASCADE,
                 account INTEGER NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
                 emoji TEXT NOT NULL,
                 PRIMARY KEY (message, account, emoji)
             );
             -- Someone answered `account`'s message: `reply` is the answer.
             CREATE TABLE IF NOT EXISTS room_notices (
                 id INTEGER PRIMARY KEY,
                 account INTEGER NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
                 reply INTEGER NOT NULL REFERENCES room_messages(id) ON DELETE CASCADE,
                 actor INTEGER NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
                 created INTEGER NOT NULL
             );
             CREATE INDEX IF NOT EXISTS room_notices_for ON room_notices(account, created);
             -- Moderators: may take down anyone's message and ban people
             -- from sending. Set on the server, never in the code.
             CREATE TABLE IF NOT EXISTS room_moderators (
                 account INTEGER PRIMARY KEY REFERENCES accounts(id) ON DELETE CASCADE,
                 added INTEGER NOT NULL
             );
             -- People who can't send, reply or react until it's lifted.
             CREATE TABLE IF NOT EXISTS room_bans (
                 account INTEGER PRIMARY KEY REFERENCES accounts(id) ON DELETE CASCADE,
                 -- The moderator, or None from the dashboard.
                 by INTEGER REFERENCES accounts(id) ON DELETE SET NULL,
                 reason TEXT NOT NULL DEFAULT '',
                 at INTEGER NOT NULL
             );
             -- What moderators did, for the dashboard: remove, ban, lift.
             CREATE TABLE IF NOT EXISTS room_log (
                 id INTEGER PRIMARY KEY,
                 -- None: done from the dashboard.
                 moderator INTEGER REFERENCES accounts(id) ON DELETE SET NULL,
                 action TEXT NOT NULL,
                 account INTEGER REFERENCES accounts(id) ON DELETE SET NULL,
                 message INTEGER,
                 reason TEXT NOT NULL DEFAULT '',
                 at INTEGER NOT NULL
             );",
        )?;
        Ok(())
    }

    fn room_version(&self) -> Result<i64> {
        Ok(self.db.query_row("SELECT COALESCE(MAX(version), 0) FROM room_messages", [], |row| row.get(0))?)
    }

    /// Everything `author` wrote changed (banned, lifted, made moderator),
    /// so apps asking what changed redraw their name.
    fn bump_room_author(&self, author: i64) -> Result<()> {
        self.db.execute(
            "UPDATE room_messages SET version = (SELECT COALESCE(MAX(version), 0) + 1 FROM room_messages) WHERE author = ?1 AND removed = 0",
            [author],
        )?;
        Ok(())
    }

    fn bump_room_message(&self, id: i64) -> Result<()> {
        self.db.execute(
            "UPDATE room_messages SET version = (SELECT COALESCE(MAX(version), 0) + 1 FROM room_messages) WHERE id = ?1",
            [id],
        )?;
        Ok(())
    }

    pub fn room_moderator(&self, account: i64) -> Result<bool> {
        Ok(self.db.query_row("SELECT 1 FROM room_moderators WHERE account = ?1", [account], |_| Ok(())).optional()?.is_some())
    }

    /// Makes `account` a moderator, or stops it being one. A new moderator's
    /// ban, if any, is lifted.
    pub fn room_set_moderator(&self, account: i64, on: bool) -> Result<()> {
        if on {
            self.db.execute("INSERT OR IGNORE INTO room_moderators (account, added) VALUES (?1, ?2)", params![account, now() as i64])?;
            self.db.execute("DELETE FROM room_bans WHERE account = ?1", [account])?;
        } else {
            self.db.execute("DELETE FROM room_moderators WHERE account = ?1", [account])?;
        }
        self.bump_room_author(account)
    }

    pub fn room_banned(&self, account: i64) -> Result<bool> {
        Ok(self.db.query_row("SELECT 1 FROM room_bans WHERE account = ?1", [account], |_| Ok(())).optional()?.is_some())
    }

    fn room_note(&self, moderator: Option<i64>, action: &str, account: Option<i64>, message: Option<i64>, reason: &str) -> Result<()> {
        self.db.execute(
            "INSERT INTO room_log (moderator, action, account, message, reason, at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![moderator, action, account, message, reason, now() as i64],
        )?;
        Ok(())
    }

    /// A moderator (or the dashboard, with None) bans `account` from
    /// sending, replying and reacting until it's lifted.
    pub fn room_ban(&self, moderator: Option<i64>, account: i64, reason: &str) -> Result<()> {
        if let Some(moderator) = moderator {
            if !self.room_moderator(moderator)? {
                bail!("only moderators can ban people from the group chat");
            }
            if moderator == account {
                bail!("you can't ban yourself");
            }
        }
        if self.account(account)?.is_none() {
            bail!("there is no such account");
        }
        if self.room_moderator(account)? {
            bail!("a moderator can't be banned");
        }
        let reason: String = reason.trim().chars().take(300).collect();
        self.db.execute(
            "INSERT OR REPLACE INTO room_bans (account, by, reason, at) VALUES (?1, ?2, ?3, ?4)",
            params![account, moderator, reason, now() as i64],
        )?;
        self.bump_room_author(account)?;
        self.room_note(moderator, "ban", Some(account), None, &reason)
    }

    /// Lifts a ban. Gives whether there was one.
    pub fn room_unban(&self, moderator: Option<i64>, account: i64) -> Result<bool> {
        if let Some(moderator) = moderator
            && !self.room_moderator(moderator)?
        {
            bail!("only moderators can lift bans");
        }
        let lifted = self.db.execute("DELETE FROM room_bans WHERE account = ?1", [account])? == 1;
        if lifted {
            self.bump_room_author(account)?;
            self.room_note(moderator, "lift", Some(account), None, "")?;
        }
        Ok(lifted)
    }

    /// Who is banned, most recent first.
    pub fn room_bans(&self) -> Result<Vec<Value>> {
        let mut statement = self.db.prepare(
            "SELECT b.account, a.name, a.picture, b.at, m.name, b.reason
             FROM room_bans b JOIN accounts a ON a.id = b.account LEFT JOIN accounts m ON m.id = b.by
             ORDER BY b.at DESC, b.account",
        )?;
        let rows = statement.query_map([], |row| {
            Ok(json!({
                "account": row.get::<_, i64>(0)?,
                "name": row.get::<_, String>(1)?,
                "picture": row.get::<_, Option<String>>(2)?,
                "banned_at": row.get::<_, i64>(3)?,
                "by": row.get::<_, Option<String>>(4)?,
                "reason": row.get::<_, String>(5)?,
            }))
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// What moderators did lately, newest first, for the dashboard.
    pub fn room_log(&self, limit: i64) -> Result<Vec<Value>> {
        let mut statement = self.db.prepare(
            "SELECT l.id, l.action, m.name, l.account, a.name, l.message, r.body, l.reason, l.at
             FROM room_log l LEFT JOIN accounts m ON m.id = l.moderator LEFT JOIN accounts a ON a.id = l.account
             LEFT JOIN room_messages r ON r.id = l.message
             ORDER BY l.id DESC LIMIT ?1",
        )?;
        let rows = statement.query_map([limit], |row| {
            Ok(json!({
                "id": row.get::<_, i64>(0)?,
                "action": row.get::<_, String>(1)?,
                "by": row.get::<_, Option<String>>(2)?,
                "account": row.get::<_, Option<i64>>(3)?,
                "name": row.get::<_, Option<String>>(4)?,
                "message": row.get::<_, Option<i64>>(5)?,
                "body": row.get::<_, Option<String>>(6)?,
                "reason": row.get::<_, String>(7)?,
                "at": row.get::<_, i64>(8)?,
            }))
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// Whether `account` may send, react or reply right now.
    fn may_talk(&self, account: i64) -> Result<()> {
        if self.is_suspended(account)? {
            bail!("this account is suspended");
        }
        if self.room_banned(account)? {
            bail!("you can't send in the group chat right now");
        }
        if self.verified_wallets(account)?.is_empty() {
            bail!("verify one of your wallets first: only accounts with a verified wallet can talk in the group chat");
        }
        Ok(())
    }

    /// The send limit: `A_MINUTE` a minute, `A_DAY` a day.
    fn room_limit(&self, account: i64) -> Result<()> {
        let since = |seconds: u64| -> Result<i64> {
            Ok(self.db.query_row(
                "SELECT COUNT(*) FROM room_messages WHERE author = ?1 AND created > ?2",
                params![account, now().saturating_sub(seconds) as i64],
                |row| row.get(0),
            )?)
        };
        if since(60)? >= A_MINUTE {
            bail!("that's {A_MINUTE} messages in a minute; wait a moment and send again");
        }
        if since(24 * 3600)? >= A_DAY {
            bail!("that's {A_DAY} messages today; you can send more tomorrow");
        }
        Ok(())
    }

    /// Checks a message before it is screened: who sends it, the limit, and
    /// its words, photos, GIF and answer. Gives the tidied words.
    pub fn room_check(&self, author: i64, body: &str, pictures: &[String], gif: Option<&Gif>, reply_to: Option<i64>) -> Result<String> {
        self.may_talk(author)?;
        self.room_limit(author)?;
        let body = body.trim().replace("\r\n", "\n");
        if body.chars().count() > MAX_MESSAGE {
            bail!("a message is at most {MAX_MESSAGE} characters");
        }
        if body.chars().any(|c| c.is_control() && c != '\n') {
            bail!("a message can't hold control characters");
        }
        if !pictures.is_empty() && gif.is_some() {
            bail!("a message has photos or a GIF, not both");
        }
        if pictures.len() > MAX_PICTURES {
            bail!("a message has at most {MAX_PICTURES} photos");
        }
        if body.is_empty() && pictures.is_empty() && gif.is_none() {
            bail!("write something first");
        }
        if let Some(gif) = gif {
            gif.checked()?;
        }
        if let Some(target) = reply_to {
            let open: bool = self
                .db
                .query_row("SELECT removed = 0 FROM room_messages WHERE id = ?1", [target], |row| row.get(0))
                .optional()?
                .unwrap_or(false);
            if !open {
                bail!("the message you're answering is gone");
            }
        }
        Ok(body)
    }

    /// Sends a message, checked by `room_check` and screened already.
    pub fn room_send(&self, author: i64, body: &str, pictures: &[String], gif: Option<&Gif>, reply_to: Option<i64>) -> Result<i64> {
        let body = self.room_check(author, body, pictures, gif, reply_to)?;
        let at = now() as i64;
        self.db.execute("BEGIN IMMEDIATE", [])?;
        let done = (|| -> Result<i64> {
            self.db.execute(
                "INSERT INTO room_messages (author, body, reply_to, created) VALUES (?1, ?2, ?3, ?4)",
                params![author, body, reply_to, at],
            )?;
            let id = self.db.last_insert_rowid();
            self.bump_room_message(id)?;
            for (position, name) in pictures.iter().enumerate() {
                let found: Option<(i64, i64)> = self
                    .db
                    .query_row(
                        "SELECT width, height FROM post_pictures WHERE name = ?1 AND account = ?2 AND post IS NULL",
                        params![name, author],
                        |row| Ok((row.get(0)?, row.get(1)?)),
                    )
                    .optional()?;
                let Some((width, height)) = found else {
                    bail!("a photo on this message wasn't uploaded by you, or is used already");
                };
                self.db.execute("DELETE FROM post_pictures WHERE name = ?1", [name])?;
                self.db.execute(
                    "INSERT INTO room_pictures (name, message, position, width, height) VALUES (?1, ?2, ?3, ?4, ?5)",
                    params![name, id, position as i64, width, height],
                )?;
            }
            if let Some(gif) = gif {
                self.db.execute(
                    "INSERT INTO room_gifs (message, gif, url, preview, width, height) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                    params![id, gif.id_text(), gif.url, gif.preview, gif.width, gif.height],
                )?;
            }
            if let Some(target) = reply_to {
                let owner: Option<i64> =
                    self.db.query_row("SELECT author FROM room_messages WHERE id = ?1", [target], |row| row.get(0)).optional()?;
                if let Some(owner) = owner
                    && owner != author
                    && !self.blocked_by(owner, author)?
                {
                    self.db.execute(
                        "INSERT INTO room_notices (account, reply, actor, created) VALUES (?1, ?2, ?3, ?4)",
                        params![owner, id, author, at],
                    )?;
                }
            }
            Ok(id)
        })();
        match done {
            Ok(id) => {
                self.db.execute("COMMIT", [])?;
                Ok(id)
            }
            Err(error) => {
                self.db.execute("ROLLBACK", [])?;
                Err(error)
            }
        }
    }

    /// Takes down `author`'s own message.
    pub fn room_remove_own(&self, author: i64, id: i64) -> Result<bool> {
        let taken = self.db.execute("UPDATE room_messages SET removed = 1 WHERE id = ?1 AND author = ?2 AND removed = 0", params![id, author])?;
        if taken == 1 {
            self.bump_room_message(id)?;
        }
        Ok(taken == 1)
    }

    /// Takes a message down: by a moderator, or from the dashboard (None).
    pub fn room_remove(&self, moderator: Option<i64>, id: i64) -> Result<bool> {
        if let Some(moderator) = moderator
            && !self.room_moderator(moderator)?
        {
            return Ok(false);
        }
        let taken = self.db.execute("UPDATE room_messages SET removed = 1 WHERE id = ?1 AND removed = 0", [id])?;
        if taken == 1 {
            self.bump_room_message(id)?;
            let author: Option<i64> = self.db.query_row("SELECT author FROM room_messages WHERE id = ?1", [id], |row| row.get(0)).optional()?;
            self.room_note(moderator, "remove", author, Some(id), "")?;
        }
        Ok(taken == 1)
    }

    pub fn room_removed(&self, id: i64) -> Result<bool> {
        Ok(self.db.query_row("SELECT removed = 1 FROM room_messages WHERE id = ?1", [id], |row| row.get(0)).optional()?.unwrap_or(true))
    }

    /// Adds `emoji` from `account` to a message, or takes it back. Gives
    /// whether it is on.
    pub fn room_react(&self, account: i64, id: i64, emoji: &str) -> Result<bool> {
        self.may_talk(account)?;
        if !REACTIONS.contains(&emoji) {
            bail!("that reaction isn't one of {}", REACTIONS.join(" "));
        }
        let author: Option<i64> = self
            .db
            .query_row("SELECT author FROM room_messages WHERE id = ?1 AND removed = 0", [id], |row| row.get(0))
            .optional()?;
        let Some(author) = author else { bail!("that message is gone") };
        if self.blocked_either(author, account)? {
            bail!("that message is gone");
        }
        let taken = self.db.execute(
            "DELETE FROM room_reactions WHERE message = ?1 AND account = ?2 AND emoji = ?3",
            params![id, account, emoji],
        )?;
        if taken == 0 {
            self.db.execute("INSERT INTO room_reactions (message, account, emoji) VALUES (?1, ?2, ?3)", params![id, account, emoji])?;
        }
        self.bump_room_message(id)?;
        Ok(taken == 0)
    }

    /// Messages as `viewer` sees them, oldest first: the newest page, the
    /// page `before` a message, or what came `after` one. With `changes`,
    /// also the messages up to `after` that changed since that version.
    pub fn room_page(&self, viewer: Option<i64>, ask: &RoomAsk) -> Result<Value> {
        let limit = ask.limit.unwrap_or(PAGE).clamp(1, MOST);
        let moderator = match viewer {
            Some(viewer) => self.room_moderator(viewer)?,
            None => false,
        };
        let (range, order, key) = match (ask.after, ask.before) {
            (Some(after), _) => ("m.id > ?1", "ASC", after),
            (None, Some(before)) => ("m.id < ?1", "DESC", before),
            (None, None) => ("?1 = ?1", "DESC", 0),
        };
        let mut messages = self.room_rows(viewer, moderator, &format!("{range} AND m.removed = 0"), order, key, limit + 1)?;
        let more = messages.len() > limit;
        messages.truncate(limit);
        if order == "DESC" {
            messages.reverse();
        }
        let newest: i64 = self.db.query_row("SELECT COALESCE(MAX(id), 0) FROM room_messages", [], |row| row.get(0))?;
        let mut answer = json!({
            "messages": messages,
            "more": more,
            "newest": newest,
            "version": self.room_version()?,
        });
        if let (Some(since), Some(after)) = (ask.changes, ask.after) {
            // What was sent before `after` and changed since: removed ones
            // say so, the rest come whole.
            let mut statement = self.db.prepare(&format!(
                "SELECT m.id, m.removed OR {} FROM room_messages m WHERE m.version > ?1 AND m.id <= ?3 ORDER BY m.id",
                hidden("m.author")
            ))?;
            let found: Vec<(i64, bool)> = statement
                .query_map(params![since, viewer.unwrap_or(-1), after], |row| Ok((row.get(0)?, row.get(1)?)))?
                .collect::<rusqlite::Result<_>>()?;
            let mut changed = Vec::new();
            for (id, gone) in found {
                if gone {
                    changed.push(json!({ "id": id, "removed": true }));
                } else if let Some(message) = self.room_rows(viewer, moderator, "m.id = ?1", "ASC", id, 1)?.pop() {
                    changed.push(message);
                }
            }
            answer["changed"] = json!(changed);
        }
        Ok(answer)
    }

    /// Messages matching `filter` (?1 is `key`, ?2 the viewer), up to `limit`.
    fn room_rows(&self, viewer: Option<i64>, moderator: bool, filter: &str, order: &str, key: i64, limit: usize) -> Result<Vec<Value>> {
        let viewer = viewer.unwrap_or(-1);
        let mut statement = self.db.prepare(&format!(
            "SELECT m.id, m.author, a.name, a.picture, m.body, m.reply_to, m.created,
                    EXISTS (SELECT 1 FROM verified_wallets v WHERE v.account = m.author AND v.proven IS NOT NULL),
                    EXISTS (SELECT 1 FROM room_bans b WHERE b.account = m.author),
                    EXISTS (SELECT 1 FROM room_moderators d WHERE d.account = m.author)
             FROM room_messages m JOIN accounts a ON a.id = m.author
             WHERE {filter} AND NOT {}
             ORDER BY m.id {order} LIMIT {limit}",
            hidden("m.author")
        ))?;
        type Row = (i64, i64, String, Option<String>, String, Option<i64>, i64, bool, bool, bool);
        let rows: Vec<Row> = statement
            .query_map(params![key, viewer], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?, row.get(5)?, row.get(6)?, row.get(7)?, row.get(8)?, row.get(9)?))
            })?
            .collect::<rusqlite::Result<_>>()?;
        let mut messages = Vec::with_capacity(rows.len());
        for (id, author, name, picture, body, reply_to, created, verified, banned, is_moderator) in rows {
            let reply_to = match reply_to {
                Some(target) => self.room_quote(target, viewer)?,
                None => None,
            };
            let mut counts =
                self.db.prepare_cached("SELECT emoji, COUNT(*), MAX(account = ?2) FROM room_reactions WHERE message = ?1 GROUP BY emoji")?;
            let mut reactions: Vec<Value> = counts
                .query_map(params![id, viewer], |row| {
                    Ok(json!({ "emoji": row.get::<_, String>(0)?, "count": row.get::<_, i64>(1)?, "me": row.get::<_, bool>(2)? }))
                })?
                .collect::<rusqlite::Result<_>>()?;
            let mut names = self.reactor_names(true, id)?;
            for reaction in &mut reactions {
                let emoji = reaction["emoji"].as_str().unwrap_or_default().to_string();
                reaction["names"] = json!(names.remove(&emoji).unwrap_or_default());
            }
            let mut who = json!({ "id": author, "name": name, "picture": picture, "verified": verified });
            // Only moderators see who is banned.
            if moderator && banned {
                who["banned"] = json!(true);
            }
            // Everyone sees who the moderators are.
            if is_moderator {
                who["moderator"] = json!(true);
            }
            messages.push(json!({
                "id": id,
                "author": who,
                "body": body,
                "pictures": self.room_pictures(id)?,
                "gif": self.room_gif(id)?,
                "reply_to": reply_to,
                "reactions": reactions,
                "created": created,
                "mine": author == viewer,
            }));
        }
        Ok(messages)
    }

    /// The start of an answered message, if `viewer` can still see it.
    fn room_quote(&self, target: i64, viewer: i64) -> Result<Option<Value>> {
        let found: Option<(String, String)> = self
            .db
            .query_row(
                &format!(
                    "SELECT a.name, m.body FROM room_messages m JOIN accounts a ON a.id = m.author
                     WHERE m.id = ?1 AND m.removed = 0 AND NOT {}",
                    hidden("m.author")
                ),
                params![target, viewer],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        let Some((name, body)) = found else { return Ok(None) };
        let words: String = body.split_whitespace().collect::<Vec<_>>().join(" ");
        let mut short: String = words.chars().take(QUOTE).collect();
        if short.chars().count() < words.chars().count() {
            short.push('…');
        }
        if short.is_empty() {
            short = if self.room_gif(target)?.is_some() { "GIF".into() } else { "Photo".into() };
        }
        Ok(Some(json!({ "id": target, "author_name": name, "body": short })))
    }

    fn room_pictures(&self, message: i64) -> Result<Vec<Value>> {
        let mut statement = self.db.prepare_cached("SELECT name, width, height FROM room_pictures WHERE message = ?1 ORDER BY position")?;
        let rows = statement.query_map([message], |row| {
            Ok(json!({ "name": row.get::<_, String>(0)?, "width": row.get::<_, i64>(1)?, "height": row.get::<_, i64>(2)? }))
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    fn room_gif(&self, message: i64) -> Result<Option<Value>> {
        Ok(self
            .db
            .query_row("SELECT gif, url, preview, width, height FROM room_gifs WHERE message = ?1", [message], |row| {
                Ok(json!({
                    "id": row.get::<_, String>(0)?,
                    "url": row.get::<_, String>(1)?,
                    "preview": row.get::<_, String>(2)?,
                    "width": row.get::<_, i64>(3)?,
                    "height": row.get::<_, i64>(4)?,
                }))
            })
            .optional()?)
    }

    /// The bell's lines for answers to `account`'s messages since `from`:
    /// one per message answered, with everyone who answered it.
    pub(crate) fn room_notices(&self, account: i64, from: u64, seen_at: u64) -> Result<Vec<Value>> {
        let mut statement = self.db.prepare(&format!(
            "SELECT n.reply, r.reply_to, n.created, a.name, a.picture, r.body
             FROM room_notices n
             JOIN room_messages r ON r.id = n.reply
             JOIN accounts a ON a.id = n.actor
             WHERE n.account = ?1 AND n.created > ?3 AND r.removed = 0 AND NOT {}
             ORDER BY n.created DESC, n.id DESC",
            hidden("n.actor")
        ))?;
        let rows: Vec<(i64, Option<i64>, i64, String, Option<String>, String)> = statement
            .query_map(params![account, account, from as i64], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?, row.get(5)?))
            })?
            .collect::<rusqlite::Result<_>>()?;
        let mut lines: Vec<Value> = Vec::new();
        for (reply, answered, created, name, picture, body) in rows {
            let found = lines.iter_mut().find(|line| line["reply_to"] == json!(answered));
            match found {
                Some(line) => {
                    if created as u64 > seen_at {
                        line["unseen"] = json!(true);
                    }
                    let people = line["people"].as_array_mut().expect("a list");
                    if !people.iter().any(|p| p["name"] == name) {
                        if people.len() < 3 {
                            people.push(json!({ "name": name, "picture": picture }));
                        } else {
                            line["others"] = json!(line["others"].as_i64().unwrap_or(0) + 1);
                        }
                    }
                }
                None => {
                    let words: String = body.split_whitespace().collect::<Vec<_>>().join(" ");
                    let cut: String = words.chars().take(90).collect();
                    let text = if cut.chars().count() < words.chars().count() { format!("{cut}…") } else { cut };
                    lines.push(json!({
                        "kind": "room_reply",
                        "message": reply,
                        "reply_to": answered,
                        "text": text,
                        "time": created,
                        "unseen": created as u64 > seen_at,
                        "people": [{ "name": name, "picture": picture }],
                        "others": 0,
                        "emojis": [],
                    }));
                }
            }
        }
        Ok(lines)
    }
}

pub fn routes() -> Router<Arc<App>> {
    Router::new()
        .route("/api/market/room/messages", get(page).post(send))
        .route("/api/market/room/messages/{id}", axum::routing::delete(remove))
        .route("/api/market/room/messages/{id}/react", post(react))
        .route("/api/market/room/bans", get(bans).post(ban))
        .route("/api/market/room/bans/{account}", axum::routing::delete(lift))
}

#[derive(Deserialize, Default)]
pub struct RoomAsk {
    after: Option<i64>,
    before: Option<i64>,
    limit: Option<usize>,
    /// The `version` of the app's last look: what changed since comes back
    /// in `changed` (only with `after`).
    changes: Option<i64>,
}

async fn page(State(app): State<Arc<App>>, headers: HeaderMap, Query(ask): Query<RoomAsk>) -> Result<Response, Failure> {
    let viewer = app.signed_in(&headers).ok();
    let store = app.store();
    let mut answer = store.room_page(viewer, &ask)?;
    answer["reactions"] = json!(REACTIONS);
    answer["max"] = json!(MAX_MESSAGE);
    answer["max_pictures"] = json!(MAX_PICTURES);
    answer["you"] = match viewer {
        Some(id) => match store.account(id)? {
            Some(account) => {
                let verified = !store.verified_wallets(id)?.is_empty();
                json!({
                    "id": id,
                    "name": account.name,
                    "picture": account.picture,
                    "verified": verified,
                    "moderator": store.room_moderator(id)?,
                    "banned": store.room_banned(id)?,
                })
            }
            None => Value::Null,
        },
        None => Value::Null,
    };
    // Each reader's answer leaves out whom they blocked.
    Ok(([(axum::http::header::CACHE_CONTROL, "private, no-store")], Json(answer)).into_response())
}

#[derive(Deserialize)]
pub struct NewMessage {
    #[serde(default)]
    body: String,
    /// Photos uploaded first through /api/market/community/pictures.
    #[serde(default)]
    pictures: Vec<String>,
    gif: Option<Gif>,
    reply_to: Option<i64>,
}

async fn send(State(app): State<Arc<App>>, headers: HeaderMap, Json(given): Json<NewMessage>) -> Result<Response, Failure> {
    let author = app.signed_in(&headers)?;
    let text = app
        .store()
        .room_check(author, &given.body, &given.pictures, given.gif.as_ref(), given.reply_to)
        .map_err(refused)?;
    if !text.is_empty() {
        screened(&app, "message", move |s| s.message(&text)).await?;
    }
    let id = app
        .store()
        .room_send(author, &given.body, &given.pictures, given.gif.as_ref(), given.reply_to)
        .map_err(refused)?;
    Ok(Json(json!({ "id": id })).into_response())
}

async fn remove(State(app): State<Arc<App>>, headers: HeaderMap, Path(id): Path<i64>) -> Result<Response, Failure> {
    let author = app.signed_in(&headers)?;
    let store = app.store();
    // Your own, or anyone's for a moderator.
    if !store.room_remove_own(author, id)? && !store.room_remove(Some(author), id)? {
        return Err(Failure::NotFound);
    }
    Ok(Json(json!({ "removed": true })).into_response())
}

/// Moderators only: who is banned.
async fn bans(State(app): State<Arc<App>>, headers: HeaderMap) -> Result<Response, Failure> {
    let me = app.signed_in(&headers)?;
    let store = app.store();
    if !store.room_moderator(me)? {
        return Err(Failure::NotFound);
    }
    Ok(([(axum::http::header::CACHE_CONTROL, "private, no-store")], Json(json!({ "bans": store.room_bans()? }))).into_response())
}

#[derive(Deserialize)]
pub struct NewBan {
    account: i64,
    #[serde(default)]
    reason: String,
}

async fn ban(State(app): State<Arc<App>>, headers: HeaderMap, Json(given): Json<NewBan>) -> Result<Response, Failure> {
    let me = app.signed_in(&headers)?;
    app.store().room_ban(Some(me), given.account, &given.reason).map_err(refused)?;
    Ok(Json(json!({ "banned": true })).into_response())
}

async fn lift(State(app): State<Arc<App>>, headers: HeaderMap, Path(account): Path<i64>) -> Result<Response, Failure> {
    let me = app.signed_in(&headers)?;
    if !app.store().room_unban(Some(me), account).map_err(refused)? {
        return Err(Failure::NotFound);
    }
    Ok(Json(json!({ "lifted": true })).into_response())
}

#[derive(Deserialize)]
pub struct Reaction {
    emoji: String,
}

async fn react(State(app): State<Arc<App>>, headers: HeaderMap, Path(id): Path<i64>, Json(given): Json<Reaction>) -> Result<Response, Failure> {
    let account = app.signed_in(&headers)?;
    let on = app.store().room_react(account, id, &given.emoji).map_err(refused)?;
    Ok(Json(json!({ "on": on })).into_response())
}

#[cfg(test)]
mod tests {
    use super::RoomAsk;
    use crate::store::Store;

    fn people() -> (Store, i64, i64, i64) {
        let store = Store::in_memory().unwrap();
        let mut ids = Vec::new();
        for (i, name) in ["Maya", "Sam", "Ana"].iter().enumerate() {
            let id = store.create_account_with_email(name, &format!("{name}@example.com")).unwrap().id;
            if i < 2 {
                store.verified_wallet_for_tests(id, &format!("{i}").repeat(64)).unwrap();
            }
            ids.push(id);
        }
        (store, ids[0], ids[1], ids[2])
    }

    fn ask(after: Option<i64>, before: Option<i64>, limit: Option<usize>, changes: Option<i64>) -> RoomAsk {
        RoomAsk { after, before, limit, changes }
    }

    #[test]
    fn only_verified_accounts_talk_and_everyone_reads() {
        let (store, maya, _sam, ana) = people();
        let id = store.room_send(maya, "  gm everyone  ", &[], None, None).unwrap();
        assert!(store.room_send(ana, "hello", &[], None, None).unwrap_err().to_string().contains("verify"));
        assert!(store.room_react(ana, id, "🔥").is_err());
        assert!(store.room_send(maya, "   ", &[], None, None).is_err());
        assert!(store.room_send(maya, &"x".repeat(501), &[], None, None).is_err());
        let page = store.room_page(None, &RoomAsk::default()).unwrap();
        let messages = page["messages"].as_array().unwrap();
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0]["body"], "gm everyone");
        assert_eq!(messages[0]["author"]["name"], "Maya");
        assert_eq!(messages[0]["author"]["verified"], true);
        assert_eq!(messages[0]["mine"], false);
        assert_eq!(page["newest"], id);
    }

    #[test]
    fn pages_come_oldest_first_and_polls_see_whats_new_and_changed() {
        let (store, maya, sam, _ana) = people();
        let mut ids = Vec::new();
        for i in 0..7 {
            let who = if i % 2 == 0 { maya } else { sam };
            ids.push(store.room_send(who, &format!("message {i}"), &[], None, None).unwrap());
            store.db.execute("UPDATE room_messages SET created = created - 120", []).unwrap();
        }
        let bodies = |page: &serde_json::Value| page["messages"].as_array().unwrap().iter().map(|m| m["body"].as_str().unwrap().to_string()).collect::<Vec<_>>();
        let newest = store.room_page(Some(maya), &ask(None, None, Some(3), None)).unwrap();
        assert_eq!(bodies(&newest), ["message 4", "message 5", "message 6"]);
        assert_eq!(newest["more"], true);
        let older = store.room_page(Some(maya), &ask(None, Some(ids[4]), Some(3), None)).unwrap();
        assert_eq!(bodies(&older), ["message 1", "message 2", "message 3"]);
        let oldest = store.room_page(Some(maya), &ask(None, Some(ids[1]), Some(3), None)).unwrap();
        assert_eq!((bodies(&oldest), oldest["more"].clone()), (vec!["message 0".to_string()], serde_json::json!(false)));

        let version = newest["version"].as_i64().unwrap();
        store.room_react(sam, ids[6], "🔥").unwrap();
        store.room_remove_own(maya, ids[4]).unwrap();
        assert!(!store.room_remove_own(maya, ids[5]).unwrap(), "only your own");
        let new = store.room_send(sam, "message 7", &[], None, Some(ids[6])).unwrap();
        let poll = store.room_page(Some(maya), &ask(Some(ids[6]), None, None, Some(version))).unwrap();
        assert_eq!(bodies(&poll), ["message 7"]);
        assert_eq!(poll["messages"][0]["reply_to"]["author_name"], "Maya");
        assert_eq!(poll["messages"][0]["reply_to"]["body"], "message 6");
        let changed = poll["changed"].as_array().unwrap();
        assert_eq!(changed.len(), 2, "{changed:#?}");
        assert_eq!(changed[0], serde_json::json!({ "id": ids[4], "removed": true }));
        assert_eq!(changed[1]["reactions"], serde_json::json!([{ "emoji": "🔥", "count": 1, "me": false, "names": ["Sam"] }]));
        let again = store.room_page(Some(maya), &ask(Some(new), None, None, Some(poll["version"].as_i64().unwrap()))).unwrap();
        assert!(again["messages"].as_array().unwrap().is_empty() && again["changed"].as_array().unwrap().is_empty());
        // Reacting twice takes it back.
        assert!(!store.room_react(sam, ids[6], "🔥").unwrap());
        assert!(store.room_react(sam, ids[6], "🙃").is_err());
    }

    #[test]
    fn blocks_hide_people_both_ways_and_answers_ring_the_bell() {
        let (store, maya, sam, _ana) = people();
        let mine = store.room_send(maya, "anyone mining tonight?", &[], None, None).unwrap();
        store.room_send(sam, "me!", &[], None, Some(mine)).unwrap();
        let lines = store.room_notices(maya, 0, 0).unwrap();
        assert_eq!(lines.len(), 1);
        assert_eq!((lines[0]["kind"].as_str(), lines[0]["text"].as_str(), lines[0]["reply_to"].as_i64()), (Some("room_reply"), Some("me!"), Some(mine)));
        assert!(store.room_notices(sam, 0, 0).unwrap().is_empty());
        let names = |viewer: Option<i64>| {
            store.room_page(viewer, &RoomAsk::default()).unwrap()["messages"]
                .as_array()
                .unwrap()
                .iter()
                .map(|m| m["author"]["name"].as_str().unwrap().to_string())
                .collect::<Vec<_>>()
        };
        store.db.execute("INSERT INTO blocks (blocker, blocked, at) VALUES (?1, ?2, 0)", [maya, sam]).unwrap();
        assert_eq!(names(Some(maya)), ["Maya"]);
        assert_eq!(names(Some(sam)), ["Sam"]);
        assert_eq!(names(None), ["Maya", "Sam"]);
        assert!(store.room_notices(maya, 0, 0).unwrap().is_empty(), "nothing from someone blocked");
        assert!(store.room_react(sam, mine, "❤️").is_err());
    }

    #[test]
    fn moderators_take_down_any_message_and_ban_people() {
        let (store, maya, sam, ana) = people();
        let theirs = store.room_send(sam, "buy my coin", &[], None, None).unwrap();
        assert!(!store.room_remove(Some(maya), theirs).unwrap(), "not a moderator yet");
        assert!(store.room_ban(Some(maya), sam, "").is_err());
        store.room_set_moderator(maya, true).unwrap();
        assert!(store.room_remove(Some(maya), theirs).unwrap());
        store.room_send(maya, "please don't", &[], None, None).unwrap();
        let page = store.room_page(None, &RoomAsk::default()).unwrap();
        assert_eq!(page["messages"][0]["author"]["moderator"], true, "everyone sees the tag");
        assert!(store.room_ban(Some(maya), maya, "").is_err(), "not yourself");
        let before = store.room_page(Some(maya), &RoomAsk::default()).unwrap()["version"].as_i64().unwrap();
        let shown = store.room_send(sam, "still here", &[], None, None).unwrap();
        store.room_ban(Some(maya), sam, "spam").unwrap();
        let poll = store.room_page(Some(maya), &ask(Some(shown), None, None, Some(before))).unwrap();
        assert!(poll["changed"].as_array().unwrap().iter().any(|m| m["id"] == shown && m["author"]["banned"] == true), "{poll:#?}");
        assert_eq!(store.room_send(sam, "hi", &[], None, None).unwrap_err().to_string(), "you can't send in the group chat right now");
        assert!(store.room_ban(None, maya, "").is_err(), "a moderator can't be banned");
        let bans = store.room_bans().unwrap();
        assert_eq!((bans[0]["name"].as_str(), bans[0]["by"].as_str()), (Some("Sam"), Some("Maya")));
        assert!(store.room_unban(Some(maya), sam).unwrap());
        store.room_send(sam, "sorry", &[], None, None).unwrap();
        let log: Vec<String> = store.room_log(10).unwrap().iter().map(|l| l["action"].as_str().unwrap().to_string()).collect();
        assert_eq!(log, ["lift", "ban", "remove"]);
        let _ = ana;
    }

    #[test]
    fn the_send_limit_holds() {
        let (store, maya, _sam, _ana) = people();
        for i in 0..10 {
            store.room_send(maya, &format!("{i}"), &[], None, None).unwrap();
        }
        assert!(store.room_send(maya, "one more", &[], None, None).unwrap_err().to_string().contains("a minute"));
    }

    #[test]
    fn reactions_name_who_left_them_first_to_react_first() {
        let (store, maya, sam, _ana) = people();
        let id = store.room_send(maya, "gm", &[], None, None).unwrap();
        store.room_react(sam, id, "🔥").unwrap();
        store.room_react(maya, id, "🔥").unwrap();
        let page = store.room_page(Some(maya), &RoomAsk::default()).unwrap();
        let fire = &page["messages"][0]["reactions"][0];
        assert_eq!(fire["count"], 2);
        assert_eq!(fire["names"], serde_json::json!(["Sam", "Maya"]));
        let post = store.add_post(maya, "First block mined", None).unwrap();
        store.react(sam, post, "⛏️").unwrap();
        let names = store.reactor_names(false, post).unwrap();
        assert_eq!(names["⛏️"], vec!["Sam".to_string()]);
    }
}
