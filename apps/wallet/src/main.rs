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
#[cfg(target_os = "linux")]
mod linux;
mod money;
mod send;
mod swaps;
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
/// restore` does: the same wallet, address and ORN as on any other computer.
#[tauri::command]
async fn restore_wallet(app: AppHandle, words: String) -> Result<(), String> {
    let master = orien_net::phrase::from_words(&words).map_err(|error| {
        let text = error.to_string();
        let mut chars = text.chars();
        chars.next().map(|first| first.to_uppercase().chain(chars).collect::<String>() + ".").unwrap_or(text)
    })?;
    let path = make_home()?;
    tauri::async_runtime::spawn_blocking(move || Wallet::from_master(&path, master, 1).map_err(|error| error.to_string()))
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

/// Swaps run with the app's client and wallet.
fn with_swaps<T: Send + 'static>(
    app: &AppHandle,
    password: Option<String>,
    work: impl FnOnce(&Path, &swaps::AppOrien) -> Result<T, String> + Send + 'static,
) -> tauri::async_runtime::JoinHandle<Result<T, String>> {
    let dir = folder(app);
    tauri::async_runtime::spawn_blocking(move || {
        let dir = dir?;
        let wallet_file = wallet_file(&dir).ok_or("no wallet yet")?;
        let genesis = genesis_file(&dir)?;
        let _turn = TURNS.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let client = connect(&genesis)?;
        let orien = swaps::AppOrien {
            client: &client,
            wallet_file,
            password,
            fees: money::fees(GENESIS),
            network: network_id().unwrap_or_default(),
            epoch_seconds: serde_json::from_str::<orien_ledger::Genesis>(GENESIS).map_or(3_600, |genesis| genesis.epoch_seconds),
        };
        let result = work(&dir, &orien);
        let _ = client.head().save(&head_file(&genesis));
        result
    })
}

fn views(swaps: &[swaps::Kept]) -> Vec<swaps::View> {
    let mut views: Vec<swaps::View> = swaps.iter().map(swaps::view).collect();
    views.sort_by_key(|view| std::cmp::Reverse(view.created));
    views
}

/// Every swap this wallet has made, newest first.
#[tauri::command]
fn swap_list(app: AppHandle) -> Result<Vec<swaps::View>, String> {
    Ok(views(&swaps::load(&folder(&app)?)))
}

/// Sells `orn` ORN for `btc` Bitcoin to the Orien address `buyer`: locks the
/// ORN for them and makes the offer code.
#[tauri::command]
async fn swap_sell(app: AppHandle, buyer: String, orn: String, btc: String, payout: String, password: Option<String>) -> Result<swaps::View, String> {
    let orn = money::read(&orn)?;
    let sats = swaps::read_btc(&btc)?;
    with_swaps(&app, password, move |dir, orien| {
        // Saved with its secret before anything is signed; then the ORN are
        // locked, and the swap saved again.
        let kept = swaps::sell(orien, &buyer, orn, sats, &payout)?;
        let mut all = swaps::load(dir);
        all.push(kept);
        swaps::save(dir, &all)?;
        let (_, chain) = swaps::bitcoin();
        let kept = all.last_mut().expect("just added");
        swaps::advance(orien, &chain, kept);
        let view = swaps::view(kept);
        let problem = kept.problem.clone();
        swaps::save(dir, &all)?;
        match problem {
            Some(problem) if problem.starts_with("password:") => Err(problem),
            _ => Ok(view),
        }
    })
    .await
    .map_err(|error| error.to_string())?
}

/// Takes up a seller's offer: checks it on Orien and makes the reply code.
#[tauri::command]
async fn swap_buy(app: AppHandle, offer: String, refund: String) -> Result<swaps::View, String> {
    with_swaps(&app, None, move |dir, orien| {
        let kept = swaps::buy(orien, &offer, &refund)?;
        let mut all = swaps::load(dir);
        if all.iter().any(|known| known.id == kept.id) {
            return Err("This offer is already taken up here.".into());
        }
        all.push(kept.clone());
        swaps::save(dir, &all)?;
        Ok(swaps::view(&kept))
    })
    .await
    .map_err(|error| error.to_string())?
}

/// The seller pastes the buyer's reply.
#[tauri::command]
async fn swap_accept(app: AppHandle, id: String, reply: String) -> Result<swaps::View, String> {
    with_swaps(&app, None, move |dir, orien| {
        let mut all = swaps::load(dir);
        let kept = all.iter_mut().find(|kept| kept.id == id).ok_or("That swap is not here.")?;
        swaps::accept(orien, kept, &reply)?;
        let view = swaps::view(kept);
        swaps::save(dir, &all)?;
        Ok(view)
    })
    .await
    .map_err(|error| error.to_string())?
}

