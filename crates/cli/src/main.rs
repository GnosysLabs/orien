use anyhow::{Context, Result, anyhow, bail};
use clap::{Args, Parser, Subcommand};
use orien_crypto::keytree::KeyTree;
use orien_ledger::{
    AccountId, Allocation, BitcoinBlock, COIN, Certificate, Genesis, Kind, Receipt, Signature, SignedOrder, Validator, ValidatorInfo,
    coins, parse_coins, sign_stake_proof,
};
use orien_ledger::Signer;
use orien_net::client::{AccountStatus, Client, Done, Head, TransferError, Unfinished};
use orien_net::keys::{OpenError, ValidatorKey, Wallet, load_genesis, save_genesis};
use orien_net::node::{Node, NodeConfig};
use orien_net::unlock::{self, Limits, Session, Signing};
use orien_net::wire::{Request, Response};
use std::fs;
use std::io::{BufRead, IsTerminal, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

mod release;

#[derive(Parser)]
#[command(name = "orien", version, about = "A quantum-proof payment network with no blocks and no global ordering")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Args)]
struct Network {
    /// Genesis file describing the network (default: ~/.orien/genesis.json,
    /// else genesis.json here)
    #[arg(short, long, env = "ORIEN_GENESIS")]
    genesis: Option<PathBuf>,
}

impl Network {
    /// The genesis file to use: the one named, else the one the installer
    /// put in ~/.orien, else one in this folder.
    fn file(&self) -> PathBuf {
        if let Some(file) = &self.genesis {
            return file.clone();
        }
        let installed = orien_home().join("genesis.json");
        if installed.is_file() { installed } else { PathBuf::from("genesis.json") }
    }
}

/// The Orien skill, which teaches AI assistants to use orien.
const SKILL: &str = include_str!("../../../skill/orien/SKILL.md");

/// The user's home folder.
fn user_home() -> PathBuf {
    PathBuf::from(std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")).unwrap_or_default())
}

/// `orien skill install`: writes the skill into each assistant's skills
/// folder, for the assistants this computer has.
fn install_skill(dir: Option<PathBuf>, refresh: bool) -> Result<()> {
    let home = user_home();
    let targets: Vec<(String, PathBuf)> = match dir {
        Some(dir) => vec![(dir.display().to_string(), dir)],
        None => [("Claude Code", ".claude"), ("Codex", ".codex")]
            .into_iter()
            .filter(|(_, folder)| home.join(folder).is_dir())
            .map(|(name, folder)| (name.to_string(), home.join(folder).join("skills")))
            .collect(),
    };
    if targets.is_empty() && !refresh {
        bail!(
            "found no AI assistant here (no ~/.claude or ~/.codex folder). For another assistant, \
             name its skills folder: orien skill install --dir FOLDER"
        );
    }
    for (name, skills) in targets {
        let file = skills.join("orien").join("SKILL.md");
        if refresh && !file.is_file() {
            continue;
        }
        fs::create_dir_all(file.parent().expect("a skill file has a folder"))?;
        fs::write(&file, SKILL).with_context(|| format!("cannot write {}", file.display()))?;
        println!("installed the Orien skill for {name}: {}", file.display());
    }
    if !refresh {
        println!("start a new conversation with your assistant, then ask it things like \"what's my ORN balance?\"");
    }
    Ok(())
}

/// Where orien keeps its files: $ORIEN_HOME, else ~/.orien.
fn orien_home() -> PathBuf {
    if let Some(home) = std::env::var_os("ORIEN_HOME") {
        return PathBuf::from(home);
    }
    let user = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")).unwrap_or_default();
    PathBuf::from(user).join(".orien")
}

/// The wallet to use when none is named: ~/.orien/wallet.
fn default_wallet() -> PathBuf {
    orien_home().join("wallet")
}

#[derive(Subcommand)]
enum Command {
    /// Create a wallet or show its address
    #[command(subcommand)]
    Wallet(WalletCommand),
    /// Show an account's balance
    Balance {
        /// A wallet file or an account address (default: your wallet)
        account: Option<String>,
        #[command(flatten)]
        network: Network,
    },
    /// Send a payment and wait until it is final
    Send {
        /// The wallet (default: ~/.orien/wallet)
        #[arg(short, long, env = "ORIEN_WALLET")]
        wallet: Option<PathBuf>,
        /// Recipient: an account address or a wallet file
        #[arg(long)]
        to: String,
        /// In coins, with up to nine decimals: 12 or 0.25
        #[arg(long, value_parser = parse_coins)]
        amount: u64,
        /// Save the proof of payment to this file
        #[arg(long)]
        receipt: Option<PathBuf>,
        #[command(flatten)]
        network: Network,
    },
    /// Lock coins as stake behind a validator, which then joins the
    /// validators at a checkpoint if its stake is among the largest
    Stake {
        /// The wallet (default: ~/.orien/wallet)
        #[arg(short, long, env = "ORIEN_WALLET")]
        wallet: Option<PathBuf>,
        /// In coins, with up to nine decimals: 12 or 0.25
        #[arg(long, value_parser = parse_coins)]
        amount: u64,
        /// The validator's key file. Its first one-time key signs that the
        /// validator backs this stake; nothing else ever uses that key.
        #[arg(long)]
        validator_key: PathBuf,
        /// Where other validators and wallets reach the validator
        #[arg(long)]
        address: String,
        #[command(flatten)]
        network: Network,
    },
    /// Start releasing staked coins. They stay locked, and can still be
    /// taken for cheating, until the wait set in the genesis file is over.
    Unstake {
        /// The wallet (default: ~/.orien/wallet)
        #[arg(short, long, env = "ORIEN_WALLET")]
        wallet: Option<PathBuf>,
        /// In coins, with up to nine decimals: 12 or 0.25
        #[arg(long, value_parser = parse_coins)]
        amount: u64,
        #[command(flatten)]
        network: Network,
    },
    /// Finish a payment that was signed but never became final
    Resume {
        /// The wallet (default: ~/.orien/wallet)
        #[arg(short, long, env = "ORIEN_WALLET")]
        wallet: Option<PathBuf>,
        /// Which payment to finish, if more than one is waiting: the first
        /// characters of its id, as `balance` shows it
        #[arg(long)]
        payment: Option<String>,
        /// Save the proof of payment to this file
        #[arg(long)]
        receipt: Option<PathBuf>,
        #[command(flatten)]
        network: Network,
    },
    /// Check a payment receipt offline, using only the genesis file
    Verify {
        receipt: PathBuf,
        #[command(flatten)]
        network: Network,
    },
    /// Do proof-of-work for an account, which earns it a share of the coins
    /// each checkpoint releases. Runs until stopped with Ctrl-C.
    Mine {
        /// The account to be paid: a wallet file or an address (default:
        /// your wallet)
        account: Option<String>,
        /// How many processor cores to use (default: all of them)
        #[arg(long)]
        threads: Option<usize>,
        #[command(flatten)]
        network: Network,
    },
    /// Show every validator's state
    Status {
        #[command(flatten)]
        network: Network,
    },
    /// Run one validator
    Node {
        /// This validator's key file
        #[arg(short, long)]
        key: PathBuf,
        /// Directory for this validator's data
        #[arg(short, long)]
        data: PathBuf,
        /// Address to listen on (default: this validator's address in the
        /// genesis file; needed for a validator that joins by stake)
        #[arg(short, long)]
        listen: Option<String>,
        #[command(flatten)]
        network: Network,
    },
    /// Create a validator key
    Keygen {
        /// Where to write the key
        out: PathBuf,
        /// The key can sign 2^height batches
        #[arg(long, default_value_t = 16)]
        height: u8,
    },
    /// Let programs you trust, such as an AI assistant, sign with a wallet
    /// that has a password, for a while and up to an amount, without
    /// learning the password. Run it yourself, in your own terminal.
    Unlock {
        /// The wallet file (default: ~/.orien/wallet)
        file: Option<PathBuf>,
        /// How long to stay unlocked: 30m, 2h, 1d (asked if not given)
        #[arg(long = "for", value_parser = parse_duration)]
        duration: Option<u64>,
        /// The most it may spend, in ORN, fees included (asked if not given)
        #[arg(long = "up-to", value_parser = parse_coins)]
        up_to: Option<u64>,
        /// Show whether the wallet is unlocked, for how long and how much
        #[arg(long, conflicts_with_all = ["duration", "up_to"])]
        status: bool,
    },
    /// End an unlock early
    Lock {
        /// The wallet file (default: ~/.orien/wallet)
        file: Option<PathBuf>,
    },
    /// The background helper `orien unlock` starts
    #[command(hide = true)]
    UnlockHelper {
        file: PathBuf,
        #[arg(long)]
        until: u64,
        #[arg(long)]
        spend: u64,
    },
    /// Teach AI assistants (Claude Code, Codex) to use orien for you
    #[command(subcommand)]
    Skill(SkillCommand),
    /// Update orien to the newest version, if the Orien release key signed it
    Update,
    /// Make and use the Orien release key (for whoever publishes releases)
    #[command(subcommand, hide = true)]
    Release(ReleaseCommand),
    /// Write or check the genesis file of a real network
    #[command(subcommand)]
    Genesis(GenesisCommand),
    /// Set up and run a local test network
    #[command(subcommand)]
    Devnet(DevnetCommand),
}

#[derive(Subcommand)]
enum WalletCommand {
    /// Create a new wallet, with a 24-word recovery phrase to write down
    New {
        /// The wallet file (default: ~/.orien/wallet)
        file: Option<PathBuf>,
    },
    /// Rebuild a wallet from its 24-word recovery phrase, typed in when asked.
    /// Use a different account number on each computer.
    Restore {
        /// The wallet file to write (default: ~/.orien/wallet)
        file: Option<PathBuf>,
        /// Which account of the phrase: 1 on your first computer, 2 on the
        /// next, and so on
        #[arg(long, default_value_t = 1)]
        account: u32,
    },
    /// Show a wallet's recovery phrase again. Anyone who sees it can spend
    /// your coins.
    Phrase {
        /// The wallet file (default: ~/.orien/wallet)
        file: Option<PathBuf>,
    },
    /// Put a password on a wallet, change it, or remove it
    Password {
        /// The wallet file (default: ~/.orien/wallet)
        file: Option<PathBuf>,
    },
    /// Show a wallet's account address
    Address {
        /// The wallet file (default: ~/.orien/wallet)
        file: Option<PathBuf>,
    },
}

#[derive(Subcommand)]
enum SkillCommand {
    /// Install the Orien skill for each AI assistant on this computer
    Install {
        /// Install into this skills folder instead, for another assistant
        #[arg(long)]
        dir: Option<PathBuf>,
        /// Only refresh copies already installed (orien update does this)
        #[arg(long, hide = true)]
        refresh: bool,
    },
    /// Print the skill, to read it or give it to another assistant
    Show,
}

#[derive(Subcommand)]
enum ReleaseCommand {
    /// Make a new release key
    Keygen { out: PathBuf },
    /// Sign a release's SHA256SUMS, writing SHA256SUMS.sig beside it
    Sign {
        #[arg(long)]
        key: PathBuf,
        sums: PathBuf,
    },
}

#[derive(Subcommand)]
enum GenesisCommand {
    /// Write the genesis file of a real network: no coins, one-hour epochs,
    /// and the launch validators as seats sharing 3 million coins of weight
    New {
        /// Where to write it
        out: PathBuf,
        /// The network's name
        #[arg(long, default_value = "orien")]
        name: String,
        /// A launch validator, as NAME,ROOT,HEIGHT,ADDRESS. `orien keygen`
        /// prints this line, with only the public part of the key. Repeat
        /// once per validator.
        #[arg(long = "validator", required = true)]
        validators: Vec<String>,
        /// The Bitcoin block announced in advance, as HEIGHT:HASH with the
        /// hash as block explorers show it
        #[arg(long, value_parser = parse_bitcoin_block)]
        bitcoin: Option<BitcoinBlock>,
        /// When the network starts, in seconds since 1970 (default: now)
        #[arg(long)]
        start: Option<u64>,
        /// Seconds from one checkpoint to the next
        #[arg(long, default_value_t = 3600)]
        epoch_seconds: u64,
    },
    /// Print what a genesis file says, and its network id, so that others
    /// can check they hold the same file
    Show { file: PathBuf },
}

#[derive(Subcommand)]
enum DevnetCommand {
    /// Create keys, wallets and a genesis file for a local network
    Init {
        #[arg(long, default_value = "devnet")]
        dir: PathBuf,
        #[arg(long, default_value_t = 4)]
        validators: usize,
        #[arg(long, default_value_t = 16)]
        height: u8,
        #[arg(long, default_value = "127.0.0.1")]
        host: String,
        #[arg(long, default_value_t = 7001)]
        base_port: u16,
        /// One validator's address, as other machines reach it. Repeat it
        /// once per validator to spread a network over several machines;
        /// this replaces --validators, --host and --base-port.
        #[arg(long = "address")]
        addresses: Vec<String>,
        /// The network's name
        #[arg(long, default_value = "orien-devnet")]
        name: String,
        /// Seconds from one checkpoint to the next. A real network uses an
        /// hour; a short one lets you watch checkpoints happen.
        #[arg(long, default_value_t = 60)]
        epoch_seconds: u64,
        /// Hand out no coins: every coin comes from mining, as on the real
        /// network. Without it, alice and bob start with a million each.
        #[arg(long)]
        from_nothing: bool,
        /// Anchor the network to a Bitcoin block, as HEIGHT:HASH with the
        /// hash as block explorers show it. Nobody can mine before it.
        #[arg(long, value_parser = parse_bitcoin_block)]
        bitcoin: Option<BitcoinBlock>,
    },
    /// Run every validator of a local network in this process
    Run {
        #[arg(long, default_value = "devnet")]
        dir: PathBuf,
    },
    /// Start a payment and abandon it halfway, as a lost device would
    Interrupt {
        #[arg(long, default_value = "devnet")]
        dir: PathBuf,
        /// Wallet that pays (a name under <dir>/wallets)
        #[arg(long, default_value = "alice")]
        wallet: String,
        /// Wallet that is paid
        #[arg(long, default_value = "carol")]
        to: String,
        #[arg(long, default_value = "250", value_parser = parse_coins)]
        amount: u64,
    },
    /// Try to cheat: sign two different payments with the same number
    DoubleSpend {
        #[arg(long, default_value = "devnet")]
        dir: PathBuf,
        /// Wallet to cheat with (a name under <dir>/wallets)
        #[arg(long, default_value = "bob")]
        wallet: String,
        /// Send each payment to a different half of the validators at the
        /// same moment, instead of one after the other
        #[arg(long)]
        split: bool,
    },
}

/// Lets the program end quietly when whatever reads its output stops
/// reading, as `orien status | head -1` does. Rust ignores that signal by
/// default, and then every later line printed is a crash with a backtrace.
#[cfg(unix)]
fn end_quietly_on_a_closed_pipe() {
    unsafe extern "C" {
        fn signal(signal: i32, handler: usize) -> usize;
    }
    const SIGPIPE: i32 = 13;
    const DEFAULT: usize = 0;
    // SAFETY: restoring a signal's default handling before any thread starts.
    unsafe {
        signal(SIGPIPE, DEFAULT);
    }
}

#[cfg(not(unix))]
fn end_quietly_on_a_closed_pipe() {}

fn main() {
    end_quietly_on_a_closed_pipe();
    if let Err(error) = run(Cli::parse()) {
        eprintln!("error: {error:#}");
        std::process::exit(1);
    }
}

fn run(cli: Cli) -> Result<()> {
    match cli.command {
        Command::Wallet(WalletCommand::New { file }) => {
            let file = file.unwrap_or_else(default_wallet);
            if let Some(folder) = file.parent() {
                fs::create_dir_all(folder)?;
            }
            let wallet = Wallet::create(&file)?;
            println!("created {}", file.display());
            println!("address  {}", wallet.address());
            println!("account  1 of its recovery phrase");
            println!();
            show_phrase(&wallet);
            let mut wallet = wallet;
            if std::io::stdin().is_terminal() {
                println!();
                println!("A password keeps this wallet file useless to anyone who copies it.");
                match choose_password()? {
                    Some(password) => {
                        wallet.set_password(Some(&password))?;
                        println!("the wallet now has a password; you will be asked for it when you send");
                    }
                    None => println!("no password set; add one later with: orien wallet password {}", file.display()),
                }
            }
            Ok(())
        }
        Command::Wallet(WalletCommand::Password { file }) => {
            let file = file.unwrap_or_else(default_wallet);
            if Session::find(&file).is_some() {
                bail!("{} is unlocked; end that first with: orien lock", file.display());
            }
            let mut wallet = open_wallet(&file)?;
            match choose_password()? {
                Some(password) => {
                    wallet.set_password(Some(&password))?;
                    println!("{} now has a new password", file.display());
                }
                None if wallet.has_password() => {
                    wallet.set_password(None)?;
                    println!("{} no longer has a password", file.display());
                }
                None => println!("{} still has no password", file.display()),
            }
            Ok(())
        }
        Command::Wallet(WalletCommand::Restore { file, account }) => {
            let file = file.unwrap_or_else(default_wallet);
            if let Some(folder) = file.parent() {
                fs::create_dir_all(folder)?;
            }
            println!("Type your 24-word recovery phrase, separated by spaces, then press Enter:");
            let mut line = String::new();
            std::io::stdin().read_line(&mut line)?;
            let master = orien_net::phrase::from_words(&line)?;
            let wallet = Wallet::from_master(&file, master, account)?;
            println!("restored {}", file.display());
            println!("address  {}", wallet.address());
            println!("account  {account} of the recovery phrase");
            println!("its balance and payment number come from the network the next time you use it");
            Ok(())
        }
        Command::Wallet(WalletCommand::Phrase { file }) => {
            let file = file.unwrap_or_else(default_wallet);
            show_phrase(&open_wallet(&file)?);
            Ok(())
        }
        Command::Wallet(WalletCommand::Address { file }) => {
            let file = file.unwrap_or_else(default_wallet);
            println!("{}", Wallet::peek(&file)?.0);
            Ok(())
        }
        Command::Balance { account, network } => balance(&account.unwrap_or_else(|| default_wallet().display().to_string()), &network.file()),
        Command::Send { wallet, to, amount, receipt, network } => {
            let to = resolve_account(&to)?;
            submit(&wallet.clone().unwrap_or_else(default_wallet), |_, _| Ok(Kind::Payment), Some(to), amount, receipt.as_deref(), &network.file())
        }
        Command::Stake { wallet, amount, validator_key, address, network } => {
            let key = ValidatorKey::load(&validator_key)?;
            let stake = |account: &AccountId, network: &orien_ledger::Genesis| {
                println!("building the validator's key tree to sign for the stake...");
                let tree = KeyTree::generate(key.seed, key.height);
                let validator = Validator { root: tree.root(), height: key.height, address: address.clone() };
                if !validator.is_usable() {
                    bail!("the address must be 1 to {} characters", orien_ledger::types::MAX_ADDRESS);
                }
                let proof = sign_stake_proof(&tree, &network.id(), account, &address);
                Ok(Kind::Stake { validator, proof })
            };
            submit(&wallet.clone().unwrap_or_else(default_wallet), stake, None, amount, None, &network.file())
        }
        Command::Unstake { wallet, amount, network } => {
            submit(&wallet.clone().unwrap_or_else(default_wallet), |_, _| Ok(Kind::Unstake), None, amount, None, &network.file())
        }
        Command::Resume { wallet, payment, receipt, network } => {
            resume(&wallet.clone().unwrap_or_else(default_wallet), payment.as_deref(), receipt.as_deref(), &network.file())
        }
        Command::Verify { receipt, network } => verify(&receipt, &network.file()),
        Command::Status { network } => status(&network.file()),
        Command::Mine { account, threads, network } => mine(&account.unwrap_or_else(|| default_wallet().display().to_string()), threads, &network.file()),
        Command::Node { key, data, listen, network } => node(&key, data, listen, &network.file()),
        Command::Keygen { out, height } => keygen(&out, height),
        Command::Unlock { file, duration, up_to, status } => {
            let file = file.unwrap_or_else(default_wallet);
            if status { unlock_status(&file) } else { unlock(&file, duration, up_to) }
        }
        Command::Lock { file } => {
            let file = file.unwrap_or_else(default_wallet);
            match Session::find(&file) {
                Some(session) => {
                    session.lock()?;
                    println!("locked {}", file.display());
                }
                None => println!("{} was not unlocked", file.display()),
            }
            Ok(())
        }
        Command::UnlockHelper { file, until, spend } => unlock_helper(&file, Limits { until, spend }),
        Command::Skill(SkillCommand::Install { dir, refresh }) => install_skill(dir, refresh),
        Command::Skill(SkillCommand::Show) => {
            print!("{SKILL}");
            Ok(())
        }
        Command::Update => release::update(),
        Command::Release(ReleaseCommand::Keygen { out }) => release::keygen(&out),
        Command::Release(ReleaseCommand::Sign { key, sums }) => release::sign(&key, &sums),
        Command::Genesis(GenesisCommand::New { out, name, validators, bitcoin, start, epoch_seconds }) => {
            genesis_new(&out, &name, &validators, bitcoin, start, epoch_seconds)
        }
        Command::Genesis(GenesisCommand::Show { file }) => genesis_show(&load_genesis(&file)?),
        Command::Devnet(DevnetCommand::Init {
            dir,
            validators,
            height,
            host,
            base_port,
            addresses,
            name,
            epoch_seconds,
            from_nothing,
            bitcoin,
        }) => {
            let addresses = match addresses.is_empty() {
                true => (0..validators).map(|i| format!("{host}:{}", base_port as usize + i)).collect(),
                false => addresses,
            };
            let start = Start { epoch_seconds, from_nothing, bitcoin };
            devnet_init(&dir, &name, &addresses, height, start)
        }
        Command::Devnet(DevnetCommand::Run { dir }) => devnet_run(&dir),
        Command::Devnet(DevnetCommand::Interrupt { dir, wallet, to, amount }) => {
            interrupt(&dir, &wallet, &to, amount)
        }
        Command::Devnet(DevnetCommand::DoubleSpend { dir, wallet, split }) => {
            double_spend(&dir, &wallet, split)
        }
    }
}

fn unix_now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |since| since.as_secs())
}

