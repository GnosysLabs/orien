//! A validator.
//!
//! One thread, the core, owns the ledger state and the signing key and does
//! everything that must happen in a definite order. Connection threads do
//! the expensive, order-free work (checking signatures) and hand the core a
//! command with the result.
//!
//! The core gathers what it is asked to sign over a few milliseconds into a
//! batch: votes for the orders it accepts, and confirmations of the
//! certificates it applies. It writes to disk once and signs the batch once.
//! One disk sync and one signature cover however many arrived.
//!
//! Once an epoch is due to end, the core stops voting and confirming, and
//! closes the epoch with the other validators (`closer`). When a checkpoint
//! is agreed it moves, in one step, to the checkpoint's state and an empty
//! log for the next epoch. One thread per other validator keeps this one
//! caught up with it, and another sends it what this one says.

use crate::client::{connect, exchange, rpc};
use crate::closer::{Closer, Env, Pace, Wanted};
use crate::keys::ValidatorKey;
use crate::store::{CertLog, Epochs, LeafCounter, OrderLog, sync_dir};
use crate::wire::{AccountView, NodeInfo, Request, Response, read_frame, write_frame};
use anyhow::{Context, Result, anyhow, bail};
use orien_crypto::keytree::{self, KeyTree, TreeSignature};
use orien_crypto::{Hash32, merkle};
use orien_ledger::close::{self, Cheat, Decided, Message, NextKey, Pin, Statement};
use orien_ledger::{
    Accepted, AccountId, Applied, Certificate, Committee, Confirmation, Genesis, Order, Proofs, Reject,
    SignedBy, SignedOrder, State, Vote, confirmation_item,
};
use std::collections::HashMap;
use std::fs;
use std::io::{self, BufReader};
use std::net::{Shutdown, SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender, TryRecvError};
use std::sync::{Arc, Mutex, RwLock};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

fn unix_now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |since| since.as_secs())
}

pub struct NodeConfig {
    pub genesis: Genesis,
    pub key: ValidatorKey,
    pub data_dir: PathBuf,
    /// Address to listen on. Defaults to this validator's genesis address.
    pub listen: Option<String>,
    /// How long to gather orders before signing them as one batch.
    pub batch_window: Duration,
    pub max_batch: usize,
    /// How often to ask other validators for certificates this one missed.
    /// Zero turns syncing off.
    pub sync_interval: Duration,
    /// Force votes to disk before answering. Only tests turn this off.
    pub durable: bool,
    /// One-time keys held back from voting on payments, for the messages
    /// that close an epoch. Never more than a quarter of the key tree.
    pub key_reserve: u32,
    /// The unit of every wait while closing an epoch. A round's proposer is
    /// given six of them; later rounds get more.
    pub close_pace: Duration,
    /// How many seconds a proposed checkpoint's time may differ from this
    /// validator's clock.
    pub clock_tolerance: u64,
    pub log: bool,
}

impl NodeConfig {
    pub fn new(genesis: Genesis, key: ValidatorKey, data_dir: PathBuf) -> NodeConfig {
        NodeConfig {
            genesis,
            key,
            data_dir,
            listen: None,
            batch_window: Duration::from_millis(5),
            max_batch: 4096,
            sync_interval: Duration::from_secs(1),
            durable: true,
            key_reserve: 128,
            close_pace: Duration::from_millis(500),
            clock_tolerance: 30,
            log: false,
        }
    }
}

// Signatures are over a kilobyte and travel inline; these values are short-lived.
#[allow(clippy::large_enum_variant)]
enum Command {
    Order { signed: SignedOrder, digest: Hash32, signed_by: SignedBy, reply: Sender<Response> },
    /// A certificate whose votes have been checked. `signed_by` is the key
    /// its owner's signature was made with. With `confirm`, the answer is a
    /// signed confirmation.
    Certificate {
        certificate: Certificate,
        digest: Hash32,
        signed_by: SignedBy,
        confirm: bool,
        reply: Sender<Response>,
    },
    /// Sign a confirmation for a payment applied earlier.
    Confirm { order: Order, digest: Hash32, reply: Sender<Response> },
    /// Would a certificate for this payment still change anything here?
    Needs { from: AccountId, seq: u64, reply: Sender<bool> },
    Query { request: Request, reply: Sender<Response> },
    /// A message about closing `epoch`, from another validator.
    Close { epoch: u64, message: Message },
    /// Where this validator stands, for a thread about to ask a peer.
    Standing { reply: Sender<Standing> },
    /// The orders in a statement's pinned log, fetched from a peer. The
    /// answer lists the ones this validator does not hold.
    Log { statement: Hash32, orders: Vec<Hash32>, reply: Sender<Vec<Hash32>> },
    /// The state `epoch` starts from, fetched from a validator already there.
    Snapshot { epoch: u64, bytes: Vec<u8> },
    /// A replacement key tree is ready.
    NextKey(ValidatorKey, Box<KeyTree>),
    /// Votes seen in a certificate someone handed over, whether or not it
    /// holds up, to look for validators that cheated.
    Seen { epoch: u64, order: Order, digest: Hash32, votes: Vec<Vote> },
    Stop,
}

struct Standing {
    epoch: u64,
    /// Pinned logs it still has to fetch.
    wanted: Vec<Wanted>,
    /// It knows the checkpoint that closes its epoch, but not yet the state
    /// that checkpoint leads to.
    needs_state: bool,
}

struct Shared {
    /// The validators of the current epoch. The core replaces it at each
    /// checkpoint.
    committee: Arc<RwLock<Arc<Committee>>>,
    data_dir: PathBuf,
    /// Set while this validator is closing its epoch.
    closing: Arc<AtomicBool>,
    commands: Sender<Command>,
    stopping: AtomicBool,
    connections: Mutex<HashMap<u64, TcpStream>>,
    next_connection: AtomicU64,
}

fn stopped() -> Response {
    Response::Unavailable("validator is stopping".into())
}

/// Whether a closing message carries the signatures it should. Checked
/// before the message is handed to the core, so that junk costs the core
/// nothing.
fn genuine(committee: &Committee, message: &Message) -> bool {
    match message {
        Message::Statement(signed) => close::verify_statement(committee, signed),
        Message::Ballot(signed) => close::verify_ballot(committee, signed),
        Message::Proposal(signed) => close::verify_proposal(committee, signed),
        Message::Decided(decided) => close::verify_decided(committee, decided).is_ok(),
    }
}

impl Shared {
    fn ask<T>(&self, command: impl FnOnce(Sender<T>) -> Command) -> Option<T> {
        let (reply, answer) = mpsc::channel();
        self.commands.send(command(reply)).ok()?;
        answer.recv().ok()
    }

    fn committee(&self) -> Arc<Committee> {
        self.committee.read().unwrap().clone()
    }

    fn handle(&self, request: Request) -> Response {
        match request {
            Request::SubmitOrder(signed) => {
                let digest = signed.order.digest(self.committee().network());
                let Some(signed_by) = signed.signature.signed_by(&digest) else {
                    return Response::Rejected(Reject::BadSignature);
                };
                if !signed.order.stake_proof_holds(self.committee().network()) {
                    return Response::Rejected(Reject::BadValidator);
                }
                self.ask(|reply| Command::Order { signed, digest, signed_by, reply })
                    .unwrap_or_else(stopped)
            }
            Request::SubmitCertificate(certificate) => {
                self.ingest(certificate, false).unwrap_or_else(stopped)
            }
            Request::Confirm(certificate) => self.ingest(certificate, true).unwrap_or_else(stopped),
            Request::Close { epoch, message } => {
                let committee = self.committee();
                if epoch != committee.epoch() {
                    return Response::Epoch(committee.epoch());
                }
                if genuine(&committee, &message) {
                    let _ = self.commands.send(Command::Close { epoch, message });
                }
                Response::Received
            }
            request => self.ask(|reply| Command::Query { request, reply }).unwrap_or_else(stopped),
        }
    }

