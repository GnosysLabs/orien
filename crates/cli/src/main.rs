use anyhow::{Context, Result, anyhow, bail};
use clap::{Args, Parser, Subcommand};
use std::fs;
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::Instant;
use token_crypto::keytree::KeyTree;
use token_ledger::{AccountId, Allocation, Certificate, Genesis, Order, SignedOrder, ValidatorInfo};
use token_net::client::{AccountStatus, Client, Unfinished};
use token_net::keys::{ValidatorKey, Wallet, load_genesis, save_genesis};
use token_net::node::{Node, NodeConfig};
use token_net::wire::{Request, Response};

#[derive(Parser)]
#[command(name = "token", version, about = "A quantum-proof payment network with no blocks and no global ordering")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Args)]
struct Network {
    /// Genesis file describing the network
    #[arg(short, long, env = "TOKEN_GENESIS", default_value = "genesis.json")]
    genesis: PathBuf,
}

#[derive(Subcommand)]
enum Command {
    /// Create a wallet or show its address
    #[command(subcommand)]
    Wallet(WalletCommand),
    /// Show an account's balance
    Balance {
        /// A wallet file or an account address
        account: String,
        #[command(flatten)]
        network: Network,
    },
    /// Send a payment and wait until it is final
    Send {
        #[arg(short, long)]
        wallet: PathBuf,
        /// Recipient: an account address or a wallet file
        #[arg(long)]
        to: String,
        #[arg(long)]
        amount: u64,
        /// Save the proof of payment to this file
        #[arg(long)]
        receipt: Option<PathBuf>,
        #[command(flatten)]
        network: Network,
    },
    /// Finish a payment that was signed but never became final
    Resume {
        #[arg(short, long)]
        wallet: PathBuf,
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
        /// Address to listen on (default: this validator's address in the genesis file)
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
    /// Set up and run a local test network
    #[command(subcommand)]
    Devnet(DevnetCommand),
}

#[derive(Subcommand)]
enum WalletCommand {
    /// Create a new wallet file
    New { file: PathBuf },
    /// Show a wallet's account address
    Address { file: PathBuf },
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
        #[arg(long, default_value_t = 250)]
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

fn main() {
    if let Err(error) = run(Cli::parse()) {
        eprintln!("error: {error:#}");
        std::process::exit(1);
    }
}

fn run(cli: Cli) -> Result<()> {
    match cli.command {
        Command::Wallet(WalletCommand::New { file }) => {
            let wallet = Wallet::create(&file)?;
            println!("created {}", file.display());
            println!("address  {}", wallet.address());
            Ok(())
        }
        Command::Wallet(WalletCommand::Address { file }) => {
            println!("{}", Wallet::load(&file)?.address());
            Ok(())
        }
        Command::Balance { account, network } => balance(&account, &network.genesis),
        Command::Send { wallet, to, amount, receipt, network } => {
            send(&wallet, &to, amount, receipt.as_deref(), &network.genesis)
        }
        Command::Resume { wallet, payment, receipt, network } => {
            resume(&wallet, payment.as_deref(), receipt.as_deref(), &network.genesis)
        }
        Command::Verify { receipt, network } => verify(&receipt, &network.genesis),
        Command::Status { network } => status(&network.genesis),
        Command::Node { key, data, listen, network } => node(&key, data, listen, &network.genesis),
        Command::Keygen { out, height } => keygen(&out, height),
        Command::Devnet(DevnetCommand::Init { dir, validators, height, host, base_port }) => {
            devnet_init(&dir, validators, height, &host, base_port)
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

/// Accepts either a wallet file or a 64-character address.
fn resolve_account(text: &str) -> Result<AccountId> {
    if Path::new(text).is_file() {
        return Ok(Wallet::load(Path::new(text))?.address());
    }
    text.parse().map_err(|_| anyhow!("'{text}' is neither a wallet file nor a 64-character address"))
}

/// Reads a wallet's account from the validators.
fn read_account(client: &Client, wallet: &Wallet) -> Result<AccountStatus> {
    let status = client.account(&wallet.address(), wallet.next_seq());
    if status.views.iter().all(|view| view.is_none()) {
        bail!("no validator could be reached");
    }
    Ok(status)
}

/// Payments under the account's current number that are signed but not
/// final: those validators have voted for, plus the one this wallet file
/// signed itself, if any. Only orders carrying this wallet's own signature
/// count.
fn unfinished(client: &Client, wallet: &Wallet, status: &AccountStatus) -> Vec<Unfinished> {
    let network = client.committee().network();
    let mut found: Vec<Unfinished> = client
        .unfinished(&wallet.address(), status)
        .into_iter()
        .filter(|payment| wallet.signed_by_this_wallet(&payment.signed, network))
        .collect();
    if let Some(own) = wallet.pending().filter(|own| own.order.seq == status.seq) {
        let digest = own.order.digest(network);
        if !found.iter().any(|payment| payment.digest == digest) {
            found.push(Unfinished { signed: own.clone(), digest, stake: 0 });
        }
    }
    found
}

/// One line about an unfinished payment, led by the id that picks it out.
fn describe_unfinished(client: &Client, all: &[Unfinished], one: &Unfinished, wallet: Option<&Wallet>) -> String {
    let order = &one.signed.order;
    let mut line = format!(
        "{}  payment #{}: {} to {}, votes cover {} of {} stake",
        one.digest.short(),
        order.seq,
        order.amount,
        order.to.short(),
        one.stake,
        client.committee().total_stake()
    );
    if !client.can_still_finish(all, one) {
        line.push_str(", can no longer become final");
    }
    if wallet.and_then(Wallet::pending).is_some_and(|own| own.order == *order) {
        line.push_str(", signed by this wallet file");
    }
    line
}

/// True when more than one payment was signed under one number and the
/// votes are split so that none of them can reach a quorum any more.
fn is_locked(client: &Client, unfinished: &[Unfinished]) -> bool {
    !unfinished.is_empty() && unfinished.iter().all(|payment| !client.can_still_finish(unfinished, payment))
}

fn locked(seq: u64) -> anyhow::Error {
    anyhow!(
        "this account is locked: more than one payment was signed under #{seq}, and the votes are \
         split so that none of them can become final. Releasing such locks is not built yet."
    )
}

fn balance(account: &str, genesis: &Path) -> Result<()> {
    let genesis = load_genesis(genesis)?;
    let client = Client::new(&genesis);
    let wallet = match Path::new(account).is_file() {
        true => Some(Wallet::load(Path::new(account))?),
        false => None,
    };
    let id = match &wallet {
        Some(wallet) => wallet.address(),
        None => resolve_account(account)?,
    };
    let status = client.account(&id, wallet.as_ref().map_or(0, Wallet::next_seq));
    let reachable = status.views.iter().flatten().count();
    if reachable == 0 {
        bail!("no validator could be reached");
    }
    println!("account        {id}");
    match status.spendable {
        Some(balance) => println!("balance        {balance}"),
        None => println!("balance        unknown (too few validators are caught up)"),
    }
    println!("payments made  {}", status.seq);
    println!("validators     {reachable} of {} answered", client.validators());

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
        println!("               locked: none of these can become final any more");
    } else if !pending.is_empty() && wallet.is_some() {
        println!("               finish it with: token resume -w {account}");
    }
    Ok(())
}

/// If this wallet file signed a payment in an earlier run and that payment
/// number has been used since, forgets the signed payment. Returns the
/// certificate when what became final is the payment this file signed.
fn settle_earlier(client: &Client, wallet: &mut Wallet, seq: u64) -> Result<Option<Certificate>> {
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
            Ok(Some(certificate))
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
        None => {
            println!("note: payment #{} from an earlier run is no longer waiting", order.seq);
            Ok(None)
        }
    }
}

fn save_receipt(certificate: &Certificate, file: Option<&Path>) -> Result<()> {
    if let Some(file) = file {
        fs::write(file, borsh::to_vec(certificate)?)?;
        println!("  receipt saved to {}", file.display());
    }
    Ok(())
}

/// Gathers votes for a signed payment, makes it final and says so.
fn finish(client: &Client, wallet: &mut Wallet, signed: &SignedOrder, receipt_file: Option<&Path>) -> Result<()> {
    let order = &signed.order;
    let started = Instant::now();
    let receipt = client.transfer(signed)?;
    let elapsed = started.elapsed();
    wallet.confirm(order.seq)?;

    let size = borsh::to_vec(&receipt.certificate)?.len();
    println!("final in {:.1} ms", elapsed.as_secs_f64() * 1000.0);
    println!(
        "  payment #{}: {} from {} to {}",
        order.seq,
        order.amount,
        order.from.short(),
        order.to.short()
    );
    println!(
        "  {} of {} validators voted, {} confirmed; proof is {} votes, {size} bytes",
        receipt.votes,
        client.validators(),
        receipt.acknowledged,
        receipt.certificate.votes.len()
    );
    save_receipt(&receipt.certificate, receipt_file)
}

fn send(wallet_file: &Path, to: &str, amount: u64, receipt_file: Option<&Path>, genesis: &Path) -> Result<()> {
    let genesis = load_genesis(genesis)?;
    let client = Client::new(&genesis);
    let network = genesis.id();
    let mut wallet = Wallet::load(wallet_file)?;
    let to = resolve_account(to)?;
    let status = read_account(&client, &wallet)?;
    let seq = status.seq;

    // A payment this wallet file signed in an earlier run may have gone
    // through since. If it is the very payment asked for now, sending it
    // again would pay twice.
    if let Some(certificate) = settle_earlier(&client, &mut wallet, seq)?
        && certificate.order.to == to
        && certificate.order.amount == amount
    {
        println!("that is the payment asked for, so nothing more was sent");
        return save_receipt(&certificate, receipt_file);
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
        [] => match status.spendable {
            Some(balance) if balance < amount as i128 => {
                bail!("insufficient funds: balance is {balance}, payment is {amount}")
            }
            None => bail!("too few validators are caught up on this account; try again shortly"),
            _ => {}
        },
        // The payment asked for is already signed, by this wallet file or by
        // another copy of the wallet. Finish that one: signing it again is
        // neither needed nor safe.
        [one] if one.signed.order.to == to && one.signed.order.amount == amount => {
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
                return Err(locked(seq));
            }
            bail!(
                "nothing else can be sent from this account until that payment is final. \
                 Finish it with: token resume -w {}",
                wallet_file.display()
            );
        }
    }

    let signed = wallet.sign_order(seq, to, amount, &network)?;
    finish(&client, &mut wallet, &signed, receipt_file)
}

fn resume(wallet_file: &Path, pick: Option<&str>, receipt_file: Option<&Path>, genesis: &Path) -> Result<()> {
    let genesis = load_genesis(genesis)?;
    let client = Client::new(&genesis);
    let network = genesis.id();
    let mut wallet = Wallet::load(wallet_file)?;
    let status = read_account(&client, &wallet)?;
    let seq = status.seq;
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
                return Err(locked(seq));
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
    println!("finishing payment #{seq}: {} to {}", order.amount, order.to.short());
    if wallet.pending().is_none_or(|own| own.order != *order) {
        wallet.adopt(&chosen.signed, &network)?;
    }
    finish(&client, &mut wallet, &chosen.signed, receipt_file).map_err(|error| match unfinished.len() {
        1 => error,
        _ => anyhow!(
            "{error}\nMore than one payment was signed under #{seq}. If none of them can become \
             final, the account stays locked. Releasing such locks is not built yet."
        ),
    })
}

fn verify(receipt: &Path, genesis: &Path) -> Result<()> {
    let genesis = load_genesis(genesis)?;
    let client = Client::new(&genesis);
    let bytes = fs::read(receipt).with_context(|| format!("cannot read {}", receipt.display()))?;
    let certificate: Certificate =
        borsh::from_slice(&bytes).map_err(|_| anyhow!("{} is not a receipt", receipt.display()))?;
    client
        .committee()
        .verify_certificate(&certificate)
        .map_err(|error| anyhow!("receipt is NOT valid: {error}"))?;
    let order = &certificate.order;
    let stake: u64 = certificate.votes.iter().map(|v| client.committee().stake_of(v.validator)).sum();
    println!("valid: this payment is final on network '{}'", genesis.network);
    println!("  from     {}", order.from);
    println!("  to       {}", order.to);
    println!("  amount   {}", order.amount);
    println!("  payment  #{}", order.seq);
    println!("  signed by validators holding {stake} of {} stake", client.committee().total_stake());
    println!("  checked offline with {} hash-based signatures and no network access", certificate.votes.len());
    Ok(())
}

fn status(genesis: &Path) -> Result<()> {
    let genesis = load_genesis(genesis)?;
    let client = Client::new(&genesis);
    println!("network '{}'  id {}", genesis.network, genesis.id().short());
    println!("{:<6} {:<22} {:>8} {:>9} {:>16}", "name", "address", "accounts", "payments", "signing keys");
    for (validator, info) in genesis.validators.iter().zip(client.info()) {
        match info {
            Ok(info) if info.network == genesis.id() => println!(
                "{:<6} {:<22} {:>8} {:>9} {:>16}",
                validator.name,
                validator.address,
                info.accounts,
                info.certificates,
                format!("{}/{}", info.keys_used, info.keys_total)
            ),
            Ok(_) => println!("{:<6} {:<22} serves a different network", validator.name, validator.address),
            Err(error) => println!("{:<6} {:<22} unreachable ({error})", validator.name, validator.address),
        }
    }
    Ok(())
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

fn devnet_init(dir: &Path, validators: usize, height: u8, host: &str, base_port: u16) -> Result<()> {
    if dir.join("genesis.json").exists() {
        bail!("{} already holds a network; delete it or pick another --dir", dir.display());
    }
    if validators == 0 {
        bail!("a network needs at least one validator");
    }
    fs::create_dir_all(dir.join("validators"))?;
    fs::create_dir_all(dir.join("wallets"))?;

    println!("creating {validators} validator keys, each good for {} signatures...", 1u64 << height);
    let mut infos = Vec::new();
    for i in 0..validators {
        let name = format!("v{}", i + 1);
        let key = ValidatorKey::generate(height);
        let root = KeyTree::generate(key.seed, key.height).root();
        key.save(&dir.join("validators").join(format!("{name}.key")))?;
        infos.push(ValidatorInfo {
            name,
            root,
            height,
            stake: 1,
            address: format!("{host}:{}", base_port as usize + i),
        });
    }

    let mut allocations = Vec::new();
    for (name, amount) in [("alice", 1_000_000u64), ("bob", 1_000_000), ("carol", 0)] {
        let wallet = Wallet::create(&dir.join("wallets").join(format!("{name}.wallet")))?;
        if amount > 0 {
            allocations.push(Allocation { account: wallet.address(), amount });
        }
        println!("  wallet {name:<6} {}  starts with {amount}", wallet.address());
    }

    let genesis = Genesis { network: "token-devnet".into(), validators: infos, allocations };
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
    let order = Order { from: wallet.address(), seq, to, amount, next_key: wallet.key(seq + 1).commitment() };
    let digest = order.digest(&genesis.id());
    SignedOrder { signature: wallet.key(seq).sign(digest.as_bytes()), order }
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
    let genesis = load_genesis(&dir.join("genesis.json"))?;
    let client = Client::new(&genesis);
    let wallets = dir.join("wallets");
    let wallet_file = wallets.join(format!("{wallet_name}.wallet"));
    let wallet = Wallet::load(&wallet_file)?;
    let to = Wallet::load(&wallets.join(format!("{to_name}.wallet")))?.address();

    let status = read_account(&client, &wallet)?;
    if !status.locks.is_empty() || wallet.pending().is_some() {
        bail!("{wallet_name} already has an unfinished payment");
    }
    match status.spendable {
        Some(balance) if balance >= amount as i128 && amount > 0 => {}
        Some(balance) => bail!("{wallet_name} holds {balance}, which does not cover a payment of {amount}"),
        None => bail!("validators are not caught up"),
    }
    // Signed with the wallet's key but never written to the wallet file. To
    // the file, this is a payment made by another copy of the wallet.
    let seq = status.seq;
    let signed = sign_raw(&wallet, &genesis, seq, to, amount);
    let reached = (client.validators() / 2).max(1);
    println!(
        "another copy of {wallet_name}'s wallet signs payment #{seq}, {amount} to {to_name}, \
         and is lost after reaching {reached} of {} validators:",
        client.validators()
    );
    for index in 0..reached {
        let answer = describe(client.call(index, &Request::SubmitOrder(signed.clone())));
        println!("  {} {answer}", genesis.validators[index].name);
    }
    println!("the payment is signed but not final, and {} has never heard of it.", wallet_file.display());
    println!("  see it:     token balance {}", wallet_file.display());
    println!("  finish it:  token resume -w {}", wallet_file.display());
    Ok(())
}

fn double_spend(dir: &Path, wallet_name: &str, split: bool) -> Result<()> {
    let genesis = load_genesis(&dir.join("genesis.json"))?;
    let client = Client::new(&genesis);
    let wallets = dir.join("wallets");
    let wallet = Wallet::load(&wallets.join(format!("{wallet_name}.wallet")))?;
    let others: Vec<&str> = ["alice", "bob", "carol"].into_iter().filter(|n| *n != wallet_name).collect();
    let first_payee = Wallet::load(&wallets.join(format!("{}.wallet", others[0])))?.address();
    let second_payee = Wallet::load(&wallets.join(format!("{}.wallet", others[1])))?.address();

    let status = client.account(&wallet.address(), wallet.next_seq());
    let balance = status.spendable.ok_or_else(|| anyhow!("validators are not caught up"))?;
    if balance <= 0 {
        bail!("{wallet_name} has nothing to double-spend");
    }
    if !status.locks.is_empty() {
        bail!("{wallet_name}'s account is already locked by an earlier double-spend attempt");
    }
    let (seq, amount) = (status.seq, balance as u64);
    let first = sign_raw(&wallet, &genesis, seq, first_payee, amount);
    let second = sign_raw(&wallet, &genesis, seq, second_payee, amount);
    println!("{wallet_name} holds {amount} and signs payment #{seq} twice with the same one-time key:");
    println!("  A: all {amount} to {}", others[0]);
    println!("  B: all {amount} to {}", others[1]);

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
        println!("no money moved. {wallet_name}'s account is now stuck on payment #{seq}; nobody else is affected.");
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
