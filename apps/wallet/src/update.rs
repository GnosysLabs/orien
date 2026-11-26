//! Updates. The app asks orien.network whether a newer version is out.
//! It downloads it quietly and installs it while the person is away from
//! the app (unless they turned that off, then only when they say so).
//!
//! Every update is signed with Orien's update key, whose public half is in
//! `tauri.conf.json`. A download without that signature is refused, even
//! one served from orien.network itself, so a broken-into server cannot
//! ship a false wallet. The private half is kept off every server.
//!
//! The update file may name a `minimum` version. A copy older than that no
//! longer works with the network's rules, and the window says so plainly.

use serde::Serialize;
use std::sync::Mutex;
use tauri::AppHandle;
use tauri_plugin_updater::{Update, UpdaterExt};

/// The update found by the last check, kept until it is installed.
static FOUND: Mutex<Option<Update>> = Mutex::new(None);

/// What the window shows about a newer version.
#[derive(Serialize)]
pub struct Available {
    pub version: String,
    pub notes: Option<String>,
    /// This copy is older than the network now accepts.
    pub required: bool,
}

/// Whether `current` is older than the `minimum` the update file names.
fn too_old(current: &str, minimum: Option<&str>) -> bool {
    let (Ok(current), Some(Ok(minimum))) = (semver::Version::parse(current), minimum.map(semver::Version::parse))
    else {
        return false;
    };
    current < minimum
}

#[tauri::command]
pub async fn check_update(app: AppHandle) -> Result<Option<Available>, String> {
    let updater = app.updater().map_err(|error| error.to_string())?;
    let Some(update) = updater.check().await.map_err(|error| error.to_string())? else {
        return Ok(None);
    };
    let minimum = update.raw_json.get("minimum").and_then(|minimum| minimum.as_str());
    let available = Available {
        version: update.version.clone(),
        notes: update.body.clone(),
        required: too_old(&update.current_version, minimum),
    };
    *FOUND.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(update);
    Ok(Some(available))
}

/// When the window stopped being the one in use, in seconds since 1970;
/// 0 while it is in use. Set as the app starts, so a window that never
/// opened counts as left alone from then.
static AWAY_SINCE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

/// The window came into use (`true`) or was left (`false`).
pub fn window_focus(focused: bool) {
    AWAY_SINCE.store(if focused { 0 } else { now() }, std::sync::atomic::Ordering::Relaxed);
}

/// How long the window has been left alone: closed, minimized, or behind
/// another app's windows. 0 while it is in use.
#[tauri::command]
pub fn away_seconds(app: AppHandle) -> u64 {
    use tauri::Manager;
    let focused = app.get_webview_window("main").is_some_and(|w| w.is_visible().unwrap_or(false) && w.is_focused().unwrap_or(false));
    if focused {
        window_focus(true);
        return 0;
    }
    let since = AWAY_SINCE.load(std::sync::atomic::Ordering::Relaxed);
    if since == 0 {
        window_focus(false);
        return 0;
    }
    now().saturating_sub(since)
}

/// The downloaded update waiting to be installed, beside the version it is.
fn ready_files(app: &AppHandle) -> Option<(std::path::PathBuf, std::path::PathBuf)> {
    let dir = crate::folder(app).ok()?;
    Some((dir.join("update-ready.bin"), dir.join("update-ready.version")))
}

/// Downloads the newest version ahead of time, so installing it later
/// takes a moment. The signature is checked now and again on installing.
/// The version that is ready, or None when this copy is the newest.
#[tauri::command]
pub async fn download_update(app: AppHandle) -> Result<Option<String>, String> {
    let updater = app.updater().map_err(|error| error.to_string())?;
    let Some(update) = updater.check().await.map_err(|error| error.to_string())? else {
        return Ok(None);
    };
    let (bin, version) = ready_files(&app).ok_or("no app folder")?;
    if std::fs::read_to_string(&version).is_ok_and(|v| v == update.version) && bin.exists() {
        return Ok(Some(update.version));
    }
    let bytes = update.download(|_, _| {}, || {}).await.map_err(|error| error.to_string())?;
    std::fs::write(&bin, &bytes).map_err(|error| error.to_string())?;
    std::fs::write(&version, &update.version).map_err(|error| error.to_string())?;
    Ok(Some(update.version))
}

/// How the window was when an update installed by itself, so the new copy
/// comes back the same way: "hidden" (closed to the menu bar or tray),
/// "minimized", or "behind" (open, under other windows).
fn window_now(app: &AppHandle) -> &'static str {
    use tauri::Manager;
    let Some(window) = app.get_webview_window("main") else { return "hidden" };
    if !window.is_visible().unwrap_or(false) {
        "hidden"
    } else if window.is_minimized().unwrap_or(false) {
        "minimized"
    } else {
        "behind"
    }
}

