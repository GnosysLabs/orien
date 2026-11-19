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
    /// RandomX's dataset sits in large pages this epoch, which mines faster.
    pub large_pages: bool,
    /// Rounds of mining reported so far, so the benchmark knows when a
    /// new thread count has had a whole round to itself.
    pub rounds: u64,
    /// The benchmark, while it runs.
    pub bench: Option<Bench>,
}

/// How far the benchmark has got.
#[derive(Clone, Default, Serialize)]
pub struct Bench {
    /// Which of `steps` thread counts it is trying now, from 1.
    pub step: usize,
    pub steps: usize,
    pub threads: usize,
}

/// What the benchmark found: the fewest threads that mine within a few
/// percent of the fastest, and the speed there.
#[derive(Clone, Copy, Serialize, serde::Deserialize)]
pub struct Sweet {
    pub threads: usize,
    pub tries_per_second: f64,
    /// The threads the computer had when it was measured.
    pub cores: usize,
}

struct Running {
    stop: Arc<AtomicBool>,
    /// How many threads mine, read by the miner every round: changing it
    /// changes the speed without getting RandomX ready again.
    threads: Arc<AtomicUsize>,
    thread: JoinHandle<()>,
    /// Tells the system mining is under way, so it keeps full speed
    /// while the window is in the background.
    _busy: Busy,
}

/// Full speed with the window in the background, while it lives. Macs
/// otherwise "App Nap" a window nobody is looking at; Windows 11 puts a
/// background app in efficiency mode, on its slowest cores.
struct Busy {
    #[cfg(target_os = "macos")]
    activity: objc2::rc::Retained<objc2::runtime::ProtocolObject<dyn objc2::runtime::NSObjectProtocol>>,
}

// SAFETY: the activity token is only handed back to NSProcessInfo, which
// takes it on any thread.
unsafe impl Send for Busy {}

impl Busy {
    fn begin() -> Busy {
        #[cfg(windows)]
        {
            use windows_sys::Win32::System::Threading::{
                GetCurrentProcess, PROCESS_POWER_THROTTLING_CURRENT_VERSION, PROCESS_POWER_THROTTLING_EXECUTION_SPEED,
                PROCESS_POWER_THROTTLING_STATE, ProcessPowerThrottling, SetProcessInformation,
            };
            // Never throttle this app's speed, in front or not.
            let state = PROCESS_POWER_THROTTLING_STATE {
                Version: PROCESS_POWER_THROTTLING_CURRENT_VERSION,
                ControlMask: PROCESS_POWER_THROTTLING_EXECUTION_SPEED,
                StateMask: 0,
            };
            // SAFETY: the state is the size passed, and lives over the call.
            unsafe {
                SetProcessInformation(
                    GetCurrentProcess(),
                    ProcessPowerThrottling,
                    (&state as *const PROCESS_POWER_THROTTLING_STATE).cast(),
                    size_of::<PROCESS_POWER_THROTTLING_STATE>() as u32,
                );
            }
        }
        #[cfg(target_os = "macos")]
        {
            use objc2_foundation::{NSActivityOptions, NSProcessInfo, NSString};
            let activity = NSProcessInfo::processInfo().beginActivityWithOptions_reason(
                NSActivityOptions::UserInitiatedAllowingIdleSystemSleep,
                &NSString::from_str("Mining ORN"),
            );
            return Busy { activity };
        }
        #[cfg(not(target_os = "macos"))]
        Busy {}
    }
}

impl Drop for Busy {
    fn drop(&mut self) {
        #[cfg(target_os = "macos")]
        // SAFETY: the token beginActivity gave back.
        unsafe {
            objc2_foundation::NSProcessInfo::processInfo().endActivity(&self.activity)
        };
    }
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

/// Starts mining for `account` on `threads` threads, stopping any mining
/// already running first. `connect` makes the client it mines with.
pub fn start(account: AccountId, threads: usize, connect: impl FnOnce() -> Result<Client, String> + Send + 'static, genesis: PathBuf) {
    stop();
    let threads = threads.clamp(1, miner::cores());
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
            Event::Prepared { light, large_pages, .. } => set(|s| {
                s.preparing = false;
                s.light = light;
                s.large_pages = large_pages;
            }),
            Event::Round { tries_per_second, sent, taken } => set(|s| {
                s.tries_per_second = tries_per_second;
                s.rounds += 1;
                s.found += sent;
                s.taken += taken;
            }),
        });
        let _ = client.head().save(&crate::head_file(&genesis));
        set(|s| s.running = false);
    });
    *RUNNING.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(Running { stop, threads: count, thread, _busy: Busy::begin() });
}

