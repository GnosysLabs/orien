//! "Keep this computer awake": while the wallet runs, the computer doesn't
//! go to sleep by itself, so swap offers stay open, payments go ahead and
//! mining carries on. The screen can still turn off. Each system's own way
//! of asking it to stay awake, held only while the wallet runs; it ends the
//! moment the wallet quits. The person's choice, kept in a file.

use std::sync::Mutex;
use tauri::AppHandle;

/// What holds the computer awake right now, if anything.
enum Hold {
    /// A helper the system provides, running while this copy does.
    #[cfg(not(windows))]
    Helper(std::process::Child),
    /// A thread that asked Windows to stay awake, and resets it when told.
    #[cfg(windows)]
    Thread(std::sync::mpsc::Sender<()>),
}

static HOLD: Mutex<Option<Hold>> = Mutex::new(None);

fn file(app: &AppHandle) -> Option<std::path::PathBuf> {
    crate::folder(app).ok().map(|dir| dir.join("stay-awake"))
}

/// Whether the person asked for it (off unless they did).
pub fn on(app: &AppHandle) -> bool {
    file(app).is_some_and(|f| f.exists())
}

/// Turns it on or off, now and for the next launch.
pub fn set(app: &AppHandle, on: bool) -> Result<(), String> {
    let f = file(app).ok_or("no app folder")?;
    if on { std::fs::write(f, b"") } else { std::fs::remove_file(f).or_else(|e| if e.kind() == std::io::ErrorKind::NotFound { Ok(()) } else { Err(e) }) }
        .map_err(|error| error.to_string())?;
    apply(on);
    Ok(())
}

/// At launch: holds the computer awake if the person asked for it.
pub fn start(app: &AppHandle) {
    if on(app) {
        apply(true);
    }
}

fn apply(on: bool) {
    let mut hold = HOLD.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some(old) = hold.take() {
        release(old);
    }
    if on {
        *hold = acquire();
    }
}

fn release(hold: Hold) {
    match hold {
        #[cfg(not(windows))]
        Hold::Helper(mut child) => {
            let _ = child.kill();
            let _ = child.wait();
        }
        #[cfg(windows)]
        Hold::Thread(stop) => {
            let _ = stop.send(());
        }
    }
}

/// Macs: caffeinate keeps the system from idle sleep (and from sleeping on
/// power) for as long as this copy runs, and ends by itself when it quits.
#[cfg(target_os = "macos")]
fn acquire() -> Option<Hold> {
    std::process::Command::new("/usr/bin/caffeinate")
        .args(["-i", "-s", "-w", &std::process::id().to_string()])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .ok()
        .map(Hold::Helper)
}

/// Linux: systemd-inhibit blocks sleep while its command runs; the command
/// waits for this copy to end, so the block never outlives the wallet.
#[cfg(target_os = "linux")]
fn acquire() -> Option<Hold> {
    let wait = format!("while kill -0 {} 2>/dev/null; do sleep 10; done", std::process::id());
    std::process::Command::new("systemd-inhibit")
        .args(["--what=sleep:idle", "--who=Orien Wallet", "--why=Keeping swaps, payments and mining going", "--mode=block", "/bin/sh", "-c", &wait])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .ok()
        .map(Hold::Helper)
}

/// Windows: a thread of its own asks Windows to keep the system awake, and
/// holds that until told to stop (or the wallet quits, which ends it too).
#[cfg(windows)]
fn acquire() -> Option<Hold> {
    use windows_sys::Win32::System::Power::{SetThreadExecutionState, ES_CONTINUOUS, ES_SYSTEM_REQUIRED};
    let (stop, told) = std::sync::mpsc::channel::<()>();
    std::thread::spawn(move || {
        unsafe { SetThreadExecutionState(ES_CONTINUOUS | ES_SYSTEM_REQUIRED) };
        let _ = told.recv();
        unsafe { SetThreadExecutionState(ES_CONTINUOUS) };
    });
    Some(Hold::Thread(stop))
}
