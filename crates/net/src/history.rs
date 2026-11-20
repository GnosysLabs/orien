//! Each account's incoming history, as this validator saw it: every payment,
//! claimed locked payment, refund, fee share, mining reward and release of
//! unstaked coins, by the epoch whose checkpoint made it final.
//!
//! This is a validator's own record, not something validators agree on.
//! It starts when a validator first runs a version that keeps it, and has a
//! gap for any epoch the validator caught up on by fetching a state instead
//! of working it out. A wallet asks several validators and takes the
//! fullest answer.

use crate::store::Journal;
use borsh::{BorshDeserialize, BorshSerialize};
use orien_ledger::{AccountId, Credit, Income};
use std::collections::{BTreeSet, HashMap};
use std::io;
use std::path::Path;

/// The most entries one answer carries, beyond the rest of the epoch it
/// stopped in.
pub const MAX_ENTRIES: u32 = 1000;

/// Coins that came into an account.
#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct Received {
    pub epoch: u64,
    /// When the checkpoint closing `epoch` was made, in seconds since 1970.
    /// None for the epoch under way, whose payments are final but whose
    /// checkpoint is still to come.
    pub time: Option<u64>,
    pub income: Income,
    pub amount: u64,
    /// The payer of a payment, the sender of a claimed locked payment, the
    /// payee of a refunded one.
    pub counterparty: Option<AccountId>,
    /// The order behind it, by sender and number.
    pub order: Option<(AccountId, u64)>,
}

/// One page of an account's history.
#[derive(Clone, Debug, Default, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct HistoryPage {
    /// The epoch the answering validator is in.
    pub epoch: u64,
    /// The first epoch this validator kept history for. It knows nothing
    /// about earlier ones.
    pub since: u64,
    /// Epochs in the range asked for that this validator has no record of.
    pub unknown: Vec<u64>,
    /// Oldest first.
    pub entries: Vec<Received>,
    /// Where to ask from for the rest, if there is more.
    pub next: Option<u64>,
}

#[derive(BorshSerialize, BorshDeserialize)]
enum Record {
    /// History is kept from this epoch on.
    Start { epoch: u64 },
    /// What the checkpoint closing `epoch` credited, or None if this
    /// validator took the epoch's outcome on trust and does not know.
    Closed { epoch: u64, time: u64, credits: Option<Vec<Credit>> },
}

/// One entry as it is kept in memory.
#[derive(Clone)]
struct Entry {
    epoch: u64,
    time: u64,
    income: Income,
    amount: u64,
    counterparty: Option<AccountId>,
    order: Option<(AccountId, u64)>,
}

pub struct History {
    journal: Journal,
    since: u64,
    /// The last epoch recorded, if any.
    last: Option<u64>,
    unknown: BTreeSet<u64>,
    /// Per account, oldest first.
    accounts: HashMap<AccountId, Vec<Entry>>,
}

impl History {
    /// Opens the history file, starting one at `epoch` if there is none.
    pub fn open(path: &Path, epoch: u64, durable: bool) -> io::Result<History> {
        let (journal, records) = Journal::open(path, durable)?;
        let mut history =
            History { journal, since: epoch, last: None, unknown: BTreeSet::new(), accounts: HashMap::new() };
        let mut started = false;
        for bytes in records {
            match borsh::from_slice::<Record>(&bytes) {
                Ok(Record::Start { epoch }) if !started => {
                    history.since = epoch;
                    started = true;
                }
                Ok(Record::Closed { epoch, time, credits }) => history.take(epoch, time, credits),
                // A record this version cannot read is skipped: history is
                // only ever a convenience.
                _ => {}
            }
        }
        if !started {
            history.journal.append(&borsh::to_vec(&Record::Start { epoch }).expect("writing to memory cannot fail"))?;
            history.journal.sync()?;
        }
        Ok(history)
    }

    /// Records what the checkpoint closing `epoch` credited, at `time`; None
    /// when this validator does not know.
    pub fn record(&mut self, epoch: u64, time: u64, credits: Option<Vec<Credit>>) -> io::Result<()> {
        if epoch < self.since {
            return Ok(());
        }
        let record = Record::Closed { epoch, time, credits };
        self.journal.append(&borsh::to_vec(&record).expect("writing to memory cannot fail"))?;
        let Record::Closed { epoch, time, credits } = record else { unreachable!() };
        self.take(epoch, time, credits);
        Ok(())
    }

    fn take(&mut self, epoch: u64, time: u64, credits: Option<Vec<Credit>>) {
        if epoch < self.since {
            return;
        }
        // A validator that stopped between writing an epoch's record and
        // moving past it writes the record again: the later one stands.
        if self.last.is_some_and(|last| epoch <= last) {
            for entries in self.accounts.values_mut() {
                entries.retain(|entry| entry.epoch < epoch);
            }
            self.unknown.retain(|unknown| *unknown < epoch);
        }
        // Epochs skipped without a record are unknown too.
        let first = self.last.map_or(self.since, |last| last + 1).min(epoch);
        self.unknown.extend(first..epoch);
        self.last = Some(epoch);
        match credits {
            None => {
                self.unknown.insert(epoch);
            }
            Some(credits) => {
                for credit in credits {
                    let entry = Entry {
                        epoch,
                        time,
                        income: credit.income,
                        amount: credit.amount,
                        counterparty: credit.counterparty,
                        order: credit.order,
                    };
                    self.accounts.entry(credit.account).or_default().push(entry);
                }
            }
        }
    }

