//! One approval links the two public addresses of one local wallet.
use crate::{Signing, biometric, board, folder, known, money, open_signing, peek};
use orien_net::store::write_replacing;
use orien_swap::solana::Keypair;
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    fs,
    path::{Path, PathBuf},
    time::Duration,
};
use tauri::AppHandle;

#[derive(Deserialize)]
struct Proof {
    address: String,
    to: String,
    orn: String,
    verified: bool,
    solana_devnet: Option<String>,
}

pub(crate) fn check_account(dir: &Path, account: i64) -> Result<(), String> {
    if board::me(dir)?.and_then(|me| me["id"].as_i64()) != Some(account) {
        return Err("The signed-in profile changed. Open the wallet card again.".into());
    }
    Ok(())
}

fn receipt(dir: &Path, account: i64, orn: &str) -> PathBuf {
    dir.join(format!("profile-link-{account}-{orn}"))
}

pub(crate) fn clear_attempt(dir: &Path, account: i64, orn: &str) {
    // Only construct receipt paths from a canonical Orien address.
    if orn.parse::<orien_ledger::AccountId>().is_err() {
        return;
    }
    let file = receipt(dir, account, orn);
    let _ = fs::remove_file(file.with_extension("attempt"));
    let _ = fs::remove_file(file);
}

fn read_proof(dir: &Path, orn: &str) -> Result<Proof, String> {
    let states: Vec<Proof> =
        serde_json::from_value(board::wallet_states(dir)?).map_err(|e| e.to_string())?;
    states
        .into_iter()
        .find(|p| p.address == orn)
        .ok_or("The link request was removed.".into())
}

impl Proof {
    fn fingerprint(&self) -> String {
        format!("{} {} {}", board::base(), self.to, self.orn)
    }
    fn submitted(&self, file: &Path) -> bool {
        fs::read_to_string(file).is_ok_and(|saved| saved == self.fingerprint())
    }
}

#[tauri::command]
pub async fn profile_link_quote(
    app: AppHandle,
    account: i64,
    path: String,
    orn: String,
) -> Result<Value, String> {
    let dir = folder(&app)?;
    let path = known(&dir, &path)?;
    tauri::async_runtime::spawn_blocking(move || {
        if peek(&path)?.0.to_string() != orn {
            return Err("The wallet changed.".into());
        }
        check_account(&dir, account)?;
        let p: Proof =
            serde_json::from_value(board::verify(&dir, &orn)?).map_err(|e| e.to_string())?;
        if p.address != orn {
            return Err("The wallet changed.".into());
        }
        let paid = p.verified || p.submitted(&receipt(&dir, account, &orn));
        Ok(
            json!({"to":p.to, "payment":if paid { None } else { Some(p.orn) },
                  "linked":p.solana_devnet.is_some(), "biometric":biometric::enabled(&path),
                  "locked":crate::has_password(&path) && biometric::unlocked(&path).is_none()}),
        )
    })
    .await
    .map_err(|e| e.to_string())?
}

