use orien_net::store::write_replacing;
use orien_net::unlock::Signing;
use orien_swap::solana::{self, Keypair, Solana};
use serde::Serialize;
use std::fs;
use std::path::Path;
use std::sync::Mutex;
use tauri::AppHandle;

// Circle's USDC mint on Solana devnet.
use orien_swap::solana::devnet_pool::USDC as DEVNET_USDC;
static DEVNET_ADDRESSES: Mutex<()> = Mutex::new(());

pub(crate) fn devnet_address(dir: &Path, path: &Path, wallet_address: &str, password: Option<&str>) -> Result<String, String> {
    let (owner, _) = crate::peek(path)?;
    if owner.to_string() != wallet_address {
        return Err("The open wallet changed. Try again.".into());
    }
    let _cache = DEVNET_ADDRESSES.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let cache_dir = dir.join("solana-devnet-addresses-v1");
    let cache = cache_dir.join(owner.to_string());
    // Only the public key is cached on this device, so checking a balance
    // after a restart never needs to open the secret again.
    if let Some(key) = fs::read_to_string(&cache).ok().and_then(|text| solana::address(&text).ok()) {
        return Ok(solana::text(&key));
    }
    let Signing::Here(wallet) = crate::open_signing(path, password)? else {
        return Err("password: needed".into());
    };
    if wallet.address() != owner {
        return Err("The open wallet changed. Try again.".into());
    }
    let key = Keypair::from_secret(&wallet.solana_devnet_secret());
    let address = solana::text(&key.pubkey());
    let _ = fs::create_dir_all(cache_dir).and_then(|()| write_replacing(&cache, address.as_bytes(), true));
    Ok(address)
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
        let network = Solana::devnet();
        let mint = solana::address(DEVNET_USDC).expect("valid devnet USDC mint");
        let sol = network.balance(&owner).ok().map(|units| crate::money::show(units.into()));
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
        Solana::devnet().confirmed_transaction(&owner, &signature).map(|entry| entry.map(activity_row)).map_err(|e| e.to_string())
    }).await.map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn solana_devnet_history(address: String) -> Result<DevnetHistory, String> {
    let owner = solana::address(&address).map_err(|error| error.to_string())?;
    tauri::async_runtime::spawn_blocking(move || {
        let mint = solana::address(DEVNET_USDC).expect("valid devnet USDC mint");
        let history = Solana::devnet().recent_history(&owner, &mint).map_err(|error| error.to_string())?;
        let rows = history.entries.into_iter().map(activity_row).collect();
        Ok(DevnetHistory { rows, incomplete: history.incomplete })
    }).await.map_err(|error| error.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;
    use orien_net::keys::Wallet;

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
