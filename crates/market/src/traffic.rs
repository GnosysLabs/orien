//! The website's own traffic counts, for the dashboard, in the manner of
//! Plausible: no cookies, nothing personal kept, no one else involved.
//!
//! Each page sends a small ping (POST /api/market/t). A visitor is a hash of
//! their address and browser with a random salt that lasts one day (UTC), so
//! nobody can be followed from one day to the next and no address is kept.
//! When a day ends its salt and hashes are dropped; only the counts remain,
//! one file a day in the traffic folder beside the database.

use crate::admin::date;
use crate::store::now;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Most different pages, sources or downloads one day keeps; the rest are "other".
const MOST_KEYS: usize = 500;
/// Pings one address may send in a minute; more are dropped.
const PINGS_PER_MINUTE: u32 = 120;
/// A visitor seen this recently is "active now".
const ACTIVE: u64 = 5 * 60;

/// One day's counts. `salt` and `seen` exist only while the day is open.
#[derive(Clone, Default, Serialize, Deserialize)]
pub struct Day {
    pub date: String,
    pub pageviews: u64,
    pub visitors: u64,
    pub downloads: u64,
    pub paths: BTreeMap<String, u64>,
    pub referrers: BTreeMap<String, u64>,
    pub countries: BTreeMap<String, u64>,
    pub devices: BTreeMap<String, u64>,
    pub files: BTreeMap<String, u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    salt: Option<String>,
    /// Visitor hash, and when it was last seen (seconds since 1970).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    seen: Option<HashMap<String, u64>>,
}

impl Day {
    fn open(date: String) -> Day {
        let mut salt = [0u8; 16];
        getrandom::fill(&mut salt).expect("the system makes random numbers");
        Day { date, salt: Some(hex::encode(salt)), seen: Some(HashMap::new()), ..Day::default() }
    }

    /// The counts alone, as kept once the day is over.
    fn closed(&self) -> Day {
        Day { salt: None, seen: None, ..self.clone() }
    }
}

/// What a page sent, and who sent it, as the request says.
pub struct Ping {
    /// "view" or "download".
    pub kind: String,
    /// The page, or the file downloaded.
    pub path: String,
    pub referrer: String,
    pub address: String,
    pub browser: String,
    pub country: Option<String>,
    pub host: String,
}

struct Inner {
    day: Day,
    dirty: bool,
    saved: Instant,
    /// Pings per address this minute.
    pings: HashMap<String, u32>,
    minute: Instant,
}

pub struct Traffic {
    /// Where the days are kept; none keeps them in memory only (tests).
    dir: Option<PathBuf>,
    inner: Mutex<Inner>,
}

#[derive(Serialize)]
pub struct Stats {
    pub active_now: usize,
    /// The last days, oldest first, today last.
    pub days: Vec<Day>,
}

fn today() -> String {
    date(now() / 86_400)
}

fn bump(counts: &mut BTreeMap<String, u64>, key: &str) {
    let key = if !counts.contains_key(key) && counts.len() >= MOST_KEYS { "other" } else { key };
    *counts.entry(key.to_string()).or_default() += 1;
}

/// Link previews, crawlers and scripts, which are not people.
fn is_bot(browser: &str) -> bool {
    let b = browser.to_ascii_lowercase();
    browser.is_empty()
        || [
            "bot", "crawl", "spider", "slurp", "headless", "preview", "facebookexternalhit", "embedly", "monitor", "curl", "wget",
            "python", "axios", "node-fetch", "go-http",
        ]
        .iter()
        .any(|word| b.contains(word))
}

fn device(browser: &str) -> &'static str {
    let b = browser.to_ascii_lowercase();
    if b.contains("ipad") || b.contains("tablet") {
        "Tablet"
    } else if b.contains("mobi") || b.contains("android") || b.contains("iphone") {
        "Phone"
    } else {
        "Computer"
    }
}

