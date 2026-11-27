use super::{DevnetReceipt, Signing, active_wallet, broadcast, refresh_receipt};
use orien_swap::solana::{self, Keypair, Solana, devnet_pool};
use serde::Serialize;
use std::{
    path::Path,
    sync::Mutex,
    time::{Duration, Instant},
};
use tauri::AppHandle;

struct Review {
    id: u64,
    owner: String,
    pay: String,
    created: Instant,
    swap: devnet_pool::Swap,
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

fn display(amount: u64, sol: bool) -> String {
    let scaled = i128::from(amount) * if sol { 1 } else { 1000 };
    format!(
        "{} {}",
        crate::money::show(scaled),
        if sol { "SOL" } else { "USDC" }
    )
}

fn record(dir: &Path, owner: &str) -> std::path::PathBuf {
    dir.join("solana-devnet-swaps-v1").join(owner)
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
        let network = Solana::devnet();
        if refresh_receipt(&network, &record(&dir, &owner))?.is_some_and(|r| r.state.unresolved()) {
            return Err("Your previous swap is still confirming.".into());
        }
        let payer = solana::address(&crate::assets::devnet_address(&dir, &path, &owner, None)?)
            .map_err(|e| e.to_string())?;
        let swap =
            devnet_pool::quote(&network, payer, sol_in, units).map_err(|e| format!("{e:#}"))?;
        let id = super::NEXT_REVIEW.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let quote = Quote {
            id,
            pay: display(units, sol_in),
            receive: display(swap.output, !sol_in),
            minimum: display(swap.minimum, !sol_in),
            fee: display(swap.fee, true),
            deposit: display(swap.account_deposit, true),
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
        let network = Solana::devnet();
        let record = record(&dir, &owner);
        if refresh_receipt(&network, &record)?.is_some_and(|r| r.state.unresolved()) {
            return Err("Your previous swap is still confirming.".into());
        }
        if plan.created.elapsed() > Duration::from_secs(60)
            || !network
                .blockhash_valid(&plan.swap.blockhash)
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
            devnet_pool::POOL.into(),
            plan.pay.clone(),
            plan.swap.last_valid_height,
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
        refresh_receipt(&Solana::devnet(), &record(&dir, &owner))
    })
    .await
    .map_err(|e| e.to_string())?
}
