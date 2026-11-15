//! Touch ID for wallets Orien Wallet locks with it, on Macs.
//!
//! Orien Wallet can seal a wallet with a long random password it never
//! shows, kept in the login Keychain as a generic password: service
//! "network.orien.wallet", account the wallet file's full path. The app
//! reads it only after Touch ID (or the Mac's password) says so, and so does
//! orien: a wallet locked that way opens here with Touch ID, where a typed
//! password would be asked otherwise. Elsewhere, and for wallets with a
//! password of their own, nothing changes.

use std::path::Path;

#[cfg(target_os = "macos")]
const SERVICE: &str = "network.orien.wallet";

/// The names the app may have kept this wallet's password under: its path
/// as given, and its full path.
#[cfg(target_os = "macos")]
fn names(wallet: &Path) -> Vec<String> {
    let mut names = vec![wallet.display().to_string()];
    if let Ok(full) = std::fs::canonicalize(wallet) {
        let full = full.display().to_string();
        if !names.contains(&full) {
            names.push(full);
        }
    }
    names
}

/// Whether Orien Wallet keeps this wallet's password behind Touch ID.
pub fn locks(wallet: &Path) -> bool {
    #[cfg(target_os = "macos")]
    {
        names(wallet).iter().any(|name| security_framework::passwords::get_generic_password(SERVICE, name).is_ok())
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = wallet;
        false
    }
}

/// Asks for Touch ID (or the Mac's password) and, if it is given, returns
/// the wallet's password from the Keychain. None when this wallet is not
/// locked that way; an error when the person said no.
pub fn password(wallet: &Path) -> Option<Result<String, String>> {
    #[cfg(target_os = "macos")]
    {
        use robius_authentication::{AndroidText, BiometricStrength, Context, PolicyBuilder, Text, WindowsText};
        let name = names(wallet)
            .into_iter()
            .find(|name| security_framework::passwords::get_generic_password(SERVICE, name).is_ok())?;
        let asked = (|| {
            let text = Text {
                android: AndroidText { title: "orien", subtitle: None, description: None },
                apple: "unlock your Orien wallet",
                windows: WindowsText::new("orien", "Unlock your wallet").ok_or("prompt text too long")?,
            };
            let policy = PolicyBuilder::new()
                .biometrics(Some(BiometricStrength::Strong))
                .password(true)
                .companion(true)
                .build()
                .ok_or("Touch ID isn't available on this Mac")?;
            let (tx, rx) = std::sync::mpsc::channel();
            Context::new(())
                .authenticate(text, &policy, move |result| {
                    let _ = tx.send(result);
                })
                .map_err(|error| format!("could not ask for Touch ID: {error:?}"))?;
            match rx.recv() {
                Ok(Ok(_)) => {}
                _ => return Err("not unlocked".to_string()),
            }
            let password = security_framework::passwords::get_generic_password(SERVICE, &name)
                .map_err(|_| "this wallet's password is no longer in the Keychain".to_string())?;
            String::from_utf8(password).map_err(|_| "the Keychain holds something damaged".to_string())
        })();
        Some(asked)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = wallet;
        None
    }
}