/// Changes how many threads mine, from the next round, without stopping
/// it. False if mining isn't running.
pub fn set_threads(threads: usize) -> bool {
    let running = RUNNING.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let Some(running) = running.as_ref() else { return false };
    let threads = threads.clamp(1, miner::cores());
    running.threads.store(threads, Ordering::Relaxed);
    set(|s| s.threads = threads);
    true
}

/// Stops mining, if it is running, and waits for its threads to end.
pub fn stop() {
    BENCH_STOP.store(true, Ordering::Relaxed);
    let running = RUNNING.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).take();
    if let Some(running) = running {
        running.stop.store(true, Ordering::Relaxed);
        let _ = running.thread.join();
    }
    set(|s| s.running = false);
}

static BENCH_STOP: AtomicBool = AtomicBool::new(false);

/// Mining's tries a second over two whole rounds on `threads` threads, or
/// None if mining stopped or the benchmark was cancelled. Real mining, so
/// shares found still count.
fn measure(threads: usize) -> Option<f64> {
    'again: loop {
        if !set_threads(threads) { return None; }
        let wait = |after: u64| -> Option<Option<f64>> {
            loop {
                if BENCH_STOP.load(Ordering::Relaxed) { return None; }
                let s = status();
                if !s.running { return None; }
                if s.preparing { return Some(None); }
                if s.rounds > after { return Some(Some(s.tries_per_second)); }
                std::thread::sleep(std::time::Duration::from_millis(250));
            }
        };
        // The round under way began on the old count: let it finish first.
        let start = status().rounds;
        let mut speeds = Vec::new();
        for after in start..start + 3 {
            match wait(after)? {
                None => {
                    // A new epoch: wait until RandomX is ready, then again.
                    while status().preparing {
                        if BENCH_STOP.load(Ordering::Relaxed) || !status().running { return None; }
                        std::thread::sleep(std::time::Duration::from_millis(500));
                    }
                    continue 'again;
                }
                Some(speed) => speeds.push(speed),
            }
        }
        return Some((speeds[1] + speeds[2]) / 2.0);
    }
}

/// Runs the benchmark on mining already running: tries several thread
/// counts, then mines on the sweet spot and hands it to `done`. When
/// cancelled, mining goes back to `back` threads and `done` isn't called.
pub fn benchmark(back: usize, done: impl FnOnce(Sweet) + Send + 'static) -> bool {
    if !status().running || status().bench.is_some() { return false; }
    BENCH_STOP.store(false, Ordering::Relaxed);
    let cores = miner::cores();
    let counts = miner::bench_counts(cores);
    set(|s| s.bench = Some(Bench { step: 0, steps: counts.len(), threads: 0 }));
    std::thread::spawn(move || {
        // Wait for RandomX to be ready and a first round to be in.
        while status().running && (status().preparing || status().rounds == 0) && !BENCH_STOP.load(Ordering::Relaxed) {
            std::thread::sleep(std::time::Duration::from_millis(500));
        }
        let mut results = Vec::new();
        for (i, &threads) in counts.iter().enumerate() {
            set(|s| s.bench = Some(Bench { step: i + 1, steps: counts.len(), threads }));
            match measure(threads) {
                Some(speed) => results.push((threads, speed)),
                None => break,
            }
        }
        set(|s| s.bench = None);
        match miner::sweet_of(&results).filter(|_| results.len() == counts.len()) {
            Some((threads, tries_per_second)) => {
                set_threads(threads);
                done(Sweet { threads, tries_per_second, cores });
            }
            None => { set_threads(back); }
        }
    });
    true
}

/// Stops the benchmark, if it runs; mining goes on.
pub fn cancel_benchmark() {
    BENCH_STOP.store(true, Ordering::Relaxed);
}
