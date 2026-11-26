//! The theme store: people share the themes they build in Orien Wallet, and
//! others browse, try, download and tip the maker. A theme is a name, a
//! dark or light base, one to three colors and a picture. The picture goes
//! through the same checker as profile pictures; each new theme is shown to
//! the founder in a private Discord channel, and can be reported and taken
//! down from the dashboard.

use std::sync::Arc;

use anyhow::{Result, bail};
use axum::extract::{Path, Query, State};
use axum::http::HeaderMap;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use base64::Engine;
use rusqlite::{OptionalExtension, params};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::api::{App, Failure, refused, screened};
use crate::images;
use crate::store::Store;

/// The longest theme name, in characters.
const MAX_NAME: usize = 24;
/// The largest picture the app sends, as JPEG bytes.
const MAX_PICTURE: usize = 1_600_000;
/// Themes one account may have up at once, and share in a day.
const LIVE_PER_ACCOUNT: i64 = 20;
const A_DAY: i64 = 5;
/// Themes on a page.
const PAGE: i64 = 24;
/// A shared picture's longest side, and its thumbnail's width, in pixels.
const FULL_SIDE: u32 = 2560;
const THUMB_WIDTH: u32 = 600;
/// What counts as lately for "popular": four weeks.
const LATELY: u64 = 28 * 86_400;

/// Orien's own themes in the store, published under "Orien": the app keeps
/// their look and pictures, and the store lists them for downloading. Their
/// ids in the app, and their names.
const OFFICIAL: [(&str, &str); 6] =
    [("midnight", "Midnight"), ("aurora", "Aurora"), ("amethyst", "Amethyst"), ("rose", "Rosé"), ("ruby", "Ruby"), ("ember", "Ember")];

fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

/// A shared theme's picture, kept whole up to FULL_SIDE, and its thumbnail.
fn pictures(bytes: &[u8]) -> Result<(Vec<u8>, Vec<u8>)> {
    let image = images::decode(bytes)?;
    let full = if image.width() > FULL_SIDE || image.height() > FULL_SIDE {
        image.resize(FULL_SIDE, FULL_SIDE, image::imageops::FilterType::Lanczos3)
    } else {
        image
    };
    let thumb = full.resize(THUMB_WIDTH, u32::MAX, image::imageops::FilterType::Lanczos3);
    Ok((images::jpeg(&full), images::jpeg(&thumb)))
}

/// A theme as the app sends it, checked.
struct Draft {
    name: String,
    base: &'static str,
    colors: Vec<String>,
}

fn checked(name: &str, base: &str, colors: &[String]) -> Result<Draft, String> {
    let name: String = name.trim().chars().filter(|c| !c.is_control()).collect();
    if name.is_empty() {
        return Err("give the theme a name".into());
    }
    if name.chars().count() > MAX_NAME {
        return Err(format!("a theme's name is at most {MAX_NAME} characters"));
    }
    let base = match base.trim() {
        "dark" => "dark",
        "light" => "light",
        _ => return Err("a theme is dark or light".into()),
    };
    if colors.is_empty() || colors.len() > 3 {
        return Err("a theme has one to three colors".into());
    }
    let mut kept = Vec::new();
    for color in colors {
        let color = color.trim().to_lowercase();
        if color.len() != 7 || !color.starts_with('#') || !color[1..].bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err("colors are written like #a1b2c3".into());
        }
        kept.push(color);
    }
    Ok(Draft { name, base, colors: kept })
}

/// The JPEG inside a data URL, or the bare base64 of one.
fn picture_bytes(data: &str) -> Result<Vec<u8>, String> {
    let data = data.trim();
    let encoded = match data.split_once(',') {
        Some((head, rest)) if head.starts_with("data:") => rest,
        _ => data,
    };
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(encoded.trim())
        .map_err(|_| "the picture didn't come through; try choosing it again".to_string())?;
    if bytes.len() > MAX_PICTURE {
        return Err("that picture is too big; pick a smaller one".into());
    }
    Ok(bytes)
}

