//! SOL and USDC transfers for the wallet's Solana Devnet account.

#[path = "solana_swap.rs"]
pub mod swap;

use orien_net::{store::write_replacing, unlock::Signing};
use orien_swap::solana::{self, Keypair, Landed, Pubkey, Solana};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, atomic::{AtomicU64, Ordering}};
use tauri::AppHandle;

// Serialize approval and receipt writes. A second submission cannot reuse a
// reviewed transfer or get ahead of an unresolved one.
static REVIEW: Mutex<Option<Prepared>> = Mutex::new(None);
static NEXT_REVIEW: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum SendAsset { Sol, Usdc }

struct Prepared {
    id: u64,
    owner: String,
    payer: Pubkey,
    recipient: Pubkey,
    instructions: Vec<solana::Instruction>,
    asset: SendAsset,
    amount: String,
    blockhash: Pubkey,
    last_valid_height: u64,
}

#[derive(Serialize)]
pub struct DevnetQuote {
    id: u64,
    recipient: String,
    amount: String,
    fee: String,
    total: String,
    after: String,
    account_deposit: Option<String>,
}

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum State {
    Pending,
    Confirmed,
    Finalized,
    Failed,
    Expired,
}

impl State {
    fn unresolved(self) -> bool {
        matches!(self, Self::Pending | Self::Confirmed)
    }
}

#[derive(Serialize, Deserialize)]
pub struct DevnetReceipt {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    asset: Option<SendAsset>,
    signature: String,
    recipient: String,
    amount: String,
    last_valid_height: u64,
    state: State,
    acknowledged: bool,
}

fn receipt_path(dir: &Path, owner: &str) -> PathBuf {
    dir.join("solana-devnet-sends-v1").join(owner)
}

fn save_receipt(path: &Path, receipt: &DevnetReceipt) -> Result<(), String> {
    let bytes = serde_json::to_vec(receipt).map_err(|error| error.to_string())?;
    fs::create_dir_all(path.parent().expect("receipt directory"))
        .and_then(|()| write_replacing(path, &bytes, true))
        .map_err(|error| format!("Could not save the Solana transfer receipt: {error}"))
}

fn refresh_receipt(network: &Solana, path: &Path) -> Result<Option<DevnetReceipt>, String> {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("Could not read the previous Solana transfer: {error}")),
    };
    let mut receipt: DevnetReceipt = serde_json::from_slice(&bytes).map_err(|_| "The previous Solana transfer receipt could not be read.")?;
    if !receipt.state.unresolved() {
        return Ok(Some(receipt));
    }
    let state = match network.transaction_status(&receipt.signature) {
        Ok(Some(status)) if status.level == Landed::Finalized => {
            if status.error.is_some() { State::Failed } else { State::Finalized }
        }
        Ok(Some(status)) if status.level == Landed::Confirmed && status.error.is_none() => State::Confirmed,
        Ok(None) if network.finalized_block_height().is_ok_and(|height| height > receipt.last_valid_height) => State::Expired,
        _ => receipt.state,
    };
    if receipt.state != state {
        receipt.state = state;
        save_receipt(path, &receipt)?;
    }
    Ok(Some(receipt))
}

fn active_wallet(app: &AppHandle, expected: &str) -> Result<(PathBuf, PathBuf, String), String> {
    let dir = crate::folder(app)?;
    let path = crate::wallet_file(&dir).ok_or("No wallet is open.")?;
    let owner = crate::peek(&path)?.0.to_string();
    if expected != owner {
        return Err("The open wallet changed. Open Send again.".into());
    }
    Ok((dir, path, owner))
}

