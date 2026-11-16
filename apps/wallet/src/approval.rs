//! Approving a swap offer once, as it's posted. Touch ID or the wallet's
//! password then counts as consent to lock exactly that offer's ORN, for
//! that offer only, and to finish its swap: when someone takes it, the
//! wallet accepts within seconds, with nobody at the computer.
//!
//! What's kept is what unlocks the wallet: on Macs in the Keychain (only
//! this app can read it without macOS asking), elsewhere in a file only
//! this user can read, as the app keeps its other keys. It's used only for
//! the approved offer's swap, and erased when that offer ends: sold,
//! withdrawn or gone from the board.

use orien_net::store::write_replacing;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

/// One approved offer, as listed (nothing secret on Macs).
#[derive(Clone, Serialize, Deserialize)]
struct Approved {
    /// The ORN approved, in units: only an offer for exactly this many.
    orn: u64,
    wallet: PathBuf,
    /// Elsewhere than Macs, the wallet's password itself.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    password: Option<String>,
}

#[cfg(target_os = "macos")]
const KEYCHAIN_SERVICE: &str = "network.orien.wallet.offer";

fn file(dir: &Path) -> PathBuf {
    dir.join("offer-approvals.json")
}

fn load(dir: &Path) -> BTreeMap<i64, Approved> {
    fs::read(file(dir)).ok().and_then(|bytes| serde_json::from_slice(&bytes).ok()).unwrap_or_default()
}

fn save(dir: &Path, all: &BTreeMap<i64, Approved>) {
    if all.is_empty() {
        let _ = fs::remove_file(file(dir));
    } else if let Ok(text) = serde_json::to_vec_pretty(all) {
        // Readable by this user only (0600).
        let _ = write_replacing(&file(dir), &text, true);
    }
}

/// Keeps the approval for `offer`: `orn` units from `wallet`, unlocked with
/// `password` (None for a wallet with no lock).
pub fn approve(dir: &Path, offer: i64, orn: u64, wallet: &Path, password: Option<&str>) {
    let mut all = load(dir);
    #[cfg(target_os = "macos")]
    let kept = {
        if let Some(password) = password {
            let _ = security_framework::passwords::set_generic_password(KEYCHAIN_SERVICE, &offer.to_string(), password.as_bytes());
        }
        None
    };
    #[cfg(not(target_os = "macos"))]
    let kept = password.map(str::to_string);
    all.insert(offer, Approved { orn, wallet: wallet.to_path_buf(), password: kept });
    save(dir, &all);
}

/// What unlocks the wallet for `offer`'s swap, if it was approved for
/// exactly `orn` units from `wallet`: Some(None) for a wallet with no lock.
pub fn approved(dir: &Path, offer: i64, orn: u64, wallet: &Path) -> Option<Option<String>> {
    let approved = load(dir).remove(&offer)?;
    if approved.orn != orn || approved.wallet != wallet {
        return None;
    }
    #[cfg(target_os = "macos")]
    {
        let password = security_framework::passwords::get_generic_password(KEYCHAIN_SERVICE, &offer.to_string())
            .ok()
            .and_then(|bytes| String::from_utf8(bytes).ok());
        Some(password.or(approved.password))
    }
    #[cfg(not(target_os = "macos"))]
    Some(approved.password)
}

/// Erases `offer`'s approval.
pub fn forget(dir: &Path, offer: i64) {
    let mut all = load(dir);
    if all.remove(&offer).is_some() {
        save(dir, &all);
    }
    #[cfg(target_os = "macos")]
    let _ = security_framework::passwords::delete_generic_password(KEYCHAIN_SERVICE, &offer.to_string());
}

/// Erases every approval but those for offers in `live`.
pub fn keep_only(dir: &Path, live: &[i64]) {
    for offer in load(dir).into_keys().filter(|offer| !live.contains(offer)) {
        forget(dir, offer);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_approval_covers_one_offer_and_amount_and_is_erased() {
        let dir = std::env::temp_dir().join(format!("orien-approval-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let wallet = dir.join("w.wallet");
        // A wallet with no lock: nothing secret kept, on any system.
        approve(&dir, 7, 5_000, &wallet, None);
        assert_eq!(approved(&dir, 7, 5_000, &wallet), Some(None));
        assert_eq!(approved(&dir, 7, 5_001, &wallet), None, "another amount");
        assert_eq!(approved(&dir, 8, 5_000, &wallet), None, "another offer");
        assert_eq!(approved(&dir, 7, 5_000, &dir.join("other.wallet")), None, "another wallet");
        keep_only(&dir, &[7]);
        assert!(approved(&dir, 7, 5_000, &wallet).is_some());
        keep_only(&dir, &[]);
        assert_eq!(approved(&dir, 7, 5_000, &wallet), None);
        assert!(!file(&dir).exists());
        let _ = fs::remove_dir_all(&dir);
    }
}
