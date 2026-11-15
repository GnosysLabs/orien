//! Mining in the app, with the same miner `orien mine` uses. It runs on its
//! own threads while the app is open, for the wallet that was open when it
//! started, and stops when asked or when the app closes.

use orien_ledger::AccountId;
use orien_net::client::Client;
use orien_net::miner::{self, Event};
use serde::Serialize;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

/// What the Mine screen shows.
#[derive(Clone, Default, Serialize)]
pub struct Status {
    pub running: bool,
    /// The address it mines to.
    pub address: Option<String>,
    pub threads: usize,
    pub cores: usize,
    pub epoch: Option<u64>,
    /// About one try in this many makes a share.
    pub one_in: Option<u64>,
    pub tries_per_second: f64,
    /// Shares found and taken by validators this epoch.
    pub found: u64,
    pub taken: u64,
    /// Why it stopped, if it stopped on its own.
    pub problem: Option<String>,
    /// Getting RandomX ready for a new epoch: up to a minute, once an hour.
    pub preparing: bool,
    /// Mining in RandomX's light mode, several times slower, for lack of
    /// the 2 GB of memory the full dataset needs.
    pub light: bool,
}

struct Running {
    stop: Arc<AtomicBool>,
    /// How many threads mine, read by the miner every round: changing it
    /// changes the speed without getting RandomX ready again.
    threads: Arc<AtomicUsize>,
    thread: JoinHandle<()>,
}

static RUNNING: Mutex<Option<Running>> = Mutex::new(None);
static STATUS: Mutex<Option<Status>> = Mutex::new(None);

fn set(change: impl FnOnce(&mut Status)) {
    let mut status = STATUS.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    change(status.get_or_insert_with(Status::default));
}

pub fn status() -> Status {
    let mut status = STATUS.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).clone().unwrap_or_default();
    status.cores = miner::cores();
    status
}

/// Starts mining for `account` on `percent` of the processor, stopping any
/// mining already running first. `connect` makes the client it mines with.
pub fn start(account: AccountId, percent: u8, connect: impl FnOnce() -> Result<Client, String> + Send + 'static, genesis: PathBuf) {
    stop();
    let threads = miner::threads_for(percent.clamp(1, 100));
    let stop = Arc::new(AtomicBool::new(false));
    set(|s| {
        *s = Status { running: true, address: Some(account.to_string()), threads, ..Status::default() };
    });
    let flag = stop.clone();
    let count = Arc::new(AtomicUsize::new(threads));
    let counted = count.clone();
    let thread = std::thread::spawn(move || {
        let client = match connect() {
            Ok(client) => client,
            Err(error) => {
                set(|s| {
                    s.running = false;
                    s.problem = Some(error);
                });
                return;
            }
        };
        miner::run_adjustable(&client, account, &counted, &flag, |_, event| match event {
            Event::Epoch { epoch, target } => set(|s| {
                s.epoch = Some(epoch);
                s.one_in = Some(u64::MAX / target.max(1));
                s.found = 0;
                s.taken = 0;
            }),
            Event::Preparing { .. } => set(|s| s.preparing = true),
            Event::Prepared { light, .. } => set(|s| {
                s.preparing = false;
                s.light = light;
            }),
            Event::Round { tries_per_second, sent, taken } => set(|s| {
                s.tries_per_second = tries_per_second;
                s.found += sent;
                s.taken += taken;
            }),
        });
        let _ = client.head().save(&crate::head_file(&genesis));
        set(|s| s.running = false);
    });
    *RUNNING.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(Running { stop, threads: count, thread });
}

/// Changes how much of the processor mining uses, within a round, without
/// stopping it. False if mining isn't running.
pub fn set_percent(percent: u8) -> bool {
    let running = RUNNING.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let Some(running) = running.as_ref() else { return false };
    let threads = miner::threads_for(percent.clamp(1, 100));
    running.threads.store(threads, Ordering::Relaxed);
    set(|s| s.threads = threads);
    true
}

/// Stops mining, if it is running, and waits for its threads to end.
pub fn stop() {
    let running = RUNNING.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).take();
    if let Some(running) = running {
        running.stop.store(true, Ordering::Relaxed);
        let _ = running.thread.join();
    }
    set(|s| s.running = false);
}
