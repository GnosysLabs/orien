//! "Report a problem" from Orien Wallet: what went wrong, in the person's
//! own words, with where in the app, the app's version, system and window,
//! the last screens they visited and the errors the app showed them. Each
//! report goes to the founder's private #app-reports channel on Discord and
//! to a list on the dashboard. Nothing else is kept: the app never sends
//! keys, addresses or balances, and fields it doesn't know are dropped.

use std::sync::Arc;
use std::time::Instant;

use anyhow::Result;
use axum::extract::State;
use axum::http::HeaderMap;
use axum::response::IntoResponse;
use axum::routing::post;
use axum::{Json, Router};
use rusqlite::params;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::api::{App, Failure};
use crate::store::{self, Store};

/// Reports one internet address may send in an hour.
const PER_ADDRESS: usize = 5;
/// Reports everyone together may send in an hour.
const PER_HOUR: usize = 200;
const MAX_WHAT: usize = 2000;
const MAX_PLACE: usize = 100;
const MAX_EMAIL: usize = 200;
const MAX_TRAIL: usize = 10;
const MAX_SCREEN: usize = 40;
const MAX_ERRORS: usize = 5;
const MAX_ERROR: usize = 200;
/// How much of the text the Discord message carries; the dashboard has it all.
const DISCORD_WHAT: usize = 1100;

const WHERE: [&str; 9] = ["wallet", "send", "mining", "swaps", "community", "messages", "explorer", "settings", "other"];
const SYSTEMS: [&str; 4] = ["macOS", "Windows", "Linux", "iPhone"];

#[derive(Deserialize)]
pub struct Report {
    #[serde(rename = "where")]
    place_kind: String,
    #[serde(default)]
    place: String,
    what: String,
    #[serde(default)]
    email: String,
    app: AppInfo,
    #[serde(default)]
    trail: Vec<Visit>,
    #[serde(default)]
    errors: Vec<Shown>,
}

#[derive(Deserialize, Serialize, Clone)]
pub struct AppInfo {
    version: String,
    system: String,
    #[serde(default)]
    window: String,
    #[serde(default)]
    big: bool,
}

#[derive(Deserialize, Serialize, Clone)]
pub struct Visit {
    screen: String,
    at: u64,
}

#[derive(Deserialize, Serialize, Clone)]
pub struct Shown {
    text: String,
    at: u64,
}

/// A report as the dashboard shows it.
#[derive(Serialize)]
pub struct Kept {
    pub id: i64,
    pub account: Option<i64>,
    pub name: Option<String>,
    #[serde(rename = "where")]
    pub place_kind: String,
    pub place: String,
    pub what: String,
    pub email: String,
    pub app: serde_json::Value,
    pub trail: serde_json::Value,
    pub errors: serde_json::Value,
    pub created: u64,
    pub done: Option<u64>,
}

fn cut(text: &str, most: usize) -> String {
    text.trim().chars().filter(|c| !c.is_control() || *c == '\n').take(most).collect()
}

/// Checks a report and keeps only what's allowed, trimmed to size.
fn checked(report: Report) -> Result<Report, String> {
    let place_kind = report.place_kind.trim().to_lowercase();
    if !WHERE.contains(&place_kind.as_str()) {
        return Err("say where in the app the problem is".into());
    }
    let place = if place_kind == "other" { cut(&report.place, MAX_PLACE) } else { String::new() };
    let what = report.what.trim().to_string();
    if what.is_empty() {
        return Err("say what went wrong".into());
    }
    if what.chars().count() > MAX_WHAT {
        return Err(format!("a report is at most {MAX_WHAT} characters"));
    }
    let email = report.email.trim();
    let email = if email.is_empty() {
        String::new()
    } else if email.chars().count() > MAX_EMAIL {
        return Err("that email address is too long".into());
    } else {
        store::email(email).map_err(|_| "that does not look like an email address".to_string())?
    };
    let system = report.app.system.trim();
    let app = AppInfo {
        version: cut(&report.app.version, 20),
        system: SYSTEMS.iter().find(|s| s.eq_ignore_ascii_case(system)).map_or_else(|| cut(system, 20), |s| s.to_string()),
        window: cut(&report.app.window, 20),
        big: report.app.big,
    };
    let trail = report
        .trail
        .iter()
        .rev()
        .take(MAX_TRAIL)
        .rev()
        .map(|v| Visit { screen: cut(&v.screen, MAX_SCREEN), at: v.at })
        .filter(|v| !v.screen.is_empty())
        .collect();
    let errors = report
        .errors
        .iter()
        .rev()
        .take(MAX_ERRORS)
        .rev()
        .map(|e| Shown { text: cut(&e.text, MAX_ERROR), at: e.at })
        .filter(|e| !e.text.is_empty())
        .collect();
    Ok(Report { place_kind, place, what: cut(&what, MAX_WHAT), email, app, trail, errors })
}

