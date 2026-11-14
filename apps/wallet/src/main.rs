//! The Orien wallet as a desktop app. The window is plain HTML in `ui/`;
//! everything that touches keys or the network happens here, in Rust,
//! through the same code the `orien` command uses.
//!
//! The wallet itself is the same file the `orien` command uses,
//! `~/.orien/wallet` (or `$ORIEN_HOME/wallet`), so the app and the command
//! line always agree. The app keeps its own things in its own folder (on a
//! Mac, `~/Library/Application Support/network.orien.wallet`): the network
//! file it was built with, how far it has followed the network, payment
//! history, contacts, and a note of any other wallet file the person chose.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod activity;
mod contacts;
mod money;
mod send;
mod update;

use orien_ledger::AccountId;
use orien_net::client::{Client, Head};
use orien_net::keys::{OpenError, Wallet, load_genesis};
use orien_net::unlock::Signing;
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

/// The balance the validators last agreed on, in units, for fee quotes.
static LAST_BALANCE: std::sync::Mutex<Option<i128>> = std::sync::Mutex::new(None);

/// What the window shows about a wallet.
#[derive(Serialize)]
struct Overview {
    address: String,
    /// As people read it, such as "1,250.5". `None` while too few
    /// validators are caught up to agree on it.
    balance: Option<String>,
    /// The most that can be sent once the fee is added, written plainly for
    /// the amount box.
    most: Option<String>,
    #[serde(skip)]
    units: Option<i128>,
    payments: u64,
    /// The wallet has a password and no `orien unlock` session is running,
    /// so sending needs the password.
    locked: bool,
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
    /// The network the choice was made on. A choice made while trying the
    /// app on a test network is ignored on any other.
    #[serde(default)]
    network: Option<String>,
}

/// The id of the network this app was built for.
fn network_id() -> Option<String> {
    serde_json::from_str::<orien_ledger::Genesis>(GENESIS).ok().map(|genesis| genesis.id().to_string())
}

/// Where orien keeps its files: `$ORIEN_HOME`, else `~/.orien`, as the
/// `orien` command does.
fn orien_home() -> PathBuf {
    if let Some(home) = std::env::var_os("ORIEN_HOME") {
        return PathBuf::from(home);
    }
    let user = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")).unwrap_or_default();
    PathBuf::from(user).join(".orien")
}

