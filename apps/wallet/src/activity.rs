//! The wallet's own record of payments, kept in `activity.json` in the
//! app's folder, one list per wallet address.
//!
//! Sent payments are recorded in full the moment they are final. Received
//! coins come from the validators' history of the account, one line per
//! payment and one per hour's mining reward, fee share, refund or unstaked
//! coins. Validators keep that history only from the version that added it,
//! so anything history does not explain is worked out from the balance: when
//! it rises by more than the app can explain, the difference was received.

use orien_ledger::Income;
use orien_net::history::Received;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Direction {
    Sent,
    Received,
}

/// Where received coins came from, when the validators' history says.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Payment,
    Claim,
    Refund,
    Fees,
    Mining,
    Unstaked,
}

impl From<Income> for Kind {
    fn from(income: Income) -> Self {
        match income {
            Income::Payment => Kind::Payment,
            Income::Claim => Kind::Claim,
            Income::Refund => Kind::Refund,
            Income::Fees => Kind::Fees,
            Income::Mining => Kind::Mining,
            Income::Unstaked => Kind::Unstaked,
        }
    }
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
    /// For received coins the validators' history explains.
    #[serde(default)]
    pub kind: Option<Kind>,
    /// The order behind it, as "sender/number", to match an entry seen
    /// while its hour was under way with the same entry once it closed.
    #[serde(default)]
    pub order: Option<String>,
}

#[derive(Default, Serialize, Deserialize)]
struct Account {
    /// The balance the app can explain: the last one it saw, plus what
    /// history has added and less what this app has sent since. `None`
    /// until it has seen one.
    expected: Option<i128>,
    /// Oldest first.
    entries: Vec<Entry>,
    /// The epoch to ask the validators' history from next; `None` until
    /// a validator first answered.
    #[serde(default)]
    history: Option<u64>,
    /// History for the hour under way, replaced on every look until the
    /// hour closes and its entries move to `entries`.
    #[serde(default)]
    open: Vec<Entry>,
    /// A difference from the balance history has not explained yet, seen
    /// once. It is recorded when the next look sees the same difference,
    /// so a payment landing between the two questions is not counted twice.
    #[serde(default)]
    suspect: Option<i128>,
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
    account.entries.push(Entry {
        direction: Direction::Sent,
        amount,
        fee,
        counterparty: Some(to.to_owned()),
        time: now(),
        kind: None,
        order: None,
    });
    if let Some(expected) = &mut account.expected {
        *expected -= amount as i128 + fee as i128;
    }
    save(dir, &book);
}

/// The epoch to ask the validators' history from, if they have answered
/// before.
pub fn history_from(dir: &Path, address: &str) -> Option<u64> {
    load(dir).0.get(address).and_then(|account| account.history)
}

/// What the validators' history said, from the epoch `history_from` gave
/// (or from `epoch`, the first time), up to `epoch`, the one under way.
pub fn heard(dir: &Path, address: &str, epoch: u64, received: Vec<Received>) {
    let mut book = load(dir);
    let account = book.0.entry(address.to_owned()).or_default();
    if account.history.is_none() {
        // The first answer. What came in before now is already in the
        // balance the app last saw (or will see first), so history starts
        // with the next hour; until then the balance explains the rest.
        account.history = Some(epoch + 1);
        save(dir, &book);
        return;
    }
    let seen = now();
    let old_open = std::mem::take(&mut account.open);
    let opened_at = |order: &Option<String>| {
        order.as_ref().and_then(|order| old_open.iter().find(|e| e.order.as_ref() == Some(order))).map(|e| e.time)
    };
    let mut added: i128 = -old_open.iter().map(|e| e.amount as i128).sum::<i128>();
    for item in received {
        let order = item.order.map(|(from, seq)| format!("{from}/{seq}"));
        // A payment seen while its hour was under way keeps the time it
        // was seen; the rest are dated by the checkpoint that closed them.
        let time = opened_at(&order).or(item.time).unwrap_or(seen);
        let entry = Entry {
            direction: Direction::Received,
            amount: item.amount,
            fee: 0,
            counterparty: item.counterparty.map(|id| id.to_string()),
            time,
            kind: Some(item.income.into()),
            order,
        };
        added += item.amount as i128;
        if item.epoch < epoch && item.time.is_some() {
            account.entries.push(entry);
        } else {
            account.open.push(entry);
        }
    }
    if let Some(expected) = &mut account.expected {
        *expected += added;
    }
    account.history = Some(account.history.map_or(epoch, |from| from.max(epoch)));
    save(dir, &book);
}

