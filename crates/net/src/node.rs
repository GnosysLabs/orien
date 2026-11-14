//! A validator.
//!
//! One thread, the core, owns the ledger state and the signing key and does
//! everything that must happen in a definite order. Connection threads do
//! the expensive, order-free work (checking signatures) and hand the core a
//! command with the result.
//!
//! The core gathers the orders it accepts over a few milliseconds into a
//! batch, writes its votes to disk once, and signs the batch once. One disk
//! sync and one signature cover however many orders arrived.

use crate::client::rpc;
use crate::keys::ValidatorKey;
use crate::store::{CertLog, LeafCounter, OrderLog, sync_dir};
use crate::wire::{AccountView, NodeInfo, Request, Response, read_frame, write_frame};
use anyhow::{Context, Result, anyhow, bail};
use std::collections::HashMap;
use std::fs;
use std::io::{self, BufReader};
use std::net::{Shutdown, SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};
use token_crypto::keytree::KeyTree;
use token_crypto::{Hash32, merkle, ots};
use token_ledger::{
    Accepted, AccountId, Applied, Certificate, Committee, Genesis, SignedOrder, State, Vote,
};

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
            log: false,
        }
    }
}

// Signatures are over a kilobyte and travel inline; these values are short-lived.
#[allow(clippy::large_enum_variant)]
enum Command {
    Order { signed: SignedOrder, digest: Hash32, signed_by: Hash32, reply: Sender<Response> },
    Certificate { certificate: Certificate, reply: Sender<Response> },
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
                let signed_by = ots::recover(&signed.signature, digest.as_bytes());
                self.ask(|reply| Command::Order { signed, digest, signed_by, reply })
                    .unwrap_or_else(stopped)
            }
            Request::SubmitCertificate(certificate) => {
                self.ingest(certificate).unwrap_or_else(stopped)
            }
            request => self.ask(|reply| Command::Query { request, reply }).unwrap_or_else(stopped),
        }
    }

    /// Verifies a certificate and applies it. `None` means the node is stopping.
    fn ingest(&self, certificate: Certificate) -> Option<Response> {
        let (from, seq) = (certificate.order.from, certificate.order.seq);
        if !self.ask(|reply| Command::Needs { from, seq, reply })? {
            // That payment number is already final here. Say so only if it
            // was this same order.
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
        self.ask(|reply| Command::Certificate { certificate, reply })
    }
}

/// An order waiting for the current batch to be signed.
struct Waiting {
    payment: (AccountId, u64),
    digest: Hash32,
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
    batch: Vec<Waiting>,
    deadline: Instant,
    /// Votes given for payments that are not final yet, so a repeated
    /// request gets the same vote back.
    issued: HashMap<(AccountId, u64), Issued>,
    window: Duration,
    max_batch: usize,
    durable: bool,
    log: bool,
}

