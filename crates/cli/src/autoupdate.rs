//! Validators keep themselves on the newest release signed with the Orien
//! release key, so that a validator run by someone nobody can reach still
//! follows the network's rules as they change.
//!
//! About once an hour a validator looks for a newer release. One it finds is
//! checked exactly as `orien update` checks it, and put in place of the
//! program. The validator then starts again on it, at a moment that keeps
//! the network running: well after a checkpoint and well before the next,
//! at a random point in that time, and only while every validator answers,
//! so no two restart together. Validators that share one program file, as
//! the launch validators do, each notice the new version and restart in
//! turn.
//!
//! `orien node --no-auto-update` turns it off.

use crate::release;
use orien_ledger::Genesis;
use orien_net::client::{Client, rpc};
use orien_net::wire::{Request, Response};
use std::ffi::OsString;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// How often to look for a new release, before a random extra.
const CHECK_EVERY: Duration = Duration::from_secs(3600);

/// How long after a checkpoint a restart may happen, as a share of the
/// epoch: from a twelfth to two thirds in, never near the next checkpoint.
const EARLIEST: (u64, u64) = (1, 12);
const LATEST: (u64, u64) = (2, 3);

pub struct Updater {
    /// The program file this validator was started from, read before any
    /// update could replace it.
    pub exe: PathBuf,
    /// The arguments it was started with, to start again with.
    pub args: Vec<OsString>,
    /// Where this validator itself answers.
    pub me: SocketAddr,
    pub genesis: Genesis,
    pub name: String,
}

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

/// A number from 0 to `below`, different on each validator and each time.
fn random_below(below: u64) -> u64 {
    let mut bytes = [0u8; 8];
    let _ = getrandom::fill(&mut bytes);
    u64::from_le_bytes(bytes) % below.max(1)
}

/// Whether `now` is a calm time to restart in an epoch that began at
/// `started` and lasts `epoch` seconds, for a validator whose own moment is
/// `pick` (0 to 999) of the way through the calm part.
pub fn calm(now: u64, started: u64, epoch: u64, pick: u64) -> bool {
    let from = started + epoch * EARLIEST.0 / EARLIEST.1;
    let until = started + epoch * LATEST.0 / LATEST.1;
    let at = from + (until - from) * pick.min(999) / 1000;
    (at..until).contains(&now)
}

impl Updater {
    pub fn spawn(self) {
        let _ = thread::Builder::new().name(format!("{}-update", self.name)).spawn(move || self.run());
    }

    fn say(&self, text: &str) {
        println!("[{}] {text}", self.name);
    }

    fn run(self) {
        let running = env!("CARGO_PKG_VERSION");
        // Validators started together do not all ask at once. A test build
        // can be told to hurry, to try updates on a local network.
        let soon = cfg!(debug_assertions) && std::env::var_os("ORIEN_AUTOUPDATE_SOON").is_some();
        let (first, every) = if soon { (2, Duration::from_secs(10)) } else { (60 + random_below(600), CHECK_EVERY) };
        thread::sleep(Duration::from_secs(first));
        let mut told_cannot = String::new();
        loop {
            match self.ready(running, &mut told_cannot) {
                Some(version) => {
                    self.restart_when_calm(&version, soon);
                }
                None => thread::sleep(every + Duration::from_secs(if soon { 0 } else { random_below(600) })),
            }
        }
    }

    /// The newer version now in the program file, installing it first if a
    /// newer release is out. None when there is nothing newer, or it cannot
    /// be installed here.
    fn ready(&self, running: &str, told_cannot: &mut String) -> Option<String> {
        // Another validator sharing this file, or its operator, may have
        // installed one already.
        if let Some(installed) = release::installed_version(&self.exe)
            && release::version(&installed) > release::version(running)
        {
            return Some(installed);
        }
        let (base, root) = release::source().ok()?;
        let work = std::env::temp_dir().join(format!("orien-autoupdate-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&work);
        std::fs::create_dir_all(&work).ok()?;
        let found = release::newer_release(&base, &root, running, &work);
        let outcome = match found {
            Ok(Some((version, program))) => match release::install(&program, &self.exe) {
                Ok(()) => {
                    self.say(&format!("installed orien {version}; restarting on it at a calm moment"));
                    Some(version)
                }
                Err(error) => {
                    if *told_cannot != version {
                        self.say(&format!(
                            "orien {version} is out, but this validator cannot update itself ({error:#}); \
                             run `orien update` as a user who may write {}",
                            self.exe.display()
                        ));
                        *told_cannot = version;
                    }
                    None
                }
            },
            Ok(None) => None,
            Err(error) => {
                self.say(&format!("could not check for a new version: {error:#}"));
                None
            }
        };
        let _ = std::fs::remove_dir_all(&work);
        outcome
    }

    /// Waits for a calm moment while every validator answers, then starts
    /// this validator again on the installed program.
    fn restart_when_calm(&self, version: &str, soon: bool) {
        let pick = random_below(1000);
        loop {
            thread::sleep(Duration::from_secs(if soon { 2 } else { 20 }));
            let Ok(Response::Info(info)) = rpc(&self.me.to_string(), &Request::Info, Duration::from_secs(5)) else {
                continue;
            };
            let epoch = self.genesis.epoch_seconds.max(60);
            if info.closing || !calm(now(), info.epoch_started, epoch, pick) {
                continue;
            }
            // Never while another validator is away, perhaps restarting too.
            let client = Client::new(&self.genesis);
            client.follow();
            if client.info().iter().any(|answer| answer.is_err()) {
                continue;
            }
            self.say(&format!("restarting on orien {version}"));
            self.start_again();
            // Only reached if starting again failed: try at the next calm moment.
            thread::sleep(Duration::from_secs(epoch / 2));
        }
    }

    #[cfg(unix)]
    fn start_again(&self) {
        use std::os::unix::process::CommandExt;
        // Replaces this process; files and sockets close as it does.
        let error = std::process::Command::new(&self.exe).args(&self.args).exec();
        self.say(&format!("could not restart on the new version: {error}"));
    }

    #[cfg(not(unix))]
    fn start_again(&self) {
        match std::process::Command::new(&self.exe).args(&self.args).spawn() {
            Ok(_) => std::process::exit(0),
            Err(error) => self.say(&format!("could not restart on the new version: {error}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn restarts_happen_in_the_calm_middle_of_an_epoch() {
        let (started, hour) = (1_000_000, 3600);
        assert!(!calm(started + 60, started, hour, 0), "not right after a checkpoint");
        assert!(calm(started + 400, started, hour, 0));
        assert!(!calm(started + 2500, started, hour, 999), "never in the last third");
        assert!(!calm(started + 3590, started, hour, 500));
        // Each validator's moment is its own.
        assert!(!calm(started + 400, started, hour, 900));
        assert!(calm(started + 2380, started, hour, 900));
    }
}
