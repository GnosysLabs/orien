//! The Orien wallet as a desktop app. The window is plain HTML in `ui/`;
//! everything that touches keys or the network happens here, in Rust,
//! through the same code the `orien` command uses.
//!
//! The app keeps everything in its own folder (on a Mac,
//! `~/Library/Application Support/network.orien.wallet`): the network file
//! it was built with, how far it has followed the network, and either the
//! wallet it created or a note of where the person's own wallet file is.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod activity;
mod send;

use orien_ledger::AccountId;
use orien_net::client::{Client, Head};
use orien_net::keys::{Wallet, load_genesis};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use tauri::{AppHandle, Manager};
use tauri_plugin_dialog::DialogExt;

/// Sending and checking the balance take turns, so a check that runs while
/// a payment is going out never sees half of it and records it twice.
static TURNS: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// The network this copy of the app was built for.
const GENESIS: &str = include_str!(concat!(env!("OUT_DIR"), "/genesis.json"));

/// What the window shows about a wallet.
#[derive(Serialize)]
struct Overview {
    address: String,
    /// `None` while too few validators are caught up to agree on it.
    balance: Option<String>,
    payments: u64,
    answered: usize,
    validators: usize,
    epoch: u64,
}

/// Which wallet the app uses, kept in `settings.json`.
#[derive(Default, Serialize, Deserialize)]
struct Settings {
    /// A wallet file the person chose. The app uses it where it is and never
    /// copies it: two copies of one wallet could sign two different payments
    /// with the same one-time key.
    wallet: Option<PathBuf>,
}

fn folder(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app.path().app_data_dir().map_err(|error| error.to_string())?;
    fs::create_dir_all(&dir).map_err(|error| format!("cannot create {}: {error}", dir.display()))?;
    Ok(dir)
}

fn settings(dir: &Path) -> Settings {
    fs::read(dir.join("settings.json")).ok().and_then(|bytes| serde_json::from_slice(&bytes).ok()).unwrap_or_default()
}

fn save_settings(dir: &Path, settings: &Settings) -> Result<(), String> {
    let text = serde_json::to_string_pretty(settings).map_err(|error| error.to_string())?;
    fs::write(dir.join("settings.json"), text).map_err(|error| error.to_string())
}

/// The wallet file in use, if there is one.
fn wallet_file(dir: &Path) -> Option<PathBuf> {
    let own = dir.join("main.wallet");
    settings(dir).wallet.filter(|path| path.is_file()).or_else(|| own.is_file().then_some(own))
}

/// Writes the built-in network file to the app's folder, where the client
/// reads it and keeps its progress next to it.
fn genesis_file(dir: &Path) -> Result<PathBuf, String> {
    if GENESIS.is_empty() {
        return Err("this copy of the app was built without a network".into());
    }
    let path = dir.join("genesis.json");
    if fs::read_to_string(&path).ok().as_deref() != Some(GENESIS) {
        fs::write(&path, GENESIS).map_err(|error| error.to_string())?;
    }
    Ok(path)
}

fn head_file(genesis: &Path) -> PathBuf {
    let mut name = genesis.as_os_str().to_owned();
    name.push(".head");
    PathBuf::from(name)
}

/// A client caught up with the network's checkpoints, starting from
/// wherever this computer last followed it to.
fn connect(genesis_file: &Path) -> Result<Client, String> {
    let genesis =
        load_genesis(genesis_file).map_err(|error| format!("could not read the network file: {error}"))?;
    let client = match Head::load(&head_file(genesis_file), &genesis) {
        Some(head) => Client::resume(&genesis, head),
        None => Client::new(&genesis),
    };
    if client.follow() > 0 {
        let _ = client.head().save(&head_file(genesis_file));
    }
    Ok(client)
}

fn open(wallet_file: &Path) -> Result<Wallet, String> {
    Wallet::load(wallet_file).map_err(|error| format!("could not open the wallet: {error}"))
}

fn overview(wallet_file: &Path, genesis_file: &Path) -> Result<Overview, String> {
    let wallet = open(wallet_file)?;
    let client = connect(genesis_file)?;

    let status = client.account(&wallet.address(), wallet.next_seq());
    let answered = status.views.iter().flatten().count();
    if answered == 0 {
        return Err("no validator could be reached".into());
    }
    Ok(Overview {
        address: wallet.address().to_string(),
        balance: status.spendable.map(|balance| balance.to_string()),
        payments: status.seq,
        answered,
        validators: client.validators(),
        epoch: client.epoch(),
    })
}

/// Whether the app has a wallet yet.
#[tauri::command]
fn has_wallet(app: AppHandle) -> Result<bool, String> {
    Ok(wallet_file(&folder(&app)?).is_some())
}

/// Makes a new wallet in the app's own folder.
#[tauri::command]
fn create_wallet(app: AppHandle) -> Result<(), String> {
    let dir = folder(&app)?;
    Wallet::create(&dir.join("main.wallet")).map_err(|error| error.to_string())?;
    save_settings(&dir, &Settings { wallet: None })
}