impl Core {
    fn run(mut self, commands: Receiver<Command>) {
        loop {
            let command = if self.batch.is_empty() {
                match commands.recv() {
                    Ok(command) => command,
                    Err(_) => break,
                }
            } else {
                let now = Instant::now();
                if now >= self.deadline || self.batch.len() >= self.max_batch {
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
                Command::Certificate { certificate, reply } => {
                    let _ = reply.send(self.on_certificate(certificate));
                }
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

    fn on_order(&mut self, signed: SignedOrder, digest: Hash32, signed_by: Hash32, reply: Sender<Response>) {
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
        self.batch.push(Waiting { payment, digest, replies: vec![reply], record });
    }

    /// Makes the batch's votes durable, signs them all at once, and answers
    /// everyone who is waiting. Disk first: a vote that is not on disk must
    /// never leave this process, or a crash could let the validator vote for
    /// a conflicting order afterwards.
    fn close_batch(&mut self) {
        let batch = std::mem::take(&mut self.batch);
        let records: Vec<&SignedOrder> = batch.iter().filter_map(|w| w.record.as_ref()).collect();
        self.orders.append(&records, self.durable).expect("cannot record votes; stopping");
        if self.durable {
            self.certs.sync().expect("cannot sync certificates; stopping");
        }

        let leaf = match self.leaves.take().expect("cannot reserve signing keys; stopping") {
            Some(leaf) => leaf,
            None => {
                for waiting in batch {
                    self.issued.remove(&waiting.payment);
                    for reply in waiting.replies {
                        let _ = reply.send(Response::Unavailable(
                            "this validator has used every one-time key in its key tree".into(),
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
            println!("[{}] voted    {} order(s), one signature (key {leaf})", self.name, batch.len());
        }
        for (position, waiting) in batch.into_iter().enumerate() {
            let vote = Vote {
                validator: self.index,
                batch_path: tree.path(position),
                signature: signature.clone(),
            };
            for reply in waiting.replies {
                let _ = reply.send(Response::Vote(vote.clone()));
            }
            // Keep the vote for repeat requests, unless the payment became
            // final while the batch was open.
            if self.state.seq(&waiting.payment.0) == waiting.payment.1 {
                self.issued.insert(waiting.payment, Issued::Signed(vote));
            } else {
                self.issued.remove(&waiting.payment);
            }
        }
    }

    fn on_certificate(&mut self, certificate: Certificate) -> Response {
        let order = &certificate.order;
        match self.state.apply(order) {
            Ok(Applied::New) => {
                self.certs.append(&certificate).expect("cannot store certificate; stopping");
                self.unfinished.remove(&order.from);
                if !matches!(self.issued.get(&(order.from, order.seq)), Some(Issued::Queued(_))) {
                    self.issued.remove(&(order.from, order.seq));
                }
                if self.log {
                    println!(
                        "[{}] final    {} paid {} to {} (payment #{})",
                        self.name,
                        order.from.short(),
                        order.amount,
                        order.to.short(),
                        order.seq
                    );
                }
                Response::Applied { fresh: true }
            }
            Ok(Applied::Already) => Response::Applied { fresh: false },
            Err(reject) => Response::Rejected(reject),
        }
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
                    key: account.key,
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
            Request::SubmitOrder(_) | Request::SubmitCertificate(_) => {
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
                if shared.ingest(certificate).is_none() {
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

/// A data directory belongs to one network. Refuse to reuse it for another.
fn claim_data_dir(dir: &Path, network: &Hash32) -> Result<()> {
    fs::create_dir_all(dir).with_context(|| format!("cannot create {}", dir.display()))?;
    let marker = dir.join("network");
    match fs::read_to_string(&marker) {
        Ok(existing) if existing.trim() == network.to_string() => Ok(()),
        Ok(_) => bail!(
            "{} holds data from a different network; use another data directory",
            dir.display()
        ),
        Err(_) => Ok(fs::write(&marker, network.to_string())?),
    }
}

/// Earlier versions kept votes in `votes.log`, in a form this version cannot
/// read. Starting without them would let the validator vote a second time
/// under a payment number it already voted on, so refuse.
fn refuse_old_vote_log(dir: &Path) -> Result<()> {
    let old = dir.join("votes.log");
    match fs::metadata(&old) {
        Ok(found) if found.len() > 0 => bail!(
            "{} holds votes recorded by an older version of this software, which this version \
             cannot read. Use a fresh data directory, and only if the whole network is starting over.",
            dir.display()
        ),
        Ok(_) => Ok(fs::remove_file(&old)?),
        Err(_) => Ok(()),
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
        let mut state = State::from_genesis(&config.genesis);
        let certs = CertLog::open(&config.data_dir.join("certs.log"), |certificate| {
            let _ = state.apply(&certificate.order);
        })?;
        refuse_old_vote_log(&config.data_dir)?;
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
        let leaves = LeafCounter::open(config.data_dir.join("leaf"), tree.capacity(), config.durable)?;
        if config.durable {
            // Make sure the log files themselves exist on disk before any
            // vote relies on them.
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
            batch: Vec::new(),
            deadline: Instant::now(),
            issued: HashMap::new(),
            window: config.batch_window,
            max_batch: config.max_batch.max(1),
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