/// Where this computer keeps how far it has followed a network: next to
/// the genesis file.
fn head_file(genesis: &Path) -> PathBuf {
    let mut name = genesis.as_os_str().to_owned();
    name.push(".head");
    PathBuf::from(name)
}

/// A client that starts from wherever this computer last followed the
/// network to, without contacting anyone.
fn offline(genesis_file: &Path) -> Result<(Genesis, Client)> {
    let genesis = load_genesis(genesis_file)?;
    let client = match Head::load(&head_file(genesis_file), &genesis) {
        Some(head) => Client::resume(&genesis, head),
        None => Client::new(&genesis),
    };
    Ok((genesis, client))
}

/// Remembers how far the client has followed the network, so the next
/// command does not start again from the genesis file.
fn remember(client: &Client, genesis_file: &Path) {
    let head = client.head();
    if head.epoch > 0 {
        let _ = head.save(&head_file(genesis_file));
    }
}

/// A client caught up with the network's checkpoints.
fn connect(genesis_file: &Path) -> Result<(Genesis, Client)> {
    let (genesis, client) = offline(genesis_file)?;
    if client.follow() > 0 {
        remember(&client, genesis_file);
    }
    Ok((genesis, client))
}

/// Roughly how long until the next checkpoint.
fn until_checkpoint(genesis: &Genesis, client: &Client) -> String {
    let due = client.head().time.saturating_add(genesis.epoch_seconds);
    match due.saturating_sub(unix_now()) {
        0 => "any moment now".into(),
        seconds if seconds < 120 => format!("in about {seconds} seconds"),
        seconds => format!("in about {} minutes", seconds.div_ceil(60)),
    }
}

