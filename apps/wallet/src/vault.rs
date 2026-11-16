//! Keeping the address book in step across devices, through the open
//! wallet's vault on the marketplace server: the book is sealed with a key
//! from the wallet's recovery phrase, so the server keeps only what it can't
//! read, and the same wallet on another device finds and opens it. No
//! marketplace account is needed.

use crate::contacts::{self, Book};
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use orien_net::keys::Wallet;
use orien_net::store::write_replacing;
use orien_net::vault::VaultKeys;
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::time::Duration;

fn keys_file(dir: &Path) -> std::path::PathBuf {
    dir.join("vault-keys.json")
}

fn saved(dir: &Path) -> BTreeMap<String, VaultKeys> {
    fs::read(keys_file(dir)).ok().and_then(|bytes| serde_json::from_slice(&bytes).ok()).unwrap_or_default()
}

#[cfg(any(target_os = "macos", target_os = "ios"))]
const KEYCHAIN_SERVICE: &str = "network.orien.wallet.vault";

/// The kept keys of one wallet: in the Keychain on Macs and iPhones, in a
/// file only this user can read elsewhere.
fn kept(dir: &Path, name: &str) -> Option<VaultKeys> {
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    {
        if let Ok(bytes) = security_framework::passwords::get_generic_password(KEYCHAIN_SERVICE, name) {
            return serde_json::from_slice(&bytes).ok();
        }
        // Kept in a file by an earlier test copy: move it into the Keychain.
        let mut all = saved(dir);
        let keys = all.remove(name)?;
        keep(dir, name, &keys);
        if all.is_empty() {
            let _ = fs::remove_file(keys_file(dir));
        } else if let Ok(text) = serde_json::to_vec_pretty(&all) {
            let _ = write_replacing(&keys_file(dir), &text, true);
        }
        Some(keys)
    }
    #[cfg(not(any(target_os = "macos", target_os = "ios")))]
    {
        saved(dir).get(name).cloned()
    }
}

fn keep(dir: &Path, name: &str, keys: &VaultKeys) {
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    {
        let _ = dir;
        if let Ok(bytes) = serde_json::to_vec(keys) {
            let _ = security_framework::passwords::set_generic_password(KEYCHAIN_SERVICE, name, &bytes);
        }
    }
    #[cfg(not(any(target_os = "macos", target_os = "ios")))]
    {
        // write_replacing makes the file readable by this user only (0600).
        let mut all = saved(dir);
        all.insert(name.to_string(), keys.clone());
        if let Ok(text) = serde_json::to_vec_pretty(&all) {
            let _ = write_replacing(&keys_file(dir), &text, true);
        }
    }
}

/// Forgets a wallet's kept keys (when the wallet is removed from the app).
pub fn forget(dir: &Path, wallet_file: &Path) {
    let name = crate::biometric::name(wallet_file);
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    let _ = security_framework::passwords::delete_generic_password(KEYCHAIN_SERVICE, &name);
    let mut all = saved(dir);
    if all.remove(&name).is_some()
        && let Ok(text) = serde_json::to_vec_pretty(&all)
    {
        let _ = write_replacing(&keys_file(dir), &text, true);
    }
}

/// The vault keys of the wallet in `wallet_file`: worked out from the
/// wallet (which needs its password, if it has one) and then kept, privately,
/// so later syncs don't need the password. None until they can be worked out.
pub fn keys(dir: &Path, wallet_file: &Path, password: Option<&str>) -> Option<VaultKeys> {
    let name = crate::biometric::name(wallet_file);
    if let Some(keys) = kept(dir, &name) {
        return Some(keys);
    }
    let keys = Wallet::open(wallet_file, password).ok()?.vault();
    keep(dir, &name, &keys);
    Some(keys)
}

fn url(keys: &VaultKeys) -> String {
    format!("{}/api/market/vault/{}", crate::board::base(), keys.id)
}

fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new().timeout(Duration::from_secs(15)).user_agent("orien-wallet").build()
}

/// The book as the vault holds it: None if there's nothing there yet.
fn get(keys: &VaultKeys) -> Result<Option<Book>, String> {
    let answer = match agent().get(&url(keys)).call() {
        Ok(response) => response.into_json::<Value>().map_err(|_| "The contacts store sent an answer this app can't read.".to_string())?,
        Err(ureq::Error::Status(404, _)) => return Ok(None),
        Err(_) => return Err("The contacts store can't be reached.".into()),
    };
    let sealed = STANDARD.decode(answer["blob"].as_str().unwrap_or_default()).map_err(|_| "The contacts store holds something damaged.")?;
    let plain = keys.open(&sealed).map_err(|error| error.to_string())?;
    serde_json::from_slice(&plain).map(Some).map_err(|_| "The contacts store holds something this app can't read.".into())
}

fn put(keys: &VaultKeys, book: &Book) -> Result<(), String> {
    let plain = serde_json::to_vec(book).map_err(|error| error.to_string())?;
    let sealed = keys.seal(&plain).map_err(|error| error.to_string())?;
    match agent().put(&url(keys)).send_json(json!({ "token": keys.token, "blob": STANDARD.encode(sealed) })) {
        Ok(_) => Ok(()),
        Err(ureq::Error::Status(_, response)) => {
            let said = response.into_json::<Value>().ok().and_then(|v| v["error"].as_str().map(str::to_string));
            Err(said.unwrap_or_else(|| "The contacts store refused the update.".into()))
        }
        Err(_) => Err("The contacts store can't be reached.".into()),
    }
}

