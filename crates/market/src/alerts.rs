//! Telling the founder what needs their attention: the Discord bot posts
//! each alert in its private #network-alerts channel.
//!
//! A trouble is said once when it starts, again now and then while it lasts
//! (hourly unless said otherwise), and once more when it's over. An event,
//! such as a new report, is said once.
//!
//! The watcher looks every five minutes at the validators, checkpoints, the
//! server's disk and Resend's email counts, and hourly at the OpenRouter
//! credit. Short blips are let pass: a validator must miss twice in a row
//! before it's said.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// How often the same trouble is said again while it lasts, by default.
const AGAIN: Duration = Duration::from_secs(3600);
const HOUR: Duration = Duration::from_secs(3600);
const DAY: Duration = Duration::from_secs(86_400);

/// Below this many signing keys left, a validator needs new keys soon. The
/// dashboard warns at the same number.
const FEW_KEYS: u32 = 5000;
/// Below this many dollars of OpenRouter credit, it's time to top up.
const LOW_CREDIT: f64 = 10.0;
/// Below this share of the disk free, or this many bytes, the server is
/// running out of room.
const LOW_DISK_SHARE: f64 = 0.10;
const LOW_DISK_BYTES: u64 = 5_000_000_000;
/// A share of Resend's daily or monthly limit that is worth a warning.
const NEAR_LIMIT: f64 = 0.8;

#[derive(Default)]
pub struct Alerts {
    /// The bot's inbox, such as http://127.0.0.1:7301/alert; none to only
    /// write alerts to the log.
    url: Option<String>,
    /// Each trouble going on, and when it was last said.
    troubles: Mutex<HashMap<String, Instant>>,
}

impl Alerts {
    pub fn to(url: Option<String>) -> Alerts {
        Alerts { url, troubles: Mutex::new(HashMap::new()) }
    }

    /// `kind` is in trouble: says `text` now if it's new, or if it was last
    /// said an hour ago.
    pub fn trouble(&self, kind: &str, text: String) {
        self.trouble_every(kind, text, AGAIN);
    }

    /// As `trouble`, said again only after `again`.
    pub fn trouble_every(&self, kind: &str, text: String, again: Duration) {
        let mut troubles = self.troubles.lock().unwrap();
        let now = Instant::now();
        let due = troubles.get(kind).is_none_or(|said| now.duration_since(*said) >= again);
        if due {
            troubles.insert(kind.to_string(), now);
            drop(troubles);
            self.say(format!("🔴 {text}"));
        }
    }

    /// `kind` works: says `text` if it was in trouble.
    pub fn fine(&self, kind: &str, text: &str) {
        if self.troubles.lock().unwrap().remove(kind).is_some() {
            self.say(format!("🟢 {text}"));
        }
    }

    /// Something happened that's worth knowing, once.
    pub fn event(&self, text: String) {
        self.say(text);
    }

    /// A problem report from Orien Wallet, for the bot's private
    /// #app-reports channel. Its words are never written to the log.
    pub fn app_report(&self, id: i64, text: String) {
        eprintln!("problem report {id} from Orien Wallet");
        self.post_to("report", format!("problem report {id}"), text);
    }

    /// A newly shared theme, for the bot's private #new-themes channel.
    pub fn theme(&self, text: String) {
        self.post_to("theme", "a new theme".into(), text);
    }

    /// Hands `text` to the bot's inbox at `/path`, in the background.
    fn post_to(&self, path: &str, what: String, text: String) {
        let id = what;
        let Some(url) = self.url.as_ref().map(|url| format!("{}/{path}", url.trim_end_matches("/alert"))) else { return };
        std::thread::spawn(move || {
            let sent = ureq::post(&url)
                .config()
                .http_status_as_error(false)
                .timeout_global(Some(Duration::from_secs(20)))
                .build()
                .send_json(serde_json::json!({ "text": text }));
            match sent {
                Ok(response) if response.status().is_success() => {}
                Ok(response) => eprintln!("the bot did not post {id} ({})", response.status()),
                Err(error) => eprintln!("cannot reach the bot for {id}: {error}"),
            }
        });
    }

