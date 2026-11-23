//! The community feed at /community/: people's posts, replies and
//! reactions, and posts the network writes by itself after each checkpoint
//! (records, new miners, milestones), beside the miner leaderboard.
//!
//! Only accounts with a verified wallet post, reply or react: a
//! verified wallet is what ties an account to real mining. Every post is
//! screened like a Market message before anyone sees it, and can be
//! reported to the dashboard.
//!
//! Mining figures come from the explorer's record of each checkpoint's
//! rewards, which covers the checkpoints validators keep records for. An
//! address belongs to an account when the account verified it.

use crate::api::{App, Failure, refused, screened};
use crate::explorer::Mined;
use crate::store::{Store, now};
use anyhow::{Result, bail};
use axum::extract::{Path, Query, State};
use axum::http::HeaderMap;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub const MAX_POST: usize = 500;
/// Photos on one post.
pub const MAX_PICTURES: usize = 4;
/// A poll's choices: at least two, at most four, each this long at most.
pub const POLL_OPTIONS: std::ops::RangeInclusive<usize> = 2..=4;
pub const MAX_OPTION: usize = 25;
/// How long a poll may run, in hours: a day, three days or a week.
pub const POLL_HOURS: [u64; 3] = [24, 72, 168];
const POSTS_AN_HOUR: i64 = 20;
const PAGE: i64 = 30;
/// The reactions anyone can leave.
pub const REACTIONS: [&str; 5] = ["⛏️", "🔥", "❤️", "👏", "😂"];
/// ORN mined that earns a milestone post.
const MILESTONES: [u64; 9] = [1, 100, 250, 500, 1_000, 5_000, 10_000, 50_000, 100_000];
/// New miners shown with a face on one hour's welcome card; more are counted.
const FACES: usize = 12;
/// New miners are welcomed in blocks of this long.
const BLOCK_SECONDS: u64 = 6 * 3600;
/// What the network posts that every app knows how to show. Newer kinds
/// (`NEWER_KINDS`) reach only apps that ask for them by name, so an older
/// Orien Wallet never shows a card it can't draw.
const KNOWN_KINDS: &str = "'post', 'record', 'welcome', 'milestone', 'streak', 'top_day'";
/// Network posts added later: a finished swap, ORN mined so far, a new validator.
pub const NEWER_KINDS: [&str; 3] = ["swap", "supply", "validator"];

/// The next round number of ORN mined to post about after `orn`: every
/// 100,000 up to a million, then every million.
pub fn next_supply_mark(orn: u64) -> u64 {
    if orn < 1_000_000 { (orn / 100_000 + 1) * 100_000 } else { (orn / 1_000_000 + 1) * 1_000_000 }
}

/// Checkpoints in a row that earn a streak post.
const STREAKS: [u64; 5] = [24, 48, 100, 168, 336];
const UNIT: u128 = 1_000_000_000;
/// Rows on each leaderboard.
const BOARD: usize = 50;

/// Who mined: an account (through its verified wallets), or an address no
/// account verified. Only accounts are ranked; addresses still count for
/// the network's news.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Miner {
    Account(i64),
    Address(String),
}

/// What the network has paid each miner, worked out from the explorer's
/// records each minute.
#[derive(Clone, Debug, Default, Serialize)]
pub struct Board {
    pub epoch: u64,
    pub since: u64,
    pub miners: Vec<MinerRow>,
    #[serde(skip)]
    pub accounts: HashMap<i64, Stats>,
}

#[derive(Clone, Debug, Default, Serialize, PartialEq, Eq)]
pub struct Stats {
    /// In ORN's smallest units, as text.
    pub mined: String,
    /// Checkpoints in a row, up to the latest, that paid this miner.
    pub streak: u64,
    pub rank: usize,
}

#[derive(Clone, Debug, Serialize)]
pub struct MinerRow {
    /// The account, for the board's own use.
    #[serde(skip)]
    pub account: Option<i64>,
    pub rank: usize,
    pub name: Option<String>,
    pub picture: Option<String>,
    pub address: Option<String>,
    pub mined: String,
    pub streak: u64,
}

/// The board, kept up to date by the watcher.
#[derive(Default)]
pub struct Community {
    board: Mutex<Option<Board>>,
    /// The verified wallets the late posts were last filled in for: they
    /// are filled in again whenever someone verifies a wallet.
    owners_told: Mutex<Option<Vec<(String, i64)>>>,
}

/// Each miner's total and streak, and which miners each checkpoint paid.
struct Tally {
    totals: HashMap<Miner, u128>,
    streaks: HashMap<Miner, u64>,
}

fn whose(address: &str, owners: &HashMap<String, i64>) -> Miner {
    owners.get(address).map_or_else(|| Miner::Address(address.to_string()), |a| Miner::Account(*a))
}

/// The miners each checkpoint paid, and how much, by miner.
fn paid_by_epoch(mined: &Mined, owners: &HashMap<String, i64>) -> Vec<(u64, HashMap<Miner, u128>)> {
    mined
        .by_epoch
        .iter()
        .map(|(epoch, paid)| {
            let mut by: HashMap<Miner, u128> = HashMap::new();
            for (address, amount) in paid {
                *by.entry(whose(address, owners)).or_default() += *amount as u128;
            }
            (*epoch, by)
        })
        .collect()
}

fn tally(mined: &Mined, owners: &HashMap<String, i64>) -> Tally {
    let epochs = paid_by_epoch(mined, owners);
    let mut totals: HashMap<Miner, u128> = HashMap::new();
    for (_, by) in &epochs {
        for (miner, amount) in by {
            *totals.entry(miner.clone()).or_default() += amount;
        }
    }
    let mut streaks: HashMap<Miner, u64> = HashMap::new();
    let paid: HashMap<u64, &HashMap<Miner, u128>> = epochs.iter().map(|(e, by)| (*e, by)).collect();
    for miner in totals.keys() {
        let mut streak = 0;
        let mut epoch = mined.upto;
        while paid.get(&epoch).is_some_and(|by| by.contains_key(miner)) {
            streak += 1;
            if epoch == 0 {
                break;
            }
            epoch -= 1;
        }
        streaks.insert(miner.clone(), streak);
    }
    Tally { totals, streaks }
}

/// The leaderboard from what the network paid. `accounts` gives each
/// account's name and picture.
pub fn board(mined: &Mined, owners: &HashMap<String, i64>, accounts: &HashMap<i64, (String, Option<String>)>) -> Board {
    let tally = tally(mined, owners);
    // Only miners with a verified wallet on their profile are ranked: the
    // board is about people, not addresses.
    let mut ranked: Vec<(&Miner, u128)> =
        tally.totals.iter().filter(|(m, _)| matches!(m, Miner::Account(_))).map(|(m, t)| (m, *t)).collect();
    ranked.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(b.0)));
    let mut board = Board { epoch: mined.upto, since: mined.since, ..Board::default() };
    for (index, (miner, total)) in ranked.iter().enumerate() {
        let streak = tally.streaks.get(*miner).copied().unwrap_or(0);
        if let Miner::Account(id) = miner {
            board.accounts.insert(*id, Stats { mined: total.to_string(), streak, rank: index + 1 });
        }
        if index < BOARD {
            let account = match miner {
                Miner::Account(id) => Some(*id),
                Miner::Address(_) => None,
            };
            let (name, picture, address) = match miner {
                Miner::Account(id) => {
                    let (name, picture) = accounts.get(id).cloned().unwrap_or_default();
                    (Some(name), picture, None)
                }
                Miner::Address(address) => (None, None, Some(address.clone())),
            };
            board.miners.push(MinerRow { account, rank: index + 1, name, picture, address, mined: total.to_string(), streak });
        }
    }
    board
}

/// A miner in a post's data: by account, so the name follows renames, or
/// by address.
fn who(miner: &Miner) -> Value {
    match miner {
        Miner::Account(id) => json!({ "account": id }),
        Miner::Address(address) => json!({ "address": address }),
    }
}