fn send_proof(dir: &Path, file: &Path, p: &Proof, signing: &mut Signing) -> Result<(), String> {
    let amount = money::read(&p.orn)?;
    let recipient = p.to.parse().map_err(|_| "Invalid proof destination.")?;
    let genesis = crate::genesis_file(dir)?;
    if recipient == signing.address() || amount == 0 || amount >= money::read("0.001")? {
        return Err("Invalid wallet-link payment.".into());
    }
    let client = crate::connect(&genesis)?;
    let fees = money::fees(crate::GENESIS);
    let kind = orien_ledger::Kind::Payment;
    if amount.saturating_add(fees.on(&kind, amount)) >= money::read("0.001")? {
        return Err("The total exceeds the approved wallet-link limit.".into());
    }
    // Reserve an order number before signing so a crash or an ambiguous
    // network response cannot turn a retry into a second payment.
    let status = client.account(&signing.address(), signing.next_seq());
    if status.views.iter().all(Option::is_none) {
        return Err("No validator could be reached.".into());
    }
    let attempt = file.with_extension("attempt");
    let fingerprint = p.fingerprint();
    let saved = match fs::read_to_string(&attempt) {
        Ok(saved) => Some(saved),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => return Err(e.to_string()),
    };
    let seq = if let Some(previous) = saved
        .as_deref()
        .and_then(|s| s.split_once('\n'))
        .filter(|(f, _)| *f == fingerprint)
    {
        previous.1.parse::<u64>().map_err(|e| e.to_string())?
    } else {
        write_replacing(
            &attempt,
            format!("{fingerprint}\n{}", status.seq).as_bytes(),
            true,
        )
        .map_err(|e| e.to_string())?;
        status.seq
    };
    if seq < status.seq {
        let cert = client.certificate(&signing.address(), seq).ok_or("Waiting for Network to recognize the previous link attempt. Check the wallet card again shortly.")?;
        if cert.order.kind != kind || cert.order.to != recipient || cert.order.amount != amount {
            fs::remove_file(attempt).map_err(|e| e.to_string())?;
            return Err(
                "Another payment used this order number. Review the link request again.".into(),
            );
        }
        // A matching certificate means we can wait for the profile to catch up.
    } else {
        let result =
            crate::send::send_order_at(&client, signing, fees, kind, recipient, amount, Some(seq));
        let _ = client.head().save(&crate::head_file(&genesis));
        let sent = result?;
        crate::activity::sent(
            dir,
            &p.address,
            sent.amount,
            sent.fee,
            &sent.to,
            Some(sent.seq),
        );
        if sent.earlier {
            fs::remove_file(attempt).map_err(|e| e.to_string())?;
            return Err("An earlier payment finished. Review and confirm linking again.".into());
        }
    }
    write_replacing(file, p.fingerprint().as_bytes(), true)
        .map_err(|_| "Payment sent. Wait for ORN confirmation before trying again.".to_string())?;
    Ok(())
}

#[tauri::command]
pub async fn profile_link_confirm(
    app: AppHandle,
    account: i64,
    path: String,
    orn: String,
    to: String,
    payment: Option<String>,
    password: Option<String>,
) -> Result<(), String> {
    let dir = folder(&app)?;
    let path = known(&dir, &path)?;
    tauri::async_runtime::spawn_blocking(move || {
        let file = receipt(&dir, account, &orn);
        let (key, mut ready, fingerprint) = {
            let _turn = crate::TURNS.lock().unwrap_or_else(|e| e.into_inner());
            if peek(&path)?.0.to_string() != orn {
                return Err("The wallet changed.".into());
            }
            check_account(&dir, account)?;
            let p = read_proof(&dir, &orn)?;
            if p.solana_devnet.is_some() {
                return Ok(());
            }
            let mut signing = open_signing(&path, password.as_deref())?;
            drop(password);
            let Signing::Here(wallet) = &signing else {
                return Err("Unlock this local wallet to link both addresses.".into());
            };
            if wallet.address().to_string() != orn {
                return Err("The wallet changed.".into());
            }
            let key = Keypair::from_secret(&wallet.solana_devnet_secret());
            crate::assets::remember_devnet_address(&dir, wallet);
            if !p.verified && !p.submitted(&file) {
                if to != p.to || payment.as_deref() != Some(&p.orn) {
                    return Err("Review the updated link request.".into());
                }
                send_proof(&dir, &file, &p, &mut signing)?;
            }
            (key, p.verified, p.fingerprint())
        };
        // Release the ORN signer and payment lock before waiting for Network.
        // An interruption can be resumed by confirming the same card without another payment.
        for _ in 0..36 {
            if ready {
                break;
            }
            std::thread::sleep(Duration::from_secs(5));
            check_account(&dir, account)?;
            let p = read_proof(&dir, &orn)?;
            if p.fingerprint() != fingerprint {
                return Err("The link request changed.".into());
            }
            if p.solana_devnet.is_some() {
                return Ok(());
            }
            ready = p.verified;
        }
        if !ready {
            return Err(
                "Your ORN payment is awaiting confirmation. Finish linking from this card later."
                    .into(),
            );
        }
        board::finish_devnet_link(&dir, account, &orn, &key)?;
        clear_attempt(&dir, account, &orn);
        Ok(())
    })
    .await
    .map_err(|e| e.to_string())?
}