fn prepare(
    review: &mut Option<Prepared>,
    network: &Solana,
    dir: &Path,
    path: &Path,
    owner: String,
    to: &str,
    asset: SendAsset,
    amount: Option<&str>,
) -> Result<DevnetQuote, String> {
    *review = None;
    let requested = amount.map(crate::money::read).transpose()?;
    let recipient = solana::address(to).map_err(|_| "Enter a Solana wallet address.")?;
    if !solana::is_wallet_key(&recipient) {
        return Err("Enter a Solana wallet address.".into());
    }
    let payer = solana::address(&crate::assets::devnet_address(dir, path, &owner, None)?).map_err(|error| error.to_string())?;
    if payer == recipient {
        return Err("Enter a different Solana wallet address.".into());
    }
    if refresh_receipt(network, &receipt_path(dir, &owner))?.is_some_and(|receipt| receipt.state.unresolved()) {
        return Err("A previous Devnet transfer is still pending. Check its status from Send.".into());
    }
    if orien_swap::sol::payable(network, &recipient).map_err(|error| error.to_string())?.is_none() {
        return Err("This address belongs to a program or token account. Enter the recipient’s Solana wallet address.".into());
    }
    let balance = network.balance(&payer).map_err(|error| error.to_string())?;
    let (blockhash, last_valid_height) = network.blockhash_lifetime().map_err(|error| error.to_string())?;
    let (instructions, amount, fee, total, after, account_deposit) = match asset {
        SendAsset::Sol => {
            let fee_for = |units| network.fee_for_message(&payer, &[solana::transfer(&payer, &recipient, units)], blockhash).map_err(|e| e.to_string());
            let units = match requested {
                Some(units) => units,
                None => balance.checked_sub(fee_for(balance)?).filter(|units| *units > 0)
                    .ok_or("You don’t have enough Devnet SOL to cover the network fee.")?,
            };
            let fee = fee_for(units)?;
            let total = units.checked_add(fee).ok_or("That amount is too large.")?;
            let after = balance.checked_sub(total).ok_or("You don’t have enough Devnet SOL for this amount and the network fee.")?;
            (vec![solana::transfer(&payer, &recipient, units)], crate::money::show(units.into()), fee, crate::money::show(total.into()), crate::money::show(after.into()), None)
        }
        SendAsset::Usdc => {
            if requested.is_some_and(|n| n % 1000 != 0) {
                return Err("USDC supports up to six decimal places.".into());
            }
            let transfer = solana::devnet_usdc::transfer(network, payer, recipient, requested.map(|n| n / 1000)).map_err(|e| format!("{e:#}"))?;
            let fee = network.fee_for_message(&payer, &transfer.instructions, blockhash).map_err(|e| e.to_string())?;
            if balance < fee.checked_add(transfer.deposit).ok_or("That amount is too large.")? {
                return Err(if transfer.deposit > 0 { "You need more devnet SOL for the network fee and the recipient’s USDC account deposit." }
                    else { "You need devnet SOL to pay the network fee." }.into());
            }
            let check = network.simulate_unsigned(&payer, &transfer.instructions, blockhash).map_err(|e| format!("{e:#}"))?;
            if !check["err"].is_null() { return Err(format!("Solana could not complete this USDC transfer in preflight ({}).", check["err"])); }
            let show = |units: u64| crate::money::show(i128::from(units) * 1000);
            (transfer.instructions, show(transfer.amount), fee, show(transfer.amount), show(transfer.balance_after),
                (transfer.deposit > 0).then(|| crate::money::show(transfer.deposit.into())))
        }
    };
    let id = NEXT_REVIEW.fetch_add(1, Ordering::Relaxed);
    *review = Some(Prepared { id, owner, payer, recipient, instructions, asset, amount: amount.clone(), blockhash, last_valid_height });
    Ok(DevnetQuote { id, recipient: solana::text(&recipient), amount, fee: crate::money::show(fee.into()), total, after, account_deposit })
}

#[tauri::command]
pub async fn solana_devnet_quote(app: AppHandle, wallet_address: String, to: String, amount: String, maximum: Option<bool>, asset: Option<SendAsset>) -> Result<DevnetQuote, String> {
    let (dir, path, owner) = active_wallet(&app, &wallet_address)?;
    tauri::async_runtime::spawn_blocking(move || {
        let mut review = REVIEW.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        prepare(&mut review, &Solana::devnet(), &dir, &path, owner, &to, asset.unwrap_or(SendAsset::Sol), if maximum == Some(true) { None } else { Some(&amount) })
    })
    .await
    .map_err(|error| error.to_string())?
}

fn approve(
    review: &mut Option<Prepared>,
    network: &Solana,
    dir: &Path,
    path: &Path,
    owner: &str,
    quote_id: u64,
    password: Option<&str>,
) -> Result<DevnetReceipt, String> {
    let plan = review.as_ref().filter(|plan| plan.id == quote_id && plan.owner == owner).ok_or("review: again")?;
    let record = receipt_path(dir, owner);
    if refresh_receipt(network, &record)?.is_some_and(|receipt| receipt.state.unresolved()) {
        return Err("A previous Devnet transfer is still pending. Check its status from Send.".into());
    }
    let Signing::Here(wallet) = crate::open_signing(path, password)? else { return Err("password: needed".into()) };
    if wallet.address().to_string() != owner {
        return Err("The open wallet changed. Open Send again.".into());
    }
    let signer = Keypair::from_secret(&wallet.solana_devnet_secret());
    if signer.pubkey() != plan.payer {
        return Err("The Solana address on this device doesn’t match this wallet.".into());
    }
    if !network.blockhash_valid(&plan.blockhash).map_err(|error| error.to_string())? {
        return Err("review: again".into());
    }
    let wire = solana::transaction(&signer, &[], &plan.instructions, plan.blockhash).map_err(|error| error.to_string())?;
    let receipt = broadcast(network, &record, &wire, solana::text(&plan.recipient), plan.amount.clone(), Some(plan.asset), plan.last_valid_height)?;
    *review = None;
    Ok(receipt)
}

fn broadcast(network: &Solana, record: &Path, wire: &[u8], recipient: String, amount: String, asset: Option<SendAsset>, last_valid_height: u64) -> Result<DevnetReceipt, String> {
    let mut receipt = DevnetReceipt {
        signature: solana::signature_of(wire), recipient, amount, asset, last_valid_height,
        state: State::Pending, acknowledged: false,
    };
    // Persist before broadcasting so a lost response cannot cause a duplicate.
    save_receipt(record, &receipt)?;
    if network.send(wire).is_ok_and(|signature| signature == receipt.signature) {
        receipt.acknowledged = true;
        let _ = save_receipt(record, &receipt);
    }
    Ok(receipt)
}