/// Moves every unfinished swap along. The window calls this every so often
/// while any swap is under way. Claiming ORN signs, so it may need the
/// wallet's password.
#[tauri::command]
async fn swap_step(app: AppHandle, password: Option<String>) -> Result<Vec<swaps::View>, String> {
    with_swaps(&app, password, move |dir, orien| {
        let mut all = swaps::load(dir);
        if swaps::active(&all) {
            swaps::step(orien, dir, &mut all)?;
        }
        Ok(views(&all))
    })
    .await
    .map_err(|error| error.to_string())?
}

fn main() {
    #[cfg(target_os = "linux")]
    linux::register_appimage();
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .invoke_handler(tauri::generate_handler![has_wallet, create_wallet, restore_wallet, set_password, choose_wallet, overview_now, send_payment, activity, quote, contacts, save_contact, delete_contact, update::check_update, update::install_update, swap_list, swap_sell, swap_buy, swap_accept, swap_step])
        .run(tauri::generate_context!())
        .expect("the wallet window could not start");
}

#[cfg(test)]
mod tests {
    /// A whole swap with the app's own code: a real lock and a real claim on
    /// a running local test network, and a pretend Bitcoin network. Start one
    /// first, with epochs short enough for a swap's 48 hours to fit in 720:
    /// `orien devnet init --dir /tmp/orien-swap-net --epoch-seconds 600 && orien devnet
    /// run --dir /tmp/orien-swap-net`, then run with ORIEN_DEVNET set to it.
    #[test]
    fn a_whole_swap_on_a_local_network() {
        use orien_swap::bitcoin::{Address, CompressedPublicKey, Network};
        use orien_swap::swap::{Offer, Orien, Reply, Stage, Swap};
        use orien_swap::testing::TestChain;
        let Ok(root) = std::env::var("ORIEN_DEVNET") else {
            eprintln!("ORIEN_DEVNET is not set; skipped");
            return;
        };
        let root = std::path::Path::new(&root);
        let genesis = root.join("genesis.json");
        let client = super::connect(&genesis).unwrap();
        let network = orien_net::keys::load_genesis(&genesis).unwrap();
        let side = |wallet: std::path::PathBuf| super::swaps::AppOrien {
            client: &client,
            wallet_file: wallet,
            password: None,
            fees: network.fee,
            network: network.id().to_string(),
            epoch_seconds: network.epoch_seconds,
        };
        let seller = side(root.join("wallets/alice.wallet"));
        let fresh = root.join(format!("buyer-{}.wallet", std::process::id()));
        orien_net::keys::Wallet::create(&fresh).unwrap();
        let buyer = side(fresh.clone());
        let btc = || {
            let (_, key) = orien_swap::htlc::new_key();
            Address::p2wpkh(&CompressedPublicKey(key.inner), Network::Regtest).to_string()
        };
        let bitcoin = TestChain::at(100);
        let chain = orien_swap::chain::Explorers::one(bitcoin.clone());

        let orn = 5 * orien_ledger::COIN;
        let mut sell = Swap::sell(&seller, Network::Regtest, buyer.me(), orn, 1_000_000, &btc()).unwrap();
        sell.advance(&chain, &seller).unwrap();
        let offer = Offer::read(&sell.offer(seller.network()).unwrap().code()).unwrap();
        let (mut buy, reply) = Swap::buy(&buyer, &chain, Network::Regtest, &offer, &btc()).unwrap();
        sell.accept(&chain, &seller, &Reply::read(&reply.code()).unwrap()).unwrap();
        bitcoin.pay(&buy.bitcoin_address().unwrap(), 1_000_000);
        bitcoin.mine(3);
        for _ in 0..4 {
            sell.advance(&chain, &seller).unwrap();
            buy.advance(&chain, &buyer).unwrap();
        }
        bitcoin.mine(3);
        sell.advance(&chain, &seller).unwrap();
        buy.advance(&chain, &buyer).unwrap();
        assert_eq!((sell.stage, buy.stage), (Stage::Done, Stage::Done));
        let status = client.account(&buyer.me(), 0);
        assert_eq!(status.spendable, Some(orn as i128 - network.fee.min as i128), "the buyer has the ORN, less the claim's fee");
        let _ = std::fs::remove_file(fresh);
    }

    /// Reads a wallet from a running local test network, if there is one:
    /// `orien devnet init && orien devnet run` in the repository root, then
    /// run with ORIEN_DEVNET=devnet.
    #[test]
    fn reads_a_wallet_from_the_local_network() {
        let Ok(root) = std::env::var("ORIEN_DEVNET") else {
            eprintln!("ORIEN_DEVNET does not name a running local test network; skipped");
            return;
        };
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
        let Ok(root) = std::env::var("ORIEN_DEVNET") else {
            eprintln!("ORIEN_DEVNET does not name a running local test network; skipped");
            return;
        };
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