    /// Whether `kind` is in trouble now.
    pub fn in_trouble(&self, kind: &str) -> bool {
        self.troubles.lock().unwrap().contains_key(kind)
    }

    fn say(&self, text: String) {
        eprintln!("alert: {text}");
        let Some(url) = self.url.clone() else { return };
        // In the background: an alert never holds anything up.
        std::thread::spawn(move || {
            let sent = ureq::post(&url)
                .config()
                .http_status_as_error(false)
                .timeout_global(Some(Duration::from_secs(20)))
                .build()
                .send_json(serde_json::json!({ "text": text }));
            match sent {
                Ok(response) if response.status().is_success() => {}
                Ok(mut response) => {
                    let detail = response.body_mut().read_to_string().unwrap_or_default();
                    eprintln!("the bot did not post the alert ({}): {detail}", response.status());
                }
                Err(error) => eprintln!("cannot reach the bot for alerts: {error}"),
            }
        });
    }
}

/// Looks every five minutes at what can go wrong, and says so.
/// `openrouter_key` is for reading the credit left.
pub fn watch(app: Arc<crate::api::App>, openrouter_key: Option<String>) {
    if let Some(url) = app.alerts().url.clone() {
        let app = app.clone();
        std::thread::spawn(move || loop {
            match deliver_referral_alert(&app, &url) {
                Ok(true) => continue,
                Ok(false) => std::thread::sleep(Duration::from_secs(5)),
                Err(error) => {
                    eprintln!("could not deliver referral alert, retrying: {error:#}");
                    std::thread::sleep(Duration::from_secs(30));
                }
            }
        });
    }
    tokio::spawn(async move {
        let mut every = tokio::time::interval(Duration::from_secs(300));
        let mut misses: HashMap<String, u32> = HashMap::new();
        let mut last_credit: Option<Instant> = None;
        loop {
            every.tick().await;
            let credit_due = last_credit.is_none_or(|at| at.elapsed() >= HOUR);
            if credit_due {
                last_credit = Some(Instant::now());
            }
            let (app2, key, held) = (app.clone(), openrouter_key.clone(), std::mem::take(&mut misses));
            misses = tokio::task::spawn_blocking(move || {
                let mut misses = held;
                look_at_network(&app2, &mut misses);
                look_at_disk(app2.alerts());
                look_at_email(&app2);
                if credit_due && let Some(key) = &key {
                    look_at_credit(app2.alerts(), key);
                }
                misses
            })
            .await
            .unwrap_or_default();
        }
    });
}

/// The queue is written in the referral transaction. Release the database lock
/// before contacting Discord, and acknowledge only after the bot confirms delivery.
fn deliver_referral_alert(app: &crate::api::App, url: &str) -> anyhow::Result<bool> {
    let pending = app.store().next_referral_alert()?;
    let Some(alert) = pending else { return Ok(false) };
    let response = ureq::post(url)
        .config()
        .timeout_global(Some(Duration::from_secs(20)))
        .build()
        .send_json(serde_json::json!({ "text": alert.message(), "nonce": alert.nonce() }))?;
    anyhow::ensure!(response.status().is_success(), "bot returned {}", response.status());
    app.store().referral_alert_sent(&alert)?;
    Ok(true)
}

/// Counts a bad look at `kind`, or clears it; gives whether it has now been
/// bad `times` looks in a row.
fn missed(misses: &mut HashMap<String, u32>, kind: &str, bad: bool, times: u32) -> bool {
    if bad {
        let count = misses.entry(kind.to_string()).or_default();
        *count += 1;
        *count >= times
    } else {
        misses.remove(kind);
        false
    }
}

fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

/// A time of day in UTC, as 13:05 UTC.
fn clock(at: u64) -> String {
    format!("{:02}:{:02} UTC", (at % 86_400) / 3600, (at % 3600) / 60)
}