    /// Verifies a certificate and applies it, signing a confirmation if
    /// `confirm` asks for one. `None` means the node is stopping.
    fn ingest(&self, certificate: Certificate, confirm: bool) -> Option<Response> {
        let committee = self.committee();
        // Votes count only in the epoch they were cast in.
        if certificate.epoch != committee.epoch() {
            return Some(Response::Epoch(committee.epoch()));
        }
        let (from, seq) = (certificate.order.from, certificate.order.seq);
        let digest = certificate.order.digest(committee.network());
        // Every vote handed over is looked at for cheating, even in a
        // certificate that turns out to be no good.
        let seen = Command::Seen {
            epoch: certificate.epoch,
            order: certificate.order.clone(),
            digest,
            votes: certificate.votes.clone(),
        };
        if self.commands.send(seen).is_err() {
            return None;
        }
        if !self.ask(|reply| Command::Needs { from, seq, reply })? {
            // That payment number is already applied here.
            if confirm {
                let order = certificate.order;
                return self.ask(|reply| Command::Confirm { order, digest, reply });
            }
            // Say so only if it was this same order.
            let request = Request::GetCertificate { account: from, seq };
            let stored = self.ask(|reply| Command::Query { request, reply })?;
            return Some(match stored {
                Response::Certificate(Some(existing)) if existing.order == certificate.order => {
                    Response::Applied { fresh: false }
                }
                _ => Response::Invalid("a different payment is already final under that number".into()),
            });
        }
        if let Err(error) = committee.verify_certificate(&certificate) {
            return Some(Response::Invalid(error.to_string()));
        }
        let Some(signed_by) = certificate.signature.signed_by(&digest) else {
            return Some(Response::Invalid("the owner's signature in this certificate is malformed".into()));
        };
        if !certificate.order.stake_proof_holds(committee.network()) {
            return Some(Response::Invalid("the validator named did not sign for this stake".into()));
        }
        self.ask(|reply| Command::Certificate { certificate, digest, signed_by, confirm, reply })
    }
}

/// A vote or a confirmation waiting for the current batch to be signed.
struct Waiting {
    payment: (AccountId, u64),
    /// The item that goes into the batch: an order's digest for a vote, the
    /// confirmation item of that digest for a confirmation.
    digest: Hash32,
    confirmation: bool,
    replies: Vec<Sender<Response>>,
    /// The order to write to disk with this batch. `None` when it is
    /// already there from before a restart.
    record: Option<SignedOrder>,
}

#[allow(clippy::large_enum_variant)]
enum Issued {
    /// Position in the batch being gathered.
    Queued(usize),
    Signed(Vote),
}

/// The confirmation signed for an account's latest payment, kept so that a
/// repeated request gets the same one back and costs no further key. The
/// signature is shared by every confirmation of its batch.
struct Confirmed {
    seq: u64,
    batch_path: merkle::Path,
    signature: Arc<TreeSignature>,
}

/// Whether a recorded vote still binds: its payment number is the account's
/// next, or a later one, and it is signed with the key the account requires.
/// A number that a checkpoint cancelled is free again.
fn binds(state: &State, network: &Hash32, signed: &SignedOrder) -> bool {
    let order = &signed.order;
    let current = state.seq(&order.from);
    order.seq > current
        || (order.seq == current && signed.signed_by(network).is_some_and(|by| state.authorizes(order, &by)))
}

/// Keeps a signed order as the account's unfinished payment, unless one
/// with an earlier number is held already: that one has to finish first.
/// Of two under the same number, the one recorded later is kept.
fn hold(unfinished: &mut HashMap<AccountId, SignedOrder>, signed: SignedOrder) {
    match unfinished.get(&signed.order.from) {
        Some(held) if held.order.seq < signed.order.seq => {}
        _ => {
            unfinished.insert(signed.order.from, signed);
        }
    }
}

/// Makes the vote promised under each unfinished payment's number the vote
/// for that payment.
fn promise(state: &mut State, network: &Hash32, unfinished: &HashMap<AccountId, SignedOrder>) {
    for signed in unfinished.values() {
        let order = &signed.order;
        state.restore_lock(order.from, order.seq, order.digest(network));
    }
}

/// Signs `items` as one batch with one one-time key. Used for the messages
/// that close an epoch, which may draw on the keys held back from payments.
fn sign_batch(
    tree: &KeyTree,
    leaves: &mut LeafCounter,
    committee: &Committee,
    index: u16,
    items: &[Hash32],
) -> Option<Vec<Vote>> {
    let leaf = leaves.take().ok()??;
    let batch = merkle::Tree::build(items);
    let signature = tree.sign_with_leaf(leaf, committee.batch_message(&batch.root()).as_bytes());
    let votes = (0..items.len())
        .map(|place| Vote { validator: index, batch_path: batch.path(place), signature: signature.clone() });
    Some(votes.collect())
}

/// What a validator outside this epoch's list says when asked to vote.
const NOT_IN_THE_LIST: &str = "this validator is not one of this epoch's validators";

/// A validator's place in an epoch's list, found by the key tree it signs
/// with. Stakes can move it from one epoch to the next, or out of the list.
fn place_of(committee: &Committee, root: &Hash32) -> Option<u16> {
    committee.members().iter().position(|member| member.root == *root).map(|place| place as u16)
}

struct Core {
    name: String,
    /// Its place in this epoch's list of validators, or `None` while it is
    /// not one of them: it then follows the network and signs nothing.
    index: Option<u16>,
    /// The validators of the current epoch.
    committee: Committee,
    /// The same, where connection threads read it.
    committee_cell: Arc<RwLock<Arc<Committee>>>,
    /// Tells the threads that talk to other validators that a close is on.
    closing: Arc<AtomicBool>,
    tree: KeyTree,
    leaves: LeafCounter,
    /// A key tree built to replace `tree`, with its secret. It is named in
    /// this validator's closing statements until a checkpoint adopts it.
    next_key: Option<(ValidatorKey, KeyTree)>,
    /// True while a replacement tree is being built.
    building_key: bool,
    data_dir: PathBuf,
    epochs: Epochs,
    epoch_seconds: u64,
    epoch_payments: u64,
    /// The checkpoint that closed the epoch before this one, or the network
    /// id in the first epoch, and its time.
    previous: Hash32,
    previous_time: u64,
    /// The state this epoch started from: what the last checkpoint fixed.
    start: State,
    /// Proofs of accounts against that state, built on first use.
    proofs: Option<Proofs>,
    state: State,
    certs: CertLog,
    /// The payments applied in this epoch, in order: each order, its
    /// digest, and where to find it by digest.
    applied: Vec<Order>,
    digests: Vec<Hash32>,
    held: HashMap<Hash32, u32>,
    /// A running commitment to `digests`.
    pin: Pin,
    closer: Closer,
    pace: Pace,
    /// When closing work was last looked at.
    tended: Instant,
    /// The decided checkpoint of this epoch is on disk.
    checkpoint_saved: bool,
    /// Where to send what this validator says: one queue per other validator.
    /// A queue of messages to each other validator of this epoch, by
    /// address. Refreshed at every checkpoint: a validator that joins is
    /// added, and one that leaves is dropped, which ends its threads.
    peers: HashMap<String, Sender<Request>>,
    shared: Arc<Shared>,
    sync_interval: Duration,
    /// The first message each validator's one-time key was seen signing
    /// this epoch, with the signature, by key tree and key number.
    key_uses: HashMap<(Hash32, u32), (Hash32, TreeSignature)>,
    /// Proof, checked, that validators of this epoch cheated, and who.
    cheats: Vec<Cheat>,
    caught: Vec<u16>,
    commands: Sender<Command>,
    orders: OrderLog,
    /// The signed order behind each vote for a payment that is not final
    /// yet: at most one per account. Anyone may ask for it, so that a
    /// payment its owner signed can be finished from anywhere.
    unfinished: HashMap<AccountId, SignedOrder>,
    /// A second, different order signed with an account's current key under
    /// its current number. With the order in `unfinished` it is proof that
    /// the owner signed twice.
    conflicts: HashMap<AccountId, SignedOrder>,
    conflict_log: OrderLog,
    /// The state root, kept until the next payment changes the state.
    root: Option<Hash32>,
    key_reserve: u32,
    batch: Vec<Waiting>,
    deadline: Instant,
    /// Votes given for payments that are not final yet, so a repeated
    /// request gets the same vote back.
    issued: HashMap<(AccountId, u64), Issued>,
    /// Confirmations in the batch being gathered, by position.
    confirming: HashMap<(AccountId, u64), usize>,
    confirmed: HashMap<AccountId, Confirmed>,
    window: Duration,
    max_batch: usize,
    /// True while requests are arriving faster than batches are written.
    hurry: bool,
    durable: bool,
    log: bool,
}