/// What the network says about checkpoint `epoch`: a record, who joined,
/// milestones, streaks, and yesterday's top miner, as (kind, subject
/// account, data).
pub fn happenings(mined: &Mined, owners: &HashMap<String, i64>, epoch: u64) -> Vec<(String, Option<i64>, Value)> {
    let epochs = paid_by_epoch(mined, owners);
    let Some((_, now_paid)) = epochs.iter().find(|(e, _)| *e == epoch) else { return Vec::new() };
    let before: Vec<&(u64, HashMap<Miner, u128>)> = epochs.iter().filter(|(e, _)| *e < epoch).collect();
    let mut said = Vec::new();
    // Nothing to compare the first hour of the records with.
    if before.is_empty() {
        return said;
    }
    let paid_at: HashMap<u64, &HashMap<Miner, u128>> = epochs.iter().map(|(e, by)| (*e, by)).collect();

    // A record: more miners paid than any hour before.
    let addresses = |e: &u64| mined.by_epoch.get(e).map_or(0, |paid| paid.len());
    let most_before = before.iter().map(|(e, _)| addresses(e)).max().unwrap_or(0);
    if addresses(&epoch) > most_before {
        said.push(("record".to_string(), None, json!({ "epoch": epoch, "miners": addresses(&epoch), "before": most_before })));
    }

    // Who was paid for the first time, in the six-hour block that has just
    // ended: one welcome card per block, every newcomer on it. The block
    // the records begin in has nothing to compare with, so it gets none.
    let block = |e: u64| mined.times.get(&e).map(|t| t / BLOCK_SECONDS);
    if let (Some(now_block), Some(last_block)) = (block(epoch), block(epoch - 1))
        && now_block != last_block
    {
        let in_block: Vec<&(u64, HashMap<Miner, u128>)> = epochs.iter().filter(|(e, _)| block(*e) == Some(last_block)).collect();
        let first = in_block.first().map(|(e, _)| *e).unwrap_or(epoch);
        if first > mined.since {
            let seen_before: HashSet<&Miner> = epochs.iter().filter(|(e, _)| *e < first).flat_map(|(_, by)| by.keys()).collect();
            let mut fresh: Vec<(&Miner, u128)> = Vec::new();
            for (_, by) in &in_block {
                for (miner, amount) in by.iter() {
                    if seen_before.contains(miner) {
                        continue;
                    }
                    match fresh.iter_mut().find(|(m, _)| *m == miner) {
                        Some(found) => found.1 += amount,
                        None => fresh.push((miner, *amount)),
                    }
                }
            }
            fresh.sort_by(|a, b| {
                let named = |m: &Miner| matches!(m, Miner::Account(_));
                named(b.0).cmp(&named(a.0)).then_with(|| b.1.cmp(&a.1)).then_with(|| a.0.cmp(b.0))
            });
            if !fresh.is_empty() {
                let subject = fresh.iter().find_map(|(m, _)| if let Miner::Account(id) = m { Some(*id) } else { None });
                let faces: Vec<Value> = fresh
                    .iter()
                    .take(FACES)
                    .map(|(m, a)| {
                        let mut face = who(m);
                        face["paid"] = json!(a.to_string());
                        face
                    })
                    .collect();
                let last = in_block.last().map(|(e, _)| *e).unwrap_or(epoch);
                said.push((
                    "welcome".to_string(),
                    subject,
                    json!({ "epoch": last, "from": first, "to": last, "hours": in_block.len(), "count": fresh.len(), "miners": faces }),
                ));
            }
        }
    }

    // Milestones and streaks, for people.
    let mut totals_before: HashMap<&Miner, u128> = HashMap::new();
    for (_, by) in &before {
        for (miner, amount) in by {
            *totals_before.entry(miner).or_default() += amount;
        }
    }
    for (miner, amount) in now_paid {
        let Miner::Account(id) = miner else { continue };
        let was = totals_before.get(miner).copied().unwrap_or(0);
        let is = was + amount;
        if let Some(mark) = MILESTONES.iter().rev().find(|m| was < **m as u128 * UNIT && is >= **m as u128 * UNIT) {
            said.push(("milestone".to_string(), Some(*id), json!({ "epoch": epoch, "orn": mark, "mined": is.to_string() })));
        }
        let mut streak = 0u64;
        let mut e = epoch;
        while paid_at.get(&e).is_some_and(|by| by.contains_key(miner)) {
            streak += 1;
            if e == 0 {
                break;
            }
            e -= 1;
        }
        if STREAKS.contains(&streak) {
            said.push(("streak".to_string(), Some(*id), json!({ "epoch": epoch, "hours": streak })));
        }
    }

    // The day before this checkpoint's, once it is over: its top miner among
    // the members, those with a verified wallet, like the leaderboard. The
    // day's totals count everyone.
    let day = |e: u64| mined.times.get(&e).map(|t| t / 86_400);
    if let (Some(today), Some(yesterday)) = (day(epoch), day(epoch - 1))
        && today != yesterday
    {
        let mut by_miner: HashMap<&Miner, u128> = HashMap::new();
        let mut hours = 0u64;
        for (e, by) in &epochs {
            if day(*e) == Some(yesterday) {
                hours += 1;
                for (miner, amount) in by {
                    *by_miner.entry(miner).or_default() += amount;
                }
            }
        }
        let total: u128 = by_miner.values().sum();
        let members = by_miner.iter().filter(|(m, _)| matches!(m, Miner::Account(_)));
        if let Some((top, amount)) = members.max_by(|a, b| a.1.cmp(b.1).then_with(|| b.0.cmp(a.0))) {
            let subject = if let Miner::Account(id) = top { Some(*id) } else { None };
            let date = chrono_date(yesterday * 86_400);
            said.push((
                "top_day".to_string(),
                subject,
                json!({ "epoch": epoch, "day": date, "miner": who(top), "mined": amount.to_string(), "total": total.to_string(), "miners": by_miner.len(), "hours": hours }),
            ));
        }
    }
    said
}

/// A day as "2026-11-19", from seconds since 1970.
fn chrono_date(seconds: u64) -> String {
    // Civil-from-days, Howard Hinnant's algorithm.
    let z = (seconds / 86_400) as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}-{m:02}-{d:02}")
}

impl Store {
    pub(crate) fn create_community_tables(&self) -> Result<()> {
        self.db.execute_batch(
            "CREATE TABLE IF NOT EXISTS posts (
                 id INTEGER PRIMARY KEY,
                 -- None for posts the network writes.
                 author INTEGER REFERENCES accounts(id) ON DELETE CASCADE,
                 -- post, or what the network says: record, welcome, milestone, streak, top_day.
                 kind TEXT NOT NULL,
                 body TEXT NOT NULL DEFAULT '',
                 data TEXT NOT NULL DEFAULT '{}',
                 -- Whom a network post is about.
                 subject INTEGER REFERENCES accounts(id) ON DELETE CASCADE,
                 parent INTEGER REFERENCES posts(id) ON DELETE CASCADE,
                 created INTEGER NOT NULL,
                 removed INTEGER NOT NULL DEFAULT 0
             );
             CREATE INDEX IF NOT EXISTS posts_feed ON posts(parent, id);
             -- Photos on posts: uploaded first (post NULL), then put on a
             -- post by its author.
             CREATE TABLE IF NOT EXISTS post_pictures (
                 name TEXT PRIMARY KEY,
                 account INTEGER NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
                 post INTEGER REFERENCES posts(id) ON DELETE CASCADE,
                 position INTEGER NOT NULL DEFAULT 0,
                 width INTEGER NOT NULL,
                 height INTEGER NOT NULL,
                 created INTEGER NOT NULL
             );
             CREATE INDEX IF NOT EXISTS post_pictures_by_post ON post_pictures(post, position);
             -- Link previews, once per address: what the page says about
             -- itself, checked (ok = 0 while fetching, or if it can't show).
             CREATE TABLE IF NOT EXISTS link_previews (
                 url TEXT PRIMARY KEY,
                 site TEXT NOT NULL DEFAULT '',
                 title TEXT NOT NULL DEFAULT '',
                 description TEXT NOT NULL DEFAULT '',
                 image TEXT,
                 width INTEGER,
                 height INTEGER,
                 fetched INTEGER NOT NULL,
                 ok INTEGER NOT NULL DEFAULT 0
             );
             -- Which link a post previews.
             CREATE TABLE IF NOT EXISTS post_links (
                 post INTEGER PRIMARY KEY REFERENCES posts(id) ON DELETE CASCADE,
                 url TEXT NOT NULL
             );
             -- A GIF on a post, as KLIPY gave it: loaded straight from
             -- KLIPY, never copied here.
             CREATE TABLE IF NOT EXISTS post_gifs (
                 post INTEGER PRIMARY KEY REFERENCES posts(id) ON DELETE CASCADE,
                 gif TEXT NOT NULL,
                 url TEXT NOT NULL,
                 preview TEXT NOT NULL,
                 width INTEGER NOT NULL,
                 height INTEGER NOT NULL
             );
             -- Polls: a post's choices, when it closes, and one final vote
             -- per account.
             CREATE TABLE IF NOT EXISTS polls (
                 post INTEGER PRIMARY KEY REFERENCES posts(id) ON DELETE CASCADE,
                 options TEXT NOT NULL,
                 ends INTEGER NOT NULL
             );
             CREATE TABLE IF NOT EXISTS poll_votes (
                 post INTEGER NOT NULL REFERENCES posts(id) ON DELETE CASCADE,
                 account INTEGER NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
                 option INTEGER NOT NULL,
                 at INTEGER NOT NULL,
                 PRIMARY KEY (post, account)
             );
             -- Tips paid to a post's author: each payment, by its sender and
             -- order number, counted once. Totals show; who tipped doesn't.
             CREATE TABLE IF NOT EXISTS post_tips (
                 sender TEXT NOT NULL,
                 seq INTEGER NOT NULL,
                 post INTEGER NOT NULL REFERENCES posts(id) ON DELETE CASCADE,
                 tipper INTEGER REFERENCES accounts(id) ON DELETE SET NULL,
                 orn INTEGER NOT NULL,
                 at INTEGER NOT NULL,
                 PRIMARY KEY (sender, seq)
             );
             CREATE INDEX IF NOT EXISTS post_tips_by_post ON post_tips(post);
             -- Validators the network has had, so a new one is posted once.
             CREATE TABLE IF NOT EXISTS seen_validators (
                 account TEXT PRIMARY KEY,
                 at INTEGER NOT NULL
             );
             CREATE INDEX IF NOT EXISTS posts_by_author ON posts(author, created);
             CREATE TABLE IF NOT EXISTS reactions (
                 post INTEGER NOT NULL REFERENCES posts(id) ON DELETE CASCADE,
                 account INTEGER NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
                 emoji TEXT NOT NULL,
                 PRIMARY KEY (post, account, emoji)
             );
             -- The last checkpoint the network has posted about.
             CREATE TABLE IF NOT EXISTS community_state (
                 key TEXT PRIMARY KEY,
                 value INTEGER NOT NULL
             );",
        )?;
        Ok(())
    }