/// Accepts either a wallet file or a 64-character address.
fn resolve_account(text: &str) -> Result<AccountId> {
    if Path::new(text).is_file() {
        return Ok(Wallet::peek(Path::new(text))?.0);
    }
    text.parse().map_err(|_| anyhow!("'{text}' is neither a wallet file nor a 64-character address"))
}

/// Reads a wallet's account from the validators.
fn read_account(client: &Client, wallet: &Signing) -> Result<AccountStatus> {
    read_account_of(client, &wallet.address(), wallet.next_seq())
}

fn read_account_of(client: &Client, address: &AccountId, next_seq: u64) -> Result<AccountStatus> {
    let status = client.account(address, next_seq);
    if status.views.iter().all(|view| view.is_none()) {
        bail!("no validator could be reached");
    }
    Ok(status)
}

/// Payments under the account's current number that are signed but not
/// final: those validators have voted for, plus the one this wallet file
/// signed itself, if any. Only orders carrying this wallet's own signature
/// count.
fn unfinished(client: &Client, wallet: &Signing, status: &AccountStatus) -> Vec<Unfinished> {
    let committee = client.committee();
    let network = committee.network();
    let mut found: Vec<Unfinished> = client
        .unfinished(&wallet.address(), status)
        .into_iter()
        .filter(|payment| wallet.signed_by_this_wallet(&payment.signed, network))
        .collect();
    let live = |own: &&SignedOrder| own.order.seq == status.seq && !retired(status, own);
    if let Some(own) = wallet.pending().filter(live) {
        let digest = own.order.digest(network);
        if !found.iter().any(|payment| payment.digest == digest) {
            found.push(Unfinished { signed: own.clone(), digest, stake: 0 });
        }
    }
    found
}

/// True when a checkpoint has cancelled the number `signed` was made under
/// and retired the key that signed it. The account now requires a spare
/// key, and this order was not signed with that one.
fn retired(status: &AccountStatus, signed: &SignedOrder) -> bool {
    match &status.signer {
        Some(Signer::Spare(leaf)) if signed.order.seq == status.seq => {
            !matches!(&signed.signature, Signature::Spare(signature) if signature.leaf == *leaf)
        }
        _ => false,
    }
}

/// If a checkpoint cancelled the payment this wallet file was holding,
/// says so and lets go of it.
fn drop_cancelled(wallet: &mut Signing, status: &AccountStatus) -> Result<()> {
    let Some(pending) = wallet.pending().filter(|pending| retired(status, pending)).cloned() else {
        return Ok(());
    };
    let order = &pending.order;
    println!(
        "note: payment #{} ({} to {}) was cancelled at a checkpoint, because two different payments \
         were signed with the same one-time key. Nothing moved. The account now signs with a spare key.",
        order.seq,
        order.amount,
        order.to.short()
    );
    wallet.forget(order.seq)
}

/// One line about an unfinished payment, led by the id that picks it out.
fn describe_unfinished(client: &Client, all: &[Unfinished], one: &Unfinished, wallet: Option<&Signing>) -> String {
    let order = &one.signed.order;
    let mut line = format!(
        "{}  payment #{}: {} to {}, votes cover {} of {} stake",
        one.digest.short(),
        order.seq,
        coins(order.amount as i128),
        order.to.short(),
        coins(one.stake as i128),
        coins(client.committee().total_stake() as i128)
    );
    if !client.can_still_finish(all, one) {
        line.push_str(", can no longer become final");
    }
    if wallet.and_then(Signing::pending).is_some_and(|own| own.order == *order) {
        line.push_str(", signed by this wallet file");
    }
    line
}

/// True when more than one payment was signed under one number and the
/// votes are split so that none of them can reach a quorum any more.
fn is_locked(client: &Client, unfinished: &[Unfinished]) -> bool {
    !unfinished.is_empty() && unfinished.iter().all(|payment| !client.can_still_finish(unfinished, payment))
}