impl Core {
    fn run(mut self, commands: Receiver<Command>) {
        loop {
            self.tend();
            let command = if self.batch.is_empty() {
                // Requests that queued up while the last batch was being
                // written are a batch already. Waiting to gather more would
                // only add delay, so the next batch closes as soon as the
                // queue is empty. An idle validator waits the full window,
                // to let requests sent at the same moment share a signature.
                match commands.try_recv() {
                    Ok(command) => {
                        self.hurry = true;
                        command
                    }
                    Err(TryRecvError::Disconnected) => break,
                    Err(TryRecvError::Empty) => {
                        self.hurry = false;
                        match commands.recv_timeout(self.rest()) {
                            Ok(command) => command,
                            // Time to look at the clock and the closing work.
                            Err(RecvTimeoutError::Timeout) => continue,
                            Err(RecvTimeoutError::Disconnected) => break,
                        }
                    }
                }
            } else if self.batch.len() >= self.max_batch {
                self.close_batch();
                continue;
            } else if self.hurry {
                match commands.try_recv() {
                    Ok(command) => command,
                    Err(TryRecvError::Disconnected) => break,
                    Err(TryRecvError::Empty) => {
                        self.close_batch();
                        continue;
                    }
                }
            } else {
                let now = Instant::now();
                if now >= self.deadline {
                    self.close_batch();
                    continue;
                }
                match commands.recv_timeout(self.deadline - now) {
                    Ok(command) => command,
                    Err(RecvTimeoutError::Timeout) => {
                        self.close_batch();
                        continue;
                    }
                    Err(RecvTimeoutError::Disconnected) => break,
                }
            };
            match command {
                Command::Order { signed, digest, signed_by, reply } => {
                    self.on_order(signed, digest, signed_by, reply)
                }
                Command::Certificate { certificate, digest, signed_by, confirm, reply } => {
                    self.on_certificate(certificate, digest, signed_by, confirm, reply)
                }
                Command::Confirm { order, digest, reply } => self.on_confirm(order, digest, reply),
                Command::Needs { from, seq, reply } => {
                    let _ = reply.send(self.state.seq(&from) <= seq);
                }
                Command::Query { request, reply } => {
                    let _ = reply.send(self.on_query(request));
                }
                Command::Close { epoch, message } => {
                    if epoch == self.committee.epoch() {
                        let said = self.close_step(|closer, env| closer.on_message(message, env));
                        self.say(said);
                        self.tended = Instant::now() - Duration::from_secs(1);
                    }
                }
                Command::Standing { reply } => {
                    let _ = reply.send(Standing {
                        epoch: self.committee.epoch(),
                        wanted: self.closer.wanted(Instant::now()),
                        needs_state: self.closer.decided().is_some(),
                    });
                }
                Command::Log { statement, orders, reply } => {
                    let missing = self.close_step(|closer, env| closer.on_log(statement, orders, env));
                    let _ = reply.send(missing.unwrap_or_default());
                }
                Command::Snapshot { epoch, bytes } => self.on_snapshot(epoch, &bytes),
                Command::NextKey(key, tree) => {
                    self.building_key = false;
                    self.next_key = Some((key, *tree));
                }
                Command::Seen { epoch, order, digest, votes } => self.on_seen(epoch, order, digest, votes),
                Command::Stop => break,
            }
        }
        if !self.batch.is_empty() {
            self.close_batch();
        }
    }

    /// How long the core may sleep when nothing arrives.
    fn rest(&self) -> Duration {
        let tick = if self.closer.busy() { Duration::from_millis(20) } else { Duration::from_millis(200) };
        match self.closer.next_wake() {
            Some(wake) => tick.min(wake.saturating_duration_since(Instant::now())),
            None => tick,
        }
    }

    /// Runs one step of the closer, lending it what it needs from here.
    fn close_step<R>(&mut self, step: impl FnOnce(&mut Closer, &mut Env) -> R) -> R {
        let Core { closer, committee, start, previous, previous_time, tree, leaves, index, held, applied, .. } = self;
        let (committee, tree, held, applied): (&Committee, &KeyTree, &HashMap<Hash32, u32>, &Vec<Order>) =
            (committee, tree, held, applied);
        // A validator outside the list is never asked to sign.
        let mut sign = |items: &[Hash32]| sign_batch(tree, leaves, committee, index.unwrap_or(u16::MAX), items);
        let order = |digest: &Hash32| held.get(digest).map(|place| &applied[*place as usize]);
        let mut env = Env {
            committee,
            start,
            previous: *previous,
            previous_time: *previous_time,
            now: Instant::now(),
            unix: unix_now(),
            sign: &mut sign,
            order: &order,
        };
        step(closer, &mut env)
    }

    /// Opens connections to every validator of this epoch it is not yet
    /// connected to, and drops those to validators that left.
    fn connect_peers(&mut self) {
        let own = self.tree.root();
        let wanted: Vec<String> = self
            .committee
            .members()
            .iter()
            .filter(|member| member.root != own)
            .map(|member| member.address.clone())
            .collect();
        self.peers.retain(|address, _| wanted.contains(address));
        for address in wanted {
            if self.peers.contains_key(&address) {
                continue;
            }
            let (queue, sending) = mpsc::channel();
            let pushing = address.clone();
            let spawned = thread::Builder::new()
                .name(format!("{}-push-{address}", self.name))
                .spawn(move || push_to_peer(pushing, sending));
            if spawned.is_err() {
                continue;
            }
            if !self.sync_interval.is_zero() {
                let (shared, interval, syncing) = (self.shared.clone(), self.sync_interval, address.clone());
                let _ = thread::Builder::new()
                    .name(format!("{}-sync-{address}", self.name))
                    .spawn(move || sync_with_peer(shared, syncing, interval));
            }
            self.peers.insert(address, queue);
        }
    }

    /// Sends closing messages to every other validator.
    fn say(&self, messages: Vec<Message>) {
        let epoch = self.committee.epoch();
        for message in messages {
            for peer in self.peers.values() {
                let _ = peer.send(Request::Close { epoch, message: message.clone() });
            }
        }
    }

    /// Looks at the clock and at the closing work. Called between commands.
    fn tend(&mut self) {
        let now = Instant::now();
        if now.saturating_duration_since(self.tended) < Duration::from_millis(5) {
            return;
        }
        self.tended = now;
        if self.index.is_some() && !self.closer.frozen() && self.closer.decided().is_none() {
            // An epoch ends when its time is up or it is full. A validator
            // also stops when others holding more than a third of the stake
            // have, which dishonest validators alone cannot bring about.
            let due = unix_now() >= self.previous_time.saturating_add(self.epoch_seconds)
                || self.applied.len() as u64 >= self.epoch_payments
                || 3 * self.closer.statement_stake() as u128 > self.closer.total_stake() as u128;
            if due {
                self.freeze();
            }
        }
        let said = self.close_step(|closer, env| closer.tick(env));
        self.say(said);
        self.closing.store(self.closer.frozen() || self.closer.decided().is_some(), Ordering::Relaxed);
        if let Some(decided) = self.closer.decided().cloned() {
            self.save_checkpoint(&decided);
            if let Some(end) = self.closer.take_end() {
                self.switch(decided, end);
            }
        }
        // Start building a new key tree in good time: a validator with no
        // keys left cannot even announce a replacement.
        if self.next_key.is_none() && !self.building_key && self.leaves.used() >= self.tree.capacity() / 2 {
            self.building_key = true;
            let (commands, height, path) = (self.commands.clone(), self.tree.height(), self.data_dir.join("next.key"));
            let _ = thread::Builder::new().name(format!("{}-key", self.name)).spawn(move || {
                let key = ValidatorKey::generate(height);
                let tree = KeyTree::generate(key.seed, key.height);
                // The secret is on disk before its root is ever announced.
                if key.save(&path).is_ok() {
                    let _ = commands.send(Command::NextKey(key, Box::new(tree)));
                }
            });
        }
    }

    /// Stops work on the epoch. From here until the checkpoint, this
    /// validator votes on no payment and confirms none.
    fn freeze(&mut self) {
        if !self.batch.is_empty() {
            self.close_batch();
        }
        // The log the statement pins must be on disk before the statement
        // leaves, or a crash could leave this validator unable to show it.
        if self.durable {
            self.certs.sync().expect("cannot sync certificates; stopping");
        }
        let mut evidence: Vec<(SignedOrder, SignedOrder)> = self
            .conflicts
            .iter()
            .filter_map(|(account, second)| self.unfinished.get(account).map(|first| (first.clone(), second.clone())))
            .collect();
        evidence.sort_by_key(|(first, _)| first.order.from);
        let statement = Statement {
            epoch: self.committee.epoch(),
            validator: self.index.expect("only a validator in the list stops work"),
            log: self.pin,
            evidence,
            next_key: self.next_key.as_ref().map(|(_, tree)| NextKey { root: tree.root(), height: tree.height() }),
            cheats: self.cheats.clone(),
        };
        let own_log = self.digests.clone();
        let said = self.close_step(|closer, env| closer.freeze(statement, own_log, env));
        if self.log && !said.is_empty() {
            println!(
                "[{}] closing  epoch {}: paused with {} payment(s) recorded",
                self.name,
                self.committee.epoch(),
                self.digests.len()
            );
        }
        self.say(said);
    }