fn look_at_network(app: &crate::api::App, misses: &mut HashMap<String, u32>) {
    let alerts = app.alerts();
    let status = match app.payments().status() {
        Ok(status) => status,
        Err(error) => {
            if missed(misses, "network", true, 2) {
                alerts.trouble("network", format!("The Market server can't reach any validator.\nReason: {error:#}"));
            }
            return;
        }
    };
    missed(misses, "network", false, 2);
    alerts.fine("network", "The Market server reaches the validators again.");

    for v in &status.validators {
        let name = if v.name.is_empty() { v.endpoint.clone() } else { v.name.clone() };
        let down = format!("down {}", v.endpoint);
        if missed(misses, &down, !v.up, 2) {
            alerts.trouble(&down, format!("{name} isn't answering (for 10 minutes or more)."));
        } else if v.up {
            alerts.fine(&down, &format!("{name} is answering again."));
        }
        let behind = format!("behind {}", v.endpoint);
        if missed(misses, &behind, v.up && v.epoch < status.epoch, 3) {
            alerts.trouble(&behind, format!("{name} is behind: at checkpoint {}, the network at {}.", v.epoch, status.epoch));
        } else if v.up && v.epoch >= status.epoch {
            alerts.fine(&behind, &format!("{name} has caught up."));
        }
        if v.up && v.keys_total > 0 {
            let left = v.keys_total.saturating_sub(v.keys_used);
            let keys = format!("keys {}", v.endpoint);
            if left < FEW_KEYS {
                alerts.trouble_every(
                    &keys,
                    format!("{name} has {left} signing keys left of {}. It needs new keys before they run out.", v.keys_total),
                    DAY,
                );
            } else {
                alerts.fine(&keys, &format!("{name} has signing keys to spare again ({left} left)."));
            }
        }
    }

    // A checkpoint should close at the end of each epoch; half an hour late
    // is a stalled network.
    let due = status.epoch_started + status.epoch_seconds;
    let late = now().saturating_sub(due);
    if status.epoch_seconds > 0 && late > 1800 {
        alerts.trouble(
            "checkpoints",
            format!(
                "No checkpoint has closed for {} minutes: checkpoint {} was due at {}.",
                now().saturating_sub(status.epoch_started) / 60,
                status.epoch,
                clock(due)
            ),
        );
    } else {
        alerts.fine("checkpoints", "Checkpoints are closing again.");
    }
}

/// Free and total bytes of the server's disk, where df can say.
fn disk() -> Option<(u64, u64)> {
    let out = std::process::Command::new("df").args(["-B1", "--output=size,avail", "/"]).output().ok()?;
    let text = String::from_utf8(out.stdout).ok()?;
    let numbers: Vec<u64> = text.lines().nth(1)?.split_whitespace().filter_map(|n| n.parse().ok()).collect();
    Some((*numbers.get(1)?, *numbers.first()?))
}

fn look_at_disk(alerts: &Alerts) {
    let Some((free, total)) = disk() else { return };
    if total == 0 {
        return;
    }
    let gb = |bytes: u64| bytes as f64 / 1e9;
    if (free as f64) < total as f64 * LOW_DISK_SHARE || free < LOW_DISK_BYTES {
        alerts.trouble_every(
            "disk",
            format!("The server's disk is nearly full: {:.1} GB free of {:.0} GB.", gb(free), gb(total)),
            Duration::from_secs(6 * 3600),
        );
    } else {
        alerts.fine("disk", &format!("The server's disk has room again: {:.1} GB free.", gb(free)));
    }
}

