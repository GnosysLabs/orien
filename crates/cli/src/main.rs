use anyhow::{Context, Result, anyhow, bail};
use clap::{Args, Parser, Subcommand};
use orien_crypto::keytree::KeyTree;
use orien_ledger::{
    AccountId, Allocation, Certificate, Genesis, Receipt, Signature, SignedOrder, ValidatorInfo,
};
use orien_ledger::Signer;
use orien_net::client::{AccountStatus, Client, Done, Head, TransferError, Unfinished};
use orien_net::keys::{ValidatorKey, Wallet, load_genesis, save_genesis};
use orien_net::node::{Node, NodeConfig};
use orien_net::wire::{Request, Response};
use std::fs;
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

#[derive(Parser)]
#[command(name = "orien", version, about = "A quantum-proof payment network with no blocks and no global ordering")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Args)]
struct Network {
    /// Genesis file describing the network
    #[arg(short, long, env = "ORIEN_GENESIS", default_value = "genesis.json")]
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
        Command::Devnet(DevnetCommand::Init {
            dir,
            validators,
            height,
            host,
            base_port,
            addresses,
            name,
            epoch_seconds,
        }) => {
            let addresses = match addresses.is_empty() {
                true => (0..validators).map(|i| format!("{host}:{}", base_port as usize + i)).collect(),
                false => addresses,
            };
            devnet_init(&dir, &name, &addresses, height, epoch_seconds)
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
fn drop_cancelled(wallet: &mut Wallet, status: &AccountStatus) -> Result<()> {
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
    wallet: &mut Wallet,
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
fn finish(client: &Client, wallet: &mut Wallet, signed: &SignedOrder, receipt_file: Option<&Path>) -> Result<()> {
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
                "  payment #{}: {} from {} to {}",
                order.seq,
                order.amount,
                order.from.short(),
                order.to.short()
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
        "  payment #{}: {} from {} to {}",
        order.seq,
        order.amount,
        order.from.short(),
        order.to.short()
    );
    println!(
        "  {} of {} validators voted and {} confirmed; the receipt is {size} bytes",
        done.votes,
        client.validators(),
        done.confirmations
    );
    save_receipt(&done.receipt, receipt_file)
}

fn send(wallet_file: &Path, to: &str, amount: u64, receipt_file: Option<&Path>, genesis: &Path) -> Result<()> {
    let genesis_file = genesis;
    let (genesis, client) = connect(genesis_file)?;
    let network = genesis.id();
    let mut wallet = Wallet::load(wallet_file)?;
    let to = resolve_account(to)?;
    let status = read_account(&client, &wallet)?;
    let seq = status.seq;
    drop_cancelled(&mut wallet, &status)?;

    // A payment this wallet file signed in an earlier run may have gone
    // through since. If it is the very payment asked for now, sending it
    // again would pay twice.
    if let Some((order, certificate)) = settle_earlier(&client, &mut wallet, seq)?
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
                return Err(locked(&genesis, &client, seq, waiting));
            }
            bail!(
                "nothing else can be sent from this account until that payment is final. \
                 Finish it with: orien resume -w {}",
                wallet_file.display()
            );
        }
    }

    let signed = wallet.sign_order(seq, to, amount, status.signer.as_ref(), &network)?;
    let outcome = finish(&client, &mut wallet, &signed, receipt_file);
    remember(&client, genesis_file);
    outcome
}

fn resume(wallet_file: &Path, pick: Option<&str>, receipt_file: Option<&Path>, genesis: &Path) -> Result<()> {
    let genesis_file = genesis;
    let (genesis, client) = connect(genesis_file)?;
    let network = genesis.id();
    let mut wallet = Wallet::load(wallet_file)?;
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
    println!("finishing payment #{seq}: {} to {}", order.amount, order.to.short());
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
        "{:<6} {:<22} {:>7} {:>8} {:>9} {:>16}  {:<8}",
        "name", "address", "epoch", "accounts", "payments", "signing keys", "state"
    );
    let roots = client.roots();
    for ((validator, info), root) in genesis.validators.iter().zip(client.info()).zip(&roots) {
        match info {
            Ok(info) if info.network == genesis.id() => println!(
                "{:<6} {:<22} {:>7} {:>8} {:>9} {:>16}  {:<8}{}",
                validator.name,
                validator.address,
                info.epoch,
                info.accounts,
                info.certificates,
                format!("{}/{}", info.keys_used, info.keys_total),
                root.map_or("-".into(), |root| root.short()),
                if info.closing { "  closing the epoch" } else { "" }
            ),
            Ok(_) => println!("{:<6} {:<22} serves a different network", validator.name, validator.address),
            Err(error) => println!("{:<6} {:<22} unreachable ({error})", validator.name, validator.address),
        }
    }
    // The state column is a fingerprint of every balance. Equal fingerprints
    // mean equal ledgers.
    let mut known: Vec<_> = roots.iter().flatten().collect();
    let reporting = known.len();
    known.dedup();
    match (reporting, known.len()) {
        (0, _) => {}
        (_, 1) => println!("all {reporting} validators that answered hold exactly the same balances"),
        _ => println!("validators differ: a payment may still be spreading. Check again in a moment."),
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

fn devnet_init(dir: &Path, network: &str, addresses: &[String], height: u8, epoch_seconds: u64) -> Result<()> {
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
        infos.push(ValidatorInfo { name, root, height, stake: 1, address: address.clone() });
    }

    let mut allocations = Vec::new();
    for (name, amount) in [("alice", 1_000_000u64), ("bob", 1_000_000), ("carol", 0)] {
        let wallet = Wallet::create(&dir.join("wallets").join(format!("{name}.wallet")))?;
        if amount > 0 {
            allocations.push(Allocation { account: wallet.address(), amount });
        }
        println!("  wallet {name:<6} {}  starts with {amount}", wallet.address());
    }

    let mut genesis = Genesis::new(network, infos, allocations);
    genesis.epoch_seconds = epoch_seconds;
    genesis.start_time = unix_now();
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
    println!("  see it:     orien balance {}", wallet_file.display());
    println!("  finish it:  orien resume -w {}", wallet_file.display());
    Ok(())
}

fn double_spend(dir: &Path, wallet_name: &str, split: bool) -> Result<()> {
    let (genesis, client) = connect(&dir.join("genesis.json"))?;
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
