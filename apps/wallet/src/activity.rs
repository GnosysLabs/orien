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
            // Shown as sent, not received: `entry` gives it no kind.
            Income::Sent => Kind::Payment,
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
    /// The hour that just closed is not in the validators' history yet, so
    /// a rise in the balance may be its reward: wait for history rather
    /// than record it from the balance.
    #[serde(default)]
    waiting: bool,
    /// Lines worked out from the balance have been matched once against
    /// the whole of the validators' history.
    #[serde(default)]
    repaired: bool,
    /// Payments sent from another device that 1.6.0 showed three times
    /// (from the balance, from history, and a correction) are one line.
    #[serde(default)]
    collapsed: bool,
    /// The wallet's past from before this app kept a record of it has been
    /// read from history: a book started by a version before 1.6.0 began at
    /// the hour it was first opened.
    #[serde(default)]
    past_read: bool,
}

/// How far apart a line worked out from the balance and history's record
/// of the same payment can be, in seconds.
const SAME_PAYMENT_WITHIN: u64 = 3 * 60 * 60;

/// Whether a line was worked out from the balance alone.
fn from_balance(entry: &Entry) -> bool {
    entry.kind.is_none() && entry.order.is_none() && entry.counterparty.is_none() && entry.fee == 0
}

/// The line worked out from the balance for the payment history now
/// describes, if there is one: same direction, the same amount with the
/// fee, and recorded around the same time.
fn guessed<'a>(entries: &'a mut [Entry], line: &Entry, seen: u64) -> Option<&'a mut Entry> {
    let total = line.amount.saturating_add(line.fee);
    entries
        .iter_mut()
        .rev()
        .find(|e| e.direction == line.direction && from_balance(e) && e.amount == total && e.time.abs_diff(seen) <= SAME_PAYMENT_WITHIN)
}