fn look_at_email(app: &crate::api::App) {
    let alerts = app.alerts();
    let usage = app.mailer().usage();
    if usage.at == 0 {
        return;
    }
    let midnight = now() / 86_400 * 86_400;
    let today = if usage.at >= midnight { usage.daily.unwrap_or(0) } else { 0 };
    let (daily, monthly) = (crate::mail::DAILY_LIMIT, crate::mail::MONTHLY_LIMIT);
    let refused_today = usage.refused.as_ref().is_some_and(|r| r.why.starts_with("daily") && r.at >= midnight);
    if refused_today || today as f64 >= daily as f64 * NEAR_LIMIT {
        let text = if refused_today {
            format!("Resend's daily limit is used up ({daily} emails): sign-in codes can't be sent until midnight UTC.")
        } else {
            format!("Emails today: {today} of Resend's {daily}. At {daily}, sign-in codes stop until midnight UTC.")
        };
        alerts.trouble_every("email today", text, Duration::from_secs(4 * 3600));
    } else {
        alerts.fine("email today", "Emails are back under Resend's daily limit.");
    }
    let month = usage.monthly.unwrap_or(0);
    let refused_month = usage.refused.as_ref().is_some_and(|r| r.why.starts_with("monthly") && r.at >= usage.at);
    if refused_month || month as f64 >= monthly as f64 * NEAR_LIMIT {
        let text = if refused_month {
            format!("Resend's monthly limit is used up ({monthly} emails): no emails, sign-in codes included, can be sent until it resets or the plan is upgraded.")
        } else {
            format!("Emails this month: {month} of Resend's {monthly}. At {monthly}, sign-in codes stop until the month resets.")
        };
        alerts.trouble_every("email month", text, DAY);
    } else {
        alerts.fine("email month", "Emails are back under Resend's monthly limit.");
    }
}

/// The OpenRouter account's credit left, in dollars.
fn credit(key: &str) -> anyhow::Result<f64> {
    let mut response = ureq::get("https://openrouter.ai/api/v1/credits")
        .header("Authorization", &format!("Bearer {key}"))
        .config()
        .timeout_global(Some(Duration::from_secs(15)))
        .build()
        .call()?;
    let answer: serde_json::Value = serde_json::from_str(&response.body_mut().read_to_string()?)?;
    let total = answer["data"]["total_credits"].as_f64().ok_or_else(|| anyhow::anyhow!("no total_credits"))?;
    let used = answer["data"]["total_usage"].as_f64().ok_or_else(|| anyhow::anyhow!("no total_usage"))?;
    Ok(total - used)
}

fn look_at_credit(alerts: &Alerts, key: &str) {
    match credit(key) {
        Ok(left) if left < LOW_CREDIT => alerts.trouble_every(
            "credit",
            format!(
                "OpenRouter credit is low: ${left:.2} left. Content checks and the Discord bot stop when it runs out. Top up at https://openrouter.ai/settings/credits"
            ),
            Duration::from_secs(6 * 3600),
        ),
        Ok(left) => alerts.fine("credit", &format!("OpenRouter credit is topped up: ${left:.2} left.")),
        Err(error) => eprintln!("cannot read the OpenRouter credit: {error:#}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trouble_is_said_once_until_it_is_over_then_once_more() {
        let alerts = Alerts::to(None);
        assert!(!alerts.in_trouble("screening"));
        alerts.fine("screening", "fine");
        assert!(!alerts.in_trouble("screening"), "nothing to say when nothing was wrong");
        alerts.trouble("screening", "down".into());
        let first = alerts.troubles.lock().unwrap()["screening"];
        alerts.trouble("screening", "down".into());
        assert_eq!(alerts.troubles.lock().unwrap()["screening"], first, "not said again within the hour");
        alerts.fine("screening", "back");
        assert!(!alerts.in_trouble("screening"));
    }

    #[test]
    fn a_blip_is_let_pass() {
        let mut misses = HashMap::new();
        assert!(!missed(&mut misses, "down a", true, 2), "one miss is a blip");
        assert!(missed(&mut misses, "down a", true, 2), "two in a row are trouble");
        assert!(!missed(&mut misses, "down a", false, 2));
        assert!(!missed(&mut misses, "down a", true, 2), "counting starts again");
    }

    #[test]
    fn times_read_as_utc_clock() {
        assert_eq!(clock(13 * 3600 + 5 * 60 + 86_400 * 3), "13:05 UTC");
    }
}