    /// The account's history from `from` on: at most `max` entries, but
    /// always whole epochs. `current` is the epoch under way, whose final
    /// payments so far are given in `pending`.
    /// `sent` includes what the account sent, for wallets that ask for it;
    /// older wallets know only what comes in.
    pub fn page(&self, account: &AccountId, from: u64, max: u32, current: u64, pending: Vec<Received>, sent: bool) -> HistoryPage {
        let max = max.clamp(1, MAX_ENTRIES) as usize;
        let mut page = HistoryPage { epoch: current, since: self.since, ..HistoryPage::default() };
        let kept = self.accounts.get(account).map_or(&[][..], Vec::as_slice);
        let start = kept.partition_point(|entry| entry.epoch < from);
        let mut stopped = None;
        for entry in kept[start..].iter().filter(|entry| sent || entry.income.incoming()) {
            if page.entries.len() >= max && page.entries.last().is_some_and(|last| last.epoch != entry.epoch) {
                stopped = Some(entry.epoch);
                break;
            }
            page.entries.push(Received {
                epoch: entry.epoch,
                time: Some(entry.time),
                income: entry.income,
                amount: entry.amount,
                counterparty: entry.counterparty,
                order: entry.order,
            });
        }
        let until = stopped.unwrap_or(current);
        // Closed epochs this validator never recorded, past its last record,
        // are unknown too, until it records them.
        let recorded_to = self.last.map_or(self.since, |last| last + 1);
        // A wallet may already know a later epoch than this validator has
        // reached (it asked another one), so `from` can be past `until`.
        if from < until {
            page.unknown = self.unknown.range(from..until).copied().chain(recorded_to.max(from)..until).collect();
        }
        match stopped {
            Some(epoch) => page.next = Some(epoch),
            None if from <= current => page.entries.extend(pending.into_iter().filter(|e| sent || e.income.incoming())),
            None => {}
        }
        page
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use orien_crypto::Hash32;

    fn credit(account: u8, income: Income, amount: u64) -> Credit {
        Credit { account: Hash32([account; 32]), income, amount, counterparty: None, order: None }
    }

    #[test]
    fn asking_from_past_the_current_epoch_is_answered_not_a_crash() {
        let dir = folder("ahead");
        let mut history = History::open(&dir.join("history.log"), 5, false).unwrap();
        history.record(5, 100, Some(vec![credit(7, Income::Mining, 10)])).unwrap();
        let alice = Hash32([7; 32]);
        for from in [7, 8, 1_000, u64::MAX] {
            let page = history.page(&alice, from, 100, 6, Vec::new(), false);
            assert!(page.entries.is_empty() && page.unknown.is_empty());
        }
    }

    #[test]
    fn what_was_sent_shows_only_to_those_who_ask() {
        let dir = folder("sent");
        let mut history = History::open(&dir.join("history.log"), 5, false).unwrap();
        history.record(5, 100, Some(vec![credit(7, Income::Sent, 40), credit(7, Income::Mining, 10)])).unwrap();
        let alice = Hash32([7; 32]);
        let incoming = history.page(&alice, 0, 100, 6, Vec::new(), false);
        assert_eq!(incoming.entries.iter().map(|e| e.income).collect::<Vec<_>>(), vec![Income::Mining]);
        let all = history.page(&alice, 0, 100, 6, Vec::new(), true);
        assert_eq!(all.entries.len(), 2);
        assert!(all.entries.iter().any(|e| e.income == Income::Sent && e.amount == 40));
    }

    fn folder(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("orien-history-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn keeps_each_accounts_credits_by_epoch_across_a_restart() {
        let dir = folder("restart");
        let path = dir.join("history.log");
        let alice = Hash32([7; 32]);
        {
            let mut history = History::open(&path, 5, false).unwrap();
            history.record(5, 100, Some(vec![credit(7, Income::Mining, 10), credit(8, Income::Fees, 3)])).unwrap();
            history.record(6, 200, None).unwrap();
        }
        let mut history = History::open(&path, 9, false).unwrap();
        // Written twice, as after a crash: the later record stands.
        history.record(7, 300, Some(vec![credit(7, Income::Payment, 1)])).unwrap();
        history.record(7, 301, Some(vec![credit(7, Income::Payment, 2)])).unwrap();
        let page = history.page(&alice, 0, 100, 8, Vec::new(), false);
        assert_eq!(page.since, 5);
        assert_eq!(page.unknown, vec![6]);
        let got: Vec<_> = page.entries.iter().map(|e| (e.epoch, e.time, e.amount)).collect();
        assert_eq!(got, vec![(5, Some(100), 10), (7, Some(301), 2)]);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn pages_end_on_whole_epochs_and_say_where_to_go_on() {
        let dir = folder("pages");
        let mut history = History::open(&dir.join("history.log"), 0, false).unwrap();
        for epoch in 0..4 {
            history.record(epoch, epoch * 10, Some(vec![credit(7, Income::Mining, 1), credit(7, Income::Payment, 2)])).unwrap();
        }
        let alice = Hash32([7; 32]);
        let pending = vec![Received { epoch: 4, time: None, income: Income::Payment, amount: 5, counterparty: None, order: None }];
        let first = history.page(&alice, 0, 3, 4, pending.clone(), false);
        assert_eq!(first.entries.len(), 4);
        assert_eq!(first.next, Some(2));
        let rest = history.page(&alice, 2, 3, 4, pending, false);
        assert_eq!(rest.entries.len(), 5);
        assert_eq!(rest.next, None);
        assert_eq!(rest.entries.last().unwrap().time, None);
        assert!(rest.unknown.is_empty());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