    fn save_checkpoint(&mut self, decided: &Decided) {
        if !self.checkpoint_saved {
            let bytes = borsh::to_vec(decided).expect("writing to memory cannot fail");
            self.epochs.save_checkpoint(self.committee.epoch(), &bytes).expect("cannot store the checkpoint; stopping");
            self.checkpoint_saved = true;
        }
    }

    /// The state the next epoch starts from, fetched from a validator that
    /// is already there. Used when this validator knows which checkpoint
    /// closed its epoch but could not work out the state itself.
    fn on_snapshot(&mut self, epoch: u64, bytes: &[u8]) {
        let Some(decided) = self.closer.decided().cloned() else { return };
        if epoch != self.committee.epoch() + 1 {
            return;
        }
        // The checkpoint's state root is what more than two thirds agreed
        // on. A state that hashes to it is that state, whoever sent it.
        match State::from_snapshot(bytes, self.state.fees()) {
            Ok(end) if end.root() == decided.checkpoint.state => {
                self.save_checkpoint(&decided);
                self.switch(decided, end);
            }
            _ => {}
        }
    }

    /// Moves to the next epoch: the checkpoint's state, an empty log, the
    /// validators the checkpoint names. Whatever this validator had applied
    /// that the checkpoint leaves out is undone, and the signed order
    /// behind it goes back to being a promise.
    fn switch(&mut self, decided: Decided, end: State) {
        if !self.batch.is_empty() {
            self.close_batch();
        }
        let checkpoint = &decided.checkpoint;
        let network = *self.committee.network();
        // A promise still binds if its payment number is the account's
        // next, or later, and it is signed with the key the account now
        // requires. One whose number the checkpoint cancelled no longer does.
        let open = |signed: &SignedOrder| binds(&end, &network, signed);
        let mut left_out = Vec::new();
        for (place, order) in self.applied.iter().enumerate() {
            if end.seq(&order.from) <= order.seq
                && let Ok(Some(certificate)) = self.certs.get(place)
            {
                let signed = certificate.signed();
                if open(&signed) {
                    left_out.push(signed);
                }
            }
        }
        // On disk before the switch, so that a restart afterwards still
        // knows these promises.
        let records: Vec<&SignedOrder> = left_out.iter().collect();
        self.orders.append(&records, self.durable).expect("cannot record votes; stopping");
        let snapshot = end.snapshot();
        self.epochs.advance(&snapshot).expect("cannot start the next epoch; stopping");

        // From here on this validator is in the next epoch.
        let cancelled = self
            .unfinished
            .keys()
            .filter(|id| {
                let before = self.start.account(id).map_or(0, |account| account.spares_used);
                end.account(id).is_some_and(|account| account.spares_used > before)
            })
            .count();
        // Votes given first, then the payments that had been applied: where
        // both exist under one number, the payment that had its certificate
        // is the one this validator stays promised to.
        let mut promises: Vec<SignedOrder> =
            self.unfinished.drain().map(|(_, signed)| signed).filter(|signed| open(signed)).collect();
        promises.extend(left_out);
        self.conflicts.retain(|_, second| open(second));
        self.state = State::from_snapshot(&snapshot, self.state.fees()).expect("a snapshot just written reads back");
        self.start = self.state.clone();
        self.proofs = None;
        for signed in promises {
            let order = &signed.order;
            self.state.restore_lock(order.from, order.seq, order.digest(&network));
            hold(&mut self.unfinished, signed);
        }
        promise(&mut self.state, &network, &self.unfinished);
        self.conflicts.retain(|account, _| self.unfinished.contains_key(account));

        self.committee = self.committee.following(checkpoint.validators.clone());
        *self.committee_cell.write().unwrap() = Arc::new(self.committee.clone());
        self.previous = checkpoint.digest();
        self.previous_time = checkpoint.time;
        self.certs = CertLog::open(&self.epochs.log(), |_| {}).expect("cannot open the certificate log; stopping");
        self.applied.clear();
        self.digests.clear();
        self.held.clear();
        self.pin = Pin::start(self.committee.epoch());
        // Votes and confirmations were for the epoch that just ended.
        self.issued.clear();
        self.confirming.clear();
        self.confirmed.clear();
        // Proof of cheating counts in the epoch it happened in.
        self.key_uses.clear();
        self.cheats.clear();
        self.caught.clear();
        self.root = None;
        self.checkpoint_saved = false;

        // A replacement key tree named in this validator's statement takes
        // over once the checkpoint lists it.
        if let Some((_, tree)) = &self.next_key
            && self.committee.members().iter().any(|member| member.root == tree.root())
        {
            let (_, tree) = self.next_key.take().expect("checked above");
            let counter = self.data_dir.join(format!("leaf-{}", tree.root().short()));
            self.leaves = LeafCounter::open(counter, tree.capacity(), self.durable)
                .expect("cannot open the key counter; stopping");
            self.tree = tree;
            // The key file follows. Until it does, a restart finds the new
            // key under its old name and still recognises it.
            let _ = fs::rename(self.data_dir.join("next.key"), self.data_dir.join("current.key"));
            if self.log {
                println!("[{}] now signing with a fresh key tree ({})", self.name, self.tree.root().short());
            }
        }

        // Its place in the list, which stakes can change from one epoch to
        // the next, or none if it is not one of the validators any more.
        let place = place_of(&self.committee, &self.tree.root());
        if self.log && place.is_none() != self.index.is_none() {
            match place {
                Some(_) => println!("[{}] is one of the validators again from epoch {}", self.name, self.committee.epoch()),
                None => println!("[{}] is not one of the validators of epoch {}; following only", self.name, self.committee.epoch()),
            }
        }
        self.index = place;
        self.connect_peers();
        self.closer = Closer::open(
            &self.epochs.journal(),
            self.index,
            &self.committee,
            self.previous,
            self.pace,
            self.durable,
        )
        .expect("cannot open the closing journal; stopping");
        if self.durable {
            self.epochs.sync().expect("cannot sync the epoch folder; stopping");
        }
        if self.log {
            println!(
                "[{}] checkpoint {} agreed in round {}: {} payment(s), {} cancelled, state {}; epoch {} begins",
                self.name,
                checkpoint.epoch,
                decided.round,
                checkpoint.payments,
                cancelled,
                checkpoint.state.short(),
                self.committee.epoch()
            );
        }
    }

    /// Keeps the second of two different orders signed with one key.
    fn keep_evidence(&mut self, signed: SignedOrder) {
        let account = signed.order.from;
        if !self.conflicts.contains_key(&account) {
            // Its signature is good, or it would not have got this far.
            // Losing this record in a crash only delays what it is evidence
            // for, so it is not synced.
            self.conflict_log.append(&[&signed], false).expect("cannot record evidence; stopping");
            self.conflicts.insert(account, signed);
        }
    }

    fn on_order(&mut self, signed: SignedOrder, digest: Hash32, signed_by: SignedBy, reply: Sender<Response>) {
        if self.index.is_none() {
            let _ = reply.send(Response::Unavailable(NOT_IN_THE_LIST.into()));
            return;
        }
        let payment = (signed.order.from, signed.order.seq);
        if self.closer.frozen() {
            // No votes until the checkpoint. A second order signed with a
            // key this validator has already voted under is still kept, as
            // evidence for the checkpoint after this one.
            let order = &signed.order;
            let conflicting = self.state.seq(&order.from) == order.seq
                && self.state.lock(&order.from, order.seq).is_some_and(|voted| *voted != digest)
                && self.state.authorizes(order, &signed_by);
            if conflicting {
                self.keep_evidence(signed);
            }
            let _ = reply.send(Response::Closing);
            return;
        }
        match self.state.check_order(&signed.order, digest, &signed_by) {
            Ok(Accepted::Fresh) => {
                self.unfinished.insert(payment.0, signed.clone());
                self.enqueue(payment, digest, reply, Some(signed));
            }
            Ok(Accepted::Repeat) => match self.issued.get(&payment) {
                Some(Issued::Signed(vote)) => {
                    let _ = reply.send(Response::Vote(vote.clone()));
                }
                Some(Issued::Queued(position)) => self.batch[*position].replies.push(reply),
                // Voted before a restart: the vote is on disk but its
                // signature is gone. Signing the same order again is harmless.
                None => self.enqueue(payment, digest, reply, None),
            },
            Err(reject) => {
                if matches!(reject, Reject::Conflict { .. }) {
                    self.keep_evidence(signed);
                }
                if self.log {
                    println!(
                        "[{}] refused  {} #{}: {reject}",
                        self.name,
                        payment.0.short(),
                        payment.1
                    );
                }
                let _ = reply.send(Response::Rejected(reject));
            }
        }
    }

