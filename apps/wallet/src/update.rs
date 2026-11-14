//! Updates. The app asks orien.network whether a newer version is out,
//! and installs it only when the person says so.
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

/// Downloads the update, checks its signature, installs it and restarts.
/// Waits for a payment that is going out to finish first, and lets none
/// start meanwhile.
#[tauri::command]
pub async fn install_update(app: AppHandle) -> Result<(), String> {
    let update = FOUND.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).take().ok_or("no update found")?;
    tauri::async_runtime::spawn_blocking(move || {
        let _turn = crate::TURNS.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        tauri::async_runtime::block_on(update.download_and_install(|_, _| {}, || {}))
            .map_err(|error| format!("The update could not be installed: {error}"))
    })
    .await
    .map_err(|error| error.to_string())??;
    app.restart()
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