/// Compares a fresh balance with what the app expected, and records the
/// difference: more than expected was received; less was sent from
/// somewhere else, such as another copy of the wallet or the `orien` command.
/// While the validators' history is in use, a difference is only recorded
/// once two looks in a row agree on it.
pub fn observe(dir: &Path, address: &str, balance: i128) {
    let mut book = load(dir);
    let account = book.0.entry(address.to_owned()).or_default();
    match account.expected {
        Some(expected) if balance != expected => {
            let difference = balance - expected;
            if account.history.is_some() && account.suspect != Some(difference) {
                account.suspect = Some(difference);
                save(dir, &book);
                return;
            }
            let (direction, amount) = match difference > 0 {
                true => (Direction::Received, difference),
                false => (Direction::Sent, -difference),
            };
            account.entries.push(Entry {
                direction,
                amount: amount.min(u64::MAX as i128) as u64,
                fee: 0,
                counterparty: None,
                time: now(),
                kind: None,
                order: None,
            });
        }
        _ => {}
    }
    account.suspect = None;
    account.expected = Some(balance);
    save(dir, &book);
}

/// Every recorded payment for an address, newest first.
pub fn list(dir: &Path, address: &str) -> Vec<Entry> {
    let mut entries: Vec<Entry> = load(dir)
        .0
        .remove(address)
        .map(|account| account.entries.into_iter().chain(account.open).collect())
        .unwrap_or_default();
    // Newest first; the sort is stable, so of entries with the same time
    // the one recorded last comes first.
    entries.reverse();
    entries.sort_by_key(|entry: &Entry| std::cmp::Reverse(entry.time));
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

    fn got(epoch: u64, closed: bool, income: Income, amount: u64, order: Option<u64>) -> Received {
        let payer: orien_ledger::AccountId = orien_crypto::Hash32([7; 32]);
        Received {
            epoch,
            time: closed.then_some(1_000 + epoch),
            income,
            amount,
            counterparty: order.map(|_| payer),
            order: order.map(|seq| (payer, seq)),
        }
    }

    #[test]
    fn one_line_per_payment_and_per_hours_reward_from_history() {
        let dir = std::env::temp_dir().join(format!("orien-history-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let _ = fs::remove_file(dir.join("activity.json"));
        let look = |history: Option<(u64, Vec<Received>)>, balance: i128| {
            if let Some((epoch, received)) = history {
                heard(&dir, "me", epoch, received);
            }
            observe(&dir, "me", balance);
        };
        let lines = || list(&dir, "me").into_iter().map(|e| (e.kind, e.amount)).collect::<Vec<_>>();

        // Before validators kept history: the balance explains it.
        look(None, 1000);
        look(None, 1100);
        assert_eq!(lines(), vec![(None, 100)]);

        // The first answer in epoch 5: history starts with epoch 6.
        look(Some((5, vec![got(5, false, Income::Payment, 40, Some(1))])), 1140);
        assert_eq!(history_from(&dir, "me"), Some(6));
        // The 40 of epoch 5 history skipped is a balance difference, seen
        // twice before it is recorded.
        assert_eq!(lines().len(), 1);
        look(Some((5, vec![])), 1140);
        assert_eq!(lines(), vec![(None, 40), (None, 100)]);

        // Epoch 6 under way: a payment, shown at once.
        look(Some((6, vec![got(6, false, Income::Payment, 30, Some(2))])), 1170);
        assert_eq!(lines()[0], (Some(Kind::Payment), 30));
        // Asked again in the same epoch: replaced, not doubled.
        look(Some((6, vec![got(6, false, Income::Payment, 30, Some(2))])), 1170);
        assert_eq!(lines().len(), 3);

        // Away for two hours: epochs 6 and 7 closed with mining rewards.
        look(
            Some((
                8,
                vec![
                    got(6, true, Income::Payment, 30, Some(2)),
                    got(6, true, Income::Mining, 12, None),
                    got(7, true, Income::Mining, 9, None),
                ],
            )),
            1191,
        );
        let now = lines();
        assert_eq!(now.len(), 5, "one line each, nothing guessed");
        assert!(now.contains(&(Some(Kind::Mining), 12)) && now.contains(&(Some(Kind::Mining), 9)));
        assert_eq!(now.iter().filter(|line| line.0 == Some(Kind::Payment)).count(), 1);
        assert_eq!(history_from(&dir, "me"), Some(8));

        // A payment the balance shows before history does: not recorded
        // twice, once history catches up on the next look.
        look(Some((8, vec![])), 1201);
        look(Some((8, vec![got(8, false, Income::Payment, 10, Some(3))])), 1201);
        assert_eq!(lines().len(), 6);
        assert_eq!(lines()[0], (Some(Kind::Payment), 10));
        fs::remove_dir_all(&dir).unwrap();
    }
}
