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

use crate::client::rpc;
use crate::keys::ValidatorKey;
use crate::store::{CertLog, Epochs, LeafCounter, OrderLog, sync_dir};
use crate::wire::{AccountView, NodeInfo, Request, Response, read_frame, write_frame};
use anyhow::{Context, Result, anyhow, bail};
use orien_crypto::keytree::{KeyTree, TreeSignature};
use orien_crypto::{Hash32, merkle};
use orien_ledger::{
    Accepted, AccountId, Applied, Certificate, Committee, Confirmation, Genesis, Order, Reject,
    SignedBy, SignedOrder, State, Vote, confirmation_item,
};
use std::collections::HashMap;
use std::fs;
use std::io::{self, BufReader};
use std::net::{Shutdown, SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender, TryRecvError};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

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
    Stop,
}

struct Shared {
    committee: Committee,
    commands: Sender<Command>,
    stopping: AtomicBool,
    connections: Mutex<HashMap<u64, TcpStream>>,
    next_connection: AtomicU64,
}

fn stopped() -> Response {
    Response::Unavailable("validator is stopping".into())
}

impl Shared {
    fn ask<T>(&self, command: impl FnOnce(Sender<T>) -> Command) -> Option<T> {
        let (reply, answer) = mpsc::channel();
        self.commands.send(command(reply)).ok()?;
        answer.recv().ok()
    }

    fn handle(&self, request: Request) -> Response {
        match request {
            Request::SubmitOrder(signed) => {
                let digest = signed.order.digest(self.committee.network());
                let Some(signed_by) = signed.signature.signed_by(&digest) else {
                    return Response::Rejected(Reject::BadSignature);
                };
                self.ask(|reply| Command::Order { signed, digest, signed_by, reply })
                    .unwrap_or_else(stopped)
            }
            Request::SubmitCertificate(certificate) => {
                self.ingest(certificate, false).unwrap_or_else(stopped)
            }
            Request::Confirm(certificate) => self.ingest(certificate, true).unwrap_or_else(stopped),
            request => self.ask(|reply| Command::Query { request, reply }).unwrap_or_else(stopped),
        }
    }

    /// Verifies a certificate and applies it, signing a confirmation if
    /// `confirm` asks for one. `None` means the node is stopping.
    fn ingest(&self, certificate: Certificate, confirm: bool) -> Option<Response> {
        let (from, seq) = (certificate.order.from, certificate.order.seq);
        let digest = certificate.order.digest(self.committee.network());
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
        if let Err(error) = self.committee.verify_certificate(&certificate) {
            return Some(Response::Invalid(error.to_string()));
        }
        let Some(signed_by) = certificate.signature.signed_by(&digest) else {
            return Some(Response::Invalid("the owner's signature in this certificate is malformed".into()));
        };
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

struct Core {
    name: String,
    index: u16,
    committee: Committee,
    tree: KeyTree,
    leaves: LeafCounter,
    state: State,
    certs: CertLog,
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
                        match commands.recv() {
                            Ok(command) => command,
                            Err(_) => break,
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
                Command::Stop => break,
            }
        }
        if !self.batch.is_empty() {
            self.close_batch();
        }
    }

