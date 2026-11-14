//! The wallet's own record of payments, kept in `activity.json` in the
//! app's folder, one list per wallet address.
//!
//! Sent payments are recorded in full the moment they are final. Received
//! payments are worked out from the balance: when it rises by more than the
//! app can explain, the difference was received. Validators cannot yet say
//! who sent it ("payments to this account" is not a question they answer),
//! so those entries have no sender until that request exists.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Direction {
    Sent,
    Received,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Entry {
    pub direction: Direction,
    pub amount: u64,
    /// The fee this wallet paid, for payments it sent.
    #[serde(default)]
    pub fee: u64,
    /// The other side's address, where known.
    pub counterparty: Option<String>,
    /// Seconds since 1970.
    pub time: u64,
}

#[derive(Default, Serialize, Deserialize)]
struct Account {
    /// The balance the app last saw, adjusted for payments it has sent
    /// since. `None` until it has seen one.
    expected: Option<i128>,
    /// Oldest first.
    entries: Vec<Entry>,
}

#[derive(Default, Serialize, Deserialize)]
struct Book(BTreeMap<String, Account>);

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |since| since.as_secs())
}

fn load(dir: &Path) -> Book {
    fs::read(dir.join("activity.json")).ok().and_then(|bytes| serde_json::from_slice(&bytes).ok()).unwrap_or_default()
}

fn save(dir: &Path, book: &Book) {
    if let Ok(text) = serde_json::to_string_pretty(book) {
        let temporary = dir.join("activity.json.new");
        if fs::write(&temporary, text).is_ok() {
            let _ = fs::rename(temporary, dir.join("activity.json"));
        }
    }
}

/// Records a payment this app sent, once it is final.
pub fn sent(dir: &Path, address: &str, amount: u64, fee: u64, to: &str) {
    let mut book = load(dir);
    let account = book.0.entry(address.to_owned()).or_default();
    account.entries.push(Entry { direction: Direction::Sent, amount, fee, counterparty: Some(to.to_owned()), time: now() });
    if let Some(expected) = &mut account.expected {
        *expected -= amount as i128 + fee as i128;
    }
    save(dir, &book);
}

/// Compares a fresh balance with what the app expected, and records the
/// difference: more than expected was received; less was sent from
/// somewhere else, such as another copy of the wallet or the `orien` command.
pub fn observe(dir: &Path, address: &str, balance: i128) {
    let mut book = load(dir);
    let account = book.0.entry(address.to_owned()).or_default();
    if let Some(expected) = account.expected
        && balance != expected
    {
        let (direction, amount) = match balance > expected {
            true => (Direction::Received, balance - expected),
            false => (Direction::Sent, expected - balance),
        };
        account.entries.push(Entry {
            direction,
            amount: amount.min(u64::MAX as i128) as u64,
            fee: 0,
            counterparty: None,
            time: now(),
        });
    }
    account.expected = Some(balance);
    save(dir, &book);
}

/// Every recorded payment for an address, newest first.
pub fn list(dir: &Path, address: &str) -> Vec<Entry> {
    let mut entries = load(dir).0.remove(address).map(|account| account.entries).unwrap_or_default();
    entries.reverse();
    entries
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(dir: &Path) -> Vec<(String, u64, bool)> {
        list(dir, "me")
            .into_iter()
            .map(|e| (format!("{:?}", matches!(e.direction, Direction::Sent)), e.amount, e.counterparty.is_some()))
            .collect()
    }

    #[test]
    fn works_out_received_and_outside_payments_from_the_balance() {
        let dir = std::env::temp_dir().join(format!("orien-activity-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let _ = fs::remove_file(dir.join("activity.json"));

        observe(&dir, "me", 1000); // first sight: nothing to record
        assert!(list(&dir, "me").is_empty());

        sent(&dir, "me", 250, 5, "them"); // this app sends 250 with a fee of 5
        observe(&dir, "me", 745); // the balance agrees: nothing new
        observe(&dir, "me", 845); // 100 more than expected: received
        observe(&dir, "me", 795); // 50 less: sent from elsewhere
        observe(&dir, "me", 795);

        // Newest first: (is sent, amount, sender or recipient known)
        assert_eq!(
            kinds(&dir),
            vec![("true".into(), 50, false), ("false".into(), 100, false), ("true".into(), 250, true)]
        );
        assert!(list(&dir, "someone else").is_empty());
        fs::remove_dir_all(&dir).unwrap();
    }
}