    fn enqueue(
        &mut self,
        payment: (AccountId, u64),
        digest: Hash32,
        reply: Sender<Response>,
        record: Option<SignedOrder>,
    ) {
        if self.batch.is_empty() {
            self.deadline = Instant::now() + self.window;
        }
        self.issued.insert(payment, Issued::Queued(self.batch.len()));
        self.batch.push(Waiting { payment, digest, confirmation: false, replies: vec![reply], record });
    }

    /// Queues a confirmation of the applied order with this digest.
    fn enqueue_confirmation(&mut self, payment: (AccountId, u64), digest: Hash32, reply: Sender<Response>) {
        if self.index.is_none() {
            let _ = reply.send(Response::Unavailable(NOT_IN_THE_LIST.into()));
            return;
        }
        // Two requests for the same confirmation share one place in the batch.
        if let Some(position) = self.confirming.get(&payment) {
            self.batch[*position].replies.push(reply);
            return;
        }
        if self.batch.is_empty() {
            self.deadline = Instant::now() + self.window;
        }
        self.confirming.insert(payment, self.batch.len());
        self.batch.push(Waiting {
            payment,
            digest: confirmation_item(&digest),
            confirmation: true,
            replies: vec![reply],
            record: None,
        });
    }

    /// Makes the batch durable, signs it all at once, and answers everyone
    /// who is waiting. Disk first. A vote that is not on disk must never
    /// leave this process, or a crash could let the validator vote for a
    /// conflicting order afterwards. A confirmation must never leave before
    /// the certificate it confirms is on disk, or the validator could forget
    /// a payment it said it had recorded.
    fn close_batch(&mut self) {
        let batch = std::mem::take(&mut self.batch);
        self.confirming.clear();
        let records: Vec<&SignedOrder> = batch.iter().filter_map(|w| w.record.as_ref()).collect();
        // Certificates are written as they are applied and need to be on
        // disk only before a confirmation of them leaves. A certificate
        // lost in a crash before that is fetched again from the others.
        let sync_certs = self.durable && batch.iter().any(|waiting| waiting.confirmation);
        let (orders, certs, durable) = (&mut self.orders, &self.certs, self.durable);
        // The two files are synced side by side, so a batch holding both
        // votes and confirmations waits for the slower sync, not for both.
        thread::scope(|scope| {
            let certs = (sync_certs && !records.is_empty()).then(|| scope.spawn(|| certs.sync()));
            orders.append(&records, durable).expect("cannot record votes; stopping");
            match certs {
                Some(syncing) => syncing.join().expect("certificate sync panicked"),
                None if sync_certs => self.certs.sync(),
                None => Ok(()),
            }
            .expect("cannot sync certificates; stopping");
        });

        let leaf = self.leaves.take_leaving(self.key_reserve).expect("cannot reserve signing keys; stopping");
        let leaf = match leaf {
            Some(leaf) => leaf,
            None => {
                for waiting in batch {
                    if !waiting.confirmation {
                        self.issued.remove(&waiting.payment);
                    }
                    for reply in waiting.replies {
                        let _ = reply.send(Response::Unavailable(
                            "this validator has no one-time keys left for payments".into(),
                        ));
                    }
                }
                return;
            }
        };

        let digests: Vec<Hash32> = batch.iter().map(|w| w.digest).collect();
        let tree = merkle::Tree::build(&digests);
        let message = self.committee.batch_message(&tree.root());
        let signature = self.tree.sign_with_leaf(leaf, message.as_bytes());
        if self.log {
            let confirmations = batch.iter().filter(|w| w.confirmation).count();
            println!(
                "[{}] signed   {} vote(s) and {confirmations} confirmation(s) with one signature (key {leaf})",
                self.name,
                batch.len() - confirmations
            );
        }
        let shared = Arc::new(signature.clone());
        for (position, waiting) in batch.into_iter().enumerate() {
            let (account, seq) = waiting.payment;
            let vote = Vote {
                validator: self.index.unwrap_or(u16::MAX),
                batch_path: tree.path(position),
                signature: signature.clone(),
            };
            if waiting.confirmation {
                for reply in waiting.replies {
                    let _ = reply.send(Response::Confirmed(vote.clone()));
                }
                // Keep it while this is still the account's latest payment.
                if self.state.seq(&account) == seq + 1 {
                    let kept = Confirmed { seq, batch_path: vote.batch_path, signature: shared.clone() };
                    self.confirmed.insert(account, kept);
                }
                continue;
            }
            for reply in waiting.replies {
                let _ = reply.send(Response::Vote(vote.clone()));
            }
            // Keep the vote for repeat requests, unless the payment was
            // applied while the batch was open.
            if self.state.seq(&account) == seq {
                self.issued.insert(waiting.payment, Issued::Signed(vote));
            } else {
                self.issued.remove(&waiting.payment);
            }
        }
    }

    fn on_certificate(
        &mut self,
        certificate: Certificate,
        digest: Hash32,
        signed_by: SignedBy,
        confirm: bool,
        reply: Sender<Response>,
    ) {
        let order = &certificate.order;
        let payment = (order.from, order.seq);
        // It was checked against the validators of an epoch that may have
        // ended while it waited in the queue.
        if certificate.epoch != self.committee.epoch() {
            let _ = reply.send(Response::Epoch(self.committee.epoch()));
            return;
        }
        // The votes say validators checked the owner's signature on the
        // order. The copy carried in the certificate is checked here, since
        // it is what this validator would be left holding if a checkpoint
        // undid the payment.
        if self.state.seq(&order.from) == order.seq && !self.state.authorizes(order, &signed_by) {
            let _ = reply.send(Response::Invalid(
                "the owner's signature in this certificate does not verify".into(),
            ));
            return;
        }
        let answer = match self.state.apply(order) {
            Ok(Applied::New) => {
                self.certs.append(&certificate).expect("cannot store certificate; stopping");
                self.held.insert(digest, self.applied.len() as u32);
                self.applied.push(order.clone());
                self.digests.push(digest);
                self.pin.add(&digest);
                self.unfinished.remove(&order.from);
                self.conflicts.remove(&order.from);
                self.root = None;
                if !matches!(self.issued.get(&(order.from, order.seq)), Some(Issued::Queued(_))) {
                    self.issued.remove(&(order.from, order.seq));
                }
                if self.log {
                    println!(
                        "[{}] applied  {} paid {} to {} (payment #{})",
                        self.name,
                        order.from.short(),
                        order.amount,
                        order.to.short(),
                        order.seq
                    );
                }
                if confirm && self.closer.frozen() {
                    // Recorded, but nothing is confirmed until the checkpoint.
                    Response::Closing
                } else if confirm {
                    return self.enqueue_confirmation(payment, digest, reply);
                } else {
                    Response::Applied { fresh: true }
                }
            }
            // It was applied between the check and now.
            Ok(Applied::Already) if confirm => return self.on_confirm(certificate.order, digest, reply),
            Ok(Applied::Already) => Response::Applied { fresh: false },
            Err(reject) => Response::Rejected(reject),
        };
        let _ = reply.send(answer);
    }

    /// Signs a confirmation for a payment this validator applied earlier.
    fn on_confirm(&mut self, order: Order, digest: Hash32, reply: Sender<Response>) {
        if self.index.is_none() {
            let _ = reply.send(Response::Unavailable(NOT_IN_THE_LIST.into()));
            return;
        }
        let payment = (order.from, order.seq);
        // Only for the exact order that was applied.
        let answer = match self.certs.find(&order.from, order.seq) {
            Ok(Some(applied)) if applied.order == order => None,
            Ok(Some(_)) => Some(Response::Invalid(
                "a different payment is already final under that number".into(),
            )),
            Ok(None) => Some(Response::Unavailable(
                "that payment was applied in an earlier epoch and is no longer confirmed".into(),
            )),
            Err(error) => Some(Response::Unavailable(format!("cannot read certificate: {error}"))),
        };
        if let Some(answer) = answer {
            let _ = reply.send(answer);
            return;
        }
        if self.closer.frozen() {
            let _ = reply.send(Response::Closing);
            return;
        }
        if let Some(kept) = self.confirmed.get(&order.from)
            && kept.seq == order.seq
        {
            let _ = reply.send(Response::Confirmed(Confirmation {
                validator: self.index.unwrap_or(u16::MAX),
                batch_path: kept.batch_path.clone(),
                signature: (*kept.signature).clone(),
            }));
            return;
        }
        // Once an account has made a later payment, validators holding a
        // quorum must have applied this one to vote for the next, so nobody
        // needs it confirmed any more. Confirming only the latest payment
        // also bounds how many keys replayed old certificates can use up.
        if self.state.seq(&order.from) != order.seq + 1 {
            let _ = reply.send(Response::Applied { fresh: false });
            return;
        }
        self.enqueue_confirmation(payment, digest, reply);
    }