/// The wallet a new or restored wallet is saved as: the one the `orien`
/// command uses too.
fn shared_wallet() -> PathBuf {
    orien_home().join("wallet")
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

/// The wallet file in use, if there is one: one the person chose, else the
/// one the `orien` command uses.
fn wallet_file(dir: &Path) -> Option<PathBuf> {
    let settings = settings(dir);
    let chosen = settings.wallet.filter(|path| path.is_file() && settings.network == network_id());
    chosen.or_else(|| Some(shared_wallet()).filter(|path| path.is_file()))
}

/// Whether the wallet file is sealed with a password, read without it.
fn has_password(wallet_file: &Path) -> bool {
    fs::read(wallet_file)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
        .is_some_and(|file| file.get("sealed").is_some_and(|sealed| !sealed.is_null()))
}

/// The wallet, ready to sign: through a running `orien unlock` session if
/// there is one, else opened here with its password if it has one. The
/// window recognises the two answers that start with "password:".
fn open_signing(wallet_file: &Path, password: Option<&str>) -> Result<Signing, String> {
    if let Some(unlocked) = Signing::unlocked(wallet_file).map_err(|error| error.to_string())? {
        return Ok(unlocked);
    }
    let password = password.filter(|password| !password.is_empty());
    Wallet::open(wallet_file, password).map(Signing::Here).map_err(|error| match error.downcast_ref::<OpenError>() {
        Some(OpenError::NeedsPassword) => "password: needed".to_string(),
        Some(OpenError::WrongPassword) => "password: wrong".to_string(),
        None => format!("could not open the wallet: {error}"),
    })
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

/// The wallet's address and how many of its payments it knows are final,
/// read without its password.
fn peek(wallet_file: &Path) -> Result<(AccountId, u64), String> {
    Wallet::peek(wallet_file).map_err(|error| format!("could not read the wallet: {error}"))
}

fn overview(wallet_file: &Path, genesis_file: &Path) -> Result<Overview, String> {
    let (address, next_seq) = peek(wallet_file)?;
    let client = connect(genesis_file)?;

    let status = client.account(&address, next_seq);
    let answered = status.views.iter().flatten().count();
    if answered == 0 {
        return Err("no validator could be reached".into());
    }
    Ok(Overview {
        address: address.to_string(),
        balance: status.spendable.map(money::show),
        most: status.spendable.map(|balance| orien_ledger::coins(money::most(&money::fees(GENESIS), balance) as i128)),
        units: status.spendable,
        payments: status.seq,
        locked: has_password(wallet_file) && orien_net::unlock::Session::find(wallet_file).is_none(),
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

/// Makes the folder the shared wallet lives in, readable by this user only.
fn make_home() -> Result<PathBuf, String> {
    let path = shared_wallet();
    if path.exists() {
        return Err(format!("{} already exists, and the app never overwrites a wallet", path.display()));
    }
    let home = orien_home();
    fs::create_dir_all(&home).map_err(|error| format!("cannot create {}: {error}", home.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(&home, fs::Permissions::from_mode(0o700));
    }
    Ok(path)
}

/// Uses the shared wallet from now on, forgetting any other file chosen.
fn use_shared(app: &AppHandle) -> Result<(), String> {
    save_settings(&folder(app)?, &Settings::default())
}

/// Makes a new wallet, as `orien wallet new` does, and returns its 24-word
/// recovery phrase for the person to write down.
#[tauri::command]
async fn create_wallet(app: AppHandle) -> Result<Vec<String>, String> {
    let path = make_home()?;
    let words = tauri::async_runtime::spawn_blocking(move || {
        let wallet = Wallet::create(&path).map_err(|error| error.to_string())?;
        Ok::<_, String>(wallet.phrase().unwrap_or_default().into_iter().map(str::to_string).collect())
    })
    .await
    .map_err(|error| error.to_string())??;
    use_shared(&app)?;
    Ok(words)
}

/// Remakes a wallet from its 24-word recovery phrase, as `orien wallet
/// restore` does. `account` is which computer's account: 1 for the first.
#[tauri::command]
async fn restore_wallet(app: AppHandle, words: String, account: u32) -> Result<(), String> {
    let master = orien_net::phrase::from_words(&words).map_err(|error| {
        let text = error.to_string();
        let mut chars = text.chars();
        chars.next().map(|first| first.to_uppercase().chain(chars).collect::<String>() + ".").unwrap_or(text)
    })?;
    if account == 0 {
        return Err("Accounts are numbered from 1.".into());
    }
    let path = make_home()?;
    tauri::async_runtime::spawn_blocking(move || Wallet::from_master(&path, master, account).map_err(|error| error.to_string()))
        .await
        .map_err(|error| error.to_string())??;
    use_shared(&app)
}

/// Puts a password on the wallet, which then has to be typed to send.
#[tauri::command]
async fn set_password(app: AppHandle, password: String) -> Result<(), String> {
    if password.chars().count() < 8 {
        return Err("Use at least 8 characters.".into());
    }
    let path = wallet_file(&folder(&app)?).ok_or("no wallet yet")?;
    tauri::async_runtime::spawn_blocking(move || {
        let mut wallet = Wallet::load(&path).map_err(|error| error.to_string())?;
        wallet.set_password(Some(&password)).map_err(|error| error.to_string())
    })
    .await
    .map_err(|error| error.to_string())?
}

/// Lets the person pick a wallet file they already have. Returns false if
/// they closed the chooser.
#[tauri::command]
async fn choose_wallet(app: AppHandle) -> Result<bool, String> {
    let picked = app.dialog().file().add_filter("Orien wallet", &["wallet"]).blocking_pick_file();
    let Some(path) = picked.and_then(|file| file.into_path().ok()) else { return Ok(false) };
    Wallet::peek(&path).map_err(|_| "that file is not an Orien wallet".to_string())?;
    save_settings(&folder(&app)?, &Settings { wallet: Some(path), network: network_id() })?;
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
        if let Some(balance) = view.units {
            activity::observe(&dir, &view.address, balance);
        }
        *LAST_BALANCE.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = view.units;
        Ok(view)
    })
    .await
    .map_err(|error| error.to_string())?
}

/// The address book, sorted by name.
#[tauri::command]
fn contacts(app: AppHandle) -> Result<Vec<contacts::Contact>, String> {
    Ok(contacts::list(&folder(&app)?))
}

/// Adds a contact, or edits the one at `previous`.
#[tauri::command]
fn save_contact(app: AppHandle, name: String, address: String, previous: Option<String>) -> Result<(), String> {
    contacts::save(&folder(&app)?, &name, &address, previous.as_deref())
}

#[tauri::command]
fn delete_contact(app: AppHandle, address: String) -> Result<(), String> {
    contacts::delete(&folder(&app)?, &address)
}

/// One payment as the window shows it.
#[derive(Serialize)]
struct ActivityView {
    direction: activity::Direction,
    amount: String,
    fee: Option<String>,
    counterparty: Option<String>,
    time: u64,
}

/// The wallet's payments, newest first.
#[tauri::command]
fn activity(app: AppHandle) -> Result<Vec<ActivityView>, String> {
    let dir = folder(&app)?;
    let (address, _) = peek(&wallet_file(&dir).ok_or("no wallet yet")?)?;
    Ok(activity::list(&dir, &address.to_string())
        .into_iter()
        .map(|entry| ActivityView {
            direction: entry.direction,
            amount: money::show(entry.amount as i128),
            fee: (entry.fee > 0).then(|| money::show(entry.fee as i128)),
            counterparty: entry.counterparty,
            time: entry.time,
        })
        .collect())
}

/// The fee and the balance afterwards for an amount being typed.
#[tauri::command]
fn quote(amount: String) -> money::Quote {
    let balance = *LAST_BALANCE.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    money::quote(&money::fees(GENESIS), balance, &amount)
}

/// Sends `amount` to the address `to` and waits until it is final.
#[tauri::command]
async fn send_payment(app: AppHandle, to: String, amount: String, password: Option<String>) -> Result<send::Sent, String> {
    let to: AccountId = to.trim().parse().map_err(|_| "That is not an Orien address. An address is 64 letters and digits.".to_string())?;
    let amount = money::read(&amount)?;
    let dir = folder(&app)?;
    let wallet_file = wallet_file(&dir).ok_or("no wallet yet")?;
    let genesis = genesis_file(&dir)?;
    tauri::async_runtime::spawn_blocking(move || {
        let _turn = TURNS.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let mut wallet = open_signing(&wallet_file, password.as_deref())?;
        if to == wallet.address() {
            return Err("That is this wallet's own address.".into());
        }
        let client = connect(&genesis)?;
        let sent = send::send(&client, &mut wallet, money::fees(GENESIS), to, amount);
        let _ = client.head().save(&head_file(&genesis));
        if let Ok(sent) = &sent {
            activity::sent(&dir, &wallet.address().to_string(), sent.amount, sent.fee, &sent.to);
        }
        sent
    })
    .await
    .map_err(|error| error.to_string())?
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .invoke_handler(tauri::generate_handler![has_wallet, create_wallet, restore_wallet, set_password, choose_wallet, overview_now, send_payment, activity, quote, contacts, save_contact, delete_contact, update::check_update, update::install_update])
        .run(tauri::generate_context!())
        .expect("the wallet window could not start");
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

    /// Reads this computer's wallet's balance from the live network the app
    /// is built for. Read-only: nothing is signed or sent. Run with
    /// `cargo test -- --ignored live`.
    #[test]
    #[ignore = "reaches the live network"]
    fn reads_the_live_network() {
        let dir = std::env::temp_dir().join("orien-wallet-live-check");
        std::fs::create_dir_all(&dir).unwrap();
        let genesis = super::genesis_file(&dir).unwrap();
        let wallet = super::shared_wallet();
        assert!(wallet.is_file(), "no wallet at {}", wallet.display());
        let view = super::overview(&wallet, &genesis).unwrap();
        println!(
            "{} holds {:?} ORN after {} payments; {} of {} validators answered; epoch {}; password: {}",
            view.address, view.balance, view.payments, view.answered, view.validators, view.epoch, view.locked
        );
        assert!(view.balance.is_some());
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
        let balance = |file: &std::path::Path| super::overview(file, &genesis).unwrap().units.unwrap();
        let (alice_before, bob_before) = (balance(&alice), balance(&bob));
        let fees = orien_net::keys::load_genesis(&genesis).unwrap().fee;

        let client = super::connect(&genesis).unwrap();
        let mut wallet = super::open_signing(&alice, None).unwrap();
        let to = super::peek(&bob).unwrap().0;
        let amount = 250_000_000; // 0.25 coin
        let sent = super::send::send(&client, &mut wallet, fees, to, amount).unwrap();
        assert!(!sent.earlier);
        assert_eq!(sent.fee, fees.on(&orien_ledger::Kind::Payment, amount));
        println!("sent {} with a fee of {} in {:.3} s", sent.amount_text, sent.fee_text, sent.seconds);

        let (alice_after, bob_after) = (balance(&alice), balance(&bob));
        assert_eq!(alice_after, alice_before - amount as i128 - sent.fee as i128);
        assert_eq!(bob_after, bob_before + amount as i128);
    }
}
