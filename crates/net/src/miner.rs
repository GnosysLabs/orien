//! Proof-of-work for an account, as `orien mine` and Orien Wallet do it: a
//! few seconds of searching at a time on several threads, each share found
//! handed to every validator, with the epoch's seed and target taken from
//! checkpoints the client checks itself.

use crate::client::Client;
use orien_crypto::Hash32;
use orien_ledger::AccountId;
use orien_ledger::close::Share;
use orien_ledger::work::{Math, Prepared};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::thread;
use std::time::{Duration, Instant};

/// How long one round of searching lasts before the epoch is checked again.
const ROUND: Duration = Duration::from_secs(5);

/// The processor cores this computer has.
pub fn cores() -> usize {
    thread::available_parallelism().map_or(1, |n| n.get())
}

/// Threads for mining on `percent` of the processor: whole cores, at least
/// one, since a share of a core would only slow it down.
pub fn threads_for(percent: u8) -> usize {
    let cores = cores();
    ((cores * percent as usize + 50) / 100).clamp(1, cores)
}

/// What mining reports as it goes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Event {
    /// Mining for a new epoch, at this target: about one try in
    /// `u64::MAX / target` makes a share.
    Epoch { epoch: u64, target: u64 },
    /// Getting RandomX ready for a new epoch: a few seconds to a minute,
    /// once an hour, with 2 GB of memory.
    Preparing { epoch: u64 },
    /// RandomX is ready. `light` when this computer lacked the memory for
    /// the full dataset and mines several times slower; `large_pages` when
    /// the dataset sits in large pages, which mines faster.
    Prepared { epoch: u64, light: bool, large_pages: bool },
    /// A round of searching ended.
    Round { tries_per_second: f64, sent: u64, taken: u64 },
}

/// Mines for `account` on `threads` threads until `stop` is set, telling
/// `report` as it goes. Stops within about a fifth of a second.
pub fn run(client: &Client, account: AccountId, threads: usize, stop: &AtomicBool, report: impl FnMut(&Client, Event)) {
    run_adjustable(client, account, &AtomicUsize::new(threads), stop, report)
}

/// The threads searching now, for one epoch's work at one target.
struct Workers {
    seed: Hash32,
    target: u64,
    threads: usize,
    halt: Arc<AtomicBool>,
    tries: Arc<AtomicU64>,
    handles: Vec<thread::JoinHandle<()>>,
    shares: std::sync::mpsc::Receiver<Share>,
}

impl Workers {
    /// Starts `threads` threads searching from nonce `next`, each taking
    /// every `threads`th nonce.
    fn start(ready: &Arc<Prepared>, account: AccountId, seed: Hash32, target: u64, threads: usize, next: u64) -> Workers {
        let batch = ready.batch();
        let halt = Arc::new(AtomicBool::new(false));
        let tries = Arc::new(AtomicU64::new(0));
        let (found, shares) = std::sync::mpsc::channel();
        let handles = (0..threads as u64)
            .map(|offset| {
                let (halt, tries, found, ready) = (halt.clone(), tries.clone(), found.clone(), ready.clone());
                let start = next.wrapping_add(offset);
                thread::spawn(move || {
                    let hasher = ready.hasher();
                    let mut nonce = start;
                    while !halt.load(Ordering::Relaxed) {
                        for _ in 0..batch {
                            let share = Share { account, nonce };
                            if hasher.number(&share) < target && found.send(share).is_err() {
                                return;
                            }
                            nonce = nonce.wrapping_add(threads as u64);
                        }
                        tries.fetch_add(batch, Ordering::Relaxed);
                    }
                })
            })
            .collect();
        Workers { seed, target, threads, halt, tries, handles, shares }
    }

    /// Stops the threads, and gives the nonce to carry on from and any
    /// shares they found last.
    fn stop(self, next: u64, batch: u64) -> (u64, Vec<Share>) {
        self.halt.store(true, Ordering::Relaxed);
        for handle in self.handles {
            let _ = handle.join();
        }
        let left = self.shares.try_iter().collect();
        (next.wrapping_add(self.tries.load(Ordering::Relaxed) + self.threads as u64 * batch), left)
    }
}

