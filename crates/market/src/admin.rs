//! The founder's dashboard: how the network and the marketplace are doing,
//! and the reports people sent, with what can be done about them.
//!
//! Served on a listener of its own (127.0.0.1:7201 on the server), which
//! `tailscale serve` puts on the tailnet only. Tailscale adds the visitor's
//! login as the Tailscale-User-Login header; a request without it did not
//! come through Tailscale and is refused. The public site never forwards
//! to this listener.

use crate::api::{App, Failure, router};
use crate::store::{Store, now};
use anyhow::Result;
use axum::extract::{Path, Query, State};
use axum::http::HeaderMap;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::sync::Arc;

/// Who may use the dashboard: anyone on the tailnet, or only these
/// Tailscale logins when any are named.
#[derive(Clone, Default)]
pub struct Admins(pub Vec<String>);

impl Admins {
    fn check(&self, headers: &HeaderMap) -> Result<String, Failure> {
        // A debug build, tried on this computer, opens without Tailscale.
        #[cfg(debug_assertions)]
        if headers.get("tailscale-user-login").is_none() {
            return Ok("this computer (a debug build)".into());
        }
        let login = headers
            .get("tailscale-user-login")
            .and_then(|value| value.to_str().ok())
            .map(str::trim)
            .filter(|login| !login.is_empty())
            .ok_or_else(|| Failure::Forbidden("the dashboard opens only through Tailscale".into()))?;
        if !self.0.is_empty() && !self.0.iter().any(|allowed| allowed.eq_ignore_ascii_case(login)) {
            return Err(Failure::Forbidden(format!("{login} may not use the dashboard")));
        }
        Ok(login.to_string())
    }
}

#[derive(Clone)]
struct Admin {
    app: Arc<App>,
    admins: Admins,
}

/// The dashboard's routes, beside the marketplace's own (for pictures),
/// and the website's files from `site` for the dashboard's page.
pub fn admin_router(app: Arc<App>, admins: Admins, site: Option<&std::path::Path>) -> Router {
    let state = Admin { app: app.clone(), admins };
    let routes = Router::new()
        .route("/api/admin/overview", get(overview))
        .route("/api/admin/tokenomics", get(tokenomics))
        .route("/api/admin/traffic", get(traffic))
        .route("/api/admin/reports", get(reports))
        .route("/api/admin/reports/{id}/handled", post(handled))
        .route("/api/admin/listings/{id}/remove", post(remove_listing))
        .route("/api/admin/accounts", get(find_accounts))
        .route("/api/admin/accounts/{id}", get(account))
        .route("/api/admin/accounts/{id}/suspend", post(suspend))
        .route("/api/admin/accounts/{id}/unsuspend", post(unsuspend))
        .route("/api/admin/accounts/{id}/picture/remove", post(remove_picture))
        .route("/api/admin/accounts/{id}/banner/remove", post(remove_banner))
        .route("/api/admin/posts/{id}/remove", post(remove_post))
        .route("/api/admin/room/{id}/remove", post(remove_room_message))
        .route("/api/admin/room", get(room))
        .route("/api/admin/room/bans/{account}/lift", post(room_lift))
        .route("/api/admin/sites", get(sites))
        .route("/api/admin/sites/{id}/off", post(site_off))
        .route("/api/admin/sites/{id}/on", post(site_on))
        .with_state(state)
        .merge(router(app));
    match site {
        Some(site) => routes.fallback_service(tower_http::services::ServeDir::new(site)),
        None => routes,
    }
}

type Answer = Result<Response, Failure>;

fn refused(error: anyhow::Error) -> Failure {
    Failure::Refused(error.to_string())
}

/// A count for each of the last `days` days, oldest first, as "2026-11-15".
#[derive(Serialize)]
pub struct Day {
    pub date: String,
    pub signups: i64,
    pub listings: i64,
    pub messages: i64,
    pub swaps: i64,
}

#[derive(Serialize)]
pub struct AdminAccount {
    pub id: i64,
    pub name: String,
    pub email: Option<String>,
    pub picture: Option<String>,
    pub banner: Option<String>,
    pub wallet: Option<String>,
    pub created: u64,
    pub suspended: Option<String>,
    pub listings: i64,
    pub swaps_done: i64,
}