    /// Looks at votes for signs of cheating: a vote for a different order
    /// than the one applied under the same number this epoch, or a key
    /// already seen signing something else.
    fn on_seen(&mut self, epoch: u64, order: Order, digest: Hash32, votes: Vec<Vote>) {
        if epoch != self.committee.epoch() {
            return;
        }
        if let Ok(Some(applied)) = self.certs.find(&order.from, order.seq)
            && applied.order != order
        {
            for vote in &votes {
                if let Some(theirs) = applied.votes.iter().find(|theirs| theirs.validator == vote.validator) {
                    let one = (applied.order.clone(), theirs.clone());
                    self.caught_cheating(Cheat::TwoVotes { one, other: (order.clone(), vote.clone()) });
                }
            }
        }
        for vote in votes {
            let Some(member) = self.committee.members().get(vote.validator as usize) else { continue };
            let Some(batch_root) = merkle::root_from_path(&digest, &vote.batch_path) else { continue };
            let message = self.committee.batch_message(&batch_root);
            let use_of_key = (member.root, vote.signature.leaf);
            match self.key_uses.get(&use_of_key) {
                Some((seen, _)) if *seen == message => {}
                Some((seen, signature)) => {
                    let one = (*seen, signature.clone());
                    self.caught_cheating(Cheat::KeyReuse { one, other: (message, vote.signature) });
                }
                // Only a genuine signature is remembered, so that a forged
                // one cannot stand in the way of catching the real thing.
                None => {
                    if keytree::verify(&member.root, member.height, &vote.signature, message.as_bytes()) {
                        self.key_uses.insert(use_of_key, (message, vote.signature));
                    }
                }
            }
        }
    }

    fn caught_cheating(&mut self, cheat: Cheat) {
        let Some(cheater) = cheat.cheater(&self.committee) else { return };
        if self.caught.contains(&cheater) {
            return;
        }
        if self.log {
            println!("[{}] caught validator {cheater} cheating; it loses its stake at the checkpoint", self.name);
        }
        self.caught.push(cheater);
        self.cheats.push(cheat);
    }

    fn on_query(&mut self, request: Request) -> Response {
        match request {
            Request::Info => Response::Info(NodeInfo {
                network: *self.committee.network(),
                validator: self.index,
                name: self.name.clone(),
                accounts: self.state.accounts() as u64,
                certificates: self.certs.len() as u64,
                keys_used: self.leaves.used(),
                keys_total: self.tree.capacity(),
                epoch: self.committee.epoch(),
                closing: self.closer.frozen(),
                epoch_started: self.previous_time,
            }),
            Request::Account(id) => Response::Account(self.state.account(&id).map(|account| {
                AccountView {
                    balance: account.balance,
                    seq: account.seq,
                    signer: account.signer.clone(),
                    spare: account.spare,
                    lock: self.state.lock(&id, account.seq).copied(),
                }
            })),
            Request::GetCertificate { account, seq } => match self.certs.find(&account, seq) {
                Ok(certificate) => Response::Certificate(certificate),
                Err(error) => Response::Unavailable(format!("cannot read certificate: {error}")),
            },
            Request::Sync { epoch, certs: from, messages, max } => {
                let mine = self.committee.epoch();
                let (mut certs, mut heard, mut decided) = (Vec::new(), Vec::new(), None);
                let (mut certs_total, mut messages_total) = (0, 0);
                if epoch == mine {
                    certs_total = self.certs.len() as u64;
                    let mut bytes = 0usize;
                    for place in from..certs_total.min(from.saturating_add(max as u64)) {
                        match self.certs.get(place as usize) {
                            Ok(Some(certificate)) => {
                                // Stay well under the frame limit.
                                bytes += 2048 * (certificate.votes.len() + 1);
                                certs.push(certificate);
                                if bytes > 4 << 20 {
                                    break;
                                }
                            }
                            _ => break,
                        }
                    }
                    let (said, total) = self.closer.messages(messages as usize, 64);
                    heard = said.to_vec();
                    messages_total = total as u64;
                } else if epoch < mine {
                    // The asker is behind. Show it how its epoch ended.
                    decided = self.checkpoint(epoch);
                }
                Response::Sync { epoch: mine, certs, certs_total, messages: heard, messages_total, decided }
            }
            Request::Log { epoch, statement, from, max } => Response::Log(
                (epoch == self.committee.epoch())
                    .then(|| self.closer.log(&statement, from as usize, max as usize).map(<[Hash32]>::to_vec))
                    .flatten(),
            ),
            Request::Payments { epoch, orders } => {
                let mut certs = Vec::new();
                let mut bytes = 0usize;
                if epoch == self.committee.epoch() {
                    for digest in orders {
                        let Some(place) = self.held.get(&digest) else { break };
                        let Ok(Some(certificate)) = self.certs.get(*place as usize) else { break };
                        bytes += 2048 * (certificate.votes.len() + 1);
                        certs.push(certificate);
                        if bytes > 4 << 20 {
                            break;
                        }
                    }
                }
                Response::Payments(certs)
            }
            Request::Snapshot { epoch, offset, max } => {
                let max = (max as usize).min(4 << 20);
                Response::Snapshot(self.epochs.state_chunk(epoch, offset, max).ok().flatten())
            }
            Request::Checkpoints { from, max } => {
                let until = self.committee.epoch().min(from.saturating_add(max.min(64) as u64));
                Response::Checkpoints((from..until).map_while(|epoch| self.checkpoint(epoch)).collect())
            }
            Request::Proof(id) => {
                let proofs = self.proofs.get_or_insert_with(|| self.start.proofs());
                let account = self.start.account(&id).cloned().zip(proofs.path(&id));
                Response::Proof { epoch: self.committee.epoch(), account }
            }
            Request::Pending(id) => Response::Pending(self.unfinished.get(&id).cloned()),
            Request::Root => Response::Root(*self.root.get_or_insert_with(|| self.state.root())),
            Request::Evidence(id) => Response::Evidence(
                self.unfinished.get(&id).cloned().zip(self.conflicts.get(&id).cloned()),
            ),
            Request::SubmitOrder(_)
            | Request::SubmitCertificate(_)
            | Request::Confirm(_)
            | Request::Close { .. } => Response::Invalid("not a query".into()),
        }
    }

    /// The checkpoint that closed `epoch`, with its proof, if it is on disk.
    fn checkpoint(&self, epoch: u64) -> Option<Decided> {
        let bytes = self.epochs.checkpoint(epoch).ok()??;
        borsh::from_slice(&bytes).ok()
    }
}

fn accept_loop(listener: TcpListener, shared: Arc<Shared>) {
    for stream in listener.incoming() {
        if shared.stopping.load(Ordering::SeqCst) {
            break;
        }
        match stream {
            Ok(stream) => {
                let shared = shared.clone();
                let _ = thread::Builder::new().spawn(move || serve(stream, shared));
            }
            Err(_) => thread::sleep(Duration::from_millis(20)),
        }
    }
}

fn serve(stream: TcpStream, shared: Arc<Shared>) {
    let _ = stream.set_nodelay(true);
    let id = shared.next_connection.fetch_add(1, Ordering::Relaxed);
    if let Ok(handle) = stream.try_clone() {
        shared.connections.lock().unwrap().insert(id, handle);
    }
    let _ = serve_requests(&stream, &shared);
    shared.connections.lock().unwrap().remove(&id);
}

fn serve_requests(stream: &TcpStream, shared: &Shared) -> io::Result<()> {
    let mut reader = BufReader::new(stream);
    let mut writer = stream;
    while let Some(frame) = read_frame(&mut reader)? {
        let response = match borsh::from_slice::<Request>(&frame) {
            Ok(request) => shared.handle(request),
            Err(_) => Response::Invalid("malformed request".into()),
        };
        write_frame(&mut writer, &borsh::to_vec(&response)?)?;
    }
    Ok(())
}

