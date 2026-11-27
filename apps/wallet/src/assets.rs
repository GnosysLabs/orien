use orien_net::keys::Wallet;
use orien_net::store::write_replacing;
use orien_net::unlock::Signing;
use orien_swap::solana::{self, Keypair};
use serde::Serialize;
use std::fs;
use std::path::Path;
use std::sync::Mutex;
use tauri::AppHandle;

static DEVNET_ADDRESSES: Mutex<()> = Mutex::new(());

/// Reading this public address for the switcher never unlocks a wallet.
pub(crate) fn cached_devnet_address(dir: &Path, owner: &str) -> Option<String> {
    let raw = fs::read_to_string(dir.join("solana-devnet-addresses-v1").join(owner)).ok()?;
    solana::address(&raw).ok().map(|key| solana::text(&key))
}

/// Keep only the public Solana address, so balances work after the wallet locks.
pub(crate) fn remember_devnet_address(dir: &Path, wallet: &Wallet) -> String {
    let key = Keypair::from_secret(&wallet.solana_devnet_secret());
    let address = solana::text(&key.pubkey());
    let owner = wallet.address().to_string();
    let _cache = DEVNET_ADDRESSES.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    if cached_devnet_address(dir, &owner).as_deref() != Some(address.as_str()) {
        let cache_dir = dir.join("solana-devnet-addresses-v1");
        let result = fs::create_dir_all(&cache_dir)
            .and_then(|()| write_replacing(&cache_dir.join(owner), address.as_bytes(), true));
        if let Err(error) = result {
            // The wallet and its recovery phrase must stay available even if a cache write fails.
            eprintln!("Could not save the public Solana address: {error}");
        }
    }
    address
}

pub(crate) fn devnet_address(dir: &Path, path: &Path, wallet_address: &str, password: Option<&str>) -> Result<String, String> {
    let (owner, _) = crate::peek(path)?;
    if owner.to_string() != wallet_address {
        return Err("The open wallet changed. Try again.".into());
    }
    // Only the public key is cached on this device, so checking a balance
    // after a restart never needs to open the secret again.
    if let Some(address) = cached_devnet_address(dir, wallet_address) {
        return Ok(address);
    }
    let Signing::Here(wallet) = crate::open_signing(path, password)? else {
        return Err("password: needed".into());
    };
    if wallet.address() != owner {
        return Err("The open wallet changed. Try again.".into());
    }
    Ok(remember_devnet_address(dir, &wallet))
}

#[tauri::command]
pub async fn solana_devnet_address(app: AppHandle, wallet_address: String, password: Option<String>) -> Result<String, String> {
    let dir = crate::folder(&app)?;
    let path = crate::wallet_file(&dir).ok_or("No wallet is open.")?;
    tauri::async_runtime::spawn_blocking(move || devnet_address(&dir, &path, &wallet_address, password.as_deref()))
        .await
        .map_err(|error| error.to_string())?
}

#[derive(Serialize)]
pub struct DevnetBalances {
    sol: Option<String>,
    usdc: Option<String>,
}

#[tauri::command]
pub async fn solana_devnet_balances(address: String) -> Result<DevnetBalances, String> {
    let owner = solana::address(&address).map_err(|error| error.to_string())?;
    tauri::async_runtime::spawn_blocking(move || {
        let network = crate::solswaps::network();
        let mint = solana::address(network.usdc).expect("valid USDC mint");
        let sol = network.shown_balance(&owner).ok().map(|units| crate::money::show(units.into()));
        let usdc = network.token_balance(&owner, &mint).ok().map(|units| crate::money::show(i128::from(units) * 1000));
        DevnetBalances { sol, usdc }
    })
    .await
    .map_err(|error| error.to_string())
}

#[derive(Serialize)]
pub struct ActivityRow {
    signature: String,
    time: Option<u64>,
    failed: bool,
    finalized: bool,
    amount: Option<String>,
    asset: Option<&'static str>,
    peer: Option<String>,
    sent: bool,
    received: Option<String>,
}

#[derive(Serialize)]
pub struct DevnetHistory {
    rows: Vec<ActivityRow>,
    incomplete: bool,
}

fn activity_row(entry: solana::history::HistoryEntry) -> ActivityRow {
    use solana::history::Movement;
    let (units, asset, peer, received) = match entry.movement {
        Some(Movement::Sol { units, peer }) => (units, Some("SOL"), Some(peer), None),
        Some(Movement::Token { units }) => (units * 1000, Some("USDC"), None, None),
        Some(Movement::PoolSwap { sol, token }) if sol > 0 => (token * 1000, Some("USDC"), None, Some(format!("{} SOL", crate::money::show(sol)))),
        Some(Movement::PoolSwap { sol, token }) => (sol, Some("SOL"), None, Some(format!("{} USDC", crate::money::show(token * 1000)))),
        None => (0, None, None, None),
    };
    ActivityRow { signature: entry.signature, time: entry.time, failed: entry.failed, finalized: entry.finalized,
        amount: asset.map(|_| crate::money::show(units.abs())), asset, peer, sent: units < 0, received }
}

#[tauri::command]
pub async fn solana_devnet_transaction_activity(address: String, signature: String) -> Result<Option<ActivityRow>, String> {
    let owner = solana::address(&address).map_err(|error| error.to_string())?;
    tauri::async_runtime::spawn_blocking(move || {
        crate::solswaps::network().confirmed_transaction(&owner, &signature).map(|entry| entry.map(activity_row)).map_err(|e| e.to_string())
    }).await.map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn solana_devnet_history(address: String) -> Result<DevnetHistory, String> {
    let owner = solana::address(&address).map_err(|error| error.to_string())?;
    tauri::async_runtime::spawn_blocking(move || {
        let network = crate::solswaps::network();
        let mint = solana::address(network.usdc).expect("valid USDC mint");
        let history = network.recent_history(&owner, &mint).map_err(|error| error.to_string())?;
        let rows = history.entries.into_iter().map(activity_row).collect();
        Ok(DevnetHistory { rows, incomplete: history.incomplete })
    }).await.map_err(|error| error.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wallet_setup_remembers_solana_before_locking() {
        let dir = std::env::temp_dir().join(format!("orien-devnet-setup-{}", std::process::id()));
        fs::create_dir(&dir).unwrap();
        let path = dir.join("wallet");
        let mut wallet = crate::initialize_wallet(&dir, &path, None, 1).unwrap();
        let owner = wallet.address().to_string();
        let address = solana::text(&Keypair::from_secret(&wallet.solana_devnet_secret()).pubkey());
        wallet.set_password(Some("fixture-password")).unwrap();
        assert_eq!(cached_devnet_address(&dir, &owner), Some(address.clone()));
        assert_eq!(devnet_address(&dir, &path, &owner, None).unwrap(), address);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn remembers_the_public_devnet_address_after_unlock() {
        let dir = std::env::temp_dir().join(format!("orien-devnet-address-{}", std::process::id()));
        fs::create_dir(&dir).unwrap();
        let path = dir.join("test-wallet");
        let mut wallet = Wallet::create(&path).unwrap();
        let owner = wallet.address().to_string();
        let password = "fixture-password";
        wallet.set_password(Some(password)).unwrap();
        assert_eq!(devnet_address(&dir, &path, &owner, None).unwrap_err(), "password: needed");
        let address = devnet_address(&dir, &path, &owner, Some(password)).unwrap();
        assert_eq!(devnet_address(&dir, &path, &owner, None).unwrap(), address);
        assert!(devnet_address(&dir, &path, "another wallet", None).is_err());
        fs::remove_dir_all(&dir).unwrap();
    }
}