/// Downloads the newest version, checks its signature, installs it and
/// restarts. Asks orien.network again first, so a release that came out
/// after the last check is the one installed, not the one before it; when
/// that question can't be answered, the update found earlier is used.
/// Waits for a payment that is going out to finish first, and lets none
/// start meanwhile.
///
/// `quiet`: the app is installing it by itself, while the person is away;
/// the new copy comes back the way the window was, without coming in front.
#[tauri::command]
pub async fn install_update(app: AppHandle, quiet: Option<bool>) -> Result<(), String> {
    let quiet = quiet.unwrap_or(false);
    let newest = match app.updater() {
        Ok(updater) => updater.check().await.ok().flatten(),
        Err(_) => None,
    };
    let earlier = FOUND.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).take();
    let update = newest.or(earlier).ok_or("no update found")?;
    // The one downloaded ahead of time, when it is this version.
    let ready = ready_files(&app).and_then(|(bin, version)| {
        std::fs::read_to_string(version).ok().filter(|v| *v == update.version).and_then(|_| std::fs::read(bin).ok())
    });
    let dir = crate::folder(&app)?;
    // Noted before installing: on Windows the installer ends this copy
    // itself, and these must already be saved by then.
    crate::background::remember_window(&app);
    crate::mine_commands::remember_for_update(&app);
    if quiet {
        let _ = std::fs::write(dir.join("reopen-as"), window_now(&app));
    }
    let installed = tauri::async_runtime::spawn_blocking(move || {
        // Waits for a payment or swap step already under way (new ones stand
        // aside meanwhile), but never forever.
        crate::UPDATING.store(true, std::sync::atomic::Ordering::Relaxed);
        let started = std::time::Instant::now();
        let _turn = loop {
            match crate::TURNS.try_lock() {
                Ok(turn) => break turn,
                Err(std::sync::TryLockError::Poisoned(poisoned)) => break poisoned.into_inner(),
                Err(std::sync::TryLockError::WouldBlock) if started.elapsed() < std::time::Duration::from_secs(90) => std::thread::sleep(std::time::Duration::from_millis(200)),
                Err(std::sync::TryLockError::WouldBlock) => {
                    crate::UPDATING.store(false, std::sync::atomic::Ordering::Relaxed);
                    return Err("A payment or swap step is still going. Try the update again in a minute.".to_string());
                }
            }
        };
        match ready {
            Some(bytes) => update.install(bytes),
            None => tauri::async_runtime::block_on(update.download_and_install(|_, _| {}, || {})),
        }
        .map_err(|error| format!("The update could not be installed: {error}"))
    })
    .await
    .map_err(|error| error.to_string())
    .and_then(|done| done);
    if let Err(error) = installed {
        crate::UPDATING.store(false, std::sync::atomic::Ordering::Relaxed);
        for file in ["reopen-as", "update-ready.bin", "update-ready.version"] {
            let _ = std::fs::remove_file(dir.join(file));
        }
        crate::mine_commands::forget_resume(&app);
        return Err(error);
    }
    #[cfg(target_os = "macos")]
    reopen_on_the_mac(&app, quiet);
    #[cfg(target_os = "linux")]
    reopen_on_linux(&app);
    app.restart()
}

/// How the window should come back after an update that installed by
/// itself, read once at start. Clears the downloaded update, installed now.
pub fn reopen_as(app: &AppHandle) -> Option<String> {
    let dir = crate::folder(app).ok()?;
    for file in ["update-ready.bin", "update-ready.version"] {
        let _ = std::fs::remove_file(dir.join(file));
    }
    let file = dir.join("reopen-as");
    let how = std::fs::read_to_string(&file).ok();
    let _ = std::fs::remove_file(file);
    how
}

/// Reopens the app on a Mac once this copy has closed, the way clicking its
/// Dock icon would. Tauri's own restart starts a second copy while this one
/// is still running, and the Mac gives that copy a Dock icon of its own
/// instead of the one the person keeps in their Dock.
#[cfg(target_os = "macos")]
fn reopen_on_the_mac(app: &AppHandle, quiet: bool) {
    let Ok(binary) = tauri::process::current_binary(&tauri::Manager::env(app)) else { return };
    // .../Orien Wallet.app/Contents/MacOS/orien-wallet
    let Some(bundle) = binary.ancestors().nth(3).filter(|b| b.extension().is_some_and(|e| e == "app")) else { return };
    // `-g`: installed by itself, so it opens without coming in front.
    let script = format!(
        "while kill -0 {} 2>/dev/null; do sleep 0.2; done; exec /usr/bin/open {}\"$0\"",
        std::process::id(),
        if quiet { "-g " } else { "" }
    );
    // In a process group of its own: the Mac ends every process in an
    // app's group when the app quits, which would take this waiter with it
    // and leave the app closed.
    use std::os::unix::process::CommandExt;
    let started = std::process::Command::new("/bin/sh")
        .arg("-c")
        .arg(script)
        .arg(bundle)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .process_group(0)
        .spawn();
    if started.is_ok() {
        std::process::exit(0);
    }
}

/// Reopens the app on Linux once this copy has closed. Tauri's own restart
/// starts the new copy while this one still runs, so it finds this one,
/// hands over and quits, and when this one then ends nothing is left open.
/// An AppImage runs from a folder that goes away when it closes, so the
/// new copy is started from the AppImage file itself.
#[cfg(target_os = "linux")]
fn reopen_on_linux(app: &AppHandle) {
    let program = std::env::var_os("APPIMAGE")
        .map(std::path::PathBuf::from)
        .or_else(|| tauri::process::current_binary(&tauri::Manager::env(app)).ok());
    let Some(program) = program else { return };
    let script = format!("while kill -0 {} 2>/dev/null; do sleep 0.2; done; exec \"$0\"", std::process::id());
    use std::os::unix::process::CommandExt;
    let started = std::process::Command::new("/bin/sh")
        .arg("-c")
        .arg(script)
        .arg(program)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .process_group(0)
        .spawn();
    if started.is_ok() {
        std::process::exit(0);
    }
}

/// "Check for Updates…" from the menu: brings the window forward, where
/// the app asks and shows the answer in its own style.
pub fn check_now(app: AppHandle) {
    use tauri::Emitter;
    crate::background::show(&app);
    let _ = app.emit("check-updates", ());
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_copy_below_the_minimum_is_too_old() {
        assert!(super::too_old("0.1.0", Some("0.2.0")));
        assert!(!super::too_old("0.2.0", Some("0.2.0")));
        assert!(!super::too_old("0.3.1", Some("0.2.0")));
        assert!(!super::too_old("0.1.0", None));
        assert!(!super::too_old("0.1.0", Some("not a version")));
    }
}