#[derive(Serialize)]
pub struct AdminListing {
    pub id: i64,
    pub title: String,
    pub seller: i64,
    pub seller_name: String,
    pub price: i64,
    pub unit: String,
    pub created: u64,
    pub open: bool,
    pub removed: bool,
}

#[derive(Serialize)]
pub struct MarketStats {
    pub accounts: i64,
    pub accounts_day: i64,
    pub accounts_week: i64,
    pub with_wallet: i64,
    pub suspended: i64,
    pub listings_open: i64,
    pub listings_week: i64,
    pub listings_removed: i64,
    pub conversations: i64,
    pub messages_day: i64,
    pub messages_week: i64,
    pub offers_open: i64,
    pub swaps_under_way: i64,
    pub swaps_done: i64,
    pub swaps_done_week: i64,
    pub reports_open: i64,
    pub daily: Vec<Day>,
    pub recent_accounts: Vec<AdminAccount>,
    pub recent_listings: Vec<AdminListing>,
}

const DAY: u64 = 24 * 3600;

/// "2026-11-15" for a day number since 1970.
pub(crate) fn date(day: u64) -> String {
    // Howard Hinnant's days-to-civil.
    let z = day as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}")
}

impl Store {
    fn count(&self, sql: &str, since: u64) -> Result<i64> {
        Ok(self.db.query_row(sql, [since as i64], |row| row.get(0))?)
    }