/// Reports a locked account, after making sure every validator has seen
/// every one of the conflicting payments. A validator that holds two of
/// them holds the proof the next checkpoint needs to free the account.
fn locked(genesis: &Genesis, client: &Client, seq: u64, payments: &[Unfinished]) -> anyhow::Error {
    for payment in payments {
        client.collect_votes(&payment.signed);
    }
    anyhow!(
        "this account is locked: more than one payment was signed under #{seq}, and the votes are \
         split so that none of them can become final. The next checkpoint, {}, cancels them all and \
         frees the account. Nothing has moved. Send again after that.",
        until_checkpoint(genesis, client)
    )
}

fn balance(account: &str, genesis: &Path) -> Result<()> {
    let (genesis, client) = connect(genesis)?;
    // A wallet with a password is read by its address alone: a balance
    // never needs the password.
    let wallet = match Path::new(account).is_file() {
        true => match Wallet::load(Path::new(account)) {
            Ok(wallet) => Some(wallet),
            Err(error) if error.downcast_ref::<OpenError>().is_some() => None,
            Err(error) => return Err(error),
        },
        false => None,
    };
    let wallet = wallet.map(Signing::Here);
    let id = match &wallet {
        Some(wallet) => wallet.address(),
        None => resolve_account(account)?,
    };
    let status = client.account(&id, wallet.as_ref().map_or(0, Signing::next_seq));
    let reachable = status.views.iter().flatten().count();
    if reachable == 0 {
        bail!("no validator could be reached");
    }
    println!("account        {id}");
    // What the last checkpoint proves, checked against it here; and what
    // validators report has happened since, which nothing proves yet.
    let proven = client.proven(&id).map_or(0, |account| account.balance);
    let source = match client.epoch() {
        0 => "as the genesis file sets it".to_string(),
        epoch => format!("proven by checkpoint {}", epoch - 1),
    };
    println!("balance        {}  ({source})", coins(proven));
    match status.spendable {
        Some(now) if now != proven => {
            let change = now - proven;
            let sign = if change > 0 { "+" } else { "" };
            println!("since then     {sign}{}  (validators report this; the next checkpoint proves it)", coins(change));
        }
        Some(_) => {}
        None => println!("since then     unknown (too few validators are caught up)"),
    }
    println!("payments made  {}", status.seq);
    println!("validators     {reachable} of {} answered", client.validators());
    if let Some(Signer::Spare(leaf)) = &status.signer {
        println!(
            "spare keys     payment #{} was cancelled at a checkpoint; the next one is signed with \
             spare key {} of 16",
            status.seq,
            leaf + 1
        );
    }

    let pending = match &wallet {
        Some(wallet) => unfinished(&client, wallet, &status),
        None => client.unfinished(&id, &status),
    };
    for payment in &pending {
        println!("pending        {}", describe_unfinished(&client, &pending, payment, wallet.as_ref()));
    }
    if pending.is_empty() && !status.locks.is_empty() {
        println!("pending        validators have voted on payment #{} but it is not final", status.seq);
    }
    if is_locked(&client, &pending) {
        println!(
            "               locked: none of these can become final. The next checkpoint, {}, cancels \
             them and frees the account.",
            until_checkpoint(&genesis, &client)
        );
    } else if !pending.is_empty() && wallet.is_some() {
        println!("               finish it with: orien resume -w {account}");
    }
    Ok(())
}

/// If this wallet file signed a payment in an earlier run and that payment
/// number has been used since, forgets the signed payment. Returns the
/// order when what became final is, as far as can be told, the payment this
/// file signed, with its certificate if that is still to be had.
fn settle_earlier(
    client: &Client,
    wallet: &mut Signing,
    seq: u64,
) -> Result<Option<(orien_ledger::Order, Option<Certificate>)>> {
    let Some(earlier) = wallet.pending().filter(|earlier| earlier.order.seq < seq).cloned() else {
        return Ok(None);
    };
    let order = &earlier.order;
    wallet.confirm(order.seq)?;
    match client.certificate(&order.from, order.seq) {
        Some(certificate) if certificate.order == *order => {
            println!(
                "note: payment #{} ({} to {}) from an earlier run is final",
                order.seq,
                order.amount,
                order.to.short()
            );
            Ok(Some((order.clone(), Some(certificate))))
        }
        Some(certificate) => {
            println!(
                "note: payment #{} became final as {} to {}, not as the {} to {} this wallet file signed",
                order.seq,
                certificate.order.amount,
                certificate.order.to.short(),
                order.amount,
                order.to.short()
            );
            Ok(None)
        }
        // Certificates are kept for the current epoch only. Past that, a
        // checkpoint has settled the number.
        None => {
            println!(
                "note: payment #{} from an earlier run was settled at a checkpoint. This wallet file \
                 had signed it as {} to {}.",
                order.seq,
                order.amount,
                order.to.short()
            );
            Ok(Some((order.clone(), None)))
        }
    }
}

fn save_receipt(receipt: &Receipt, file: Option<&Path>) -> Result<()> {
    if let Some(file) = file {
        fs::write(file, borsh::to_vec(receipt)?)?;
        println!("  receipt saved to {}", file.display());
    }
    Ok(())
}

/// Says what a failed payment means for the person who made it.
fn explain(error: TransferError) -> anyhow::Error {
    match error {
        TransferError::NotConfirmed { .. } => anyhow!(
            "{error}\nNothing is lost. The payment keeps its number and no other payment can take \
             it. Run the same command again, or `orien resume`, to finish it."
        ),
        other if other.closing() => anyhow!(
            "{other}\nThe network was closing an epoch and did not finish in time. Nothing is lost: \
             the payment is signed and keeps its number. Run `orien resume` to finish it."
        ),
        other => other.into(),
    }
}

/// Gathers votes and confirmations for a signed payment, which makes it
/// final, and says so. Only then does the wallet move on.
fn finish(client: &Client, wallet: &mut Signing, signed: &SignedOrder, receipt_file: Option<&Path>) -> Result<()> {
    let order = &signed.order;
    let started = Instant::now();
    // A checkpoint pauses payments for a few seconds. Wait it out.
    let done = client.pay(signed, Duration::from_secs(90)).map_err(explain)?;
    let elapsed = started.elapsed();
    wallet.confirm(order.seq)?;
    let done = match done {
        Done::Receipt(done) => done,
        Done::InCheckpoint(epoch) => {
            println!("final in {:.1} s", elapsed.as_secs_f64());
            println!(
                "  {}",
                what(order)
            );
            println!("  checkpoint {epoch} took it in while the network was pausing to close the epoch");
            if receipt_file.is_some() {
                println!("  no receipt was saved: receipts for payments settled by a checkpoint are not built yet");
            }
            return Ok(());
        }
    };

    let size = borsh::to_vec(&done.receipt)?.len();
    println!("final in {:.1} ms", elapsed.as_secs_f64() * 1000.0);
    println!(
        "  {}",
        what(order)
    );
    println!(
        "  {} of {} validators voted and {} confirmed; the receipt is {size} bytes",
        done.votes,
        client.validators(),
        done.confirmations
    );
    save_receipt(&done.receipt, receipt_file)
}

/// Signs an order and waits until it is final: a payment to `to`, or with
/// `to` left out, a stake or unstake order on the wallet's own account.
/// `kind` makes the order's kind once the wallet's account is known.
/// One line saying what a final order did.
fn what(order: &orien_ledger::Order) -> String {
    let (seq, amount, from) = (order.seq, coins(order.amount as i128), order.from.short());
    match &order.kind {
        Kind::Payment => {
            format!("payment #{seq}: {amount} from {from} to {} (fee {})", order.to.short(), coins(order.max_fee as i128))
        }
        Kind::Stake { validator, .. } => {
            format!("order #{seq}: {from} staked {amount} behind the validator at {}", validator.address)
        }
        Kind::Unstake => format!("order #{seq}: {from} began releasing {amount} of its stake"),
    }
}