    /// Verified wallets and the accounts they belong to.
    pub(crate) fn wallet_owners(&self) -> Result<HashMap<String, i64>> {
        let mut query = self.db.prepare(
            "SELECT address, account FROM verified_wallets WHERE proven IS NOT NULL
               AND account NOT IN (SELECT account FROM suspended)",
        )?;
        let rows = query.query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)))?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    fn names(&self) -> Result<HashMap<i64, (String, Option<String>)>> {
        let mut query = self.db.prepare("SELECT id, name, picture FROM accounts")?;
        let rows = query.query_map([], |row| Ok((row.get::<_, i64>(0)?, (row.get(1)?, row.get(2)?))))?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    fn may_post(&self, account: i64) -> Result<()> {
        if self.verified_wallets(account)?.is_empty() {
            bail!("verify one of your wallets first: only accounts with a verified wallet can post");
        }
        let lately: i64 = self.db.query_row(
            "SELECT COUNT(*) FROM posts WHERE author = ?1 AND created > ?2",
            params![account, now().saturating_sub(3600) as i64],
            |row| row.get(0),
        )?;
        if lately >= POSTS_AN_HOUR {
            bail!("that's {POSTS_AN_HOUR} posts in an hour; take a breather and post again soon");
        }
        Ok(())
    }

    /// A post, or a reply to `parent`. Screened before this is called.
    pub fn add_post(&self, author: i64, body: &str, parent: Option<i64>) -> Result<i64> {
        let body = post_text(body)?;
        self.may_post(author)?;
        if let Some(parent) = parent {
            let open: bool = self
                .db
                .query_row("SELECT removed = 0 AND parent IS NULL FROM posts WHERE id = ?1", [parent], |row| row.get(0))
                .optional()?
                .unwrap_or(false);
            if !open {
                bail!("that post is gone, or is itself a reply");
            }
        }
        self.db.execute(
            "INSERT INTO posts (author, kind, body, parent, created) VALUES (?1, 'post', ?2, ?3, ?4)",
            params![author, body, parent, now() as i64],
        )?;
        let id = self.db.last_insert_rowid();
        if let Some(parent) = parent {
            self.notice_reply(id, author, parent)?;
        }
        Ok(id)
    }

    /// Something the network says, dated `at`: the checkpoint's time.
    pub fn add_happening(&self, kind: &str, subject: Option<i64>, data: &Value, at: u64) -> Result<()> {
        self.db.execute(
            "INSERT INTO posts (kind, data, subject, created) VALUES (?1, ?2, ?3, ?4)",
            params![kind, data.to_string(), subject, at as i64],
        )?;
        self.notice_happening(self.db.last_insert_rowid())
    }

    pub fn remove_own_post(&self, author: i64, post: i64) -> Result<bool> {
        Ok(self.db.execute("UPDATE posts SET removed = 1 WHERE id = ?1 AND author = ?2", params![post, author])? == 1)
    }

    /// Takes a post down, from the dashboard.
    pub fn remove_post(&self, post: i64) -> Result<bool> {
        Ok(self.db.execute("UPDATE posts SET removed = 1 WHERE id = ?1", [post])? == 1)
    }

    pub fn post_removed(&self, post: i64) -> Result<bool> {
        Ok(self.db.query_row("SELECT removed = 1 FROM posts WHERE id = ?1", [post], |row| row.get(0)).optional()?.unwrap_or(true))
    }

    /// Adds `emoji` from `account`, or takes it back. Gives whether it is on.
    pub fn react(&self, account: i64, post: i64, emoji: &str) -> Result<bool> {
        if !REACTIONS.contains(&emoji) {
            bail!("that reaction isn't one of {}", REACTIONS.join(" "));
        }
        let there: bool = self
            .db
            .query_row("SELECT removed = 0 FROM posts WHERE id = ?1", [post], |row| row.get(0))
            .optional()?
            .unwrap_or(false);
        if !there {
            bail!("that post is gone");
        }
        let taken = self.db.execute(
            "DELETE FROM reactions WHERE post = ?1 AND account = ?2 AND emoji = ?3",
            params![post, account, emoji],
        )?;
        if taken == 0 {
            self.db.execute("INSERT INTO reactions (post, account, emoji) VALUES (?1, ?2, ?3)", params![post, account, emoji])?;
        }
        self.notice_reaction(post, account, emoji, taken == 0, Some(now()))?;
        Ok(taken == 0)
    }

    /// Feed posts newest first, before post `before`, or a post's replies
    /// oldest first. As seen by `viewer`. `only` narrows the feed to
    /// "people" or the "network".
    /// The feed, newest first, from `before` on; `replies_to` a post's
    /// replies instead, oldest first; `only` people or the network; `about`
    /// an account: its own posts, and the network's posts that name it.
    pub fn feed(
        &self,
        viewer: Option<i64>,
        before: Option<i64>,
        replies_to: Option<i64>,
        only: Option<&str>,
        about: Option<i64>,
        kinds: &str,
    ) -> Result<Vec<Value>> {
        let who = match only {
            Some("people") => " AND p.kind = 'post'",
            Some("network") => " AND p.kind <> 'post'",
            _ => "",
        };
        // ?3 is the account, or -1 for everyone.
        let whom = " AND (?3 < 0 OR p.author = ?3 OR p.subject = ?3 OR json_extract(p.data, '$.miner.account') = ?3
                      OR EXISTS (SELECT 1 FROM json_each(p.data, '$.miners') WHERE json_extract(value, '$.account') = ?3))";
        let (filter, order) = match replies_to {
            Some(_) => (format!("p.parent = ?1{whom}"), "p.created ASC, p.id ASC"),
            None => (
                format!(
                    "p.parent IS NULL{who}{whom} AND (p.created, p.id) < (SELECT COALESCE((SELECT created FROM posts WHERE id = ?1), 9223372036854775807), ?1)"
                ),
                "p.created DESC, p.id DESC",
            ),
        };
        let key = replies_to.unwrap_or(before.unwrap_or(i64::MAX));
        self.posts_where(viewer, &filter, order, key, about, Some(kinds))
    }

    /// One post as `viewer` sees it, if it is still up and not a reply.
    pub fn one_post(&self, viewer: Option<i64>, id: i64) -> Result<Option<Value>> {
        Ok(self.posts_where(viewer, "p.id = ?1 AND p.parent IS NULL AND ?3 < 0", "p.id", id, None, None)?.into_iter().next())
    }

    /// Posts matching `filter`, where ?1 is `key`, ?2 the viewer and ?3
    /// `about`. With `kinds`, the newer kinds of network post only where
    /// named there (comma-separated): what the asking app can show.
    fn posts_where(
        &self,
        viewer: Option<i64>,
        filter: &str,
        order: &str,
        key: i64,
        about: Option<i64>,
        kinds: Option<&str>,
    ) -> Result<Vec<Value>> {
        let mut query = self.db.prepare(&format!(
            "SELECT p.id, p.kind, p.body, p.data, p.created, p.parent,
                    a.name, a.picture, s.name, s.picture,
                    (SELECT COUNT(*) FROM posts r WHERE r.parent = p.id AND r.removed = 0
                       AND (r.author IS NULL OR r.author NOT IN (SELECT account FROM suspended))
                       AND (r.author IS NULL OR r.author NOT IN (SELECT blocker FROM blocks WHERE blocked = ?2 UNION SELECT blocked FROM blocks WHERE blocker = ?2))),
                    p.author = ?2, a.banner,
                    -- Where a tip goes: the author's Market wallet if it's
                    -- verified, otherwise their first verified wallet.
                    COALESCE(
                        (SELECT a.payout WHERE a.payout IS NOT NULL AND EXISTS (SELECT 1 FROM verified_wallets v
                            WHERE v.account = p.author AND v.address = a.payout AND v.proven IS NOT NULL)),
                        (SELECT v.address FROM verified_wallets v WHERE v.account = p.author AND v.proven IS NOT NULL
                            ORDER BY v.proven, v.address LIMIT 1)),
                    (SELECT COUNT(*) FROM post_tips t WHERE t.post = p.id),
                    (SELECT COALESCE(SUM(t.orn), 0) FROM post_tips t WHERE t.post = p.id)
             FROM posts p LEFT JOIN accounts a ON a.id = p.author LEFT JOIN accounts s ON s.id = p.subject
             WHERE {filter} AND p.removed = 0
               AND (p.author IS NULL OR p.author NOT IN (SELECT account FROM suspended))
               AND (p.subject IS NULL OR p.subject NOT IN (SELECT account FROM suspended))
               -- A block either way: each is gone for the other, as on other social apps.
               AND (p.author IS NULL OR p.author NOT IN (SELECT blocker FROM blocks WHERE blocked = ?2 UNION SELECT blocked FROM blocks WHERE blocker = ?2))
               AND (p.subject IS NULL OR p.subject NOT IN (SELECT blocker FROM blocks WHERE blocked = ?2 UNION SELECT blocked FROM blocks WHERE blocker = ?2))
               AND (?4 IS NULL OR p.kind IN ({KNOWN_KINDS}) OR instr(',' || ?4 || ',', ',' || p.kind || ',') > 0)
             ORDER BY {order} LIMIT {PAGE}"
        ))?;
        let rows = query.query_map(params![key, viewer.unwrap_or(-1), about.unwrap_or(-1), kinds], |row| {
            let person = |name: Option<String>, picture: Option<String>| name.map(|name| json!({ "name": name, "picture": picture }));
            Ok(json!({
                "id": row.get::<_, i64>(0)?,
                "kind": row.get::<_, String>(1)?,
                "body": row.get::<_, String>(2)?,
                "data": serde_json::from_str::<Value>(&row.get::<_, String>(3)?).unwrap_or(json!({})),
                "created": row.get::<_, i64>(4)?,
                "parent": row.get::<_, Option<i64>>(5)?,
                "author": person(row.get(6)?, row.get(7)?).map(|mut author| {
                    author["banner"] = json!(row.get::<_, Option<String>>(12).ok().flatten());
                    author["tip_to"] = json!(row.get::<_, Option<String>>(13).ok().flatten());
                    author
                }),
                "tips": { "count": row.get::<_, i64>(14)?, "orn": row.get::<_, i64>(15)? },
                "subject": person(row.get(8)?, row.get(9)?),
                "replies": row.get::<_, i64>(10)?,
                "mine": row.get::<_, Option<bool>>(11)?.unwrap_or(false),
            }))
        })?;
        let mut posts: Vec<Value> = rows.collect::<rusqlite::Result<_>>()?;
        self.name_faces(&mut posts)?;
        for post in &mut posts {
            let id = post["id"].as_i64().unwrap_or(0);
            let pictures = self.post_pictures(id)?;
            if !pictures.is_empty() {
                post["pictures"] = json!(pictures);
            }
            if let Some(gif) = self.post_gif(id)? {
                post["gif"] = gif;
            }
            if let Some(link) = self.post_link(id)? {
                post["link"] = link;
            }
            let mine = post["mine"].as_bool().unwrap_or(false);
            if let Some(poll) = self.poll(id, viewer, mine)? {
                post["poll"] = poll;
            }
        }
        for post in &mut posts {
            let id = post["id"].as_i64().unwrap_or(0);
            let mut counts = self.db.prepare("SELECT emoji, COUNT(*), MAX(account = ?2) FROM reactions WHERE post = ?1 GROUP BY emoji")?;
            let reactions: Vec<(String, i64, bool)> = counts
                .query_map(params![id, viewer.unwrap_or(-1)], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))?
                .collect::<rusqlite::Result<_>>()?;
            post["reactions"] = json!(reactions
                .iter()
                .map(|(emoji, count, me)| json!({ "emoji": emoji, "count": count, "me": me }))
                .collect::<Vec<_>>());
        }
        Ok(posts)
    }

    /// Fills in the name and picture of every account a post's data
    /// mentions (`miners[].account`, `miner.account`), so names follow
    /// renames. Suspended accounts lose their face.
    fn name_faces(&self, posts: &mut [Value]) -> Result<()> {
        // A face shown by its address whose wallet someone has verified
        // since becomes theirs.
        let owners = self.wallet_owners()?;
        let claim = |v: &mut Value| {
            if v.get("account").is_none()
                && let Some(id) = v.get("address").and_then(Value::as_str).and_then(|a| owners.get(a))
            {
                v["account"] = json!(id);
            }
        };
        for post in posts.iter_mut() {
            if let Some(list) = post["data"]["miners"].as_array_mut() {
                list.iter_mut().for_each(claim);
            }
            if post["data"]["miner"].is_object() {
                claim(&mut post["data"]["miner"]);
            }
        }
        let mut ids: Vec<i64> = Vec::new();
        let mut visit = |v: &Value| {
            if let Some(id) = v.get("account").and_then(Value::as_i64) {
                ids.push(id);
            }
        };
        for post in posts.iter() {
            if let Some(list) = post["data"]["miners"].as_array() {
                list.iter().for_each(&mut visit);
            }
            visit(&post["data"]["miner"]);
        }
        if ids.is_empty() {
            return Ok(());
        }
        let mut names: HashMap<i64, (String, Option<String>)> = HashMap::new();
        for id in ids {
            if let Some(found) = self
                .db
                .query_row(
                    "SELECT name, picture FROM accounts WHERE id = ?1 AND id NOT IN (SELECT account FROM suspended)",
                    [id],
                    |row| Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?)),
                )
                .optional()?
            {
                names.insert(id, found);
            }
        }
        let fill = |v: &mut Value| {
            if let Some(id) = v.get("account").and_then(Value::as_i64) {
                if let Some((name, picture)) = names.get(&id) {
                    v["name"] = json!(name);
                    v["picture"] = json!(picture);
                }
            }
        };
        for post in posts.iter_mut() {
            if let Some(list) = post["data"]["miners"].as_array_mut() {
                list.iter_mut().for_each(fill);
            }
            if post["data"]["miner"].is_object() {
                fill(&mut post["data"]["miner"]);
            }
        }
        Ok(())
    }

    /// Notes a photo uploaded for a post that isn't written yet.
    pub fn add_post_picture(&self, account: i64, name: &str, width: u32, height: u32) -> Result<()> {
        self.db.execute(
            "INSERT INTO post_pictures (name, account, width, height, created) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![name, account, width, height, now() as i64],
        )?;
        Ok(())
    }

    /// A post's photos, in order: {name, width, height}.
    fn post_pictures(&self, post: i64) -> Result<Vec<Value>> {
        let mut statement = self.db.prepare("SELECT name, width, height FROM post_pictures WHERE post = ?1 ORDER BY position")?;
        let rows = statement.query_map([post], |row| {
            Ok(json!({ "name": row.get::<_, String>(0)?, "width": row.get::<_, i64>(1)?, "height": row.get::<_, i64>(2)? }))
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// A post's link preview, once it's fetched and passed its checks:
    /// {url, site, title, description, image: {name, width, height} | null}.
    fn post_link(&self, post: i64) -> Result<Option<Value>> {
        Ok(self
            .db
            .query_row(
                "SELECT l.url, l.site, l.title, l.description, l.image, l.width, l.height
                 FROM post_links p JOIN link_previews l ON l.url = p.url WHERE p.post = ?1 AND l.ok = 1",
                [post],
                |row| {
                    let image: Option<String> = row.get(4)?;
                    Ok(json!({
                        "url": row.get::<_, String>(0)?,
                        "site": row.get::<_, String>(1)?,
                        "title": row.get::<_, String>(2)?,
                        "description": row.get::<_, String>(3)?,
                        "image": image.map(|name| json!({ "name": name, "width": row.get::<_, Option<i64>>(5).ok().flatten(), "height": row.get::<_, Option<i64>>(6).ok().flatten() })),
                    }))
                },
            )
            .optional()?)
    }

    /// Notes the link a new post previews. Returns whether its preview
    /// still has to be fetched: not fetched before, or over a week ago.
    pub fn link_post(&self, post: i64, url: &str) -> Result<bool> {
        self.db.execute("INSERT OR REPLACE INTO post_links (post, url) VALUES (?1, ?2)", params![post, url])?;
        let week_ago = now().saturating_sub(7 * 24 * 3600) as i64;
        // Claimed here, so a link posted twice at once is fetched once.
        let claimed = self.db.execute(
            "INSERT INTO link_previews (url, fetched, ok) VALUES (?1, ?2, 0)
             ON CONFLICT (url) DO UPDATE SET fetched = ?2, ok = 0 WHERE link_previews.fetched < ?3",
            params![url, now() as i64, week_ago],
        )?;
        Ok(claimed == 1)
    }

    /// Keeps a fetched preview: shown if `ok`.
    pub fn save_preview(&self, url: &str, page: &crate::links::Page, image: Option<(&str, u32, u32)>, ok: bool) -> Result<()> {
        self.db.execute(
            "UPDATE link_previews SET site = ?2, title = ?3, description = ?4, image = ?5, width = ?6, height = ?7, ok = ?8 WHERE url = ?1",
            params![url, page.site, page.title, page.description, image.map(|i| i.0), image.map(|i| i.1), image.map(|i| i.2), ok],
        )?;
        Ok(())
    }

    /// A post's GIF: {id, url, preview, width, height}.
    fn post_gif(&self, post: i64) -> Result<Option<Value>> {
        Ok(self
            .db
            .query_row("SELECT gif, url, preview, width, height FROM post_gifs WHERE post = ?1", [post], |row| {
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

    /// A poll as `viewer` sees it: how many voted always; each choice's
    /// votes once they voted, it closed, or it's their own.
    fn poll(&self, post: i64, viewer: Option<i64>, mine: bool) -> Result<Option<Value>> {
        let found: Option<(String, i64)> = self
            .db
            .query_row("SELECT options, ends FROM polls WHERE post = ?1", [post], |row| Ok((row.get(0)?, row.get(1)?)))
            .optional()?;
        let Some((options, ends)) = found else { return Ok(None) };
        let options: Vec<String> = serde_json::from_str(&options).unwrap_or_default();
        let mut counts = vec![0i64; options.len()];
        let mut statement = self.db.prepare("SELECT option, COUNT(*) FROM poll_votes WHERE post = ?1 GROUP BY option")?;
        for row in statement.query_map([post], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)))? {
            let (option, count) = row?;
            if let Some(slot) = counts.get_mut(option as usize) {
                *slot = count;
            }
        }
        let voted: Option<i64> = match viewer {
            Some(viewer) => self
                .db
                .query_row("SELECT option FROM poll_votes WHERE post = ?1 AND account = ?2", params![post, viewer], |row| row.get(0))
                .optional()?,
            None => None,
        };
        let closed = now() as i64 >= ends;
        let shown = voted.is_some() || closed || mine;
        let options: Vec<Value> = options
            .iter()
            .zip(&counts)
            .map(|(text, votes)| json!({ "text": text, "votes": if shown { json!(votes) } else { Value::Null } }))
            .collect();
        Ok(Some(json!({ "options": options, "total": counts.iter().sum::<i64>(), "ends": ends, "closed": closed, "voted": voted })))
    }

    /// Puts photos the author uploaded on their new post, in order.
    fn attach_pictures(&self, author: i64, post: i64, names: &[String]) -> Result<()> {
        if names.len() > MAX_PICTURES {
            bail!("a post has at most {MAX_PICTURES} photos");
        }
        for (position, name) in names.iter().enumerate() {
            let taken = self.db.execute(
                "UPDATE post_pictures SET post = ?3, position = ?4 WHERE name = ?1 AND account = ?2 AND post IS NULL",
                params![name, author, post, position as i64],
            )?;
            if taken == 0 {
                bail!("a photo on this post wasn't uploaded by you, or is on another post already");
            }
        }
        Ok(())
    }

    /// A poll's choices, tidied, or why they can't be one.
    pub fn poll_options(options: &[String], hours: u64) -> Result<Vec<String>> {
        let options: Vec<String> = options.iter().map(|o| o.split_whitespace().collect::<Vec<_>>().join(" ")).collect();
        if !POLL_OPTIONS.contains(&options.len()) {
            bail!("a poll has 2 to 4 choices");
        }
        if options.iter().any(|o| o.is_empty() || o.chars().count() > MAX_OPTION) {
            bail!("each choice is 1 to {MAX_OPTION} characters");
        }
        if !POLL_HOURS.contains(&hours) {
            bail!("a poll runs for 1 day, 3 days or 7 days");
        }
        Ok(options)
    }

    /// A new post with photos or a poll. Checked and screened already.
    pub fn add_post_with(
        &self,
        author: i64,
        body: &str,
        pictures: &[String],
        poll: Option<(&[String], u64)>,
        gif: Option<&Gif>,
    ) -> Result<i64> {
        self.may_post(author)?;
        let body = body.trim().replace("\r\n", "\n");
        if body.chars().count() > MAX_POST {
            bail!("a post is at most {MAX_POST} characters");
        }
        if body.is_empty() && ((pictures.is_empty() && gif.is_none()) || poll.is_some()) {
            bail!("write something first");
        }
        if let Some(gif) = gif {
            gif.checked()?;
        }
        // Checked before anything is saved.
        let poll = match poll {
            Some((options, hours)) => Some((Store::poll_options(options, hours)?, hours)),
            None => None,
        };
        if pictures.len() > MAX_PICTURES {
            bail!("a post has at most {MAX_PICTURES} photos");
        }
        let at = now();
        self.db.execute("INSERT INTO posts (author, kind, body, created) VALUES (?1, 'post', ?2, ?3)", params![author, body, at as i64])?;
        let id = self.db.last_insert_rowid();
        let done = (|| -> Result<()> {
            self.attach_pictures(author, id, pictures)?;
            if let Some(gif) = gif {
                self.db.execute(
                    "INSERT INTO post_gifs (post, gif, url, preview, width, height) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                    params![id, gif.id_text(), gif.url, gif.preview, gif.width, gif.height],
                )?;
            }
            if let Some((options, hours)) = poll {
                self.db.execute(
                    "INSERT INTO polls (post, options, ends) VALUES (?1, ?2, ?3)",
                    params![id, serde_json::to_string(&options)?, (at + hours * 3600) as i64],
                )?;
            }
            Ok(())
        })();
        if let Err(error) = done {
            self.db.execute("DELETE FROM posts WHERE id = ?1", [id])?;
            self.db.execute("UPDATE post_pictures SET post = NULL WHERE post = ?1", [id])?;
            return Err(error);
        }
        Ok(id)
    }

    /// `account` votes for choice `option` on a poll: once, and finally.
    pub fn vote(&self, post: i64, account: i64, option: usize) -> Result<Value> {
        let found: Option<(String, i64)> = self
            .db
            .query_row(
                "SELECT l.options, l.ends FROM polls l JOIN posts p ON p.id = l.post WHERE l.post = ?1 AND p.removed = 0",
                [post],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        let Some((options, ends)) = found else { bail!("there is no such poll") };
        let count = serde_json::from_str::<Vec<String>>(&options).map_or(0, |o| o.len());
        if option >= count {
            bail!("there is no such choice");
        }
        if now() as i64 >= ends {
            bail!("this poll has closed");
        }
        let new = self.db.execute(
            "INSERT OR IGNORE INTO poll_votes (post, account, option, at) VALUES (?1, ?2, ?3, ?4)",
            params![post, account, option as i64, now() as i64],
        )? == 1;
        if !new {
            bail!("you voted on this poll already");
        }
        let mine: bool = self.db.query_row("SELECT author = ?2 FROM posts WHERE id = ?1", params![post, account], |row| row.get(0))?;
        Ok(self.poll(post, Some(account), mine)?.expect("just voted"))
    }

    /// Who a tip on `post` goes to, and the wallets it may be paid to: a
    /// person's post or reply that's still up.
    pub fn tip_target(&self, post: i64) -> Result<(i64, Vec<String>)> {
        let author: Option<i64> = self
            .db
            .query_row(
                "SELECT author FROM posts WHERE id = ?1 AND kind = 'post' AND removed = 0 AND author IS NOT NULL",
                [post],
                |row| row.get(0),
            )
            .optional()?;
        let Some(author) = author else { bail!("there is no such post") };
        let wallets = self.verified_wallets(author)?.into_iter().map(|w| w.address).collect();
        Ok((author, wallets))
    }

    /// A post's tips: how many, and how much in all (ORN's smallest units).
    pub fn tips_of(&self, post: i64) -> Result<(i64, i64)> {
        Ok(self.db.query_row(
            "SELECT COUNT(*), COALESCE(SUM(orn), 0) FROM post_tips WHERE post = ?1",
            [post],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?)
    }

    /// Whether a payment already counts as a tip.
    pub fn tip_known(&self, sender: &str, seq: u64) -> Result<bool> {
        Ok(self
            .db
            .query_row("SELECT 1 FROM post_tips WHERE sender = ?1 AND seq = ?2", params![sender, seq as i64], |_| Ok(()))
            .optional()?
            .is_some())
    }

    /// Counts a verified payment as a tip on `post`, once, and tells its
    /// author who sent it. Returns whether it was new.
    pub fn add_tip(&self, post: i64, tipper: i64, sender: &str, seq: u64, orn: u64) -> Result<bool> {
        let at = now();
        let new = self.db.execute(
            "INSERT OR IGNORE INTO post_tips (sender, seq, post, tipper, orn, at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![sender, seq as i64, post, tipper, orn as i64, at as i64],
        )? == 1;
        if new && let (author, _) = self.tip_target(post)? {
            self.notice_tip(author, post, tipper, orn, at)?;
        }
        Ok(new)
    }

    fn state(&self, key: &str) -> Result<Option<u64>> {
        Ok(self
            .db
            .query_row("SELECT value FROM community_state WHERE key = ?1", [key], |row| row.get::<_, i64>(0))
            .optional()?
            .map(|v| v as u64))
    }

    fn set_state(&self, key: &str, value: u64) -> Result<()> {
        self.db.execute(
            "INSERT INTO community_state (key, value) VALUES (?1, ?2) ON CONFLICT (key) DO UPDATE SET value = ?2",
            params![key, value as i64],
        )?;
        Ok(())
    }

    /// Posts the round number of ORN mined that `released` (whole ORN) has
    /// just passed, once: every 100,000 up to a million, then every million.
    /// The first time, it notes where things stand and posts nothing, so
    /// only marks passed from then on are posted. `cap` is all the ORN
    /// there will ever be.
    pub fn tell_supply(&self, released: u64, cap: u64, at: u64) -> Result<bool> {
        let passed = if released < 1_000_000 { released / 100_000 * 100_000 } else { released / 1_000_000 * 1_000_000 };
        let Some(told) = self.state("supply_told")? else {
            self.set_state("supply_told", passed)?;
            return Ok(false);
        };
        if passed < next_supply_mark(told) {
            return Ok(false);
        }
        let share = if cap > 0 { passed as f64 / cap as f64 * 100.0 } else { 0.0 };
        self.add_happening("supply", None, &json!({ "orn": passed, "cap": cap, "share": share }), at)?;
        self.set_state("supply_told", passed)?;
        Ok(true)
    }

    /// Posts each validator the network hasn't had before: `seats` is every
    /// seat now, (account, seat). The first time, the ones there are noted
    /// without a post.
    pub fn tell_validators(&self, seats: &[(String, usize)], at: u64) -> Result<usize> {
        let first: bool = self.db.query_row("SELECT COUNT(*) = 0 FROM seen_validators", [], |row| row.get(0))?;
        let mut told = 0;
        for (account, seat) in seats {
            let new = self.db.execute("INSERT OR IGNORE INTO seen_validators (account, at) VALUES (?1, ?2)", params![account, at as i64])? == 1;
            if new && !first {
                self.add_happening("validator", None, &json!({ "account": account, "seat": seat + 1, "seats": seats.len() }), at)?;
                told += 1;
            }
        }
        Ok(told)
    }

    fn last_told(&self) -> Result<Option<u64>> {
        Ok(self
            .db
            .query_row("SELECT value FROM community_state WHERE key = 'told'", [], |row| row.get::<_, i64>(0))
            .optional()?
            .map(|v| v as u64))
    }

    fn set_told(&self, epoch: u64) -> Result<()> {
        self.db.execute(
            "INSERT INTO community_state (key, value) VALUES ('told', ?1) ON CONFLICT (key) DO UPDATE SET value = ?1",
            [epoch as i64],
        )?;
        Ok(())
    }

    /// Posts what the network has to say about checkpoints since the last
    /// time. The first time, that is every checkpoint the records cover,
    /// each post dated when its checkpoint was made, so the feed has the
    /// network's whole story from the start.
    pub fn tell(&self, mined: &Mined, owners: &HashMap<String, i64>) -> Result<()> {
        let told = self.last_told()?.unwrap_or(mined.since);
        for epoch in (told + 1)..=mined.upto {
            let at = mined.times.get(&epoch).copied().unwrap_or_else(now);
            for (kind, subject, data) in happenings(mined, owners, epoch) {
                self.add_happening(&kind, subject, &data, at)?;
            }
            self.set_told(epoch)?;
        }
        Ok(())
    }

    /// Fills in what the network would have said had today's verified
    /// wallets been verified all along: the milestones and streaks of
    /// someone who verified their wallet after they started mining, and
    /// the top miner of each past day that has no post yet. Each is dated
    /// at its own checkpoint, so it sits in the feed where it happened.
    /// A day that already has its top miner keeps it. Nothing is said
    /// twice, a post taken down included, so this can run again and again.
    pub fn tell_late(&self, mined: &Mined, owners: &HashMap<String, i64>) -> Result<usize> {
        let mut added = 0;
        for epoch in (mined.since + 1)..=mined.upto {
            for (kind, subject, data) in happenings(mined, owners, epoch) {
                let posted: bool = match (kind.as_str(), subject) {
                    ("top_day", _) => self.db.query_row(
                        "SELECT COUNT(*) > 0 FROM posts WHERE kind = 'top_day' AND json_extract(data, '$.day') = ?1",
                        [data["day"].as_str().unwrap_or_default()],
                        |row| row.get(0),
                    )?,
                    ("milestone" | "streak", Some(account)) => self.db.query_row(
                        "SELECT COUNT(*) > 0 FROM posts WHERE kind = ?1 AND subject = ?2 AND json_extract(data, '$.epoch') = ?3",
                        params![kind, account, epoch as i64],
                        |row| row.get(0),
                    )?,
                    // Records and welcome cards are about everyone, and were said at the time.
                    _ => true,
                };
                if !posted {
                    let at = mined.times.get(&epoch).copied().unwrap_or_else(now);
                    self.add_happening(&kind, subject, &data, at)?;
                    added += 1;
                }
            }
        }
        Ok(added)
    }
}

fn post_text(text: &str) -> Result<String> {
    let text = text.trim().replace("\r\n", "\n");
    if text.is_empty() {
        bail!("write something first");
    }
    if text.chars().count() > MAX_POST {
        bail!("a post is at most {MAX_POST} characters");
    }
    if text.chars().any(|c| c.is_control() && c != '\n') {
        bail!("a post can't hold control characters");
    }
    Ok(text)
}

impl Community {
    fn refresh(&self, app: &App) {
        let Some(mined) = app.explorer().mined(app.payments().as_ref()) else { return };
        let store = app.store();
        let (Ok(owners), Ok(names)) = (store.wallet_owners(), store.names()) else { return };
        if let Err(error) = store.tell(&mined, &owners) {
            eprintln!("community: could not post the network's news: {error:#}");
        }
        // On start, and whenever someone verifies (or loses) a wallet.
        let mut now_owners: Vec<(String, i64)> = owners.iter().map(|(a, id)| (a.clone(), *id)).collect();
        now_owners.sort();
        let mut told = self.owners_told.lock().unwrap();
        if told.as_ref() != Some(&now_owners) {
            match store.tell_late(&mined, &owners) {
                Ok(added) => {
                    if added > 0 {
                        eprintln!("community: filled in {added} posts from before wallets were verified");
                    }
                    *told = Some(now_owners);
                }
                Err(error) => eprintln!("community: could not fill in past posts: {error:#}"),
            }
        }
        drop(told);
        drop(store);
        *self.board.lock().unwrap() = Some(board(&mined, &owners, &names));
        // The network's own news: ORN mined so far, and new validators.
        if let Ok(overview) = app.explorer().overview(app.payments().as_ref()) {
            let store = app.store();
            let at = crate::store::now();
            if let Some(supply) = &overview.supply {
                let whole = |units: &str| units.parse::<u128>().map_or(0, |n| (n / 1_000_000_000) as u64);
                if let Err(error) = store.tell_supply(whole(&supply.released), whole(&supply.cap), at) {
                    eprintln!("community: could not post ORN mined: {error:#}");
                }
            }
            let seats: Vec<(String, usize)> =
                // Known by account, or by address for the launch validators, which have none.
                overview
                    .validators
                    .iter()
                    .map(|v| (v.account.clone().unwrap_or_else(|| v.address.clone()), v.seat))
                    .filter(|(id, _)| !id.is_empty())
                    .collect();
            if !seats.is_empty()
                && let Err(error) = store.tell_validators(&seats, at)
            {
                eprintln!("community: could not post new validators: {error:#}");
            }
        }
    }

    fn snapshot(&self) -> Option<Board> {
        self.board.lock().unwrap().clone()
    }

    /// Members whose names contain `text`, the closest first: name and
    /// picture, and whether they verified a wallet.
    pub(crate) fn find_members(&self, store: &Store, text: &str, viewer: Option<i64>) -> Result<Vec<Value>> {
        let query = text.trim().to_lowercase();
        if query.chars().count() < 2 {
            return Ok(Vec::new());
        }
        let escaped: String = query.chars().flat_map(|c| if matches!(c, '%' | '_' | '\\') { vec!['\\', c] } else { vec![c] }).collect();
        let mut statement = store.db.prepare(
            "SELECT a.id, a.name, a.picture,
                    EXISTS (SELECT 1 FROM verified_wallets v WHERE v.account = a.id AND v.proven IS NOT NULL)
             FROM accounts a
             WHERE lower(a.name) LIKE ?1 ESCAPE '\\' AND a.id NOT IN (SELECT account FROM suspended)
               AND a.id NOT IN (SELECT blocker FROM blocks WHERE blocked = ?4 UNION SELECT blocked FROM blocks WHERE blocker = ?4)
             ORDER BY lower(a.name) = ?2 DESC, lower(a.name) LIKE ?3 ESCAPE '\\' DESC, lower(a.name), a.id
             LIMIT 8",
        )?;
        let rows = statement.query_map(params![format!("%{escaped}%"), query, format!("{escaped}%"), viewer.unwrap_or(-1)], |row| {
            let id: i64 = row.get(0)?;
            Ok((id, row.get::<_, String>(1)?, row.get::<_, Option<String>>(2)?, row.get::<_, bool>(3)?))
        })?;
        let mut found = Vec::new();
        for row in rows {
            let (id, name, picture, verified) = row?;
            let rank = self.stats_of(id).map(|stats| stats.rank);
            found.push(json!({ "name": name, "picture": picture, "verified": verified, "rank": rank }));
        }
        Ok(found)
    }

    /// An account's rank, ORN mined and streak, when its verified wallets
    /// have been paid for mining.
    pub(crate) fn stats_of(&self, account: i64) -> Option<Stats> {
        self.board.lock().unwrap().as_ref().and_then(|b| b.accounts.get(&account).cloned())
    }
}

/// Keeps the leaderboards current and posts the network's news, each minute.
pub fn watch(app: Arc<App>) {
    std::thread::spawn(move || {
        loop {
            app.community().refresh(&app);
            std::thread::sleep(Duration::from_secs(60));
        }
    });
}

pub fn routes() -> Router<Arc<App>> {
    Router::new()
        .route("/api/market/community/feed", get(feed))
        .route("/api/market/community/board", get(board_answer))
        .route("/api/market/community/posts", post(new_post))
        .route("/api/market/community/posts/{id}", get(replies).delete(remove_post))
        .route("/api/market/community/posts/{id}/react", post(react))
        .route("/api/market/community/posts/{id}/tips", post(tip))
        .route("/api/market/community/posts/{id}/vote", post(vote))
        .route("/api/market/community/gifs/key", get(gif_key))
        .route(
            "/api/market/community/pictures",
            post(add_picture).layer(axum::extract::DefaultBodyLimit::max(crate::images::MAX_UPLOAD)),
        )
        .route("/api/market/community/members", get(members))
}

fn viewer(app: &App, headers: &HeaderMap) -> Option<i64> {
    app.signed_in(headers).ok()
}

#[derive(Deserialize)]
pub struct Before {
    before: Option<i64>,
    only: Option<String>,
    /// The newer kinds of network post this app can show, comma-separated
    /// ("swap,supply,validator"). Older apps send none and see none.
    #[serde(default)]
    kinds: String,
    /// An account's name: only its own posts and the network's posts about it.
    about: Option<String>,
}

#[derive(Deserialize)]
pub struct Find {
    q: String,
}

async fn members(State(app): State<Arc<App>>, headers: HeaderMap, Query(find): Query<Find>) -> Result<Response, Failure> {
    let me = viewer(&app, &headers);
    let found = app.community().find_members(&app.store(), &find.q, me)?;
    // Each viewer's answer leaves out who blocked them: kept by their browser only.
    Ok(([(axum::http::header::CACHE_CONTROL, "private, max-age=30")], Json(json!({ "members": found }))).into_response())
}

async fn feed(State(app): State<Arc<App>>, headers: HeaderMap, Query(page): Query<Before>) -> Result<Response, Failure> {
    let me = viewer(&app, &headers);
    let store = app.store();
    let about = match &page.about {
        Some(name) => {
            let id = store.account_by_name(name)?.ok_or(Failure::NotFound)?;
            // A block either way makes them nobody to each other.
            if let Some(me) = me
                && store.blocked_either(id, me)?
            {
                return Err(Failure::NotFound);
            }
            Some(id)
        }
        None => None,
    };
    let posts = store.feed(me, page.before, None, page.only.as_deref(), about, &page.kinds)?;
    let more = posts.len() as i64 == PAGE;
    Ok(Json(json!({ "posts": posts, "more": more, "reactions": REACTIONS, "max": MAX_POST })).into_response())
}

async fn replies(State(app): State<Arc<App>>, headers: HeaderMap, Path(id): Path<i64>) -> Result<Response, Failure> {
    let me = viewer(&app, &headers);
    let store = app.store();
    Ok(Json(json!({ "post": store.one_post(me, id)?, "replies": store.feed(me, None, Some(id), None, None, "")? })).into_response())
}

async fn board_answer(State(app): State<Arc<App>>, headers: HeaderMap) -> Result<Response, Failure> {
    let me = viewer(&app, &headers);
    let board = app.community().snapshot();
    let you = match me {
        Some(account) => {
            let store = app.store();
            let name = store.account(account)?.map(|a| (a.name, a.picture));
            let verified = !store.verified_wallets(account)?.is_empty();
            let stats = board.as_ref().and_then(|b| b.accounts.get(&account).cloned());
            name.map(|(name, picture)| json!({ "name": name, "picture": picture, "verified": verified, "stats": stats }))
        }
        None => None,
    };
    Ok(Json(json!({ "board": board, "you": you })).into_response())
}

#[derive(Deserialize)]
pub struct NewPost {
    #[serde(default)]
    body: String,
    parent: Option<i64>,
    /// Photos uploaded first, by name, in order: up to four.
    #[serde(default)]
    pictures: Vec<String>,
    /// A poll: its choices and how many hours it runs.
    poll: Option<NewPoll>,
    /// A GIF picked in KLIPY's search.
    gif: Option<Gif>,
}

/// A GIF the person picked in KLIPY's search, as KLIPY gave it to their
/// app: its id, the address to show it from, and a smaller one.
#[derive(Deserialize)]
pub struct Gif {
    id: Value,
    url: String,
    preview: String,
    width: u32,
    height: u32,
}

impl Gif {
    fn id_text(&self) -> String {
        match &self.id {
            Value::String(text) => text.clone(),
            other => other.to_string(),
        }
    }

    /// Only KLIPY's own addresses, as it gave them, and a sensible size.
    fn checked(&self) -> Result<()> {
        let klipy = |url: &str| {
            url.len() <= 500
                && url.strip_prefix("https://").and_then(|rest| rest.split_once('/')).is_some_and(|(host, _)| {
                    host == "static.klipy.com" || (host.starts_with("static") && host.ends_with(".klipy.com") && host[6..host.len() - 10].bytes().all(|b| b.is_ascii_digit()))
                })
        };
        let id = self.id_text();
        if id.is_empty() || id.len() > 40 || !klipy(&self.url) || !klipy(&self.preview) {
            bail!("that GIF isn't one from KLIPY");
        }
        if self.width == 0 || self.height == 0 || self.width > 4000 || self.height > 4000 {
            bail!("that GIF's size doesn't look right");
        }
        Ok(())
    }
}

#[derive(Deserialize)]
pub struct NewPoll {
    options: Vec<String>,
    hours: u64,
}

async fn new_post(State(app): State<Arc<App>>, headers: HeaderMap, Json(given): Json<NewPost>) -> Result<Response, Failure> {
    let author = app.signed_in(&headers)?;
    let media = !given.pictures.is_empty() || given.poll.is_some() || given.gif.is_some();
    let kinds = usize::from(!given.pictures.is_empty()) + usize::from(given.poll.is_some()) + usize::from(given.gif.is_some());
    if kinds > 1 {
        return Err(Failure::Refused("a post has photos, a GIF or a poll: one of them".into()));
    }
    if media && given.parent.is_some() {
        return Err(Failure::Refused("replies are words only for now".into()));
    }
    if !given.pictures.is_empty() && given.poll.is_some() {
        return Err(Failure::Refused("a post has photos or a poll, not both".into()));
    }
    if !media {
        let store = app.store();
        post_text(&given.body).map_err(refused)?;
        store.may_post(author).map_err(refused)?;
    } else {
        app.store().may_post(author).map_err(refused)?;
        if let Some(poll) = &given.poll {
            Store::poll_options(&poll.options, poll.hours).map_err(refused)?;
        }
    }
    // The words, and a poll's choices, are read like any post.
    let mut text = given.body.trim().to_string();
    if let Some(poll) = &given.poll {
        for option in &poll.options {
            text.push_str(&format!("\n- {option}"));
        }
    }
    if !text.is_empty() {
        screened(&app, "post", move |s| s.message(&text)).await?;
    }
    let id = if media {
        let poll = given.poll.as_ref().map(|p| (p.options.as_slice(), p.hours));
        app.store().add_post_with(author, &given.body, &given.pictures, poll, given.gif.as_ref()).map_err(refused)?
    } else {
        app.store().add_post(author, &given.body, given.parent).map_err(refused)?
    };
    // The first link's preview, fetched in the background: it shows on the
    // next load of the feed.
    if let Some(link) = crate::links::first_link(&given.body)
        && app.store().link_post(id, &link).unwrap_or(false)
    {
        let app = app.clone();
        tokio::task::spawn_blocking(move || preview(&app, &link));
    }
    Ok(Json(json!({ "id": id })).into_response())
}

/// Fetches a link's preview and keeps it, checked: the words as a post's
/// are, the picture as an uploaded photo is. A picture that can't be fetched
/// or doesn't pass is left out; words that don't pass hide the preview.
fn preview(app: &App, link: &str) {
    let page = match crate::links::fetch_page(link) {
        Ok(page) if !page.title.is_empty() || !page.description.is_empty() => page,
        _ => return,
    };
    let words_ok = crate::links::words_allowed(&app.screener(), &page).unwrap_or(false);
    let mut image: Option<(String, u32, u32)> = None;
    if words_ok
        && let Some(src) = &page.image
        && let Ok(bytes) = crate::links::fetch_picture(src, crate::images::MAX_UPLOAD)
        && let Ok((jpeg, width, height)) = crate::images::post_picture(&bytes)
        && matches!(app.screener().picture(&jpeg), Ok(crate::screen::Verdict::Allowed))
        && let Ok(name) = app.images().save(&jpeg)
    {
        image = Some((name, width, height));
    }
    let kept = image.as_ref().map(|(n, w, h)| (n.as_str(), *w, *h));
    if let Err(error) = app.store().save_preview(link, &page, kept, words_ok) {
        eprintln!("community: could not keep a link preview: {error:#}");
    }
}

/// A photo for a post about to be written, sent as the request's whole
/// body: checked like a profile picture, then kept until the post is.
async fn add_picture(State(app): State<Arc<App>>, headers: HeaderMap, body: axum::body::Bytes) -> Result<Response, Failure> {
    let author = app.signed_in(&headers)?;
    app.store().may_post(author).map_err(refused)?;
    let (jpeg, width, height) = tokio::task::spawn_blocking(move || crate::images::post_picture(&body))
        .await
        .map_err(|e| Failure::Broken(e.into()))?
        .map_err(refused)?;
    let checked = jpeg.clone();
    screened(&app, "picture", move |s| s.picture(&checked)).await?;
    let name = app.images().save(&jpeg)?;
    app.store().add_post_picture(author, &name, width, height)?;
    Ok(Json(json!({ "name": name, "width": width, "height": height })).into_response())
}

/// What a signed-in app needs to search KLIPY from the person's own
/// device: the key, and an id KLIPY keeps per person that says nothing
/// about them (the account's random handle). 404 when GIFs are off.
async fn gif_key(State(app): State<Arc<App>>, headers: HeaderMap) -> Result<Response, Failure> {
    let account = app.signed_in(&headers)?;
    if app.klipy_key().is_empty() {
        return Err(Failure::NotFound);
    }
    let handle: String = app.store().handle_of(account)?;
    Ok((
        [(axum::http::header::CACHE_CONTROL, "private, no-store")],
        Json(json!({ "key": app.klipy_key(), "customer_id": handle, "placeholder": "Search KLIPY" })),
    )
        .into_response())
}

#[derive(Deserialize)]
pub struct Vote {
    option: usize,
}

/// A vote on a poll: one per account, final. Answers the poll.
async fn vote(State(app): State<Arc<App>>, headers: HeaderMap, Path(id): Path<i64>, Json(given): Json<Vote>) -> Result<Response, Failure> {
    let account = app.signed_in(&headers)?;
    let poll = app.store().vote(id, account, given.option).map_err(refused)?;
    Ok(Json(poll).into_response())
}

async fn remove_post(State(app): State<Arc<App>>, headers: HeaderMap, Path(id): Path<i64>) -> Result<Response, Failure> {
    let author = app.signed_in(&headers)?;
    if !app.store().remove_own_post(author, id)? {
        return Err(Failure::NotFound);
    }
    Ok(Json(json!({ "removed": true })).into_response())
}

#[derive(Deserialize)]
pub struct Reaction {
    emoji: String,
}

#[derive(Deserialize)]
pub struct Tip {
    /// The sending wallet, 64 hex characters.
    from: String,
    /// The payment's order number in that wallet's account.
    seq: u64,
}

/// A tip someone paid a post's author from their wallet: counted once the
/// network shows the payment settled to one of the author's verified
/// wallets. Answers the post's totals.
async fn tip(State(app): State<Arc<App>>, headers: HeaderMap, Path(id): Path<i64>, Json(given): Json<Tip>) -> Result<Response, Failure> {
    let tipper = app.signed_in(&headers)?;
    let sender: orien_ledger::AccountId =
        given.from.trim().parse().map_err(|_| Failure::Refused("that is not an Orien address".into()))?;
    let sender_hex = hex::encode(sender.as_bytes());
    let (author, wallets) = app.store().tip_target(id).map_err(|_| Failure::NotFound)?;
    if author == tipper {
        return Err(Failure::Refused("you can't tip your own post".into()));
    }
    if wallets.is_empty() {
        return Err(Failure::Refused("this person has no verified wallet to tip".into()));
    }
    let answer = |count: i64, orn: i64| Json(json!({ "count": count, "orn": orn })).into_response();
    // Counted already: the totals as they are.
    if app.store().tip_known(&sender_hex, given.seq)? {
        let (count, orn) = app.store().tips_of(id)?;
        return Ok(answer(count, orn));
    }
    let key = format!("{sender_hex}:{}", given.seq);
    let payments = app.payments();
    let paid = tokio::task::spawn_blocking(move || -> anyhow::Result<Option<u64>> {
        let epoch = payments.epoch()?;
        for wallet in &wallets {
            let Ok(to) = wallet.parse::<orien_ledger::AccountId>() else { continue };
            let found = payments.credits(&to, epoch.saturating_sub(48))?.into_iter().find(|c| !c.mined && c.key == key);
            if let Some(credit) = found {
                return Ok(Some(credit.amount));
            }
        }
        Ok(None)
    })
    .await
    .map_err(|e| Failure::Broken(e.into()))?
    .map_err(|_| Failure::Unavailable("the Orien network is not answering right now; try again in a minute".into()))?;
    let Some(orn) = paid else {
        return Err(Failure::Refused("that payment to this post's author isn't settled on the network yet".into()));
    };
    let new = app.store().add_tip(id, tipper, &sender_hex, given.seq, orn)?;
    if new {
        let name = app.store().account(tipper)?.map(|a| a.name).unwrap_or_default();
        app.tell(crate::push::Event {
            account: author,
            address: None,
            body: format!("{name} tipped you {} ORN for your post", crate::store::orn(orn)),
            kind: "tip",
            open: json!({ "post": id }),
        });
    }
    let (count, orn) = app.store().tips_of(id)?;
    Ok(answer(count, orn))
}

async fn react(State(app): State<Arc<App>>, headers: HeaderMap, Path(id): Path<i64>, Json(given): Json<Reaction>) -> Result<Response, Failure> {
    let account = app.signed_in(&headers)?;
    let on = app.store().react(account, id, &given.emoji).map_err(refused)?;
    Ok(Json(json!({ "on": on })).into_response())
}