    fn per_day(&self, sql: &str, from: u64) -> Result<std::collections::HashMap<u64, i64>> {
        let mut statement = self.db.prepare(sql)?;
        let rows = statement.query_map([from as i64], |row| Ok((row.get::<_, i64>(0)? as u64, row.get::<_, i64>(1)?)))?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    pub fn market_stats(&self) -> Result<MarketStats> {
        let now = now();
        let (day, week) = (now.saturating_sub(DAY), now.saturating_sub(7 * DAY));
        let days = 14;
        let first = now / DAY + 1 - days;
        let signups = self.per_day("SELECT created / 86400, COUNT(*) FROM accounts WHERE created >= ?1 GROUP BY 1", first * DAY)?;
        let listings = self.per_day("SELECT created / 86400, COUNT(*) FROM listings WHERE created >= ?1 GROUP BY 1", first * DAY)?;
        let messages = self.per_day("SELECT created / 86400, COUNT(*) FROM messages WHERE created >= ?1 GROUP BY 1", first * DAY)?;
        let swaps = self.per_day(
            "SELECT updated / 86400, COUNT(*) FROM swap_offers WHERE state = 'done' AND updated >= ?1 GROUP BY 1",
            first * DAY,
        )?;
        let daily = (first..first + days)
            .map(|d| Day {
                date: date(d),
                signups: signups.get(&d).copied().unwrap_or(0),
                listings: listings.get(&d).copied().unwrap_or(0),
                messages: messages.get(&d).copied().unwrap_or(0),
                swaps: swaps.get(&d).copied().unwrap_or(0),
            })
            .collect();
        Ok(MarketStats {
            accounts: self.count("SELECT COUNT(*) FROM accounts WHERE ?1 >= 0", 0)?,
            accounts_day: self.count("SELECT COUNT(*) FROM accounts WHERE created >= ?1", day)?,
            accounts_week: self.count("SELECT COUNT(*) FROM accounts WHERE created >= ?1", week)?,
            with_wallet: self.count("SELECT COUNT(*) FROM accounts WHERE payout IS NOT NULL AND ?1 >= 0", 0)?,
            suspended: self.count("SELECT COUNT(*) FROM suspended WHERE ?1 >= 0", 0)?,
            listings_open: self.count("SELECT COUNT(*) FROM listings WHERE open = 1 AND ?1 >= 0", 0)?,
            listings_week: self.count("SELECT COUNT(*) FROM listings WHERE created >= ?1", week)?,
            listings_removed: self.count("SELECT COUNT(*) FROM removed_listings WHERE ?1 >= 0", 0)?,
            conversations: self.count("SELECT COUNT(*) FROM conversations WHERE ?1 >= 0", 0)?,
            messages_day: self.count("SELECT COUNT(*) FROM messages WHERE created >= ?1", day)?,
            messages_week: self.count("SELECT COUNT(*) FROM messages WHERE created >= ?1", week)?,
            offers_open: self.count("SELECT COUNT(*) FROM swap_offers WHERE state = 'open' AND ?1 >= 0", 0)?,
            swaps_under_way: self.count("SELECT COUNT(*) FROM swap_offers WHERE state IN ('held', 'swapping') AND ?1 >= 0", 0)?,
            swaps_done: self.count("SELECT COUNT(*) FROM swap_offers WHERE state = 'done' AND ?1 >= 0", 0)?,
            swaps_done_week: self.count("SELECT COUNT(*) FROM swap_offers WHERE state = 'done' AND updated >= ?1", week)?,
            reports_open: self.count("SELECT COUNT(*) FROM reports WHERE handled = 0 AND ?1 >= 0", 0)?,
            daily,
            recent_accounts: self.admin_accounts("1 = 1", "", 15)?,
            recent_listings: self.admin_listings(10)?,
        })
    }

    /// Accounts matching `filter` (SQL on `a`, with ?1 bound to `term`),
    /// newest first.
    fn admin_accounts(&self, filter: &str, term: &str, limit: i64) -> Result<Vec<AdminAccount>> {
        let sql = format!(
            "SELECT a.id, a.name, a.email, a.picture, a.payout, a.created, s.reason,
                    (SELECT COUNT(*) FROM listings WHERE seller = a.id),
                    (SELECT COUNT(*) FROM swap_offers WHERE (seller = a.id OR taker = a.id) AND state = 'done'),
                    a.banner
             FROM accounts a LEFT JOIN suspended s ON s.account = a.id
             WHERE ({filter}) AND ?1 = ?1 ORDER BY a.id DESC LIMIT {limit}"
        );
        let mut statement = self.db.prepare(&sql)?;
        let rows = statement.query_map([term], |row| {
            Ok(AdminAccount {
                id: row.get(0)?,
                name: row.get(1)?,
                email: row.get(2)?,
                picture: row.get(3)?,
                wallet: row.get(4)?,
                created: row.get(5)?,
                suspended: row.get(6)?,
                listings: row.get(7)?,
                swaps_done: row.get(8)?,
                banner: row.get(9)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    fn admin_listings(&self, limit: i64) -> Result<Vec<AdminListing>> {
        let mut statement = self.db.prepare(
            "SELECT l.id, l.title, l.seller, a.name, l.price, l.unit, l.created, l.open,
                    EXISTS (SELECT 1 FROM removed_listings WHERE listing = l.id)
             FROM listings l JOIN accounts a ON a.id = l.seller ORDER BY l.id DESC LIMIT ?1",
        )?;
        let rows = statement.query_map([limit], |row| {
            Ok(AdminListing {
                id: row.get(0)?,
                title: row.get(1)?,
                seller: row.get(2)?,
                seller_name: row.get(3)?,
                price: row.get(4)?,
                unit: row.get(5)?,
                created: row.get(6)?,
                open: row.get(7)?,
                removed: row.get(8)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// Accounts by id, email, name or wallet, as typed.
    pub fn find_accounts(&self, typed: &str) -> Result<Vec<AdminAccount>> {
        let typed = typed.trim();
        if typed.is_empty() {
            return Ok(Vec::new());
        }
        if let Ok(id) = typed.parse::<i64>() {
            return self.admin_accounts("a.id = CAST(?1 AS INTEGER)", &id.to_string(), 1);
        }
        let like = format!("%{}%", typed.to_lowercase().replace(['%', '_'], ""));
        self.admin_accounts("lower(a.email) LIKE ?1 OR lower(a.name) LIKE ?1 OR lower(a.payout) LIKE ?1", &like, 25)
    }

    pub fn admin_account(&self, id: i64) -> Result<Option<AdminAccount>> {
        Ok(self.admin_accounts("a.id = CAST(?1 AS INTEGER)", &id.to_string(), 1)?.into_iter().next())
    }

    /// The accounts that named `address` as their wallet, as "Maya, Sam".
    pub fn wallet_owner(&self, address: &str) -> Result<Option<String>> {
        let mut statement = self.db.prepare("SELECT name FROM accounts WHERE payout = ?1 ORDER BY id")?;
        let names: Vec<String> = statement.query_map([address], |row| row.get(0))?.collect::<rusqlite::Result<_>>()?;
        Ok((!names.is_empty()).then(|| names.join(", ")))
    }

    /// Who a report is about: a listing's seller, the account itself, or a
    /// message's sender.
    pub fn report_owner(&self, kind: &str, target: i64) -> Result<Option<i64>> {
        let sql = match kind {
            "listing" => "SELECT seller FROM listings WHERE id = ?1",
            "account" => "SELECT id FROM accounts WHERE id = ?1",
            "message" => "SELECT sender FROM messages WHERE id = ?1",
            "post" => "SELECT author FROM posts WHERE id = ?1",
            "room" => "SELECT author FROM room_messages WHERE id = ?1",
            _ => return Ok(None),
        };
        Ok(self.db.query_row(sql, params![target], |row| row.get(0)).optional()?)
    }
}

/// Free and total bytes on the server's disk, and whether each service runs.
fn server() -> serde_json::Value {
    let disk = std::process::Command::new("df")
        .args(["-B1", "--output=size,avail", "/"])
        .output()
        .ok()
        .and_then(|out| {
            let text = String::from_utf8(out.stdout).ok()?;
            let numbers: Vec<u64> = text.lines().nth(1)?.split_whitespace().filter_map(|n| n.parse().ok()).collect();
            Some(json!({ "total": numbers.first()?, "free": numbers.get(1)? }))
        });
    // Only what runs on this server: validators 2 to 4 have servers of their
    // own, and are asked over the network instead.
    let services: Vec<serde_json::Value> = ["orien-validator@1", "orien-market", "orien-discord", "nginx"]
        .iter()
        .map(|name| {
            let state = std::process::Command::new("systemctl")
                .args(["is-active", name])
                .output()
                .ok()
                .map(|out| String::from_utf8_lossy(&out.stdout).trim().to_string())
                .filter(|state| !state.is_empty())
                .unwrap_or_else(|| "unknown".into());
            json!({ "name": name, "state": state })
        })
        .collect();
    json!({ "disk": disk, "services": services })
}

async fn overview(State(admin): State<Admin>, headers: HeaderMap) -> Answer {
    let login = admin.admins.check(&headers)?;
    let market = admin.app.store().market_stats()?;
    let payments = admin.app.payments();
    let app = admin.app.clone();
    let (network, server, validators) = tokio::task::spawn_blocking(move || {
        // Each validator asked directly, wherever it runs, as the explorer does.
        let validators = app.explorer().overview(payments.as_ref()).ok().map(|o| {
            o.validators
                .iter()
                .map(|v| json!({ "seat": v.seat, "name": format!("Validator {}", v.seat + 1), "up": v.up, "epoch": v.epoch }))
                .collect::<Vec<_>>()
        });
        (payments.status(), server(), validators)
    })
    .await
    .map_err(|e| Failure::Broken(e.into()))?;
    let network = match network {
        Ok(network) => json!(network),
        Err(error) => json!({ "error": format!("{error:#}") }),
    };
    let usage = admin.app.mailer().usage();
    let email = json!({
        "daily": usage.daily,
        "monthly": usage.monthly,
        "at": usage.at,
        "daily_limit": crate::mail::DAILY_LIMIT,
        "monthly_limit": crate::mail::MONTHLY_LIMIT,
        "refused": usage.refused,
    });
    Ok(Json(json!({ "login": login, "generated": now(), "network": network, "market": market, "server": server, "validators": validators, "email": email }))
        .into_response())
}

/// The website's visitors and downloads over the last 30 days.
async fn traffic(State(admin): State<Admin>, headers: HeaderMap) -> Answer {
    admin.admins.check(&headers)?;
    Ok(Json(admin.app.traffic().stats(30)).into_response())
}

async fn tokenomics(State(admin): State<Admin>, headers: HeaderMap) -> Answer {
    admin.admins.check(&headers)?;
    let payments = admin.app.payments();
    let found = tokio::task::spawn_blocking(move || payments.tokenomics()).await.map_err(|e| Failure::Broken(e.into()))?;
    match found {
        Ok(tokenomics) => {
            // Wallets the marketplace knows, by the accounts that linked them.
            let mut names: serde_json::Map<String, serde_json::Value> = serde_json::Map::new();
            let store = admin.app.store();
            for holder in &tokenomics.top {
                if let Some(name) = store.wallet_owner(&holder.address)? {
                    names.insert(holder.address.clone(), json!(name));
                }
            }
            names.insert(admin.app.own_address().to_string(), json!("The marketplace's own wallet"));
            Ok(Json(json!({ "tokenomics": tokenomics, "names": names })).into_response())
        }
        Err(error) => Err(Failure::Unavailable(format!("{error:#}"))),
    }
}

async fn reports(State(admin): State<Admin>, headers: HeaderMap) -> Answer {
    admin.admins.check(&headers)?;
    let store = admin.app.store();
    let reports: Vec<serde_json::Value> = store
        .open_reports()?
        .into_iter()
        .map(|report| {
            let owner = store.report_owner(&report.kind, report.target).ok().flatten();
            let owner = owner.and_then(|id| store.admin_account(id).ok().flatten());
            let removed = match report.kind.as_str() {
                "listing" => store.is_removed(report.target).unwrap_or(false),
                "post" => store.post_removed(report.target).unwrap_or(false),
                "room" => store.room_removed(report.target).unwrap_or(false),
                _ => false,
            };
            json!({ "report": report, "owner": owner, "removed": removed })
        })
        .collect();
    Ok(Json(json!({ "reports": reports })).into_response())
}

#[derive(Deserialize)]
struct Reason {
    #[serde(default)]
    reason: String,
}

async fn handled(State(admin): State<Admin>, headers: HeaderMap, Path(id): Path<i64>) -> Answer {
    admin.admins.check(&headers)?;
    admin.app.store().mark_handled(id).map_err(refused)?;
    Ok(Json(json!({ "ok": true })).into_response())
}

async fn remove_listing(State(admin): State<Admin>, headers: HeaderMap, Path(id): Path<i64>, Json(body): Json<Reason>) -> Answer {
    let login = admin.admins.check(&headers)?;
    let reason = if body.reason.trim().is_empty() { format!("taken down by {login}") } else { body.reason };
    admin.app.store().remove_listing(id, &reason).map_err(refused)?;
    Ok(Json(json!({ "ok": true })).into_response())
}

#[derive(Deserialize)]
struct Search {
    #[serde(default)]
    q: String,
}

async fn find_accounts(State(admin): State<Admin>, headers: HeaderMap, Query(search): Query<Search>) -> Answer {
    admin.admins.check(&headers)?;
    let accounts = admin.app.store().find_accounts(&search.q)?;
    Ok(Json(json!({ "accounts": accounts })).into_response())
}

async fn account(State(admin): State<Admin>, headers: HeaderMap, Path(id): Path<i64>) -> Answer {
    admin.admins.check(&headers)?;
    let store = admin.app.store();
    let account = store.admin_account(id)?.ok_or_else(|| Failure::Refused(format!("there is no account {id}")))?;
    let listings = store.listings_of(id)?;
    Ok(Json(json!({ "account": account, "listings": listings })).into_response())
}

async fn suspend(State(admin): State<Admin>, headers: HeaderMap, Path(id): Path<i64>, Json(body): Json<Reason>) -> Answer {
    let login = admin.admins.check(&headers)?;
    let reason = if body.reason.trim().is_empty() { format!("suspended by {login}") } else { body.reason };
    admin.app.store().suspend(id, &reason).map_err(refused)?;
    Ok(Json(json!({ "ok": true })).into_response())
}

async fn unsuspend(State(admin): State<Admin>, headers: HeaderMap, Path(id): Path<i64>) -> Answer {
    admin.admins.check(&headers)?;
    admin.app.store().unsuspend(id).map_err(refused)?;
    Ok(Json(json!({ "ok": true })).into_response())
}

/// A profile picture that slipped past the checker: gone, and the robot
/// face shows instead. The account is otherwise untouched.
async fn remove_picture(State(admin): State<Admin>, headers: HeaderMap, Path(id): Path<i64>) -> Answer {
    let login = admin.admins.check(&headers)?;
    if let Some(old) = admin.app.store().clear_picture(id).map_err(refused)? {
        admin.app.images.remove(&old);
    }
    println!("{login} removed account {id}'s picture");
    Ok(Json(json!({ "ok": true })).into_response())
}

/// A banner that slipped past the checker: gone, as if never set.
async fn remove_banner(State(admin): State<Admin>, headers: HeaderMap, Path(id): Path<i64>) -> Answer {
    let login = admin.admins.check(&headers)?;
    if let Some(old) = admin.app.store().set_banner(id, None).map_err(refused)? {
        admin.app.images.remove(&old);
    }
    println!("{login} removed account {id}'s banner");
    Ok(Json(json!({ "ok": true })).into_response())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates_read_as_calendar_days() {
        assert_eq!(date(0), "1970-01-01");
        assert_eq!(date(20_407), "2025-11-15");
        assert_eq!(date(20_772), "2026-11-15");
    }

    #[test]
    fn counts_what_the_marketplace_holds() {
        let store = Store::in_memory().unwrap();
        let maya = store.create_account("Maya", "maya").unwrap();
        store.verified_wallet_for_tests(maya.id, &"a".repeat(64)).unwrap();
        let stats = store.market_stats().unwrap();
        assert_eq!((stats.accounts, stats.accounts_day, stats.with_wallet), (1, 1, 1));
        assert_eq!(stats.daily.len(), 14);
        assert_eq!(stats.daily.last().unwrap().signups, 1);
        assert_eq!(store.find_accounts("may").unwrap().len(), 1);
        assert_eq!(store.find_accounts(&maya.id.to_string()).unwrap()[0].name, "Maya");
        assert!(store.find_accounts("nobody").unwrap().is_empty());
    }
}

/// Every website registered for Sign in with Orien.
async fn sites(State(admin): State<Admin>, headers: HeaderMap) -> Answer {
    admin.admins.check(&headers)?;
    Ok(Json(json!({ "sites": admin.app.store().admin_sites()? })).into_response())
}

async fn site_off(State(admin): State<Admin>, headers: HeaderMap, Path(id): Path<i64>) -> Answer {
    admin.admins.check(&headers)?;
    admin.app.store().set_site_off(id, true)?.then_some(()).ok_or(Failure::NotFound)?;
    Ok(Json(json!({ "ok": true })).into_response())
}

async fn site_on(State(admin): State<Admin>, headers: HeaderMap, Path(id): Path<i64>) -> Answer {
    admin.admins.check(&headers)?;
    admin.app.store().set_site_off(id, false)?.then_some(()).ok_or(Failure::NotFound)?;
    Ok(Json(json!({ "ok": true })).into_response())
}

/// Takes a community post down.
async fn remove_post(State(admin): State<Admin>, headers: HeaderMap, Path(id): Path<i64>) -> Answer {
    admin.admins.check(&headers)?;
    admin.app.store().remove_post(id)?.then_some(()).ok_or(Failure::NotFound)?;
    Ok(Json(json!({ "ok": true })).into_response())
}

async fn remove_room_message(State(admin): State<Admin>, headers: HeaderMap, Path(id): Path<i64>) -> Answer {
    admin.admins.check(&headers)?;
    admin.app.store().room_remove(None, id)?.then_some(()).ok_or(Failure::NotFound)?;
    Ok(Json(json!({ "ok": true })).into_response())
}

/// The group chat: who is banned and what moderators did lately.
async fn room(State(admin): State<Admin>, headers: HeaderMap) -> Answer {
    admin.admins.check(&headers)?;
    let store = admin.app.store();
    Ok(Json(json!({ "bans": store.room_bans()?, "log": store.room_log(30)? })).into_response())
}

async fn room_lift(State(admin): State<Admin>, headers: HeaderMap, Path(account): Path<i64>) -> Answer {
    admin.admins.check(&headers)?;
    admin.app.store().room_unban(None, account)?.then_some(()).ok_or(Failure::NotFound)?;
    Ok(Json(json!({ "ok": true })).into_response())
}