fn submit(
    wallet_file: &Path,
    kind: impl FnOnce(&AccountId, &Genesis) -> Result<Kind>,
    to: Option<AccountId>,
    amount: u64,
    receipt_file: Option<&Path>,
    genesis: &Path,
) -> Result<()> {
    let genesis_file = genesis;
    let (genesis, client) = connect(genesis_file)?;
    let network = genesis.id();
    let mut wallet = open_signing(wallet_file)?;
    wallet.accept_fee(genesis.fee);
    let to = to.unwrap_or(wallet.address());
    let kind = kind(&wallet.address(), &genesis)?;
    let status = read_account(&client, &wallet)?;
    let seq = status.seq;
    drop_cancelled(&mut wallet, &status)?;

    // A payment this wallet file signed in an earlier run may have gone
    // through since. If it is the very payment asked for now, sending it
    // again would pay twice.
    if let Some((order, certificate)) = settle_earlier(&client, &mut wallet, seq)?
        && order.kind == kind
        && order.to == to
        && order.amount == amount
    {
        println!("that is the payment asked for, so nothing more was sent");
        match certificate {
            Some(certificate) if receipt_file.is_some() => {
                let done = client.finish(certificate, 0).map_err(explain)?;
                save_receipt(&done.receipt, receipt_file)?;
            }
            None => println!("to pay the same amount again, run the command once more"),
            _ => {}
        }
        return Ok(());
    }

    let unfinished = unfinished(&client, &wallet, &status);
    // A vote that no validator can back with a payment this wallet's key
    // signed means the key for this number may be in use somewhere this
    // wallet cannot see.
    if status.locks.iter().any(|lock| !unfinished.iter().any(|payment| payment.digest == *lock)) {
        bail!(
            "validators have voted on payment #{seq}, but none of them could show a payment signed \
             with this wallet's key. Signing now could reuse a one-time key, so this wallet will \
             not. Check the balance and try again."
        );
    }
    match unfinished.as_slice() {
        // Validators check the stake an unstake order draws on.
        [] if kind == Kind::Unstake => {}
        [] => match status.spendable {
            Some(balance) if balance < amount as i128 + genesis.fee.on(&kind, amount) as i128 => {
                bail!(
                    "insufficient funds: balance is {}, payment is {} and the fee {}",
                    coins(balance),
                    coins(amount as i128),
                    coins(genesis.fee.on(&kind, amount) as i128)
                )
            }
            None => bail!("too few validators are caught up on this account; try again shortly"),
            _ => {}
        },
        // The payment asked for is already signed, by this wallet file or by
        // another copy of the wallet. Finish that one: signing it again is
        // neither needed nor safe.
        [one] if one.signed.order.kind == kind && one.signed.order.to == to && one.signed.order.amount == amount => {
            if wallet.pending().is_none_or(|own| own.order != one.signed.order) {
                println!("note: another copy of this wallet already signed this payment; finishing it");
                wallet.adopt(&one.signed, &network)?;
            }
        }
        waiting => {
            println!("payment #{seq} is already signed and not final:");
            for payment in waiting {
                println!("  {}", describe_unfinished(&client, waiting, payment, Some(&wallet)));
            }
            if is_locked(&client, waiting) {
                return Err(locked(&genesis, &client, seq, waiting));
            }
            bail!(
                "nothing else can be sent from this account until that payment is final. \
                 Finish it with: orien resume -w {}",
                wallet_file.display()
            );
        }
    }

    let signed = wallet.sign(kind, seq, to, amount, status.signer.as_ref(), &network)?;
    let outcome = finish(&client, &mut wallet, &signed, receipt_file);
    remember(&client, genesis_file);
    outcome
}

fn resume(wallet_file: &Path, pick: Option<&str>, receipt_file: Option<&Path>, genesis: &Path) -> Result<()> {
    let genesis_file = genesis;
    let (genesis, client) = connect(genesis_file)?;
    let network = genesis.id();
    let mut wallet = open_signing(wallet_file)?;
    wallet.accept_fee(genesis.fee);
    let status = read_account(&client, &wallet)?;
    let seq = status.seq;
    drop_cancelled(&mut wallet, &status)?;
    settle_earlier(&client, &mut wallet, seq)?;

    let unfinished = unfinished(&client, &wallet, &status);
    let chosen = match (unfinished.as_slice(), pick) {
        ([], _) => {
            println!("nothing to finish: no payment from this wallet is waiting");
            return Ok(());
        }
        ([one], None) => one,
        (several, None) => {
            println!("payment #{seq} was signed {} times with the same one-time key:", several.len());
            for payment in several {
                println!("  {}", describe_unfinished(&client, several, payment, Some(&wallet)));
            }
            if is_locked(&client, several) {
                return Err(locked(&genesis, &client, seq, several));
            }
            bail!("say which one to finish with --payment <id>");
        }
        (all, Some(prefix)) => {
            let prefix = prefix.to_lowercase();
            let mut matching = all.iter().filter(|payment| payment.digest.to_string().starts_with(&prefix));
            match (matching.next(), matching.next()) {
                (Some(one), None) => one,
                (None, _) => bail!("no unfinished payment has an id starting with '{prefix}'"),
                _ => bail!("more than one unfinished payment has an id starting with '{prefix}'"),
            }
        }
    };

    let order = &chosen.signed.order;
    println!("finishing payment #{seq}: {} to {}", coins(order.amount as i128), order.to.short());
    if wallet.pending().is_none_or(|own| own.order != *order) {
        wallet.adopt(&chosen.signed, &network)?;
    }
    let outcome = finish(&client, &mut wallet, &chosen.signed, receipt_file);
    remember(&client, genesis_file);
    outcome.map_err(|error| match unfinished.len() {
        1 => error,
        _ => anyhow!(
            "{error}\nMore than one payment was signed under #{seq}. If none of them can become \
             final, the next checkpoint cancels them all and frees the account."
        ),
    })
}

fn verify(receipt: &Path, genesis: &Path) -> Result<()> {
    let genesis_file = genesis;
    let (genesis, client) = offline(genesis_file)?;
    let bytes = fs::read(receipt).with_context(|| format!("cannot read {}", receipt.display()))?;
    let receipt: Receipt =
        borsh::from_slice(&bytes).map_err(|_| anyhow!("{} is not a receipt", receipt.display()))?;
    // A receipt is checked against the validators of the epoch it was made
    // in. For the first epoch the genesis file names them. For a later one
    // this computer must have followed the network's checkpoints that far.
    let epoch = receipt.certificate.epoch;
    let mut online = false;
    let committee = match client.committee_at(epoch) {
        Some(committee) => committee,
        None => {
            online = true;
            client.follow();
            remember(&client, genesis_file);
            client.committee_at(epoch).ok_or_else(|| {
                anyhow!(
                    "this receipt is from epoch {epoch}, and this computer has followed the network \
                     only as far as epoch {}. Checking it needs the checkpoints up to epoch {epoch}, \
                     which any validator can supply.",
                    client.epoch()
                )
            })?
        }
    };
    committee.verify_receipt(&receipt).map_err(|error| anyhow!("receipt is NOT valid: {error}"))?;
    let order = &receipt.certificate.order;
    let stake = |signatures: &[orien_ledger::Vote]| -> u64 {
        signatures.iter().map(|signature| committee.stake_of(signature.validator)).sum()
    };
    println!("valid: this payment is final on network '{}'", genesis.network);
    println!("  from     {}", order.from);
    println!("  to       {}", order.to);
    println!("  amount   {}", order.amount);
    println!("  payment  #{}", order.seq);
    println!(
        "  voted for by validators holding {} of {} stake, confirmed by {}",
        stake(&receipt.certificate.votes),
        committee.total_stake(),
        stake(&receipt.confirmations)
    );
    let signatures = receipt.certificate.votes.len() + receipt.confirmations.len();
    match online {
        false => println!("  checked offline with {signatures} hash-based signatures and no network access"),
        true => println!(
            "  checked with {signatures} hash-based signatures, after fetching the checkpoints up to epoch {epoch}"
        ),
    }
    Ok(())
}

fn status(genesis: &Path) -> Result<()> {
    let (genesis, client) = connect(genesis)?;
    println!("network '{}'  id {}", genesis.network, genesis.id().short());
    println!(
        "epoch {}, a checkpoint every {} seconds, the next one {}",
        client.epoch(),
        genesis.epoch_seconds,
        until_checkpoint(&genesis, &client)
    );
    println!(
        "staking: at least {} coins to become a validator, at most {} validators, unstaked coins wait {} epochs",
        coins(genesis.min_stake as i128),
        genesis.max_validators,
        genesis.unstake_epochs
    );
    let fee = genesis.fee;
    println!(
        "fees: {}% of each payment, at least {} and at most {}; stake and unstake orders pay {}",
        fee.rate as f64 / 10_000.0,
        coins(fee.min as i128),
        coins(fee.max as i128),
        coins(fee.min as i128)
    );
    // Coins released by the checkpoints so far, which the mint counts as a
    // debt.
    let released = client.proven(&orien_ledger::MINT).map_or(0, |mint| -mint.balance);
    println!(
        "coins: {} released for work so far, of at most {}{}",
        coins(released),
        coins(genesis.cap as i128),
        genesis.bitcoin.map_or(String::new(), |block| format!("; started from Bitcoin block {}", block.height))
    );
    // Who votes this epoch, as the checkpoints say. The launch seats come
    // first, in the genesis file's order.
    let committee = client.committee();
    let total = committee.total_stake().max(1) as f64;
    println!("validators of epoch {}:", committee.epoch());
    let mut seats = genesis.validators.iter();
    for member in committee.members() {
        let who = match member.account {
            None => format!("{} (launch seat)", seats.next().map_or("?", |seat| seat.name.as_str())),
            Some(account) => format!("staked by {}", account.short()),
        };
        println!(
            "  {:<26} {:>6.2}% of the vote  {:>16} stake  at {}",
            who,
            100.0 * member.stake as f64 / total,
            coins(member.stake as i128),
            member.address
        );
    }
    println!("their servers:");
    println!(
        "{:<14} {:<22} {:>7} {:>8} {:>9} {:>16}  {:<8}",
        "name", "address", "epoch", "accounts", "payments", "signing keys", "state"
    );
    let roots = client.roots();
    let infos = client.info();
    // Only validators of this network count towards "the same balances".
    let mut known: Vec<_> = infos
        .iter()
        .zip(&roots)
        .filter(|(info, _)| info.as_ref().is_ok_and(|info| info.network == genesis.id()))
        .filter_map(|(_, root)| root.as_ref())
        .collect();
    for ((address, info), root) in client.endpoints().iter().zip(infos).zip(&roots) {
        match info {
            Ok(info) if info.network == genesis.id() => println!(
                "{:<14} {:<22} {:>7} {:>8} {:>9} {:>16}  {:<8}{}",
                info.name,
                address,
                info.epoch,
                info.accounts,
                info.certificates,
                format!("{}/{}", info.keys_used, info.keys_total),
                root.map_or("-".into(), |root| root.short()),
                if info.closing { "  closing the epoch" } else { "" }
            ),
            Ok(_) => println!("{:<14} {:<22} serves a different network", "-", address),
            Err(error) => println!("{:<14} {:<22} unreachable ({error})", "-", address),
        }
    }
    // The state column is a fingerprint of every balance. Equal fingerprints
    // mean equal ledgers.
    let reporting = known.len();
    known.dedup();
    match (reporting, known.len()) {
        (0, _) => {}
        (_, 1) => println!("all {reporting} validators that answered hold exactly the same balances"),
        _ => println!("validators differ: a payment may still be spreading. Check again in a moment."),
    }
    Ok(())
}