#[tauri::command]
pub async fn solana_devnet_send(app: AppHandle, wallet_address: String, quote_id: u64, password: Option<String>) -> Result<DevnetReceipt, String> {
    let (dir, path, owner) = active_wallet(&app, &wallet_address)?;
    tauri::async_runtime::spawn_blocking(move || {
        let mut review = REVIEW.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        approve(&mut review, &Solana::devnet(), &dir, &path, &owner, quote_id, password.as_deref())
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn solana_devnet_receipt(app: AppHandle, wallet_address: String) -> Result<Option<DevnetReceipt>, String> {
    let (dir, _, owner) = active_wallet(&app, &wallet_address)?;
    tauri::async_runtime::spawn_blocking(move || {
        let _review = REVIEW.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        refresh_receipt(&Solana::devnet(), &receipt_path(&dir, &owner))
    })
    .await
    .map_err(|error| error.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::{Engine, engine::general_purpose::STANDARD};
    use orien_net::keys::Wallet;
    use serde_json::{Value, json};
    use std::io::{BufRead, BufReader, Read, Write};
    use std::net::TcpListener;

    #[test]
    fn reviews_once_and_recovers_an_unacknowledged_transfer() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let worker = std::thread::spawn(move || {
            let mut message = String::new();
            loop {
                let (stream, _) = listener.accept().unwrap();
                let mut input = BufReader::new(stream);
                let mut length = 0;
                loop {
                    let mut line = String::new();
                    assert_ne!(input.read_line(&mut line).unwrap(), 0);
                    if line == "\r\n" { break; }
                    if let Some(value) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                        length = value.trim().parse().unwrap();
                    }
                }
                let mut body = vec![0; length];
                input.read_exact(&mut body).unwrap();
                let request: Value = serde_json::from_slice(&body).unwrap();
                let method = request["method"].as_str().unwrap();
                let result = match method {
                    "getAccountInfo" => json!({ "value": null }),
                    "getBalance" => json!({ "value": 2_000_000_000_u64 }),
                    "getLatestBlockhash" => json!({ "value": { "blockhash": solana::text(&[7; 32]), "lastValidBlockHeight": 99 } }),
                    "getFeeForMessage" => {
                        message = request["params"][0].as_str().unwrap().to_string();
                        json!({ "value": 5_000 })
                    }
                    "isBlockhashValid" => json!({ "value": true }),
                    "sendTransaction" => {
                        let wire = STANDARD.decode(request["params"][0].as_str().unwrap()).unwrap();
                        assert_eq!(&wire[65..], STANDARD.decode(&message).unwrap());
                        Value::Null
                    }
                    "getSignatureStatuses" => json!({ "value": [{ "confirmationStatus": "finalized", "err": null }] }),
                    _ => unreachable!(),
                };
                let body = if method == "sendTransaction" {
                    json!({ "jsonrpc": "2.0", "id": 1, "error": { "code": -32000, "message": "response lost" } })
                } else {
                    json!({ "jsonrpc": "2.0", "id": 1, "result": result })
                }.to_string();
                write!(input.get_mut(), "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
                if method == "getSignatureStatuses" { break; }
            }
        });

        let dir = std::env::temp_dir().join(format!("orien-devnet-send-{}", std::process::id()));
        fs::create_dir(&dir).unwrap();
        let path = dir.join("test-wallet");
        let mut wallet = Wallet::create(&path).unwrap();
        let owner = wallet.address().to_string();
        let _ = crate::assets::devnet_address(&dir, &path, &owner, None).unwrap();
        wallet.set_password(Some("fixture-password")).unwrap();
        let recipient = solana::text(&Keypair::random().pubkey());
        let network = Solana::with(&[&url], None);
        let mut review = None;
        let quote = prepare(&mut review, &network, &dir, &path, owner.clone(), &recipient, SendAsset::Sol, Some("1.25")).unwrap();
        assert_eq!(quote.fee, "0.000005");
        assert_eq!(quote.total, "1.250005");
        assert_eq!(quote.after, "0.749995");
        let quote = prepare(&mut review, &network, &dir, &path, owner.clone(), &recipient, SendAsset::Sol, None).unwrap();
        assert_eq!(quote.amount, "1.999995");
        assert_eq!(quote.after, "0");
        assert!(approve(&mut review, &network, &dir, &path, &owner, quote.id, None).is_err());
        let sent = approve(&mut review, &network, &dir, &path, &owner, quote.id, Some("fixture-password")).unwrap();
        assert!(sent.state == State::Pending && !sent.acknowledged);
        assert!(approve(&mut review, &network, &dir, &path, &owner, quote.id, Some("fixture-password")).is_err());
        let found = refresh_receipt(&network, &receipt_path(&dir, &owner)).unwrap().unwrap();
        assert!(found.state == State::Finalized);
        assert_eq!(found.signature, sent.signature);
        worker.join().unwrap();
        fs::remove_dir_all(&dir).unwrap();
    }
}