    fn on_order(&mut self, signed: SignedOrder, digest: Hash32, signed_by: SignedBy, reply: Sender<Response>) {
        let payment = (signed.order.from, signed.order.seq);
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
                if matches!(reject, Reject::Conflict { .. }) && !self.conflicts.contains_key(&payment.0) {
                    // Its signature is good, or the answer would have been
                    // a bad signature. Losing this record in a crash only
                    // delays what it is evidence for, so it is not synced.
                    self.conflict_log.append(&[&signed], false).expect("cannot record evidence; stopping");
                    self.conflicts.insert(payment.0, signed);
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
                validator: self.index,
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
                if confirm {
                    return self.enqueue_confirmation(payment, digest, reply);
                }
                Response::Applied { fresh: true }
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
        if let Some(kept) = self.confirmed.get(&order.from)
            && kept.seq == order.seq
        {
            let _ = reply.send(Response::Confirmed(Confirmation {
                validator: self.index,
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
            Request::CertsSince { index, max } => {
                let total = self.certs.len() as u64;
                let mut certs = Vec::new();
                let mut bytes = 0usize;
                for i in index..total.min(index.saturating_add(max as u64)) {
                    match self.certs.get(i as usize) {
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
                Response::Certs { certs, total }
            }
            Request::Pending(id) => Response::Pending(self.unfinished.get(&id).cloned()),
            Request::Root => Response::Root(*self.root.get_or_insert_with(|| self.state.root())),
            Request::Evidence(id) => Response::Evidence(
                self.unfinished.get(&id).cloned().zip(self.conflicts.get(&id).cloned()),
            ),
            Request::SubmitOrder(_) | Request::SubmitCertificate(_) | Request::Confirm(_) => {
                Response::Invalid("not a query".into())
            }
        }
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

/// Keeps this validator caught up with one peer. Every validator logs the
/// certificates it applies; reading a peer's log from where we left off
/// picks up anything we missed while down or unreachable.
fn sync_from_peer(shared: Arc<Shared>, peer: String, cursor_file: PathBuf, interval: Duration) {
    const TIMEOUT: Duration = Duration::from_secs(3);
    let mut cursor: u64 =
        fs::read_to_string(&cursor_file).ok().and_then(|text| text.trim().parse().ok()).unwrap_or(0);
    loop {
        let wake = Instant::now() + interval;
        while Instant::now() < wake {
            if shared.stopping.load(Ordering::SeqCst) {
                return;
            }
            thread::sleep(Duration::from_millis(25).min(interval));
        }
        loop {
            if shared.stopping.load(Ordering::SeqCst) {
                return;
            }
            let request = Request::CertsSince { index: cursor, max: 256 };
            let Ok(Response::Certs { certs, total }) = rpc(&peer, &request, TIMEOUT) else { break };
            if cursor > total {
                // The peer's log is shorter than what we already read: it
                // was wiped and restarted. Read it again from the top.
                cursor = 0;
                continue;
            }
            if certs.is_empty() {
                break;
            }
            for certificate in certs {
                if shared.ingest(certificate, false).is_none() {
                    return;
                }
                cursor += 1;
            }
            let _ = fs::write(&cursor_file, cursor.to_string());
            if cursor >= total {
                break;
            }
        }
    }
}

/// The layout and encoding of a validator's files. Raised whenever either
/// changes in a way an older or newer version would misread.
const DATA_FORMAT: &str = "3";

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
    pub index: u16,
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
        let committee = Committee::new(&config.genesis);
        let tree = KeyTree::generate(config.key.seed, config.key.height);
        let index = config
            .genesis
            .validators
            .iter()
            .position(|v| v.root == tree.root() && v.height == config.key.height)
            .ok_or_else(|| anyhow!("this validator key is not listed in the genesis file"))?;
        let info = &config.genesis.validators[index];
        let name = info.name.clone();

        claim_data_dir(&config.data_dir, committee.network())?;
        let epochs = Epochs::open(&config.data_dir, config.durable)?;
        let mut state = match epochs.snapshot()? {
            Some(snapshot) => State::from_snapshot(&snapshot).context("the state snapshot is corrupt")?,
            None => State::from_genesis(&config.genesis),
        };
        let certs = CertLog::open(&epochs.log(), |certificate| {
            let _ = state.apply(&certificate.order);
        })?;
        let mut unfinished = HashMap::new();
        let orders = OrderLog::open(&config.data_dir.join("orders.log"), config.durable, |signed| {
            let order = &signed.order;
            if order.seq < state.seq(&order.from) {
                return false;
            }
            state.restore_lock(order.from, order.seq, order.digest(committee.network()));
            unfinished.insert(order.from, signed.clone());
            true
        })?;
        let mut conflicts = HashMap::new();
        let conflict_log = OrderLog::open(&config.data_dir.join("conflicts.log"), config.durable, |signed| {
            let order = &signed.order;
            let live = order.seq == state.seq(&order.from) && unfinished.contains_key(&order.from);
            if live {
                conflicts.entry(order.from).or_insert_with(|| signed.clone());
            }
            live
        })?;
        let tree_capacity = tree.capacity();
        let leaves = LeafCounter::open(config.data_dir.join("leaf"), tree_capacity, config.durable)?;
        if config.durable {
            // Make sure the log files themselves exist on disk before any
            // vote relies on them.
            epochs.sync()?;
            sync_dir(&config.data_dir)?;
        }

        let listener = match listener {
            Some(listener) => listener,
            None => {
                let address = config.listen.clone().unwrap_or_else(|| info.address.clone());
                TcpListener::bind(&address).with_context(|| format!("cannot listen on {address}"))?
            }
        };
        let addr = listener.local_addr()?;

        let (commands, receiver) = mpsc::channel();
        let shared = Arc::new(Shared {
            committee: committee.clone(),
            commands,
            stopping: AtomicBool::new(false),
            connections: Mutex::new(HashMap::new()),
            next_connection: AtomicU64::new(0),
        });
        let core = Core {
            name: name.clone(),
            index: index as u16,
            committee,
            tree,
            leaves,
            state,
            certs,
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
        let core = thread::Builder::new().name(format!("{name}-core")).spawn(move || core.run(receiver))?;
        let accepting = shared.clone();
        let listener = thread::Builder::new()
            .name(format!("{name}-listen"))
            .spawn(move || accept_loop(listener, accepting))?;

        if !config.sync_interval.is_zero() {
            for (peer, validator) in config.genesis.validators.iter().enumerate() {
                if peer == index {
                    continue;
                }
                let shared = shared.clone();
                let address = validator.address.clone();
                let cursor_file = config.data_dir.join(format!("sync-{peer}.cursor"));
                let interval = config.sync_interval;
                thread::Builder::new()
                    .name(format!("{name}-sync-{peer}"))
                    .spawn(move || sync_from_peer(shared, address, cursor_file, interval))?;
            }
        }

        Ok(Node { addr, index: index as u16, name, shared, core: Some(core), listener: Some(listener) })
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