/// Mines for `account` until stopped. Each round asks the validators for
/// the epoch's seed and target, searches with every thread for a few
/// seconds, and hands every share found to all the validators.
fn mine(account: &str, threads: Option<usize>, genesis_file: &Path) -> Result<()> {
    use orien_ledger::close::Share;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

    let (_, client) = connect(genesis_file)?;
    let account = resolve_account(account)?;
    let threads = threads.unwrap_or_else(|| thread::available_parallelism().map_or(1, |n| n.get())).max(1);
    // Two machines mining for one account must not find the same shares.
    let mut next = u64::from_le_bytes(orien_net::keys::random_seed()[..8].try_into().expect("8 bytes"));
    println!("mining for {} with {threads} thread(s); stop with Ctrl-C", account.short());
    let mut shown_epoch = None;
    loop {
        // The seed and target come from the checkpoints this client checks
        // itself, so nobody can send it off doing worthless work.
        client.follow();
        let (epoch, seed, target) = client.followed_work();
        if shown_epoch != Some(epoch) {
            let balance = client.proven(&account).map_or(0, |a| a.balance);
            println!(
                "epoch {epoch}: about one try in {} makes a share; the account holds {}",
                u64::MAX / target.max(1),
                coins(balance)
            );
            shown_epoch = Some(epoch);
        }
        // Search for a few seconds, then check the epoch again.
        let stop = Arc::new(AtomicBool::new(false));
        let tries = Arc::new(AtomicU64::new(0));
        let (found, shares) = std::sync::mpsc::channel();
        let workers: Vec<_> = (0..threads as u64)
            .map(|offset| {
                let (stop, tries, found) = (stop.clone(), tries.clone(), found.clone());
                let start = next.wrapping_add(offset);
                thread::spawn(move || {
                    let mut nonce = start;
                    while !stop.load(Ordering::Relaxed) {
                        for _ in 0..4096 {
                            let share = Share { account, nonce };
                            if share.counts(&seed, target) && found.send(share).is_err() {
                                return;
                            }
                            nonce = nonce.wrapping_add(threads as u64);
                        }
                        tries.fetch_add(4096, Ordering::Relaxed);
                    }
                })
            })
            .collect();
        drop(found);
        let started = Instant::now();
        let (mut sent, mut taken) = (0u64, 0u64);
        while started.elapsed() < Duration::from_secs(5) {
            if let Ok(share) = shares.recv_timeout(Duration::from_millis(200)) {
                sent += 1;
                if client.submit_share(share) > 0 {
                    taken += 1;
                }
            }
        }
        stop.store(true, Ordering::Relaxed);
        for worker in workers {
            let _ = worker.join();
        }
        while let Ok(share) = shares.try_recv() {
            sent += 1;
            if client.submit_share(share) > 0 {
                taken += 1;
            }
        }
        next = next.wrapping_add(tries.load(Ordering::Relaxed) + threads as u64 * 4096);
        let rate = tries.load(Ordering::Relaxed) as f64 / started.elapsed().as_secs_f64() / 1e6;
        println!("  {rate:.1} million tries a second; {sent} share(s) found, {taken} taken by validators");
    }
}

/// The wallet to sign a payment with: through its unlocked session when
/// `orien unlock` is running for it, otherwise opened here, asking for its
/// password if it has one.
fn open_signing(path: &Path) -> Result<Signing> {
    if let Some(unlocked) = Signing::unlocked(path)? {
        return Ok(unlocked);
    }
    let wallet = Wallet::load(path);
    if let Err(error) = &wallet
        && matches!(error.downcast_ref::<OpenError>(), Some(OpenError::NeedsPassword))
        && !std::io::stdin().is_terminal()
    {
        bail!(
            "{} has a password and is not unlocked. Its owner can type the password by running this command \
             in their own terminal, or allow it for a while with: orien unlock",
            path.display()
        );
    }
    Ok(Signing::Here(open_wallet(path)?))
}

/// Opens a wallet to sign with, asking for its password if it has one.
fn open_wallet(path: &Path) -> Result<Wallet> {
    match Wallet::load(path) {
        Err(error) if matches!(error.downcast_ref::<OpenError>(), Some(OpenError::NeedsPassword)) => {
            for attempt in 1..=3 {
                let password = rpassword::prompt_password(format!("password for {}: ", path.display()))?;
                match Wallet::open(path, Some(&password)) {
                    Err(error)
                        if attempt < 3 && matches!(error.downcast_ref::<OpenError>(), Some(OpenError::WrongPassword)) =>
                    {
                        println!("that is not this wallet's password; try again");
                    }
                    other => return other,
                }
            }
            unreachable!("the third attempt returns")
        }
        other => other,
    }
}

/// Asks for a wallet's password until it opens the wallet, up to three
/// times, and returns it.
fn ask_password(path: &Path) -> Result<String> {
    for attempt in 1..=3 {
        let password = rpassword::prompt_password(format!("password for {}: ", path.display())).map_err(|_| {
            anyhow!("orien unlock asks for the wallet's password, so its owner must run it in their own terminal")
        })?;
        match Wallet::open(path, Some(&password)) {
            Ok(_) => return Ok(password),
            Err(error)
                if attempt < 3 && matches!(error.downcast_ref::<OpenError>(), Some(OpenError::WrongPassword)) =>
            {
                println!("that is not this wallet's password; try again");
            }
            Err(error) => return Err(error),
        }
    }
    unreachable!("the third attempt returns")
}

/// A length of time such as 30m, 2h or 1d, in seconds.
fn parse_duration(text: &str) -> Result<u64, String> {
    let text = text.trim();
    let (number, unit) = text.split_at(text.find(|c: char| !c.is_ascii_digit()).unwrap_or(text.len()));
    let number: u64 = number.parse().map_err(|_| format!("'{text}' is not a length of time like 30m, 2h or 1d"))?;
    let seconds = match unit.trim() {
        "m" | "min" | "minutes" | "minute" => 60,
        "h" | "hour" | "hours" => 3_600,
        "d" | "day" | "days" => 86_400,
        _ => return Err(format!("'{text}' is not a length of time like 30m, 2h or 1d")),
    };
    match number.checked_mul(seconds) {
        Some(total) if total > 0 => Ok(total),
        _ => Err(format!("'{text}' is not a usable length of time")),
    }
}

/// A length of time in words: "1 hour 20 minutes".
fn describe_duration(seconds: u64) -> String {
    let plural = |n: u64, word: &str| format!("{n} {word}{}", if n == 1 { "" } else { "s" });
    let (days, hours, minutes) = (seconds / 86_400, seconds % 86_400 / 3_600, seconds % 3_600 / 60);
    let parts: Vec<String> = [(days, "day"), (hours, "hour"), (minutes, "minute")]
        .into_iter()
        .filter(|(n, _)| *n > 0)
        .map(|(n, word)| plural(n, word))
        .collect();
    if parts.is_empty() { "less than a minute".to_string() } else { parts.join(" ") }
}

/// Asks a question with a default answer, read from the terminal.
fn ask<T>(question: &str, default: &str, parse: impl Fn(&str) -> Result<T, String>) -> Result<T> {
    loop {
        print!("{question} [{default}]: ");
        std::io::stdout().flush()?;
        let mut line = String::new();
        std::io::stdin().read_line(&mut line)?;
        let answer = if line.trim().is_empty() { default } else { line.trim() };
        match parse(answer) {
            Ok(value) => return Ok(value),
            Err(problem) => println!("{problem}"),
        }
    }
}