/// Where a visitor came from: another site's name, or "Direct".
fn source(referrer: &str, host: &str) -> String {
    let bare = |h: &str| h.trim_start_matches("www.").to_ascii_lowercase();
    match url::Url::parse(referrer).ok().and_then(|u| u.host_str().map(bare)) {
        Some(from) if from != bare(host) => from,
        _ => "Direct".into(),
    }
}

impl Traffic {
    /// Counts kept in `dir`, picking up today's if the server restarted.
    pub fn open(dir: PathBuf) -> Traffic {
        let _ = std::fs::create_dir_all(&dir);
        let mut traffic = Traffic::in_memory();
        let date = today();
        if let Some(day) = read(&dir, &date).filter(|day| day.salt.is_some()) {
            traffic.inner.get_mut().unwrap().day = day;
        }
        traffic.dir = Some(dir);
        traffic
    }

    pub fn in_memory() -> Traffic {
        Traffic {
            dir: None,
            inner: Mutex::new(Inner {
                day: Day::open(today()),
                dirty: false,
                saved: Instant::now(),
                pings: HashMap::new(),
                minute: Instant::now(),
            }),
        }
    }

    /// Counts a ping from a page. Anything odd is dropped without a word.
    pub fn record(&self, ping: Ping) {
        if is_bot(&ping.browser) || !ping.path.starts_with('/') {
            return;
        }
        let mut inner = self.inner.lock().unwrap();
        if inner.minute.elapsed() > Duration::from_secs(60) {
            inner.pings.clear();
            inner.minute = Instant::now();
        }
        let pings = inner.pings.entry(ping.address.clone()).or_default();
        *pings += 1;
        if *pings > PINGS_PER_MINUTE {
            return;
        }
        self.roll(&mut inner);
        let day = &mut inner.day;
        let path: String = ping.path.split(['?', '#']).next().unwrap_or("/").chars().take(80).collect();
        match ping.kind.as_str() {
            "view" => {
                let visitor = blake3::hash(format!("{}|{}|{}", day.salt.as_deref().unwrap_or(""), ping.address, ping.browser).as_bytes())
                    .to_hex()[..20]
                    .to_string();
                let seen = day.seen.get_or_insert_with(HashMap::new);
                let first = seen.insert(visitor, now()).is_none();
                day.pageviews += 1;
                bump(&mut day.paths, &path);
                if first {
                    day.visitors += 1;
                    bump(&mut day.referrers, &source(&ping.referrer, &ping.host));
                    let country = ping.country.filter(|c| c.len() == 2 && c != "XX").map(|c| c.to_ascii_uppercase());
                    bump(&mut day.countries, country.as_deref().unwrap_or("??"));
                    bump(&mut day.devices, device(&ping.browser));
                }
            }
            "download" => {
                day.downloads += 1;
                bump(&mut day.files, path.rsplit('/').next().unwrap_or(&path));
            }
            _ => return,
        }
        inner.dirty = true;
        if inner.saved.elapsed() > Duration::from_secs(30) {
            self.save(&mut inner);
        }
    }

    /// The last `days` days, today included, and who is here now.
    pub fn stats(&self, days: u64) -> Stats {
        let mut inner = self.inner.lock().unwrap();
        self.roll(&mut inner);
        self.save(&mut inner);
        let today = now() / 86_400;
        let list = (0..days)
            .rev()
            .map(|back| {
                let date = date(today - back);
                if date == inner.day.date {
                    inner.day.closed()
                } else {
                    self.dir.as_ref().and_then(|dir| read(dir, &date)).map(|d| d.closed()).unwrap_or(Day { date, ..Day::default() })
                }
            })
            .collect();
        let recent = now().saturating_sub(ACTIVE);
        let active_now = inner.day.seen.as_ref().map_or(0, |seen| seen.values().filter(|&&at| at >= recent).count());
        Stats { active_now, days: list }
    }

