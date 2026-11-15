//! Touch ID and Face ID instead of typing the wallet's password, on Macs and
//! iPhones. The password itself is kept in the Keychain; the app reads it
//! only after the system's own Touch ID or Face ID prompt (or the device's
//! passcode) succeeds, and then keeps it in memory for a few minutes, so a
//! swap's next step doesn't ask again. Windows and Linux keep the typed
//! password.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// How long an unlock lasts.
const UNLOCKED_FOR: Duration = Duration::from_secs(10 * 60);

/// Passwords unlocked by Touch ID or Face ID, by wallet, and until when.
static UNLOCKED: Mutex<Option<HashMap<String, (String, Instant)>>> = Mutex::new(None);

#[cfg(any(target_os = "macos", target_os = "ios"))]
const SERVICE: &str = "network.orien.wallet";

/// The name a wallet's password is kept under in the Keychain. On an
/// iPhone, relative to the app's home folder, which the phone moves when
/// the app is installed again; on a Mac, the full path, which the `orien`
/// command looks up too.
pub fn name(wallet: &Path) -> String {
    #[cfg(target_os = "ios")]
    if let Some(home) = std::env::var_os("HOME")
        && let Ok(inside) = wallet.strip_prefix(&home)
    {
        return format!("~/{}", inside.display());
    }
    wallet.display().to_string()
}

/// On an iPhone, a password kept by an earlier test copy under the full
/// path of a folder the phone has since moved: kept again under the name
/// that lasts. Found by what follows the app's own folder in the path.
#[cfg(target_os = "ios")]
fn moved(wallet: &Path) {
    use security_framework::item::{ItemClass, ItemSearchOptions, Limit, SearchResult};
    let lasting = name(wallet);
    let Some(tail) = lasting.strip_prefix("~/") else { return };
    if security_framework::passwords::get_generic_password(SERVICE, &lasting).is_ok() {
        return;
    }
    let Ok(found) = ItemSearchOptions::new()
        .class(ItemClass::generic_password())
        .service(SERVICE)
        .load_attributes(true)
        .limit(Limit::All)
        .search()
    else {
        return;
    };
    for item in found {
        let SearchResult::Dict(_) = &item else { continue };
        let Some(attributes) = item.simplify_dict() else { continue };
        let Some(account) = attributes.get("acct") else { continue };
        if account.starts_with('/') && account.ends_with(&format!("/{tail}"))
            && let Ok(password) = security_framework::passwords::get_generic_password(SERVICE, account)
            && security_framework::passwords::set_generic_password(SERVICE, &lasting, &password).is_ok()
        {
            let _ = security_framework::passwords::delete_generic_password(SERVICE, account);
            return;
        }
    }
}

/// Whether this system offers Touch ID or Face ID here.
pub fn available() -> bool {
    cfg!(any(target_os = "macos", target_os = "ios"))
}

/// Whether this wallet's password is kept behind Touch ID or Face ID.
pub fn enabled(wallet: &Path) -> bool {
    #[cfg(target_os = "ios")]
    moved(wallet);
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    {
        security_framework::passwords::get_generic_password(SERVICE, &name(wallet)).is_ok()
    }
    #[cfg(not(any(target_os = "macos", target_os = "ios")))]
    {
        let _ = wallet;
        false
    }
}

/// Keeps the wallet's password behind Touch ID or Face ID.
pub fn remember(wallet: &Path, password: &str) -> Result<(), String> {
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    {
        security_framework::passwords::set_generic_password(SERVICE, &name(wallet), password.as_bytes())
            .map_err(|error| format!("could not keep the password in the Keychain: {error}"))
    }
    #[cfg(not(any(target_os = "macos", target_os = "ios")))]
    {
        let _ = (wallet, password);
        Err("Touch ID and Face ID are on Macs and iPhones only.".into())
    }
}

/// Stops using Touch ID or Face ID for this wallet.
pub fn forget(wallet: &Path) {
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    let _ = security_framework::passwords::delete_generic_password(SERVICE, &name(wallet));
    if let Some(all) = UNLOCKED.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).as_mut() {
        all.remove(&name(wallet));
    }
}

/// The password, if Touch ID or Face ID unlocked it a moment ago.
pub fn unlocked(wallet: &Path) -> Option<String> {
    let mut guard = UNLOCKED.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let all = guard.get_or_insert_with(HashMap::new);
    match all.get(&name(wallet)) {
        Some((password, until)) if Instant::now() < *until => Some(password.clone()),
        Some(_) => {
            all.remove(&name(wallet));
            None
        }
        None => None,
    }
}

/// Asks for Touch ID or Face ID (or the device's passcode), and on success
/// keeps the wallet unlocked for a few minutes. Blocks until answered.
pub fn unlock(wallet: &Path) -> Result<(), String> {
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    {
        use robius_authentication::{AndroidText, BiometricStrength, Context, PolicyBuilder, Text, WindowsText};
        let text = Text {
            android: AndroidText { title: "Orien Wallet", subtitle: None, description: None },
            apple: "unlock your Orien wallet",
            windows: WindowsText::new("Orien Wallet", "Unlock your wallet").ok_or("prompt text too long")?,
        };
        let policy = PolicyBuilder::new()
            .biometrics(Some(BiometricStrength::Strong))
            .password(true)
            .companion(true)
            .build()
            .ok_or("Touch ID and Face ID aren't available here")?;
        let (tx, rx) = std::sync::mpsc::channel();
        Context::new(())
            .authenticate(text, &policy, move |result| {
                let _ = tx.send(result);
            })
            .map_err(|error| format!("could not ask for Touch ID or Face ID: {error:?}"))?;
        match rx.recv() {
            Ok(Ok(_)) => {}
            _ => return Err("Not unlocked.".into()),
        }
        let password = security_framework::passwords::get_generic_password(SERVICE, &name(wallet))
            .map_err(|_| "This wallet's password isn't in the Keychain any more. Type it instead.".to_string())?;
        let password = String::from_utf8(password).map_err(|_| "The Keychain holds something damaged.".to_string())?;
        UNLOCKED
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get_or_insert_with(HashMap::new)
            .insert(name(wallet), (password, Instant::now() + UNLOCKED_FOR));
        Ok(())
    }
    #[cfg(not(any(target_os = "macos", target_os = "ios")))]
    {
        let _ = wallet;
        Err("Touch ID and Face ID are on Macs and iPhones only.".into())
    }
}

/// A new password for a wallet that will only ever be unlocked by Touch ID
/// or Face ID: long and random, never shown.
pub fn new_password() -> String {
    orien_net::keys::random_seed().iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The Keychain side: kept, found, removed. The Touch ID prompt itself
    /// needs a person, so it's tried by hand.
    #[test]
    #[cfg(target_os = "macos")]
    #[ignore = "writes to this Mac's Keychain"]
    fn keeps_and_forgets_a_password_in_the_keychain() {
        let wallet = std::env::temp_dir().join(format!("orien-bio-{}.wallet", std::process::id()));
        assert!(!enabled(&wallet));
        remember(&wallet, "a long random password").unwrap();
        assert!(enabled(&wallet));
        forget(&wallet);
        assert!(!enabled(&wallet));
        assert_eq!(new_password().len(), 64);
    }
}