/// Undoes what 1.6.0 did with a payment sent from another device: a Sent
/// line from the balance, history's Sent line for the same payment, and
/// often a Received line from the balance correcting the double count. One
/// line is left, history's, at the time the balance first showed it.
fn collapse(account: &mut Account) {
    let mut index = 0;
    while index < account.entries.len() + account.open.len() {
        let line = if index < account.entries.len() { &account.entries[index] } else { &account.open[index - account.entries.len()] };
        index += 1;
        if line.direction != Direction::Sent || line.order.is_none() {
            continue;
        }
        let (total, time) = (line.amount.saturating_add(line.fee), line.time);
        let near = |e: &Entry| e.amount == total && from_balance(e) && e.time.abs_diff(time) <= SAME_PAYMENT_WITHIN;
        let Some(guess) = account.entries.iter().position(|e| e.direction == Direction::Sent && near(e)) else { continue };
        let first_seen = account.entries[guess].time;
        account.entries.remove(guess);
        match account.entries.iter().position(|e| e.direction == Direction::Received && near(e)) {
            Some(correction) => {
                account.entries.remove(correction);
            }
            // Not corrected yet: the app expects the payment taken twice.
            None => {
                if let Some(expected) = &mut account.expected {
                    *expected += total as i128;
                }
                account.suspect = None;
            }
        }
        index = 0;
        if let Some(line) = account.entries.iter_mut().chain(account.open.iter_mut()).find(|e| e.direction == Direction::Sent && e.order.is_some() && e.amount.saturating_add(e.fee) == total && e.time == time) {
            line.time = first_seen.min(line.time);
        }
    }
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
pub fn sent(dir: &Path, address: &str, amount: u64, fee: u64, to: &str, seq: Option<u64>) {
    let mut book = load(dir);
    let account = book.0.entry(address.to_owned()).or_default();
    account.entries.push(Entry {
        direction: Direction::Sent,
        amount,
        fee,
        counterparty: Some(to.to_owned()),
        time: now(),
        kind: None,
        order: seq.map(|seq| format!("{address}/{seq}")),
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

/// An entry for what the validators' history says: received, or sent by
/// this account (from anywhere, this app or another), with the fee it paid.
fn entry(item: &Received, time: u64, fee_of: &dyn Fn(u64) -> u64) -> Entry {
    let order = item.order.map(|(from, seq)| format!("{from}/{seq}"));
    let sent = item.income == Income::Sent;
    Entry {
        direction: if sent { Direction::Sent } else { Direction::Received },
        amount: item.amount,
        fee: if sent { fee_of(item.amount) } else { 0 },
        counterparty: item.counterparty.map(|id| id.to_string()),
        time,
        kind: if sent { None } else { Some(item.income.into()) },
        order,
    }
}

/// What an entry did to the balance.
fn effect(entry: &Entry) -> i128 {
    match entry.direction {
        Direction::Received => entry.amount as i128,
        Direction::Sent => -(entry.amount as i128 + entry.fee as i128),
    }
}

/// What the validators' history said, from the epoch `history_from` gave
/// (or from the start, the first time), up to `epoch`, the one under way.
/// `unknown` lists the closed epochs it has not recorded yet. `fee_of` is
/// the fee a payment of an amount paid, for what this account sent.
pub fn heard(dir: &Path, address: &str, epoch: u64, unknown: &[u64], received: Vec<Received>, fee_of: &dyn Fn(u64) -> u64) {
    let mut book = load(dir);
    let account = book.0.entry(address.to_owned()).or_default();
    if !account.collapsed {
        collapse(account);
        account.collapsed = true;
    }
    // Just after a checkpoint, the validators may not have written the hour
    // it closed yet. Keep that hour (and later ones) open, to ask about it
    // again, rather than move past it and lose its entries. An older hour
    // still missing was never recorded: the balance explains it instead.
    let last_closed = epoch.saturating_sub(1);
    let cut = if unknown.contains(&last_closed) && last_closed > 0 { last_closed } else { epoch };
    account.waiting = cut < epoch;
    // What this app sent and recorded itself: history's record of the same
    // payment is the same line, not another.
    let known: Vec<String> = account.entries.iter().filter_map(|e| e.order.clone()).collect();
    let seen = now();
    if account.history.is_none() {
        // The first answer, for a wallet new to this app or restored: all of
        // history from the start, so its past shows here too. Closed hours
        // are already in the balance the app sees; the hour under way is kept
        // open, and counted on top of the balance the app last saw, as it
        // came in after it.
        for item in &received {
            let line = entry(item, item.time.unwrap_or(seen), fee_of);
            if line.order.as_ref().is_some_and(|o| known.contains(o)) {
                continue;
            }
            if item.epoch < cut && item.time.is_some() {
                account.entries.push(line);
            } else {
                account.open.push(line);
            }
        }
        let held: i128 = account.open.iter().map(effect).sum();
        if let Some(expected) = &mut account.expected {
            *expected += held;
        }
        account.history = Some(cut);
        account.past_read = true;
        save(dir, &book);
        return;
    }
    let old_open = std::mem::take(&mut account.open);
    let opened_at = |order: &Option<String>| {
        order.as_ref().and_then(|order| old_open.iter().find(|e| e.order.as_ref() == Some(order))).map(|e| e.time)
    };
    let mut added: i128 = -old_open.iter().map(effect).sum::<i128>();
    for item in received {
        let order = item.order.map(|(from, seq)| format!("{from}/{seq}"));
        if order.as_ref().is_some_and(|o| known.contains(o)) {
            continue;
        }
        // A payment seen while its hour was under way keeps the time it
        // was seen; the rest are dated by the checkpoint that closed them.
        let time = opened_at(&order).or(item.time).unwrap_or(seen);
        let line = entry(&item, time, fee_of);
        // Newly heard of, but the balance already showed it: it becomes
        // that line, at the time it was first seen. The balance counted it
        // already, so it changes nothing the app expects.
        if opened_at(&line.order).is_none()
            && let Some(guess) = guessed(&mut account.entries, &line, seen)
        {
            *guess = Entry { time: guess.time, ..line };
            continue;
        }
        added += effect(&line);
        if item.epoch < cut && item.time.is_some() {
            account.entries.push(line);
        } else {
            account.open.push(line);
        }
    }
    if let Some(expected) = &mut account.expected {
        *expected += added;
    }
    account.history = Some(account.history.map_or(cut, |from| from.max(cut)));
    save(dir, &book);
}

/// Whether the wallet's past, from before this app kept a record, still
/// needs reading from history.
pub fn needs_past(dir: &Path, address: &str) -> bool {
    load(dir).0.get(address).is_some_and(|account| account.history.is_some() && !account.past_read)
}

/// Adds what history holds from before the first line this app recorded:
/// the wallet's past, which the balance already includes, so nothing the
/// app expects changes. Nothing on or after that first line is touched, so
/// nothing shows twice. Done once per wallet.
pub fn past(dir: &Path, address: &str, history: Vec<Received>, fee_of: &dyn Fn(u64) -> u64) {
    let mut book = load(dir);
    let Some(account) = book.0.get_mut(address) else { return };
    let first = account.entries.iter().chain(&account.open).map(|e| e.time).min().unwrap_or(u64::MAX);
    let orders: Vec<String> = account.entries.iter().chain(&account.open).filter_map(|e| e.order.clone()).collect();
    let mut earlier: Vec<Entry> = history
        .iter()
        .filter_map(|item| item.time.filter(|time| *time < first).map(|time| entry(item, time, fee_of)))
        .filter(|line| !line.order.as_ref().is_some_and(|o| orders.contains(o)))
        .collect();
    earlier.append(&mut account.entries);
    account.entries = earlier;
    account.past_read = true;
    save(dir, &book);
}

/// Whether lines worked out from the balance still need matching against
/// the validators' history.
pub fn needs_repair(dir: &Path, address: &str) -> bool {
    load(dir).0.get(address).is_some_and(|account| {
        !account.repaired && account.entries.iter().any(|e| e.direction == Direction::Received && e.kind.is_none())
    })
}

/// Gives a line worked out from the balance what the validators' history
/// says it was, where history has an entry of exactly that amount: such as
/// a mining reward recorded before the app read history for its hour.
/// Amounts to the last unit don't match by chance. Each history entry
/// explains one line at most, and lines history already explains are left
/// as they are. Done once per wallet.
pub fn repair(dir: &Path, address: &str, history: Vec<Received>) {
    let mut book = load(dir);
    let Some(account) = book.0.get_mut(address) else { return };
    let explained: Vec<Option<String>> = account.entries.iter().filter(|e| e.kind.is_some()).map(|e| e.order.clone()).collect();
    let mut taken = vec![false; history.len()];
    // History entries already shown as their own lines are not used again.
    for (index, item) in history.iter().enumerate() {
        let order = item.order.map(|(from, seq)| format!("{from}/{seq}"));
        if order.is_some() && explained.contains(&order) {
            taken[index] = true;
        }
    }
    for entry in account.entries.iter_mut().filter(|e| e.direction == Direction::Received && e.kind.is_none()) {
        let found = history.iter().enumerate().find(|(index, item)| !taken[*index] && item.time.is_some() && item.amount == entry.amount);
        if let Some((index, item)) = found {
            taken[index] = true;
            entry.kind = Some(item.income.into());
            entry.counterparty = item.counterparty.map(|id| id.to_string());
            entry.order = item.order.map(|(from, seq)| format!("{from}/{seq}"));
        }
    }
    account.repaired = true;
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
            // Only for this look: `heard` sets it again on the next.
            if account.history.is_some() && std::mem::take(&mut account.waiting) {
                account.suspect = None;
                save(dir, &book);
                return;
            }
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

        sent(&dir, "me", 250, 5, "them", None); // this app sends 250 with a fee of 5
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

    #[test]
    fn the_reward_of_the_hour_history_starts_in_comes_from_history() {
        let dir = std::env::temp_dir().join(format!("orien-history-first-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let _ = fs::remove_file(dir.join("activity.json"));
        let lines = || list(&dir, "me").into_iter().map(|e| (e.direction, e.kind, e.amount)).collect::<Vec<_>>();

        // Before history: the balance, 1000.
        observe(&dir, "me", 1000);
        // The first answer, mid-epoch 16, with a payment of 25 under way.
        heard(&dir, "me", 16, &[], vec![got(16, false, Income::Payment, 25, Some(1))], &|_| 0);
        observe(&dir, "me", 1025);
        observe(&dir, "me", 1025);
        assert_eq!(lines(), vec![(Direction::Received, Some(Kind::Payment), 25)]);
        // Epoch 16 closes with a mining reward of 240: from history, labelled
        // so, with nothing from the balance and no phantom Sent.
        let closed = vec![got(16, true, Income::Payment, 25, Some(1)), got(16, true, Income::Mining, 240, None)];
        heard(&dir, "me", 17, &[], closed.clone(), &|_| 0);
        observe(&dir, "me", 1265);
        heard(&dir, "me", 17, &[], vec![], &|_| 0);
        observe(&dir, "me", 1265);
        let now = lines();
        assert_eq!(now.len(), 2, "{now:?}");
        assert!(now.contains(&(Direction::Received, Some(Kind::Mining), 240)));
        assert!(now.iter().all(|line| line.1.is_some() && line.0 == Direction::Received));
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn lines_from_the_balance_take_what_history_says_they_were() {
        let dir = std::env::temp_dir().join(format!("orien-history-repair-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let _ = fs::remove_file(dir.join("activity.json"));
        // Three lines from the balance: two rewards history knows, and one
        // from before it kept any.
        observe(&dir, "me", 1000);
        for balance in [1106, 1304, 1545] {
            observe(&dir, "me", balance);
        }
        assert!(needs_repair(&dir, "me"));
        repair(&dir, "me", vec![got(15, true, Income::Mining, 198, None), got(16, true, Income::Mining, 241, None)]);
        let lines: Vec<_> = list(&dir, "me").into_iter().map(|e| (e.kind, e.amount)).collect();
        assert_eq!(lines, vec![(Some(Kind::Mining), 241), (Some(Kind::Mining), 198), (None, 106)]);
        assert!(!needs_repair(&dir, "me"), "once");
        fs::remove_dir_all(&dir).unwrap();
    }

    fn sent_item(epoch: u64, amount: u64, seq: u64) -> Received {
        let me: orien_ledger::AccountId = orien_crypto::Hash32([1; 32]);
        let them: orien_ledger::AccountId = orien_crypto::Hash32([9; 32]);
        Received { epoch, time: Some(1_000 + epoch), income: Income::Sent, amount, counterparty: Some(them), order: Some((me, seq)) }
    }

    #[test]
    fn a_restored_wallet_shows_its_past_and_nothing_twice() {
        let dir = std::env::temp_dir().join(format!("orien-history-restore-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let _ = fs::remove_file(dir.join("activity.json"));
        let me = orien_crypto::Hash32([1; 32]).to_string();
        let fee = |amount: u64| amount / 1000;
        let lines = || list(&dir, &me).into_iter().map(|e| (e.direction, e.kind, e.amount)).collect::<Vec<_>>();

        // Restored: the first answer brings the whole past, received and sent.
        heard(&dir, &me, 20, &[], vec![got(12, true, Income::Mining, 500, None), sent_item(15, 100, 1)], &fee);
        observe(&dir, &me, 399_900);
        let now = lines();
        assert_eq!(now.len(), 2, "{now:?}");
        assert!(now.contains(&(Direction::Sent, None, 100)) && now.contains(&(Direction::Received, Some(Kind::Mining), 500)));

        // This app sends 40 as payment number 2; history then reports it.
        sent(&dir, &me, 40, 1, "them", Some(2));
        observe(&dir, &me, 399_859);
        heard(&dir, &me, 21, &[], vec![sent_item(20, 40, 2)], &fee);
        observe(&dir, &me, 399_859);
        assert_eq!(lines().iter().filter(|l| l.2 == 40).count(), 1, "one line for one payment");

        // Sent from elsewhere (the orien command): from history, as sent,
        // and nothing guessed from the balance.
        heard(&dir, &me, 22, &[], vec![sent_item(21, 1000, 3)], &fee);
        observe(&dir, &me, 399_859 - 1001);
        observe(&dir, &me, 399_859 - 1001);
        let now = lines();
        assert!(now.contains(&(Direction::Sent, None, 1000)));
        assert_eq!(now.len(), 4, "{now:?}");
        fs::remove_dir_all(&dir).unwrap();
    }

    /// One payment sent from another device, 100 with a fee of 1: the
    /// balance shows it before history does. One Sent line, to its payee,
    /// dated when the balance showed it, and no correction afterwards.
    #[test]
    fn a_payment_sent_elsewhere_is_one_line() {
        let dir = std::env::temp_dir().join(format!("orien-sent-elsewhere-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let _ = fs::remove_file(dir.join("activity.json"));
        let me = orien_crypto::Hash32([1; 32]).to_string();
        let fee = |amount: u64| amount / 100;
        let look = |received: Vec<Received>, epoch: u64, balance: i128| {
            heard(&dir, &me, epoch, &[], received, &fee);
            observe(&dir, &me, balance);
        };
        let open = |seq| Received { time: None, ..sent_item(5, 100, seq) };

        observe(&dir, &me, 1000);
        look(vec![], 5, 1000);
        // Sent elsewhere: the balance drops by 101 before history has it.
        look(vec![], 5, 899);
        look(vec![], 5, 899);
        let first = list(&dir, &me);
        assert_eq!(first.len(), 1);
        let when = first[0].time;
        // History catches up, while the hour is under way, then once it closes.
        for _ in 0..3 {
            look(vec![open(3)], 5, 899);
        }
        look(vec![sent_item(5, 100, 3)], 6, 899);
        look(vec![], 6, 899);
        let now = list(&dir, &me);
        assert_eq!(now.len(), 1, "one payment, one line");
        let line = &now[0];
        assert_eq!((line.direction, line.amount, line.fee), (Direction::Sent, 100, 1));
        assert!(line.counterparty.is_some() && line.order.is_some());
        assert_eq!(line.time, when);
        fs::remove_dir_all(&dir).unwrap();
    }

    /// A book 1.6.0 left with three lines for that payment becomes one.
    #[test]
    fn three_lines_from_1_6_0_become_one() {
        let dir = std::env::temp_dir().join(format!("orien-collapse-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let me = orien_crypto::Hash32([1; 32]).to_string();
        let guess = |direction, time| Entry { direction, amount: 101, fee: 0, counterparty: None, time, kind: None, order: None };
        let t = now();
        let from_history = Entry { time: t + 700, ..entry(&sent_item(5, 100, 3), 0, &|a| a / 100) };
        let mut book = Book::default();
        book.0.insert(
            me.clone(),
            Account {
                expected: Some(899),
                entries: vec![guess(Direction::Sent, t), from_history, guess(Direction::Received, t + 760)],
                history: Some(6),
                ..Default::default()
            },
        );
        save(&dir, &book);
        heard(&dir, &me, 6, &[], vec![], &|a| a / 100);
        observe(&dir, &me, 899);
        observe(&dir, &me, 899);
        let now = list(&dir, &me);
        assert_eq!(now.len(), 1);
        assert_eq!((now[0].direction, now[0].amount, now[0].time), (Direction::Sent, 100, t));
        fs::remove_dir_all(&dir).unwrap();
    }

    /// A record started before 1.6.0 began at the hour the wallet was first
    /// opened: its past is read once, and nothing shows twice.
    #[test]
    fn an_older_record_gets_the_wallets_past_once() {
        let dir = std::env::temp_dir().join(format!("orien-past-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let _ = fs::remove_file(dir.join("activity.json"));
        let mut book = Book::default();
        let mining = |epoch, amount| Entry { kind: Some(Kind::Mining), ..entry(&got(epoch, true, Income::Mining, amount, None), 1_000 + epoch, &|_| 0) };
        book.0.insert("me".into(), Account { expected: Some(5000), entries: vec![mining(10, 300), mining(11, 310)], history: Some(12), ..Default::default() });
        save(&dir, &book);
        assert!(needs_past(&dir, "me"));
        let all: Vec<Received> = (7..12).map(|epoch| got(epoch, true, Income::Mining, 290 + epoch, None)).collect();
        past(&dir, "me", all, &|_| 0);
        let amounts: Vec<u64> = list(&dir, "me").iter().map(|e| e.amount).collect();
        assert_eq!(amounts, vec![310, 300, 299, 298, 297], "epochs 7 to 9 added, 10 and 11 not twice");
        assert!(!needs_past(&dir, "me"));
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
                heard(&dir, "me", epoch, &[], received, &|_| 0);
            }
            observe(&dir, "me", balance);
        };
        let lines = || list(&dir, "me").into_iter().map(|e| (e.kind, e.amount)).collect::<Vec<_>>();

        // Before validators kept history: the balance explains it.
        look(None, 1000);
        look(None, 1100);
        assert_eq!(lines(), vec![(None, 100)]);

        // The first answer, in epoch 5 with a payment of 40 under way:
        // history starts with epoch 5, and the 40 is shown from it, once.
        look(Some((5, vec![got(5, false, Income::Payment, 40, Some(1))])), 1140);
        assert_eq!(history_from(&dir, "me"), Some(5));
        look(Some((5, vec![got(5, false, Income::Payment, 40, Some(1))])), 1140);
        assert_eq!(lines(), vec![(Some(Kind::Payment), 40), (None, 100)]);

        // Epoch 6 under way: epoch 5 closed, and a payment, shown at once.
        let epoch5 = got(5, true, Income::Payment, 40, Some(1));
        look(Some((6, vec![epoch5.clone(), got(6, false, Income::Payment, 30, Some(2))])), 1170);
        assert_eq!(lines()[0], (Some(Kind::Payment), 30));
        // Asked again in the same epoch: replaced, not doubled.
        look(Some((6, vec![got(6, false, Income::Payment, 30, Some(2))])), 1170);
        assert_eq!(lines().iter().filter(|line| line.1 == 40).count(), 1, "{:?}", lines());
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
        assert_eq!(now.iter().filter(|line| line.0 == Some(Kind::Payment)).count(), 2, "the 40 and the 30, once each");
        assert_eq!(history_from(&dir, "me"), Some(8));

        // A payment the balance shows before history does: not recorded
        // twice, once history catches up on the next look.
        look(Some((8, vec![])), 1201);
        look(Some((8, vec![got(8, false, Income::Payment, 10, Some(3))])), 1201);
        assert_eq!(lines().len(), 6);
        assert_eq!(lines()[0], (Some(Kind::Payment), 10));

        // Just after the checkpoint closing epoch 8, the balance has its
        // mining reward but history has not recorded the epoch yet: wait,
        // and ask about epoch 8 again rather than move past it.
        for _ in 0..3 {
            heard(&dir, "me", 9, &[8], vec![got(8, true, Income::Payment, 10, Some(3))], &|_| 0);
            observe(&dir, "me", 1221);
        }
        assert_eq!(lines().len(), 6, "nothing guessed from the balance");
        assert_eq!(history_from(&dir, "me"), Some(8));
        // Recorded now: one mining line, nothing from the balance.
        heard(&dir, "me", 9, &[], vec![got(8, true, Income::Payment, 10, Some(3)), got(8, true, Income::Mining, 20, None)], &|_| 0);
        observe(&dir, "me", 1221);
        observe(&dir, "me", 1221);
        let now = lines();
        assert_eq!(now.len(), 7);
        assert!(now.contains(&(Some(Kind::Mining), 20)));
        assert!(now.iter().all(|line| line.0.is_some() || line.1 == 40 || line.1 == 100));
        assert_eq!(history_from(&dir, "me"), Some(9));
        fs::remove_dir_all(&dir).unwrap();
    }
}