/// Keeps this validator caught up with one other. Every round it asks the
/// peer for the certificates and closing messages it has not read yet, and
/// fetches whatever pinned logs this validator still needs for the close.
/// If the peer is in a later epoch, it asks how this validator's epoch
/// ended and for the state the next one starts from.
/// Keeps up with one other validator, for as long as it is one of the
/// current epoch's validators.
fn sync_with_peer(shared: Arc<Shared>, address: String, interval: Duration) {
    const TIMEOUT: Duration = Duration::from_secs(3);
    /// How long a log is left to the validator it belongs to before every
    /// peer is asked for it.
    const PATIENCE: Duration = Duration::from_millis(400);
    let named: String = address.chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '_' }).collect();
    let cursor_file = |epoch: u64| Epochs::folder_in(&shared.data_dir, epoch).join(format!("sync-{named}.cursor"));
    // Its place in the current list, if it is still in it.
    let place = || shared.committee().members().iter().position(|m| m.address == address).map(|p| p as u16);
    let stopping = || shared.stopping.load(Ordering::SeqCst) || place().is_none();
    let (mut epoch, mut certs, mut messages) = (u64::MAX, 0u64, 0u64);
    loop {
        // While an epoch is closing, every moment of delay is a moment in
        // which nobody can pay. So the wait is cut short as soon as a close
        // begins, and stays short until it is over.
        let quick = interval.min(Duration::from_millis(40));
        let slept = Instant::now();
        loop {
            if stopping() {
                return;
            }
            let pause = if shared.closing.load(Ordering::Relaxed) { quick } else { interval };
            if slept.elapsed() >= pause {
                break;
            }
            thread::sleep(Duration::from_millis(10).min(pause));
        }
        let Some(standing) = shared.ask(|reply| Command::Standing { reply }) else { return };
        if standing.epoch != epoch {
            epoch = standing.epoch;
            messages = 0;
            certs = fs::read_to_string(cursor_file(epoch)).ok().and_then(|text| text.trim().parse().ok()).unwrap_or(0);
        }

        loop {
            if stopping() {
                return;
            }
            let request = Request::Sync { epoch, certs, messages, max: 256 };
            let Ok(Response::Sync { epoch: theirs, certs: got, certs_total, messages: heard, messages_total, decided }) =
                rpc(&address, &request, TIMEOUT)
            else {
                break;
            };
            if theirs > epoch {
                // The peer has moved on. Learn how this epoch ended, and if
                // this validator cannot work out the resulting state, take
                // it from the peer. It is checked against the checkpoint.
                if let Some(decided) = decided {
                    let _ = shared.commands.send(Command::Close { epoch, message: Message::Decided(decided) });
                }
                let needs_state = shared
                    .ask(|reply| Command::Standing { reply })
                    .is_some_and(|standing| standing.epoch == epoch && standing.needs_state);
                if needs_state && let Some(bytes) = fetch_snapshot(&address, epoch + 1, TIMEOUT) {
                    let _ = shared.commands.send(Command::Snapshot { epoch: epoch + 1, bytes });
                }
                break;
            }
            if theirs < epoch {
                break;
            }
            if certs > certs_total || messages > messages_total {
                // The peer's lists are shorter than what was read from them:
                // it was wiped and restarted. Read them again from the top.
                (certs, messages) = (0, 0);
                continue;
            }
            if got.is_empty() && heard.is_empty() {
                break;
            }
            for certificate in got {
                if shared.ingest(certificate, false).is_none() {
                    return;
                }
                certs += 1;
            }
            for message in heard {
                if genuine(&shared.committee(), &message) {
                    let _ = shared.commands.send(Command::Close { epoch, message });
                }
                messages += 1;
            }
            let _ = fs::write(cursor_file(epoch), certs.to_string());
            if certs >= certs_total && messages >= messages_total {
                break;
            }
        }

        // The logs other validators pinned. Each is asked of its own
        // validator first, and of everyone once that has taken too long.
        for wanted in standing.wanted {
            if stopping() {
                return;
            }
            if Some(wanted.validator) == place() || wanted.age > PATIENCE {
                fetch_log(&shared, &address, epoch, &wanted, TIMEOUT);
            }
        }
    }
}

/// Fetches the state `epoch` starts from, in pieces.
fn fetch_snapshot(address: &str, epoch: u64, timeout: Duration) -> Option<Vec<u8>> {
    let mut bytes = Vec::new();
    loop {
        let request = Request::Snapshot { epoch, offset: bytes.len() as u64, max: 4 << 20 };
        let Ok(Response::Snapshot(Some((piece, total)))) = rpc(address, &request, timeout) else { return None };
        if piece.is_empty() && (bytes.len() as u64) < total {
            return None;
        }
        bytes.extend_from_slice(&piece);
        if bytes.len() as u64 >= total {
            return Some(bytes);
        }
    }
}

/// Fetches a pinned log from a peer, and then the payments in it that this
/// validator lacks, in the order the log lists them.
fn fetch_log(shared: &Shared, address: &str, epoch: u64, wanted: &Wanted, timeout: Duration) {
    let mut orders: Vec<Hash32> = Vec::new();
    while (orders.len() as u64) < wanted.pin.length {
        let request = Request::Log { epoch, statement: wanted.statement, from: orders.len() as u64, max: 50_000 };
        match rpc(address, &request, timeout) {
            Ok(Response::Log(Some(page))) if !page.is_empty() => orders.extend(page),
            _ => return,
        }
    }
    let statement = wanted.statement;
    let Some(missing) = shared.ask(|reply| Command::Log { statement, orders, reply }) else { return };
    for wanted in missing.chunks(128) {
        let request = Request::Payments { epoch, orders: wanted.to_vec() };
        let Ok(Response::Payments(certificates)) = rpc(address, &request, timeout) else { return };
        let complete = certificates.len() == wanted.len();
        for certificate in certificates {
            if shared.ingest(certificate, false).is_none() {
                return;
            }
        }
        if !complete {
            return;
        }
    }
}

/// Sends this validator's closing messages to one other, in order. A
/// message that cannot be delivered is dropped: the other validator also
/// asks for what it has missed.
fn push_to_peer(address: String, queue: Receiver<Request>) {
    let mut stream: Option<TcpStream> = None;
    for request in queue {
        for _ in 0..2 {
            if stream.is_none() {
                stream = connect(&address, Duration::from_secs(1)).ok();
            }
            let sent = stream.as_mut().is_some_and(|open| exchange(open, &request).is_ok());
            if sent {
                break;
            }
            stream = None;
        }
    }
}

/// The layout and encoding of a validator's files. Raised whenever either
/// changes in a way an older or newer version would misread.
const DATA_FORMAT: &str = "5";

/// A data directory belongs to one network and is written in one format.
/// Refuse to reuse it for another network, or to read a format this version
/// does not write: misreading old records could make a validator forget
/// payments it applied or votes it promised.
fn claim_data_dir(dir: &Path, network: &Hash32) -> Result<()> {
    fs::create_dir_all(dir).with_context(|| format!("cannot create {}", dir.display()))?;
    let (marker, format) = (dir.join("network"), dir.join("format"));
    match fs::read_to_string(&marker) {
        Ok(existing) if existing.trim() == network.to_string() => {
            // Directories from before the marker existed count as format 1.
            let found = fs::read_to_string(&format).unwrap_or_else(|_| "1".into());
            if found.trim() != DATA_FORMAT {
                bail!(
                    "{} was written in data format {}, and this version reads and writes format \
                     {DATA_FORMAT}. Use a fresh data directory, and only if the whole network is \
                     starting over.",
                    dir.display(),
                    found.trim()
                );
            }
            Ok(())
        }
        Ok(_) => bail!(
            "{} holds data from a different network; use another data directory",
            dir.display()
        ),
        Err(_) => {
            fs::write(&format, DATA_FORMAT)?;
            Ok(fs::write(&marker, network.to_string())?)
        }
    }
}

/// A running validator. Stops when dropped.
pub struct Node {
    pub addr: SocketAddr,
    /// Its place among the genesis file's validators, if it is one of them.
    pub index: Option<u16>,
    pub name: String,
    shared: Arc<Shared>,
    core: Option<JoinHandle<()>>,
    listener: Option<JoinHandle<()>>,
}