impl Store {
    pub(crate) fn create_app_report_table(&self) -> Result<()> {
        self.db.execute_batch(
            "CREATE TABLE IF NOT EXISTS app_reports (
                 id INTEGER PRIMARY KEY,
                 account INTEGER REFERENCES accounts(id) ON DELETE SET NULL,
                 place_kind TEXT NOT NULL,
                 place TEXT NOT NULL,
                 what TEXT NOT NULL,
                 email TEXT NOT NULL,
                 app TEXT NOT NULL,
                 trail TEXT NOT NULL,
                 errors TEXT NOT NULL,
                 created INTEGER NOT NULL,
                 done INTEGER
             );",
        )?;
        Ok(())
    }

    fn add_app_report(&self, account: Option<i64>, report: &Report, now: u64) -> Result<i64> {
        self.db.execute(
            "INSERT INTO app_reports (account, place_kind, place, what, email, app, trail, errors, created)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                account,
                report.place_kind,
                report.place,
                report.what,
                report.email,
                serde_json::to_string(&report.app)?,
                serde_json::to_string(&report.trail)?,
                serde_json::to_string(&report.errors)?,
                now as i64,
            ],
        )?;
        Ok(self.db.last_insert_rowid())
    }

    /// The reports not dealt with yet, newest first, then the last few done.
    pub fn app_reports(&self) -> Result<Vec<Kept>> {
        let mut statement = self.db.prepare(
            "SELECT * FROM (
                 SELECT r.id, r.account, a.name, r.place_kind, r.place, r.what, r.email, r.app, r.trail, r.errors, r.created, r.done
                 FROM app_reports r LEFT JOIN accounts a ON a.id = r.account WHERE r.done IS NULL
                 UNION ALL
                 SELECT * FROM (
                     SELECT r.id, r.account, a.name, r.place_kind, r.place, r.what, r.email, r.app, r.trail, r.errors, r.created, r.done
                     FROM app_reports r LEFT JOIN accounts a ON a.id = r.account WHERE r.done IS NOT NULL
                     ORDER BY r.done DESC LIMIT 20
                 )
             ) ORDER BY done IS NOT NULL, created DESC",
        )?;
        let json = |text: String| serde_json::from_str(&text).unwrap_or(serde_json::Value::Null);
        let rows = statement.query_map([], |row| {
            Ok(Kept {
                id: row.get(0)?,
                account: row.get(1)?,
                name: row.get(2)?,
                place_kind: row.get(3)?,
                place: row.get(4)?,
                what: row.get(5)?,
                email: row.get(6)?,
                app: json(row.get(7)?),
                trail: json(row.get(8)?),
                errors: json(row.get(9)?),
                created: row.get::<_, i64>(10)? as u64,
                done: row.get::<_, Option<i64>>(11)?.map(|d| d as u64),
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    pub fn app_report_done(&self, id: i64, now: u64) -> Result<()> {
        let changed = self.db.execute("UPDATE app_reports SET done = ?2 WHERE id = ?1 AND done IS NULL", params![id, now as i64])?;
        if changed == 0 && self.db.query_row("SELECT COUNT(*) FROM app_reports WHERE id = ?1", [id], |r| r.get::<_, i64>(0))? == 0 {
            anyhow::bail!("there is no report {id}");
        }
        Ok(())
    }
}

/// "12:04 UTC", from seconds since 1970.
fn clock(at: u64) -> String {
    format!("{:02}:{:02} UTC", (at / 3600) % 24, (at / 60) % 60)
}

fn label(kind: &str) -> &str {
    match kind {
        "wallet" => "Wallet",
        "send" => "Send",
        "mining" => "Mining",
        "swaps" => "Swaps",
        "community" => "Community",
        "messages" => "Messages",
        "explorer" => "Explorer",
        "settings" => "Settings",
        _ => "Other",
    }
}

/// The Discord message: one report, plain, nothing that can ping anyone.
fn message(id: i64, name: Option<&str>, report: &Report) -> String {
    let place = if report.place_kind == "other" && !report.place.is_empty() {
        format!("Other: {}", report.place)
    } else {
        label(&report.place_kind).to_string()
    };
    let mut what: String = report.what.chars().take(DISCORD_WHAT).collect();
    if report.what.chars().count() > DISCORD_WHAT {
        what.push_str("… (the rest is on the dashboard)");
    }
    let quoted: String = what.lines().map(|line| format!("> {line}\n")).collect();
    let mut text = format!("🐞 **Problem report {id}** · {place}\n{quoted}");
    let app = &report.app;
    let window = if app.window.is_empty() { String::new() } else { format!(" · {}", app.window) };
    text.push_str(&format!(
        "Orien Wallet {} · {}{window}{}\n",
        app.version,
        app.system,
        if app.big { " (big window)" } else { "" }
    ));
    text.push_str(&format!("From: {}", name.unwrap_or("someone signed out")));
    if !report.email.is_empty() {
        text.push_str(&format!(" · {}", report.email));
    }
    text.push('\n');
    if !report.trail.is_empty() {
        let screens: Vec<&str> = report.trail.iter().map(|v| v.screen.as_str()).collect();
        text.push_str(&format!("Trail: {}\n", screens.join(" → ")));
    }
    for shown in &report.errors {
        text.push_str(&format!("Error at {}: {}\n", clock(shown.at), shown.text));
    }
    text
}

fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

/// POST /api/market/app-reports: signed in or not.
async fn report(State(app): State<Arc<App>>, headers: HeaderMap, Json(report): Json<Report>) -> Result<axum::response::Response, Failure> {
    let report = checked(report).map_err(Failure::Refused)?;
    let header = |name: &str| headers.get(name).and_then(|v| v.to_str().ok()).unwrap_or("").trim().to_string();
    let address = [header("cf-connecting-ip"), header("x-forwarded-for").split(',').next().unwrap_or("").trim().to_string()]
        .into_iter()
        .find(|a| !a.is_empty())
        .unwrap_or_else(|| "unknown".into());
    {
        let mut sent = app.app_reports_sent.lock().unwrap();
        let hour = std::time::Duration::from_secs(3600);
        sent.retain(|(at, _)| at.elapsed() < hour);
        if sent.iter().filter(|(_, from)| *from == address).count() >= PER_ADDRESS || sent.len() >= PER_HOUR {
            return Err(Failure::Refused("too many reports for now; try again in an hour".into()));
        }
        sent.push((Instant::now(), address));
    }
    let account = app.signed_in(&headers).ok();
    let (id, name) = {
        let store = app.store();
        let id = store.add_app_report(account, &report, now())?;
        let name = match account {
            Some(account) => store.account(account)?.map(|a| a.name),
            None => None,
        };
        (id, name)
    };
    app.alerts().app_report(id, message(id, name.as_deref(), &report));
    Ok(Json(json!({ "ok": true, "id": id })).into_response())
}

pub fn routes() -> Router<Arc<App>> {
    Router::new().route("/api/market/app-reports", post(report))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> serde_json::Value {
        json!({
            "where": "send",
            "what": "  The Send button did nothing.  ",
            "email": "Someone@Example.com",
            "app": { "version": "1.9.19", "system": "macos", "window": "1200×800", "big": true, "key": "never kept" },
            "trail": (0..14).map(|i| json!({ "screen": format!("Screen {i}"), "at": 1_795_600_000 + i })).collect::<Vec<_>>(),
            "errors": [{ "text": "Could not reach the network", "at": 1_795_600_100 }],
            "balance": 12
        })
    }

    #[test]
    fn a_report_is_checked_trimmed_and_kept_without_unknown_fields() {
        let report = checked(serde_json::from_value(sample()).unwrap()).unwrap();
        assert_eq!(report.what, "The Send button did nothing.");
        assert_eq!(report.email, "someone@example.com");
        assert_eq!(report.app.system, "macOS");
        assert_eq!(report.trail.len(), 10, "the last ten screens");
        assert_eq!(report.trail[0].screen, "Screen 4");
        let store = Store::in_memory().unwrap();
        let id = store.add_app_report(None, &report, 1_795_600_200).unwrap();
        let kept = store.app_reports().unwrap();
        assert_eq!(kept.len(), 1);
        assert!(!serde_json::to_string(&kept[0].app).unwrap().contains("never kept"));
        store.app_report_done(id, 1_795_600_300).unwrap();
        assert_eq!(store.app_reports().unwrap()[0].done, Some(1_795_600_300));
        assert!(store.app_report_done(99, 1).is_err());
        let text = message(id, Some("Maya"), &report);
        assert!(text.contains("Problem report 1** · Send"));
        assert!(text.contains("Trail: Screen 4 → Screen 5"));
        assert!(text.contains("Error at 09:48 UTC: Could not reach the network"));
    }

    #[test]
    fn bad_reports_are_refused_in_plain_words() {
        let mut no_where = sample();
        no_where["where"] = json!("kitchen");
        assert!(checked(serde_json::from_value(no_where).unwrap()).is_err());
        let mut empty = sample();
        empty["what"] = json!("   ");
        assert_eq!(checked(serde_json::from_value(empty).unwrap()).err().unwrap(), "say what went wrong");
        let mut long = sample();
        long["what"] = json!("x".repeat(2001));
        assert!(checked(serde_json::from_value(long).unwrap()).is_err());
        let mut email = sample();
        email["email"] = json!("not an address");
        assert!(checked(serde_json::from_value(email).unwrap()).is_err());
        let mut other = sample();
        other["where"] = json!("other");
        other["place"] = json!("The theme builder");
        assert_eq!(checked(serde_json::from_value(other).unwrap()).unwrap().place, "The theme builder");
    }
}
