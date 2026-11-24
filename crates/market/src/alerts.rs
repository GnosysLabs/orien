//! Telling the founder when part of the network's services is in trouble:
//! the Discord bot posts each alert in its private #network-alerts channel.
//! Each kind of trouble is said once when it starts, again at most once an
//! hour while it lasts, and once more when it's over.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// How often the same trouble is said again while it lasts.
const AGAIN: Duration = Duration::from_secs(3600);

#[derive(Default)]
pub struct Alerts {
    /// The bot's inbox, such as http://127.0.0.1:7301/alert; none to only
    /// write alerts to the log.
    url: Option<String>,
    /// Each kind of trouble going on, and when it was last said.
    troubles: Mutex<HashMap<&'static str, Instant>>,
}

impl Alerts {
    pub fn to(url: Option<String>) -> Alerts {
        Alerts { url, troubles: Mutex::new(HashMap::new()) }
    }

    /// `kind` is in trouble: says `text` now if it's new, or if it was last
    /// said an hour ago.
    pub fn trouble(&self, kind: &'static str, text: String) {
        let mut troubles = self.troubles.lock().unwrap();
        let now = Instant::now();
        let due = troubles.get(kind).is_none_or(|said| now.duration_since(*said) >= AGAIN);
        if due {
            troubles.insert(kind, now);
            drop(troubles);
            self.say(format!("🔴 {text}"));
        }
    }

    /// `kind` works: says `text` if it was in trouble.
    pub fn fine(&self, kind: &'static str, text: &str) {
        if self.troubles.lock().unwrap().remove(kind).is_some() {
            self.say(format!("🟢 {text}"));
        }
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
}