/// Brings the book into step with the vault of every wallet in the app
/// whose keys are known, not only the open one: the book (contacts and
/// wallet icons) is the device's, and another device may have a different
/// wallet open. Ok if at least one vault was reached.
pub fn sync_all(dir: &Path, open: Option<&Path>) -> Result<bool, String> {
    let mut wallets: Vec<std::path::PathBuf> = open.map(Path::to_path_buf).into_iter().collect();
    for named in crate::known_wallets(dir) {
        if !wallets.contains(&named.path) {
            wallets.push(named.path);
        }
    }
    let (mut reached, mut last) = (false, None);
    for wallet in &wallets {
        let Some(keys) = keys(dir, wallet, None) else { continue };
        match sync(dir, &keys) {
            Ok(()) => reached = true,
            Err(error) => last = Some(error),
        }
    }
    // A second round, so what came in from a later vault reaches the earlier ones.
    if reached {
        for wallet in &wallets {
            if let Some(keys) = keys(dir, wallet, None) {
                let _ = sync(dir, &keys);
            }
        }
    }
    match (reached, last) {
        (false, Some(error)) => Err(error),
        _ => Ok(reached),
    }
}

/// Brings this device's address book and the vault's into step: takes in
/// what the vault has, then puts back the merged book if it differs.
pub fn sync(dir: &Path, keys: &VaultKeys) -> Result<(), String> {
    let remote = get(keys)?;
    if let Some(remote) = &remote {
        contacts::merge(dir, remote)?;
    }
    let mine = contacts::book(dir);
    let same = remote.as_ref().is_some_and(|remote| {
        let names = |b: &Book| b.contacts.iter().map(|c| (c.address.clone(), c.name.clone(), c.updated)).collect::<Vec<_>>();
        let mut a = names(remote);
        let mut b = names(&mine);
        a.sort();
        b.sort();
        a == b && remote.deleted == mine.deleted && remote.icons == mine.icons
    });
    if !same {
        put(keys, &mine)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The keys are worked out once, kept (in the Keychain here, never in a
    /// file), found again without the wallet, and forgotten with it.
    #[test]
    #[cfg(target_os = "macos")]
    #[ignore = "writes to this Mac's Keychain"]
    fn keeps_the_vault_keys_in_the_keychain() {
        let base = std::env::temp_dir().join(format!("orien-vkeys-{}", std::process::id()));
        fs::create_dir_all(&base).unwrap();
        let wallet = base.join("test.wallet");
        let made = Wallet::create(&wallet).unwrap();
        let first = keys(&base, &wallet, None).unwrap();
        assert_eq!(first.id, made.vault().id);
        assert!(!keys_file(&base).exists(), "never kept in a file on a Mac");
        fs::remove_file(&wallet).unwrap();
        assert_eq!(keys(&base, &wallet, None).unwrap().id, first.id, "found again without the wallet");
        forget(&base, &wallet);
        assert!(keys(&base, &wallet, None).is_none());
        let _ = fs::remove_dir_all(&base);
    }

    /// Two devices with the same wallet keep one address book: added on one,
    /// shown on the other; removed on one, removed on the other. Against the
    /// live store, with a throwaway wallet.
    #[test]
    #[ignore = "reaches the live contacts store"]
    fn two_devices_share_one_address_book() {
        let base = std::env::temp_dir().join(format!("orien-vault-{}", std::process::id()));
        let (mac, phone) = (base.join("mac"), base.join("phone"));
        fs::create_dir_all(&mac).unwrap();
        fs::create_dir_all(&phone).unwrap();
        let wallet = base.join("test.wallet");
        let made = Wallet::create(&wallet).unwrap();
        let keys = made.vault();
        let bob = "01487c022b1d56bcff496df629ddca5c1488e6dcffec284311c228eecdea321b";
        let carol = "25e6dcd54bf195cb5a9fa522ed03db563254fab0898e37d440498086e1995b91";

        contacts::save(&mac, "Bob", bob, None).unwrap();
        sync(&mac, &keys).unwrap();
        sync(&phone, &keys).unwrap();
        assert_eq!(contacts::list(&phone).iter().map(|c| c.name.as_str()).collect::<Vec<_>>(), vec!["Bob"]);

        contacts::save(&phone, "Carol", carol, None).unwrap();
        sync(&phone, &keys).unwrap();
        std::thread::sleep(Duration::from_millis(1100));
        contacts::delete(&mac, bob).unwrap();
        sync(&mac, &keys).unwrap();
        sync(&phone, &keys).unwrap();
        let names = |dir: &Path| contacts::list(dir).iter().map(|c| c.name.clone()).collect::<Vec<_>>();
        assert_eq!(names(&mac), vec!["Carol"]);
        assert_eq!(names(&phone), vec!["Carol"], "a removal on one device is a removal on the other");
        let _ = fs::remove_dir_all(&base);
    }
}