impl Node {
    /// Starts a validator. Pass a listener to use a socket that is already
    /// bound; otherwise the configured or genesis address is bound.
    pub fn start(config: NodeConfig, listener: Option<TcpListener>) -> Result<Node> {
        config.genesis.validate().map_err(|e| anyhow!("invalid genesis: {e}"))?;
        let genesis_committee = Committee::new(&config.genesis);
        let network = *genesis_committee.network();
        let original = KeyTree::generate(config.key.seed, config.key.height);
        // A launch seat is named in the genesis file. Any other validator
        // starts outside the list, follows the checkpoints, and takes part
        // once a stake gets it chosen.
        let index = config
            .genesis
            .validators
            .iter()
            .position(|v| v.root == original.root() && v.height == config.key.height);
        let info = index.map(|index| &config.genesis.validators[index]);
        let name = info.map_or_else(|| format!("new-{}", original.root().short()), |info| info.name.clone());
        let genesis_address = info.map(|info| info.address.clone());
        if genesis_address.is_none() && config.listen.is_none() && listener.is_none() {
            bail!("this validator is not in the genesis file, so it needs an address to listen on (--listen)");
        }
        let data_dir = config.data_dir.clone();

        claim_data_dir(&data_dir, &network)?;
        let epochs = Epochs::open(&data_dir, config.durable)?;
        let epoch = epochs.current();
        // The validators of the current epoch, and the checkpoint before it.
        let (committee, previous, previous_time) = match epoch {
            0 => (genesis_committee, network, config.genesis.start_time),
            _ => {
                let bytes = epochs
                    .checkpoint(epoch - 1)?
                    .ok_or_else(|| anyhow!("the checkpoint that closed epoch {} is missing", epoch - 1))?;
                let decided: Decided = borsh::from_slice(&bytes).context("a stored checkpoint is corrupt")?;
                let checkpoint = decided.checkpoint;
                let committee = genesis_committee.at_epoch(epoch - 1).following(checkpoint.validators.clone());
                (committee, checkpoint.digest(), checkpoint.time)
            }
        };

        // Which key tree this validator signs with now: the one it was
        // given, or a replacement it built later and a checkpoint adopted.
        let stored = |file: &str| -> Result<Option<(ValidatorKey, KeyTree)>> {
            let path = data_dir.join(file);
            if !path.exists() {
                return Ok(None);
            }
            let key = ValidatorKey::load(&path)?;
            let tree = KeyTree::generate(key.seed, key.height);
            Ok(Some((key, tree)))
        };
        let mut next_key = stored("next.key")?;
        let listed = |tree: &KeyTree| place_of(&committee, &tree.root()).is_some();
        let current = stored("current.key")?;
        let tree = if next_key.as_ref().is_some_and(|(_, tree)| listed(tree)) {
            // It took over just before a restart; the file was not renamed.
            fs::rename(data_dir.join("next.key"), data_dir.join("current.key"))?;
            next_key.take().expect("checked above").1
        } else if let Some((_, tree)) = current {
            // A replacement adopted earlier, whether or not this validator
            // is in this epoch's list.
            tree
        } else {
            original
        };
        let place = place_of(&committee, &tree.root());

        let mut state = match epochs.snapshot()? {
            Some(snapshot) => {
                State::from_snapshot(&snapshot, config.genesis.fee).context("the state snapshot is corrupt")?
            }
            None => State::from_genesis(&config.genesis),
        };
        let start = state.clone();
        let (mut applied, mut digests, mut held) = (Vec::new(), Vec::new(), HashMap::new());
        let mut pin = Pin::start(epoch);
        let certs = CertLog::open(&epochs.log(), |certificate| {
            let order = &certificate.order;
            let _ = state.apply(order);
            let digest = order.digest(&network);
            held.insert(digest, applied.len() as u32);
            applied.push(order.clone());
            digests.push(digest);
            pin.add(&digest);
        })?;
        let mut unfinished = HashMap::new();
        let orders = OrderLog::open(&data_dir.join("orders.log"), config.durable, |signed| {
            let order = &signed.order;
            let live = binds(&state, &network, signed);
            if live {
                state.restore_lock(order.from, order.seq, order.digest(&network));
                hold(&mut unfinished, signed.clone());
            }
            live
        })?;
        promise(&mut state, &network, &unfinished);
        let mut conflicts = HashMap::new();
        let conflict_log = OrderLog::open(&data_dir.join("conflicts.log"), config.durable, |signed| {
            let order = &signed.order;
            let live = order.seq == state.seq(&order.from)
                && unfinished.contains_key(&order.from)
                && binds(&state, &network, signed);
            if live {
                conflicts.entry(order.from).or_insert_with(|| signed.clone());
            }
            live
        })?;
        let tree_capacity = tree.capacity();
        let counter = data_dir.join(format!("leaf-{}", tree.root().short()));
        let leaves = LeafCounter::open(counter, tree_capacity, config.durable)?;

        let pace = Pace { unit: config.close_pace, clock_tolerance: config.clock_tolerance };
        let mut closer = Closer::open(&epochs.journal(), place, &committee, previous, pace, config.durable)?;
        if let Some(pinned) = closer.own_pin() {
            // It had stopped work on this epoch before the restart.
            let own_log = digests[..digests.len().min(pinned.length as usize)].to_vec();
            if !closer.restore_own_log(own_log) {
                bail!(
                    "{} no longer holds the payments this validator said it had recorded when it \
                     stopped work on epoch {epoch}",
                    data_dir.display()
                );
            }
        }
        let checkpoint_saved = epochs.checkpoint(epoch)?.is_some();
        if config.durable {
            // Make sure the log files themselves exist on disk before any
            // vote relies on them.
            epochs.sync()?;
            sync_dir(&data_dir)?;
        }

        let listener = match listener {
            Some(listener) => listener,
            None => {
                let address = config.listen.clone().or(genesis_address).expect("checked above");
                TcpListener::bind(&address).with_context(|| format!("cannot listen on {address}"))?
            }
        };
        let addr = listener.local_addr()?;

        let (commands, receiver) = mpsc::channel();
        let committee_cell = Arc::new(RwLock::new(Arc::new(committee.clone())));
        let closing = Arc::new(AtomicBool::new(closer.frozen()));
        let shared = Arc::new(Shared {
            committee: committee_cell.clone(),
            data_dir: data_dir.clone(),
            closing: closing.clone(),
            commands: commands.clone(),
            stopping: AtomicBool::new(false),
            connections: Mutex::new(HashMap::new()),
            next_connection: AtomicU64::new(0),
        });
        let mut core = Core {
            name: name.clone(),
            index: place,
            committee,
            committee_cell,
            closing,
            tree,
            leaves,
            next_key,
            building_key: false,
            data_dir,
            epochs,
            epoch_seconds: config.genesis.epoch_seconds,
            epoch_payments: config.genesis.epoch_payments,
            previous,
            previous_time,
            start,
            proofs: None,
            state,
            certs,
            applied,
            digests,
            held,
            pin,
            closer,
            pace,
            tended: Instant::now(),
            checkpoint_saved,
            peers: HashMap::new(),
            key_uses: HashMap::new(),
            cheats: Vec::new(),
            caught: Vec::new(),
            shared: shared.clone(),
            sync_interval: config.sync_interval,
            commands,
            orders,
            unfinished,
            conflicts,
            conflict_log,
            root: None,
            key_reserve: config.key_reserve.min(tree_capacity / 4),
            batch: Vec::new(),
            deadline: Instant::now(),
            issued: HashMap::new(),
            confirming: HashMap::new(),
            confirmed: HashMap::new(),
            window: config.batch_window,
            max_batch: config.max_batch.max(1),
            hurry: false,
            durable: config.durable,
            log: config.log,
        };
        core.connect_peers();
        let core = thread::Builder::new().name(format!("{name}-core")).spawn(move || core.run(receiver))?;
        let accepting = shared.clone();
        let listener = thread::Builder::new()
            .name(format!("{name}-listen"))
            .spawn(move || accept_loop(listener, accepting))?;

        Ok(Node { addr, index: index.map(|index| index as u16), name, shared, core: Some(core), listener: Some(listener) })
    }

    pub fn stop(mut self) {
        self.shutdown();
    }

    fn shutdown(&mut self) {
        if self.shared.stopping.swap(true, Ordering::SeqCst) {
            return;
        }
        let _ = self.shared.commands.send(Command::Stop);
        // The accept loop only notices the flag when a connection arrives.
        let _ = TcpStream::connect_timeout(&self.addr, Duration::from_millis(500));
        for (_, connection) in self.shared.connections.lock().unwrap().drain() {
            let _ = connection.shutdown(Shutdown::Both);
        }
        if let Some(core) = self.core.take() {
            let _ = core.join();
        }
        if let Some(listener) = self.listener.take() {
            let _ = listener.join();
        }
    }
}

impl Drop for Node {
    fn drop(&mut self) {
        self.shutdown();
    }
}