/// `orien unlock`: asks for the password and the limits, then starts the
/// helper in the background and hands it the password through a pipe.
fn unlock(file: &Path, duration: Option<u64>, up_to: Option<u64>) -> Result<()> {
    if let Some(session) = Session::find(file) {
        let status = session.status()?;
        bail!(
            "{} is already unlocked for {} more, up to {} ORN more. To change that, run orien lock first.",
            file.display(),
            describe_duration(status.limits.until.saturating_sub(unix_now())),
            coins(status.left() as i128)
        );
    }
    match Wallet::load(file) {
        Ok(_) => bail!(
            "{} has no password, so any program on this computer can already sign with it. \
             Give it one first with: orien wallet password",
            file.display()
        ),
        Err(error) if matches!(error.downcast_ref::<OpenError>(), Some(OpenError::NeedsPassword)) => {}
        Err(error) => return Err(error),
    }
    let password = ask_password(file)?;
    let duration = match duration {
        Some(duration) => duration,
        None => ask("stay unlocked for how long?", "1h", parse_duration)?,
    };
    let spend = match up_to {
        Some(spend) => spend,
        None => ask("spend at most how much ORN, fees included?", "100", parse_coins)?,
    };

    let mut helper = std::process::Command::new(std::env::current_exe()?);
    helper
        .arg("unlock-helper")
        .arg(file)
        .args(["--until", &(unix_now() + duration).to_string(), "--spend", &spend.to_string()])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null());
    // The helper must outlive this terminal command, and the terminal.
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        helper.process_group(0);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const DETACHED_PROCESS: u32 = 0x0000_0008;
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
        helper.creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP);
    }
    let mut helper = helper.spawn().context("cannot start the unlock helper")?;
    {
        let mut stdin = helper.stdin.take().ok_or_else(|| anyhow!("cannot reach the unlock helper"))?;
        stdin.write_all(password.as_bytes())?;
        stdin.write_all(b"\n")?;
    }
    drop(password);
    let mut line = String::new();
    std::io::BufReader::new(helper.stdout.take().ok_or_else(|| anyhow!("cannot reach the unlock helper"))?)
        .read_line(&mut line)?;
    match line.trim() {
        "ready" => {
            println!("unlocked {} for {}, up to {} ORN", file.display(), describe_duration(duration), coins(spend as i128));
            println!("programs on this computer can now send from it within those limits. End it early with: orien lock");
            Ok(())
        }
        "" => bail!("the unlock helper stopped before it was ready"),
        problem => bail!("{}", problem.trim_start_matches("error: ")),
    }
}

/// The background helper: reads the password from its parent, opens the
/// wallet and serves it until the time is up or it is locked.
fn unlock_helper(file: &Path, limits: Limits) -> Result<()> {
    let mut password = String::new();
    std::io::stdin().read_line(&mut password)?;
    let wallet = match Wallet::open(file, Some(password.trim_end_matches(['\r', '\n']))) {
        Ok(wallet) => wallet,
        Err(error) => {
            println!("error: {error}");
            return Ok(());
        }
    };
    drop(password);
    let served = unlock::serve(wallet, limits, || {
        println!("ready");
        let _ = std::io::stdout().flush();
    });
    if let Err(error) = served {
        println!("error: {error}");
    }
    Ok(())
}

/// `orien unlock --status`.
fn unlock_status(file: &Path) -> Result<()> {
    match Session::find(file) {
        Some(session) => {
            let status = session.status()?;
            println!(
                "{} is unlocked for {} more, and may spend {} more ORN of {}",
                file.display(),
                describe_duration(status.limits.until.saturating_sub(unix_now())),
                coins(status.left() as i128),
                coins(status.limits.spend as i128)
            );
        }
        None => println!("{} is locked", file.display()),
    }
    Ok(())
}

/// Asks for a new password twice. An empty one means none.
fn choose_password() -> Result<Option<String>> {
    loop {
        let password = rpassword::prompt_password("choose a password (or press Enter for none): ")?;
        if password.is_empty() {
            return Ok(None);
        }
        if password.chars().count() < 8 {
            println!("use at least 8 characters");
            continue;
        }
        if rpassword::prompt_password("type it again: ")? == password {
            return Ok(Some(password));
        }
        println!("the two did not match; try again");
    }
}

/// Prints a wallet's recovery phrase, numbered, with what it means.
fn show_phrase(wallet: &Wallet) {
    let Some(words) = wallet.phrase() else {
        println!("this wallet was made before recovery phrases and has none; keep a copy of its file instead");
        return;
    };
    println!("Your recovery phrase. Write these 24 words on paper, in order, and keep them safe:");
    println!();
    for (row, chunk) in words.chunks(4).enumerate() {
        let cells: Vec<String> =
            chunk.iter().enumerate().map(|(i, word)| format!("{:>2}. {word:<9}", row * 4 + i + 1)).collect();
        println!("  {}", cells.join("  "));
    }
    println!();
    println!("With these words, `orien wallet restore` rebuilds your wallet on any computer.");
    println!("Anyone who sees them can take your coins. Never type them into a website or share them,");
    println!("and never give them to anyone who asks, an AI assistant included.");
}

fn keygen(out: &Path, height: u8) -> Result<()> {
    if out.exists() {
        bail!("{} already exists; refusing to overwrite a key", out.display());
    }
    let key = ValidatorKey::generate(height);
    let root = KeyTree::generate(key.seed, key.height).root();
    key.save(out)?;
    println!("created {}", out.display());
    println!("root     {root}");
    println!("height   {height}");
    println!("keep this file secret. For a launch genesis file, share only this line, with the name and address filled in:");
    println!("  --validator \"NAME,{root},{height},HOST:PORT\"");
    Ok(())
}

/// Reads NAME,ROOT,HEIGHT,ADDRESS.
fn parse_validator(text: &str) -> Result<ValidatorInfo> {
    let parts: Vec<&str> = text.split(',').map(str::trim).collect();
    let [name, root, height, address] = parts[..] else {
        bail!("'{text}' is not NAME,ROOT,HEIGHT,ADDRESS");
    };
    let root = root.parse().map_err(|_| anyhow!("'{root}' is not a 64-character key root"))?;
    let height = height.parse().map_err(|_| anyhow!("'{height}' is not a key height"))?;
    Ok(ValidatorInfo { name: name.into(), root, height, stake: 0, address: address.into() })
}

fn genesis_new(
    out: &Path,
    name: &str,
    validators: &[String],
    bitcoin: Option<BitcoinBlock>,
    start: Option<u64>,
    epoch_seconds: u64,
) -> Result<()> {
    if out.exists() {
        bail!("{} already exists; refusing to overwrite a genesis file", out.display());
    }
    let mut infos = validators.iter().map(|text| parse_validator(text)).collect::<Result<Vec<_>>>()?;
    // The launch seats share the seats' weight equally.
    let each = SEAT_WEIGHT / infos.len().max(1) as u64;
    for info in &mut infos {
        info.stake = each;
    }
    let mut genesis = Genesis::new(name, infos, Vec::new());
    genesis.epoch_seconds = epoch_seconds;
    genesis.start_time = start.unwrap_or_else(unix_now);
    genesis.bitcoin = bitcoin;
    genesis.validate().map_err(|e| anyhow!("{e}"))?;
    save_genesis(out, &genesis)?;
    println!("written to {}", out.display());
    genesis_show(&genesis)
}

fn genesis_show(genesis: &Genesis) -> Result<()> {
    println!("network     '{}'", genesis.network);
    println!("id          {}", genesis.id());
    match genesis.bitcoin {
        Some(block) => println!("anchored to Bitcoin block {} ({})", block.height, block.hash),
        None => println!("anchored to no Bitcoin block"),
    }
    let now = unix_now();
    let when = match genesis.start_time.cmp(&now) {
        std::cmp::Ordering::Greater => format!("in {} minutes", (genesis.start_time - now) / 60),
        _ => format!("{} minutes ago", (now - genesis.start_time) / 60),
    };
    println!("starts      {} seconds after 1970, {when}", genesis.start_time);
    println!("epochs      a checkpoint every {} seconds", genesis.epoch_seconds);
    println!(
        "coins       {} handed out at the start; at most {} released for work, starting at a target of one share in {} tries",
        coins(genesis.supply() as i128),
        coins(genesis.cap as i128),
        u64::MAX / genesis.first_target.max(1)
    );
    println!(
        "fees        {}% of each payment, at least {} and at most {}",
        genesis.fee.rate as f64 / 10_000.0,
        coins(genesis.fee.min as i128),
        coins(genesis.fee.max as i128)
    );
    println!(
        "staking     at least {} coins, at most {} validators, unstaked coins wait {} epochs",
        coins(genesis.min_stake as i128),
        genesis.max_validators,
        genesis.unstake_epochs
    );
    println!("launch validators ({}):", genesis.validators.len());
    let total: u64 = genesis.validators.iter().map(|v| v.stake).sum();
    for v in &genesis.validators {
        println!(
            "  {:<12} {:>6.2}% of the seats' weight  key {} (height {})  at {}",
            v.name,
            100.0 * v.stake as f64 / total.max(1) as f64,
            v.root.short(),
            v.height,
            v.address
        );
    }
    // Things worth a second look before anyone starts mining.
    if genesis.bitcoin.is_none() {
        println!("note: with no Bitcoin block, whoever holds this file before it is published could mine early");
    }
    if genesis.supply() > 0 {
        println!("note: this file hands out coins at the start");
    }
    if genesis.validators.iter().any(|v| v.address.is_empty() || !v.address.contains(':')) {
        println!("warning: every launch validator needs an address as HOST:PORT that the others can reach");
    }
    Ok(())
}

fn node(key: &Path, data: PathBuf, listen: Option<String>, genesis: &Path) -> Result<()> {
    let genesis = load_genesis(genesis)?;
    let key = ValidatorKey::load(key)?;
    let mut config = NodeConfig::new(genesis, key, data);
    config.listen = listen;
    config.log = true;
    println!("building the key tree...");
    let node = Node::start(config, None)?;
    println!("validator {} is listening on {}", node.name, node.addr);
    loop {
        thread::park();
    }
}

/// The voting weight the launch seats share on a new test network.
const SEAT_WEIGHT: u64 = 3_000_000 * COIN;

/// How a new test network starts.
struct Start {
    epoch_seconds: u64,
    from_nothing: bool,
    bitcoin: Option<BitcoinBlock>,
}