    /// A new day: the old one is kept as counts only, and a fresh one begins.
    fn roll(&self, inner: &mut Inner) {
        let date = today();
        if inner.day.date != date {
            if let Some(dir) = &self.dir {
                write(dir, &inner.day.closed());
            }
            inner.day = Day::open(date);
            inner.dirty = true;
        }
    }

    fn save(&self, inner: &mut Inner) {
        if let (Some(dir), true) = (&self.dir, inner.dirty) {
            write(dir, &inner.day);
        }
        inner.dirty = false;
        inner.saved = Instant::now();
    }
}

fn read(dir: &std::path::Path, date: &str) -> Option<Day> {
    serde_json::from_slice(&std::fs::read(dir.join(format!("{date}.json"))).ok()?).ok()
}

fn write(dir: &std::path::Path, day: &Day) {
    let file = dir.join(format!("{}.json", day.date));
    let temporary = file.with_extension("json.tmp");
    if let Ok(bytes) = serde_json::to_vec(day)
        && std::fs::write(&temporary, bytes).is_ok()
    {
        let _ = std::fs::rename(&temporary, &file);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ping(kind: &str, path: &str, address: &str, browser: &str) -> Ping {
        Ping {
            kind: kind.into(),
            path: path.into(),
            referrer: "https://x.com/OrienNetwork".into(),
            address: address.into(),
            browser: browser.into(),
            country: Some("us".into()),
            host: "orien.network".into(),
        }
    }

    #[test]
    fn counts_views_visitors_downloads_and_leaves_bots_out() {
        let traffic = Traffic::in_memory();
        let mac = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) Safari/605";
        let phone = "Mozilla/5.0 (iPhone; CPU iPhone OS 18_0) Mobile Safari";
        traffic.record(ping("view", "/", "1.2.3.4", mac));
        traffic.record(ping("view", "/try/?x=1", "1.2.3.4", mac));
        traffic.record(ping("view", "/", "5.6.7.8", phone));
        traffic.record(ping("view", "/", "9.9.9.9", "Twitterbot/1.0"));
        traffic.record(ping("download", "/wallet/Orien-Wallet.dmg", "1.2.3.4", mac));
        traffic.record(ping("view", "not a path", "1.2.3.4", mac));
        let stats = traffic.stats(7);
        assert_eq!(stats.days.len(), 7);
        let day = stats.days.last().unwrap();
        assert_eq!((day.pageviews, day.visitors, day.downloads), (3, 2, 1));
        assert_eq!(day.paths["/"], 2);
        assert_eq!(day.paths["/try/"], 1);
        assert_eq!(day.referrers["x.com"], 2);
        assert_eq!(day.countries["US"], 2);
        assert_eq!((day.devices["Computer"], day.devices["Phone"]), (1, 1));
        assert_eq!(day.files["Orien-Wallet.dmg"], 1);
        assert_eq!(stats.active_now, 2);
        assert!(day.salt.is_none() && day.seen.is_none(), "no hashes leave the server");
    }

    #[test]
    fn a_flood_from_one_address_is_cut_off() {
        let traffic = Traffic::in_memory();
        for _ in 0..500 {
            traffic.record(ping("view", "/", "1.2.3.4", "Mozilla/5.0 Firefox"));
        }
        assert_eq!(traffic.stats(1).days[0].pageviews, u64::from(PINGS_PER_MINUTE));
    }

    #[test]
    fn days_are_kept_as_counts_in_their_folder() {
        let mut random = [0u8; 8];
        getrandom::fill(&mut random).unwrap();
        let dir = std::env::temp_dir().join(format!("orien-traffic-test-{}", hex::encode(random)));
        let traffic = Traffic::open(dir.clone());
        traffic.record(ping("view", "/", "1.2.3.4", "Mozilla/5.0 Firefox"));
        traffic.stats(1);
        let again = Traffic::open(dir.clone());
        assert_eq!(again.stats(1).days[0].pageviews, 1, "picked up after a restart");
        let _ = std::fs::remove_dir_all(dir);
    }
}
