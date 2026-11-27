use super::{DevnetReceipt, Signing, active_wallet, broadcast, refresh_receipt};
use orien_swap::solana::{self, Keypair, Solana, devnet_pool, jupiter};
use serde::Serialize;
use std::{
    path::Path,
    sync::Mutex,
    time::{Duration, Instant},
};
use tauri::AppHandle;

/// A swap through the test network's pool, or through Jupiter on Solana.
enum Planned {
    Pool(devnet_pool::Swap),
    Jupiter(jupiter::Swap),
}

impl Planned {
    fn blockhash(&self) -> &solana::Pubkey {
        match self { Planned::Pool(s) => &s.blockhash, Planned::Jupiter(s) => &s.blockhash }
    }
    fn last_valid_height(&self) -> u64 {
        match self { Planned::Pool(s) => s.last_valid_height, Planned::Jupiter(s) => s.last_valid_height }
    }
    fn signed(&self, key: &Keypair) -> anyhow::Result<Vec<u8>> {
        match self { Planned::Pool(s) => s.signed(key), Planned::Jupiter(s) => s.signed(key) }
    }
    fn through(&self) -> String {
        match self { Planned::Pool(_) => devnet_pool::POOL.into(), Planned::Jupiter(_) => "Jupiter".into() }
    }
}

struct Review {
    id: u64,
    owner: String,
    pay: String,
    created: Instant,
    swap: Planned,
}
static REVIEW: Mutex<Option<Review>> = Mutex::new(None);

#[derive(Serialize)]
pub struct Quote {
    id: u64,
    pay: String,
    receive: String,
    minimum: String,
    fee: String,
    deposit: String,
}

/// The test network's pool for test copies; Solana itself, through Jupiter, otherwise.
fn network() -> Solana {
    crate::solswaps::network()
}

fn display(amount: u64, sol: bool) -> String {
    let scaled = i128::from(amount) * if sol { 1 } else { 1000 };
    format!(
        "{} {}",
        crate::money::show(scaled),
        if sol { "SOL" } else { "USDC" }
    )
}

/// Kept apart per network, as sends are.
fn record(dir: &Path, owner: &str) -> std::path::PathBuf {
    let folder = if crate::solswaps::cluster() == "devnet" { "solana-devnet-swaps-v1" } else { "solana-swaps-v1" };
    dir.join(folder).join(owner)
}

#[tauri::command]
pub async fn devnet_swap_quote(
    app: AppHandle,
    wallet_address: String,
    asset: String,
    amount: String,
) -> Result<Quote, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let (dir, path, owner) = active_wallet(&app, &wallet_address)?;
        let mut review = REVIEW.lock().unwrap_or_else(|p| p.into_inner());
        *review = None;
        let sol_in = match asset.as_str() {
            "SOL" => true,
            "USDC" => false,
            _ => return Err("Choose SOL or USDC.".into()),
        };
        let mut units = crate::money::read(&amount)?;
        if !sol_in {
            if units % 1000 != 0 {
                return Err("USDC supports up to six decimal places.".into());
            }
            units /= 1000;
        }
        let network = network();
        if refresh_receipt(&network, &record(&dir, &owner))?.is_some_and(|r| r.state.unresolved()) {
            return Err("Your previous swap is still confirming.".into());
        }
        let payer = solana::address(&crate::assets::devnet_address(&dir, &path, &owner, None)?)
            .map_err(|e| e.to_string())?;
        let id = super::NEXT_REVIEW.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let (swap, output, minimum, fee, deposit) = if crate::solswaps::cluster() == "devnet" {
            let s = devnet_pool::quote(&network, payer, sol_in, units).map_err(|e| format!("{e:#}"))?;
            let figures = (s.output, s.minimum, s.fee, s.account_deposit);
            (Planned::Pool(s), figures.0, figures.1, figures.2, figures.3)
        } else {
            let s = jupiter::quote(&network, payer, sol_in, units).map_err(|e| format!("{e:#}"))?;
            let figures = (s.output, s.minimum, s.fee, s.account_deposit);
            (Planned::Jupiter(s), figures.0, figures.1, figures.2, figures.3)
        };
        let quote = Quote {
            id,
            pay: display(units, sol_in),
            receive: display(output, !sol_in),
            minimum: display(minimum, !sol_in),
            fee: display(fee, true),
            deposit: display(deposit, true),
        };
        *review = Some(Review {
            id,
            owner,
            pay: quote.pay.clone(),
            created: Instant::now(),
            swap,
        });
        Ok(quote)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn devnet_swap_approve(
    app: AppHandle,
    wallet_address: String,
    quote_id: u64,
    password: Option<String>,
) -> Result<DevnetReceipt, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let mut review = REVIEW.lock().unwrap_or_else(|p| p.into_inner());
        let (dir, path, owner) = active_wallet(&app, &wallet_address)?;
        let plan = review
            .as_ref()
            .filter(|p| p.id == quote_id && p.owner == owner)
            .ok_or("review: again")?;
        let network = network();
        let record = record(&dir, &owner);
        if refresh_receipt(&network, &record)?.is_some_and(|r| r.state.unresolved()) {
            return Err("Your previous swap is still confirming.".into());
        }
        if plan.created.elapsed() > Duration::from_secs(60)
            || !network
                .blockhash_valid(plan.swap.blockhash())
                .map_err(|e| e.to_string())?
        {
            return Err("review: again".into());
        }
        let Signing::Here(wallet) = crate::open_signing(&path, password.as_deref())? else {
            return Err("password: needed".into());
        };
        let wire = plan
            .swap
            .signed(&Keypair::from_secret(&wallet.solana_devnet_secret()))
            .map_err(|e| e.to_string())?;
        active_wallet(&app, &owner)?;
        let receipt = broadcast(
            &network,
            &record,
            &wire,
            plan.swap.through(),
            plan.pay.clone(),
            None,
            plan.swap.last_valid_height(),
        )?;
        *review = None;
        Ok(receipt)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn devnet_swap_status(
    app: AppHandle,
    wallet_address: String,
) -> Result<Option<DevnetReceipt>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let (dir, _, owner) = active_wallet(&app, &wallet_address)?;
        let _guard = REVIEW.lock().unwrap_or_else(|p| p.into_inner());
        refresh_receipt(&network(), &record(&dir, &owner))
    })
    .await
    .map_err(|e| e.to_string())?
}