/// Lets the person pick a wallet file they already have. Returns false if
/// they closed the chooser.
#[tauri::command]
async fn choose_wallet(app: AppHandle) -> Result<bool, String> {
    let picked = app.dialog().file().add_filter("Orien wallet", &["wallet"]).blocking_pick_file();
    let Some(path) = picked.and_then(|file| file.into_path().ok()) else { return Ok(false) };
    Wallet::load(&path).map_err(|_| "that file is not an Orien wallet".to_string())?;
    save_settings(&folder(&app)?, &Settings { wallet: Some(path) })?;
    Ok(true)
}

/// Asks the validators about the wallet's account.
#[tauri::command]
async fn overview_now(app: AppHandle) -> Result<Overview, String> {
    let dir = folder(&app)?;
    let wallet = wallet_file(&dir).ok_or("no wallet yet")?;
    let genesis = genesis_file(&dir)?;
    tauri::async_runtime::spawn_blocking(move || {
        let _turn = TURNS.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let view = overview(&wallet, &genesis)?;
        if let Some(balance) = view.balance.as_deref().and_then(|text| text.parse().ok()) {
            activity::observe(&dir, &view.address, balance);
        }
        Ok(view)
    })
    .await
    .map_err(|error| error.to_string())?
}

/// The wallet's payments, newest first.
#[tauri::command]
fn activity(app: AppHandle) -> Result<Vec<activity::Entry>, String> {
    let dir = folder(&app)?;
    let wallet = open(&wallet_file(&dir).ok_or("no wallet yet")?)?;
    Ok(activity::list(&dir, &wallet.address().to_string()))
}

/// Sends `amount` to the address `to` and waits until it is final.
#[tauri::command]
async fn send_payment(app: AppHandle, to: String, amount: String) -> Result<send::Sent, String> {
    let to: AccountId = to.trim().parse().map_err(|_| "That is not an Orien address. An address is 64 letters and digits.".to_string())?;
    let amount: u64 = amount
        .trim()
        .replace(',', "")
        .parse()
        .ok()
        .filter(|amount| *amount > 0)
        .ok_or("Enter an amount: a whole number above zero.")?;
    let dir = folder(&app)?;
    let wallet_file = wallet_file(&dir).ok_or("no wallet yet")?;
    let genesis = genesis_file(&dir)?;
    tauri::async_runtime::spawn_blocking(move || {
        let _turn = TURNS.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let mut wallet = open(&wallet_file)?;
        if to == wallet.address() {
            return Err("That is this wallet's own address.".into());
        }
        let client = connect(&genesis)?;
        let sent = send::send(&client, &mut wallet, to, amount);
        let _ = client.head().save(&head_file(&genesis));
        if let Ok(sent) = &sent {
            activity::sent(&dir, &wallet.address().to_string(), sent.amount, &sent.to);
        }
        sent
    })
    .await
    .map_err(|error| error.to_string())?
}

#[cfg(test)]
mod tests {
    /// Reads a wallet from a running local test network, if there is one:
    /// `orien devnet init && orien devnet run` in the repository root.
    #[test]
    fn reads_a_wallet_from_the_local_network() {
        let root = std::env::var("ORIEN_DEVNET").unwrap_or_else(|_| "../../devnet".into());
        let root = std::path::Path::new(&root);
        if !root.join("genesis.json").is_file() {
            eprintln!("no local test network at {}; skipped", root.display());
            return;
        }
        let view = super::overview(&root.join("wallets/alice.wallet"), &root.join("genesis.json")).unwrap();
        assert_eq!(view.address.len(), 64);
        assert!(view.balance.is_some());
        assert_eq!(view.answered, view.validators);
        println!("{} holds {:?}", view.address, view.balance);
    }

    /// Sends 1 from alice to bob on a running local test network, through
    /// the app's own sending code, and checks both balances moved.
    #[test]
    fn sends_a_payment_on_the_local_network() {
        let root = std::env::var("ORIEN_DEVNET").unwrap_or_else(|_| "../../devnet".into());
        let root = std::path::Path::new(&root);
        if !root.join("genesis.json").is_file() {
            eprintln!("no local test network at {}; skipped", root.display());
            return;
        }
        let (alice, bob, genesis) =
            (root.join("wallets/alice.wallet"), root.join("wallets/bob.wallet"), root.join("genesis.json"));
        let balance = |file: &std::path::Path| super::overview(file, &genesis).unwrap().balance.unwrap();
        let (alice_before, bob_before): (i128, i128) =
            (balance(&alice).parse().unwrap(), balance(&bob).parse().unwrap());

        let client = super::connect(&genesis).unwrap();
        let mut wallet = super::open(&alice).unwrap();
        let to = super::open(&bob).unwrap().address();
        let sent = super::send::send(&client, &mut wallet, to, 1).unwrap();
        assert!(!sent.earlier);
        println!("sent 1 in {:.3} s", sent.seconds);

        let (alice_after, bob_after): (i128, i128) =
            (balance(&alice).parse().unwrap(), balance(&bob).parse().unwrap());
        assert_eq!(alice_after, alice_before - 1);
        assert_eq!(bob_after, bob_before + 1);
    }
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![has_wallet, create_wallet, choose_wallet, overview_now, send_payment, activity])
        .run(tauri::generate_context!())
        .expect("the wallet window could not start");
}