/// `run`, on as many threads as `threads` says: changing it takes effect
/// within a few seconds, and keeps everything RandomX has prepared for the
/// epoch. The threads keep searching while the epoch is checked, and start
/// again only when the work or their number changes.
pub fn run_adjustable(
    client: &Client,
    account: AccountId,
    threads: &AtomicUsize,
    stop: &AtomicBool,
    mut report: impl FnMut(&Client, Event),
) {
    // Two machines mining for one account must not find the same shares.
    let mut next = u64::from_le_bytes(crate::keys::random_seed()[..8].try_into().expect("8 bytes"));
    let mut shown_epoch = None;
    let mut prepared: Option<(Hash32, Arc<Prepared>)> = None;
    let mut workers: Option<Workers> = None;
    let submit = |share: Share, sent: &mut u64, taken: &mut u64| {
        *sent += 1;
        if client.submit_share(share) > 0 {
            *taken += 1;
        }
    };
    while !stop.load(Ordering::Relaxed) {
        let (mut sent, mut taken) = (0u64, 0u64);
        // The seed and target come from the checkpoints this client checks
        // itself, so nobody can send it off doing worthless work. The
        // threads keep searching meanwhile.
        client.follow();
        let (epoch, seed, target) = client.followed_work();
        if shown_epoch != Some(epoch) {
            report(client, Event::Epoch { epoch, target });
            shown_epoch = Some(epoch);
        }
        let math = client.committee().work().math(epoch);
        let want = threads.load(Ordering::Relaxed).clamp(1, cores());
        let fresh = prepared.as_ref().is_none_or(|(key, ready)| *key != seed || ready.math() != math);
        // Other work, or another number of threads: the old threads stop.
        if workers.as_ref().is_some_and(|w| fresh || w.seed != seed || w.target != target || w.threads != want) {
            let batch = prepared.as_ref().map_or(1, |(_, ready)| ready.batch());
            let (carry_on, left) = workers.take().expect("checked above").stop(next, batch);
            next = carry_on;
            for share in left {
                submit(share, &mut sent, &mut taken);
            }
        }
        if fresh {
            // The old dataset goes first, so two never hold 4 GB at once.
            drop(prepared.take());
            if math == Math::RandomX {
                report(client, Event::Preparing { epoch });
            }
            let ready = Prepared::for_mining(math, &seed);
            if math == Math::RandomX {
                report(client, Event::Prepared { epoch, light: ready.light(), large_pages: ready.large_pages() });
            }
            prepared = Some((seed, Arc::new(ready)));
            if stop.load(Ordering::Relaxed) {
                break;
            }
        }
        let ready = &prepared.as_ref().expect("set above").1;
        let running = workers.get_or_insert_with(|| Workers::start(ready, account, seed, target, want, next));
        // Search for a few seconds, then check the epoch again.
        let (started, before) = (Instant::now(), running.tries.load(Ordering::Relaxed));
        while started.elapsed() < ROUND && !stop.load(Ordering::Relaxed) {
            if let Ok(share) = running.shares.recv_timeout(Duration::from_millis(200)) {
                submit(share, &mut sent, &mut taken);
            }
        }
        let tries = running.tries.load(Ordering::Relaxed) - before;
        let tries_per_second = tries as f64 / started.elapsed().as_secs_f64().max(0.001);
        report(client, Event::Round { tries_per_second, sent, taken });
    }
    if let Some(running) = workers.take() {
        let batch = prepared.as_ref().map_or(1, |(_, ready)| ready.batch());
        for share in running.stop(next, batch).1 {
            let _ = client.submit_share(share);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn whole_cores_at_least_one() {
        let cores = cores();
        assert_eq!(threads_for(100), cores);
        assert_eq!(threads_for(1), 1);
        assert!((1..=cores).contains(&threads_for(50)));
    }
}