/// Reads HEIGHT:HASH.
fn parse_bitcoin_block(text: &str) -> Result<BitcoinBlock, String> {
    let (height, hash) = text.split_once(':').ok_or("expected HEIGHT:HASH")?;
    let height = height.trim().parse().map_err(|_| format!("'{height}' is not a block height"))?;
    let hash = hash.trim().parse().map_err(|_| format!("'{hash}' is not a 64-character block hash"))?;
    Ok(BitcoinBlock { height, hash })
}

fn devnet_init(dir: &Path, network: &str, addresses: &[String], height: u8, start: Start) -> Result<()> {
    let epoch_seconds = start.epoch_seconds;
    if dir.join("genesis.json").exists() {
        bail!("{} already holds a network; delete it or pick another --dir", dir.display());
    }
    let validators = addresses.len();
    if validators == 0 {
        bail!("a network needs at least one validator");
    }
    fs::create_dir_all(dir.join("validators"))?;
    fs::create_dir_all(dir.join("wallets"))?;

    println!("creating {validators} validator keys, each good for {} signatures...", 1u64 << height);
    let mut infos = Vec::new();
    for (i, address) in addresses.iter().enumerate() {
        let name = format!("v{}", i + 1);
        let key = ValidatorKey::generate(height);
        let root = KeyTree::generate(key.seed, key.height).root();
        key.save(&dir.join("validators").join(format!("{name}.key")))?;
        // The launch seats share 3 million coins of voting weight, which
        // fades as real stake arrives.
        let stake = SEAT_WEIGHT / validators as u64;
        infos.push(ValidatorInfo { name, root, height, stake, address: address.clone() });
    }

    let mut allocations = Vec::new();
    let given = if start.from_nothing { 0 } else { 1_000_000 * COIN };
    for (name, amount) in [("alice", given), ("bob", given), ("carol", 0)] {
        let wallet = Wallet::create(&dir.join("wallets").join(format!("{name}.wallet")))?;
        if amount > 0 {
            allocations.push(Allocation { account: wallet.address(), amount });
        }
        println!("  wallet {name:<6} {}  starts with {}", wallet.address(), coins(amount as i128));
    }

    let mut genesis = Genesis::new(network, infos, allocations);
    genesis.epoch_seconds = epoch_seconds;
    genesis.start_time = unix_now();
    genesis.bitcoin = start.bitcoin;
    genesis.validate().map_err(|e| anyhow!("{e}"))?;
    save_genesis(&dir.join("genesis.json"), &genesis)?;
    println!("network id {}", genesis.id());
    println!("written to {}", dir.display());
    Ok(())
}

fn devnet_run(dir: &Path) -> Result<()> {
    let genesis = load_genesis(&dir.join("genesis.json"))?;
    println!("starting {} validators (building key trees)...", genesis.validators.len());
    let mut nodes = Vec::new();
    for validator in &genesis.validators {
        let key = ValidatorKey::load(&dir.join("validators").join(format!("{}.key", validator.name)))?;
        let listener = TcpListener::bind(&validator.address)
            .with_context(|| format!("cannot listen on {}", validator.address))?;
        let mut config = NodeConfig::new(genesis.clone(), key, dir.join("data").join(&validator.name));
        config.log = true;
        let node = Node::start(config, Some(listener))?;
        println!("  {} listening on {}", node.name, node.addr);
        nodes.push(node);
    }
    println!("network '{}' is up. Ctrl-C stops it; its data is kept.", genesis.network);
    loop {
        thread::park();
    }
}

/// Signs a payment with a specific one-time key, bypassing the wallet's
/// guard against using a key twice.
fn sign_raw(wallet: &Wallet, genesis: &Genesis, seq: u64, to: AccountId, amount: u64) -> SignedOrder {
    let order = wallet.order(seq, to, amount);
    let digest = order.digest(&genesis.id());
    SignedOrder { signature: Signature::Key(wallet.key(seq).sign(digest.as_bytes())), order }
}

fn describe(response: std::io::Result<Response>) -> String {
    match response {
        Ok(Response::Vote(_)) => "voted for it".into(),
        Ok(Response::Rejected(reject)) => format!("refused: {reject}"),
        Ok(other) => format!("{other:?}"),
        Err(error) => format!("unreachable ({error})"),
    }
}

fn interrupt(dir: &Path, wallet_name: &str, to_name: &str, amount: u64) -> Result<()> {
    let (genesis, client) = connect(&dir.join("genesis.json"))?;
    let wallets = dir.join("wallets");
    let wallet_file = wallets.join(format!("{wallet_name}.wallet"));
    let mut wallet = open_wallet(&wallet_file)?;
    wallet.accept_fee(genesis.fee);
    let to = Wallet::peek(&wallets.join(format!("{to_name}.wallet")))?.0;

    let status = read_account_of(&client, &wallet.address(), wallet.next_seq())?;
    if !status.locks.is_empty() || wallet.pending().is_some() {
        bail!("{wallet_name} already has an unfinished payment");
    }
    match status.spendable {
        Some(balance) if balance >= (amount + genesis.fee.on(&Kind::Payment, amount)) as i128 && amount > 0 => {}
        Some(balance) => bail!(
            "{wallet_name} holds {}, which does not cover a payment of {} and its fee",
            coins(balance),
            coins(amount as i128)
        ),
        None => bail!("validators are not caught up"),
    }
    // Signed with the wallet's key but never written to the wallet file. To
    // the file, this is a payment made by another copy of the wallet.
    let seq = status.seq;
    let signed = sign_raw(&wallet, &genesis, seq, to, amount);
    let reached = (client.validators() / 2).max(1);
    println!(
        "another copy of {wallet_name}'s wallet signs payment #{seq}, {} to {to_name}, \
         and is lost after reaching {reached} of {} validators:",
        coins(amount as i128),
        client.validators()
    );
    for index in 0..reached {
        let answer = describe(client.call(index, &Request::SubmitOrder(signed.clone())));
        println!("  {} {answer}", genesis.validators[index].name);
    }
    println!("the payment is signed but not final, and {} has never heard of it.", wallet_file.display());
    println!("  see it:     orien balance {}", wallet_file.display());
    println!("  finish it:  orien resume -w {}", wallet_file.display());
    Ok(())
}

fn double_spend(dir: &Path, wallet_name: &str, split: bool) -> Result<()> {
    let (genesis, client) = connect(&dir.join("genesis.json"))?;
    let wallets = dir.join("wallets");
    let mut wallet = open_wallet(&wallets.join(format!("{wallet_name}.wallet")))?;
    wallet.accept_fee(genesis.fee);
    let others: Vec<&str> = ["alice", "bob", "carol"].into_iter().filter(|n| *n != wallet_name).collect();
    let first_payee = Wallet::peek(&wallets.join(format!("{}.wallet", others[0])))?.0;
    let second_payee = Wallet::peek(&wallets.join(format!("{}.wallet", others[1])))?.0;

    let status = client.account(&wallet.address(), wallet.next_seq());
    let balance = status.spendable.ok_or_else(|| anyhow!("validators are not caught up"))?;
    if balance <= 0 {
        bail!("{wallet_name} has nothing to double-spend");
    }
    if !status.locks.is_empty() {
        bail!("{wallet_name}'s account is already locked by an earlier double-spend attempt");
    }
    // As much as the balance covers with the fee: the fee is at most a
    // share of the amount, so taking it off the balance leaves enough.
    let all = balance as u64;
    let (seq, amount) = (status.seq, all - genesis.fee.on(&Kind::Payment, all));
    let first = sign_raw(&wallet, &genesis, seq, first_payee, amount);
    let second = sign_raw(&wallet, &genesis, seq, second_payee, amount);
    let amount_shown = coins(amount as i128);
    println!("{wallet_name} holds {} and signs payment #{seq} twice with the same one-time key:", coins(balance));
    println!("  A: {amount_shown} to {}", others[0]);
    println!("  B: {amount_shown} to {}", others[1]);

    if split {
        let half = client.validators() / 2;
        println!("sending A to the first {half} validators and B to the rest:");
        for index in 0..client.validators() {
            let (label, order) = if index < half { ("A", &first) } else { ("B", &second) };
            let answer = describe(client.call(index, &Request::SubmitOrder(order.clone())));
            println!("  {} got {label}: {answer}", genesis.validators[index].name);
        }
        for (label, order) in [("A", &first), ("B", &second)] {
            match client.transfer(order) {
                Ok(_) => println!("{label} became final"),
                Err(_) => println!("{label} cannot become final: it will never get more than two thirds of the votes"),
            }
        }
        println!(
            "no money moved. {wallet_name}'s account is stuck on payment #{seq} until the next checkpoint, {}, \
             which cancels both payments and frees it. Nobody else is affected.",
            until_checkpoint(&genesis, &client)
        );
    } else {
        println!("sending A to everyone first:");
        match client.transfer(&first) {
            Ok(receipt) => println!("  A is final ({} validators voted)", receipt.votes),
            Err(error) => bail!("A did not become final: {error}"),
        }
        println!("now sending B, which spends the same money again:");
        for index in 0..client.validators() {
            let answer = describe(client.call(index, &Request::SubmitOrder(second.clone())));
            println!("  {}: {answer}", genesis.validators[index].name);
        }
        println!("B is dead. {} was paid once; {} got nothing.", others[0], others[1]);
    }
    Ok(())
}