impl Store {
    pub(crate) fn create_theme_tables(&self) -> Result<()> {
        self.db.execute_batch(
            "CREATE TABLE IF NOT EXISTS themes (
                 id INTEGER PRIMARY KEY,
                 -- NULL for Orien's own themes, which have `builtin`.
                 author INTEGER REFERENCES accounts(id) ON DELETE CASCADE,
                 name TEXT NOT NULL,
                 base TEXT NOT NULL,
                 colors TEXT NOT NULL,
                 picture TEXT,
                 thumb TEXT,
                 created INTEGER NOT NULL,
                 removed INTEGER NOT NULL DEFAULT 0,
                 -- Orien's own theme: its id in the app.
                 builtin TEXT UNIQUE
             );
             CREATE INDEX IF NOT EXISTS themes_by_author ON themes(author, created);
             -- Who downloaded each theme: an account, or a hash of an
             -- internet address for people signed out. Counted once each.
             CREATE TABLE IF NOT EXISTS theme_downloads (
                 theme INTEGER NOT NULL REFERENCES themes(id) ON DELETE CASCADE,
                 who TEXT NOT NULL,
                 at INTEGER NOT NULL,
                 PRIMARY KEY (theme, who)
             );
             CREATE TABLE IF NOT EXISTS theme_tips (
                 sender TEXT NOT NULL,
                 seq INTEGER NOT NULL,
                 theme INTEGER NOT NULL REFERENCES themes(id) ON DELETE CASCADE,
                 tipper INTEGER REFERENCES accounts(id) ON DELETE SET NULL,
                 orn INTEGER NOT NULL,
                 at INTEGER NOT NULL,
                 PRIMARY KEY (sender, seq)
             );
             CREATE INDEX IF NOT EXISTS theme_tips_by_theme ON theme_tips(theme);",
        )?;
        self.allow_official_themes()?;
        let at = now() as i64;
        for (builtin, name) in OFFICIAL {
            self.db.execute(
                "INSERT OR IGNORE INTO themes (author, name, base, colors, picture, thumb, created, builtin)
                 VALUES (NULL, ?1, 'dark', '[]', NULL, NULL, ?2, ?3)",
                params![name, at, builtin],
            )?;
        }
        Ok(())
    }

    /// Stores from before Orien's own themes: a theme had to have an author
    /// and pictures. SQLite cannot drop NOT NULL, so the table is rebuilt
    /// once, keeping every id, with foreign keys off so downloads and tips stay.
    fn allow_official_themes(&self) -> Result<()> {
        let has_builtin: bool = self.db.query_row(
            "SELECT COUNT(*) > 0 FROM pragma_table_info('themes') WHERE name = 'builtin'",
            [],
            |row| row.get(0),
        )?;
        if has_builtin {
            return Ok(());
        }
        self.db.execute_batch(
            "PRAGMA foreign_keys = OFF;
             BEGIN;
             CREATE TABLE themes_new (
                 id INTEGER PRIMARY KEY,
                 author INTEGER REFERENCES accounts(id) ON DELETE CASCADE,
                 name TEXT NOT NULL,
                 base TEXT NOT NULL,
                 colors TEXT NOT NULL,
                 picture TEXT,
                 thumb TEXT,
                 created INTEGER NOT NULL,
                 removed INTEGER NOT NULL DEFAULT 0,
                 builtin TEXT UNIQUE
             );
             INSERT INTO themes_new (id, author, name, base, colors, picture, thumb, created, removed)
                 SELECT id, author, name, base, colors, picture, thumb, created, removed FROM themes;
             DROP TABLE themes;
             ALTER TABLE themes_new RENAME TO themes;
             CREATE INDEX IF NOT EXISTS themes_by_author ON themes(author, created);
             COMMIT;
             PRAGMA foreign_keys = ON;",
        )?;
        Ok(())
    }

    /// Whether `account` may share a theme now.
    fn may_share_theme(&self, account: i64) -> Result<()> {
        if self.verified_wallets(account)?.is_empty() {
            bail!("verify one of your wallets first: only accounts with a verified wallet can share themes");
        }
        let live: i64 =
            self.db.query_row("SELECT COUNT(*) FROM themes WHERE author = ?1 AND removed = 0", [account], |r| r.get(0))?;
        if live >= LIVE_PER_ACCOUNT {
            bail!("you have {LIVE_PER_ACCOUNT} themes shared; delete one to share another");
        }
        let today: i64 = self.db.query_row(
            "SELECT COUNT(*) FROM themes WHERE author = ?1 AND created > ?2",
            params![account, now().saturating_sub(86_400) as i64],
            |r| r.get(0),
        )?;
        if today >= A_DAY {
            bail!("that's {A_DAY} themes shared today; share more tomorrow");
        }
        Ok(())
    }

    fn add_theme(&self, author: i64, draft: &Draft, picture: &str, thumb: &str) -> Result<i64> {
        self.db.execute(
            "INSERT INTO themes (author, name, base, colors, picture, thumb, created) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![author, draft.name, draft.base, serde_json::to_string(&draft.colors)?, picture, thumb, now() as i64],
        )?;
        Ok(self.db.last_insert_rowid())
    }

    /// Takes a theme down, by its author or the founder. Gives its picture
    /// files, which can go.
    pub fn remove_theme(&self, id: i64, author: Option<i64>) -> Result<(String, String)> {
        let found: Option<(Option<i64>, Option<String>, Option<String>)> = self
            .db
            .query_row("SELECT author, picture, thumb FROM themes WHERE id = ?1 AND removed = 0", [id], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?))
            })
            .optional()?;
        let Some((owner, picture, thumb)) = found else { bail!("there is no such theme") };
        let Some(owner) = owner else { bail!("Orien's own themes stay in the store") };
        let (Some(picture), Some(thumb)) = (picture, thumb) else { bail!("there is no such theme") };
        if author.is_some_and(|a| a != owner) {
            bail!("that theme isn't yours");
        }
        self.db.execute("UPDATE themes SET removed = 1 WHERE id = ?1", [id])?;
        Ok((picture, thumb))
    }

    pub fn theme_removed(&self, id: i64) -> Result<bool> {
        Ok(self.db.query_row("SELECT removed FROM themes WHERE id = ?1", [id], |r| r.get::<_, i64>(0)).optional()?.unwrap_or(1) != 0)
    }

    /// Counts one download by `who`, once. Gives the theme's downloads.
    fn download_theme(&self, id: i64, who: &str) -> Result<i64> {
        if self.theme_removed(id)? {
            bail!("there is no such theme");
        }
        self.db.execute(
            "INSERT OR IGNORE INTO theme_downloads (theme, who, at) VALUES (?1, ?2, ?3)",
            params![id, who, now() as i64],
        )?;
        Ok(self.db.query_row("SELECT COUNT(*) FROM theme_downloads WHERE theme = ?1", [id], |r| r.get(0))?)
    }

    /// Who a tip on a theme goes to: its author and their verified wallets.
    fn theme_tip_target(&self, id: i64) -> Result<(i64, Vec<String>)> {
        let author: Option<Option<i64>> =
            self.db.query_row("SELECT author FROM themes WHERE id = ?1 AND removed = 0", [id], |r| r.get(0)).optional()?;
        let Some(author) = author.flatten() else { bail!("there is no such theme to tip") };
        let wallets = self.verified_wallets(author)?.into_iter().map(|w| w.address).collect();
        Ok((author, wallets))
    }

    /// Whether a payment already counts as a tip, on a post or a theme.
    fn any_tip_known(&self, sender: &str, seq: u64) -> Result<bool> {
        let theme =
            self.db.query_row("SELECT 1 FROM theme_tips WHERE sender = ?1 AND seq = ?2", params![sender, seq as i64], |_| Ok(())).optional()?;
        Ok(theme.is_some() || self.tip_known(sender, seq)?)
    }

    fn add_theme_tip(&self, id: i64, tipper: i64, sender: &str, seq: u64, orn: u64) -> Result<bool> {
        Ok(self.db.execute(
            "INSERT OR IGNORE INTO theme_tips (sender, seq, theme, tipper, orn, at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![sender, seq as i64, id, tipper, orn as i64, now() as i64],
        )? == 1)
    }

    fn theme_tips(&self, id: i64) -> Result<(i64, i64)> {
        Ok(self.db.query_row("SELECT COUNT(*), COALESCE(SUM(orn), 0) FROM theme_tips WHERE theme = ?1", [id], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })?)
    }

    /// Themes shown to `viewer`: `filter` is SQL on `t`, `order` the order.
    fn themes_where(&self, viewer: Option<i64>, filter: &str, order: &str, limit: i64, offset: i64) -> Result<Vec<Value>> {
        let lately = now().saturating_sub(LATELY) as i64;
        let mut query = self.db.prepare(&format!(
            "SELECT t.id, t.name, t.base, t.colors, t.picture, t.thumb, t.created, t.author = ?1,
                    a.id, a.name, a.picture, t.builtin,
                    EXISTS (SELECT 1 FROM verified_wallets v WHERE v.account = a.id AND v.proven IS NOT NULL),
                    -- Where a tip goes, as for posts: the Market wallet if
                    -- verified, otherwise the first verified wallet.
                    COALESCE(
                        (SELECT a.payout WHERE a.payout IS NOT NULL AND EXISTS (SELECT 1 FROM verified_wallets v
                            WHERE v.account = a.id AND v.address = a.payout AND v.proven IS NOT NULL)),
                        (SELECT v.address FROM verified_wallets v WHERE v.account = a.id AND v.proven IS NOT NULL
                            ORDER BY v.proven, v.address LIMIT 1)),
                    (SELECT COUNT(*) FROM theme_downloads d WHERE d.theme = t.id) AS downloads,
                    (SELECT COUNT(*) FROM theme_tips p WHERE p.theme = t.id) AS tips,
                    (SELECT COALESCE(SUM(p.orn), 0) FROM theme_tips p WHERE p.theme = t.id),
                    -- Popular: downloads and tips, the last four weeks
                    -- counting three times as much.
                    (SELECT COUNT(*) FROM theme_downloads d WHERE d.theme = t.id)
                      + 2 * (SELECT COUNT(*) FROM theme_downloads d WHERE d.theme = t.id AND d.at > ?2)
                      + 3 * (SELECT COUNT(*) FROM theme_tips p WHERE p.theme = t.id)
                      + 6 * (SELECT COUNT(*) FROM theme_tips p WHERE p.theme = t.id AND p.at > ?2) AS score
             FROM themes t LEFT JOIN accounts a ON a.id = t.author
             WHERE t.removed = 0 AND ({filter})
               AND (t.author IS NULL OR t.author NOT IN (SELECT account FROM suspended))
               AND (t.author IS NULL OR t.author NOT IN (SELECT blocker FROM blocks WHERE blocked = ?1 UNION SELECT blocked FROM blocks WHERE blocker = ?1))
             ORDER BY {order} LIMIT {limit} OFFSET {offset}"
        ))?;
        let rows = query.query_map(params![viewer.unwrap_or(-1), lately], |row| {
            let builtin: Option<String> = row.get(11)?;
            // Orien's own themes: published under Orien, with no tips.
            let author = match &builtin {
                Some(_) => json!({ "id": null, "name": "Orien", "picture": null, "verified": true, "official": true }),
                None => json!({
                    "id": row.get::<_, i64>(8)?,
                    "name": row.get::<_, String>(9)?,
                    "picture": row.get::<_, Option<String>>(10)?,
                    "verified": row.get::<_, bool>(12)?,
                    "tip_to": row.get::<_, Option<String>>(13)?,
                }),
            };
            let mut theme = json!({
                "id": row.get::<_, i64>(0)?,
                "name": row.get::<_, String>(1)?,
                "base": row.get::<_, String>(2)?,
                "colors": serde_json::from_str::<Value>(&row.get::<_, String>(3)?).unwrap_or(json!([])),
                "picture": row.get::<_, Option<String>>(4)?,
                "thumb": row.get::<_, Option<String>>(5)?,
                "created": row.get::<_, i64>(6)?,
                "mine": row.get::<_, Option<bool>>(7)?.unwrap_or(false),
                "author": author,
                "downloads": row.get::<_, i64>(14)?,
                "tips": { "count": row.get::<_, i64>(15)?, "orn": row.get::<_, i64>(16)? },
            });
            if let Some(builtin) = builtin {
                theme["builtin"] = json!(builtin);
            }
            Ok(theme)
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    fn theme(&self, viewer: Option<i64>, id: i64) -> Result<Option<Value>> {
        Ok(self.themes_where(viewer, &format!("t.id = {id}"), "t.id", 1, 0)?.into_iter().next())
    }

    /// The dashboard's list: every theme up, newest first.
    pub fn admin_themes(&self) -> Result<Vec<Value>> {
        self.themes_where(None, "1", "t.created DESC", 200, 0)
    }
}

#[derive(Deserialize)]
struct Browse {
    #[serde(default)]
    sort: String,
    #[serde(default)]
    before: Option<String>,
    /// "1": only the signed-in person's own themes.
    #[serde(default)]
    mine: Option<String>,
}

/// GET /community/themes: popular or new, a page at a time.
async fn list(State(app): State<Arc<App>>, headers: HeaderMap, Query(asked): Query<Browse>) -> Result<Response, Failure> {
    let mine = asked.mine.as_deref().is_some_and(|m| m == "1" || m == "true");
    let viewer = if mine { Some(app.signed_in(&headers)?) } else { app.signed_in(&headers).ok() };
    // Your own: newest first, whatever sort was asked.
    let whose = match (mine, viewer) {
        (true, Some(me)) => format!(" AND t.author = {me}"),
        _ => String::new(),
    };
    let store = app.store();
    let (themes, next) = if asked.sort == "new" || mine {
        let before = asked.before.as_deref().and_then(|b| b.parse::<i64>().ok()).unwrap_or(i64::MAX);
        let themes = store.themes_where(viewer, &format!("t.id < {before}{whose}"), "t.id DESC", PAGE + 1, 0)?;
        let next = themes.get(PAGE as usize - 1).and_then(|t| t["id"].as_i64()).map(|id| id.to_string());
        (themes, next)
    } else {
        // Popular moves as people download, so its cursor is a place in the list.
        let offset = asked.before.as_deref().and_then(|b| b.parse::<i64>().ok()).unwrap_or(0).max(0);
        let themes = store.themes_where(viewer, "1", "score DESC, t.id DESC", PAGE + 1, offset)?;
        (themes, Some((offset + PAGE).to_string()))
    };
    drop(store);
    let more = themes.len() as i64 > PAGE;
    let themes: Vec<Value> = themes.into_iter().take(PAGE as usize).collect();
    Ok(Json(json!({ "themes": themes, "more": more, "next": if more { next } else { None } })).into_response())
}

async fn one(State(app): State<Arc<App>>, headers: HeaderMap, Path(id): Path<i64>) -> Result<Response, Failure> {
    let viewer = app.signed_in(&headers).ok();
    let theme = app.store().theme(viewer, id)?.ok_or(Failure::NotFound)?;
    Ok(Json(theme).into_response())
}

#[derive(Deserialize)]
struct Share {
    name: String,
    base: String,
    colors: Vec<String>,
    picture: String,
}

/// POST /community/themes: share a theme.
async fn share(State(app): State<Arc<App>>, headers: HeaderMap, Json(given): Json<Share>) -> Result<Response, Failure> {
    let author = app.signed_in(&headers)?;
    let draft = checked(&given.name, &given.base, &given.colors).map_err(Failure::Refused)?;
    app.store().may_share_theme(author).map_err(refused)?;
    let bytes = picture_bytes(&given.picture).map_err(Failure::Refused)?;
    let (full, thumb) = tokio::task::spawn_blocking(move || pictures(&bytes))
        .await
        .map_err(|e| Failure::Broken(e.into()))?
        .map_err(refused)?;
    let name = draft.name.clone();
    screened(&app, "theme name", move |s| s.message(&name)).await?;
    let checked_picture = thumb.clone();
    screened(&app, "picture", move |s| s.picture(&checked_picture)).await?;
    let picture = app.images.save(&full)?;
    let small = app.images.save(&thumb)?;
    let id = app.store().add_theme(author, &draft, &picture, &small)?;
    let theme = app.store().theme(Some(author), id)?.ok_or(Failure::NotFound)?;
    let maker = theme["author"]["name"].as_str().unwrap_or("someone").to_string();
    app.alerts().theme(format!(
        "🎨 **New theme {id}: {}** by {maker} · {} · {}\n{}",
        draft.name,
        draft.base,
        draft.colors.join(" "),
        app.absolute(&format!("/api/market/images/{small}")),
    ));
    Ok(Json(theme).into_response())
}

async fn delete(State(app): State<Arc<App>>, headers: HeaderMap, Path(id): Path<i64>) -> Result<Response, Failure> {
    let author = app.signed_in(&headers)?;
    let (picture, thumb) = app.store().remove_theme(id, Some(author)).map_err(refused)?;
    app.images.remove(&picture);
    app.images.remove(&thumb);
    Ok(Json(json!({ "ok": true })).into_response())
}

/// POST /community/themes/{id}/download: once per account, or per internet
/// address (kept only as a hash) for people signed out.
async fn download(State(app): State<Arc<App>>, headers: HeaderMap, Path(id): Path<i64>) -> Result<Response, Failure> {
    let who = match app.signed_in(&headers).ok() {
        Some(account) => format!("a:{account}"),
        None => {
            let header = |name: &str| headers.get(name).and_then(|v| v.to_str().ok()).unwrap_or("").trim().to_string();
            let address = [header("cf-connecting-ip"), header("x-forwarded-for").split(',').next().unwrap_or("").trim().to_string()]
                .into_iter()
                .find(|a| !a.is_empty())
                .unwrap_or_else(|| "unknown".into());
            format!("ip:{}", &blake3::hash(format!("orien theme download {address}").as_bytes()).to_hex()[..32])
        }
    };
    let downloads = app.store().download_theme(id, &who).map_err(|_| Failure::NotFound)?;
    Ok(Json(json!({ "downloads": downloads })).into_response())
}

/// POST /community/themes/{id}/tips: as a post's tips, checked on the network.
async fn tip(State(app): State<Arc<App>>, headers: HeaderMap, Path(id): Path<i64>, Json(given): Json<crate::community::Tip>) -> Result<Response, Failure> {
    let tipper = app.signed_in(&headers)?;
    let (from, seq) = given.parts();
    let sender: orien_ledger::AccountId = from.trim().parse().map_err(|_| Failure::Refused("that is not an Orien address".into()))?;
    let sender_hex = hex::encode(sender.as_bytes());
    let (author, wallets) = app.store().theme_tip_target(id).map_err(|_| Failure::NotFound)?;
    if author == tipper {
        return Err(Failure::Refused("you can't tip your own theme".into()));
    }
    if wallets.is_empty() {
        return Err(Failure::Refused("this person has no verified wallet to tip".into()));
    }
    let answer = |count: i64, orn: i64| Json(json!({ "count": count, "orn": orn })).into_response();
    if app.store().any_tip_known(&sender_hex, seq)? {
        let (count, orn) = app.store().theme_tips(id)?;
        return Ok(answer(count, orn));
    }
    let key = format!("{sender_hex}:{seq}");
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
        return Err(Failure::Refused("that payment to this theme's maker isn't settled on the network yet".into()));
    };
    if app.store().add_theme_tip(id, tipper, &sender_hex, seq, orn)? {
        let name = app.store().account(tipper)?.map(|a| a.name).unwrap_or_default();
        app.tell(crate::push::Event {
            account: author,
            address: None,
            body: format!("{name} tipped you {} ORN for your theme", crate::store::orn(orn)),
            kind: "tip",
            open: json!({ "theme": id }),
        });
    }
    let (count, orn) = app.store().theme_tips(id)?;
    Ok(answer(count, orn))
}

#[derive(Deserialize)]
struct Reason {
    #[serde(default)]
    reason: String,
}

async fn report(State(app): State<Arc<App>>, headers: HeaderMap, Path(id): Path<i64>, Json(given): Json<Reason>) -> Result<Response, Failure> {
    let reporter = app.signed_in(&headers)?;
    app.store().report(reporter, "theme", id, &given.reason).map_err(refused)?;
    let reason: String = given.reason.trim().chars().take(300).collect();
    let why = if reason.is_empty() { String::new() } else { format!(": “{reason}”") };
    app.alerts().event(format!("🚩 Someone reported a theme (#{id}){why}. It's on the dashboard under Reports."));
    Ok(Json(json!({})).into_response())
}

pub fn routes() -> Router<Arc<App>> {
    Router::new()
        .route(
            "/api/market/community/themes",
            get(list).post(share).layer(axum::extract::DefaultBodyLimit::max(MAX_PICTURE * 2)),
        )
        .route("/api/market/community/themes/{id}", get(one).delete(delete))
        .route("/api/market/community/themes/{id}/download", post(download))
        .route("/api/market/community/themes/{id}/tips", post(tip))
        .route("/api/market/community/themes/{id}/report", post(report))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn maker() -> (Store, i64, i64) {
        let store = Store::in_memory().unwrap();
        let maya = store.create_account_with_email("Maya", "maya@example.com").unwrap().id;
        let sam = store.create_account_with_email("Sam", "sam@example.com").unwrap().id;
        store.verified_wallet_for_tests(maya, &"a".repeat(64)).unwrap();
        (store, maya, sam)
    }

    #[test]
    fn a_theme_is_checked_in_plain_words() {
        let colors = |c: &[&str]| c.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        let draft = checked("  Nebula  ", "dark", &colors(&["#A1B2C3", "#000000"])).unwrap();
        assert_eq!((draft.name.as_str(), draft.base), ("Nebula", "dark"));
        assert_eq!(draft.colors, vec!["#a1b2c3", "#000000"]);
        assert!(checked("", "dark", &colors(&["#000000"])).is_err());
        assert!(checked(&"x".repeat(25), "dark", &colors(&["#000000"])).is_err());
        assert!(checked("A", "dim", &colors(&["#000000"])).is_err());
        assert!(checked("A", "dark", &colors(&[])).is_err());
        assert!(checked("A", "dark", &colors(&["#1", "#2", "#3", "#4"])).is_err());
        assert!(checked("A", "dark", &colors(&["red"])).is_err());
        assert!(picture_bytes("data:image/jpeg;base64,!!!").is_err());
        assert_eq!(picture_bytes("data:image/jpeg;base64,aGk=").unwrap(), b"hi");
    }

    #[test]
    fn themes_are_shared_listed_downloaded_once_and_taken_down() {
        let (store, maya, sam) = maker();
        let draft = checked("Nebula", "dark", &["#112233".to_string()]).unwrap();
        assert!(store.may_share_theme(sam).unwrap_err().to_string().contains("verify"));
        store.may_share_theme(maya).unwrap();
        let a = store.add_theme(maya, &draft, "p1.jpg", "t1.jpg").unwrap();
        let b = store.add_theme(maya, &draft, "p2.jpg", "t2.jpg").unwrap();
        assert_eq!(store.download_theme(a, "a:2").unwrap(), 1);
        assert_eq!(store.download_theme(a, "a:2").unwrap(), 1, "once per person");
        assert_eq!(store.download_theme(a, "ip:x").unwrap(), 2);
        let popular = store.themes_where(Some(sam), "1", "score DESC, t.id DESC", 10, 0).unwrap();
        assert_eq!(popular[0]["id"], a, "the downloaded one first");
        assert_eq!(popular[0]["downloads"], 2);
        assert_eq!(popular[0]["author"]["verified"], true);
        assert_eq!(popular[0]["mine"], false);
        assert_eq!(store.theme(Some(maya), b).unwrap().unwrap()["mine"], true);
        assert!(store.remove_theme(a, Some(sam)).is_err(), "only its maker");
        assert_eq!(store.remove_theme(a, Some(maya)).unwrap(), ("p1.jpg".into(), "t1.jpg".into()));
        assert!(store.theme(None, a).unwrap().is_none());
        assert!(store.download_theme(a, "a:2").is_err());
        for _ in 0..3 {
            store.add_theme(maya, &draft, "p.jpg", "t.jpg").unwrap();
        }
        assert!(store.may_share_theme(maya).unwrap_err().to_string().contains("today"));
    }

    #[test]
    fn a_payment_counts_as_one_tip_once_across_posts_and_themes() {
        let (store, maya, sam) = maker();
        let draft = checked("Nebula", "dark", &["#112233".to_string()]).unwrap();
        let id = store.add_theme(maya, &draft, "p.jpg", "t.jpg").unwrap();
        assert!(store.add_theme_tip(id, sam, "ab", 3, 1_000_000_000).unwrap());
        assert!(!store.add_theme_tip(id, sam, "ab", 3, 1_000_000_000).unwrap());
        assert!(store.any_tip_known("ab", 3).unwrap());
        assert_eq!(store.theme_tips(id).unwrap(), (1, 1_000_000_000));
    }

    #[test]
    fn orien_s_own_themes_are_listed_downloaded_and_never_taken_down_or_tipped() {
        let (store, maya, sam) = maker();
        let all = store.themes_where(Some(sam), "1", "t.id", 50, 0).unwrap();
        let official: Vec<&Value> = all.iter().filter(|t| t.get("builtin").is_some()).collect();
        assert_eq!(official.len(), 6);
        let rose = official.iter().find(|t| t["builtin"] == "rose").unwrap();
        assert_eq!(rose["name"], "Rosé");
        assert_eq!(rose["author"], json!({ "id": null, "name": "Orien", "picture": null, "verified": true, "official": true }));
        assert_eq!((rose["picture"].is_null(), rose["thumb"].is_null(), rose["mine"].as_bool()), (true, true, Some(false)));
        let id = rose["id"].as_i64().unwrap();
        assert_eq!(store.download_theme(id, "a:2").unwrap(), 1);
        assert!(store.remove_theme(id, None).is_err(), "not even from the dashboard");
        assert!(store.remove_theme(id, Some(maya)).is_err());
        assert!(store.theme_tip_target(id).is_err());
        assert!(store.report(sam, "theme", id, "no").is_err());
        // Never in anyone's own list.
        assert!(store.themes_where(Some(maya), &format!("t.author = {maya}"), "t.id", 50, 0).unwrap().is_empty());
        // Opening the store again adds nothing twice.
        store.create_theme_tables().unwrap();
        assert_eq!(store.themes_where(None, "t.builtin IS NOT NULL", "t.id", 50, 0).unwrap().len(), 6);
    }

    #[test]
    fn a_store_from_before_keeps_its_themes_ids_and_downloads() {
        let (store, maya, _sam) = maker();
        store
            .db
            .execute_batch(
                "PRAGMA foreign_keys = OFF; DROP TABLE themes; PRAGMA foreign_keys = ON;
                 CREATE TABLE themes (id INTEGER PRIMARY KEY, author INTEGER NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
                     name TEXT NOT NULL, base TEXT NOT NULL, colors TEXT NOT NULL, picture TEXT NOT NULL, thumb TEXT NOT NULL,
                     created INTEGER NOT NULL, removed INTEGER NOT NULL DEFAULT 0);",
            )
            .unwrap();
        store.db.execute("INSERT INTO themes VALUES (4, ?1, 'Nebula', 'dark', '[\"#112233\"]', 'p.jpg', 't.jpg', 1, 0)", [maya]).unwrap();
        store.db.execute("INSERT INTO theme_downloads VALUES (4, 'a:9', 1)", []).unwrap();
        store.create_theme_tables().unwrap();
        let nebula = store.theme(None, 4).unwrap().unwrap();
        assert_eq!((nebula["name"].as_str(), nebula["downloads"].as_i64()), (Some("Nebula"), Some(1)));
        assert_eq!(nebula["author"]["name"], "Maya");
        assert_eq!(store.themes_where(None, "t.builtin IS NOT NULL", "t.id", 50, 0).unwrap().len(), 6);
    }
}
