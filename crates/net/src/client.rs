//! The client side: asking validators to vote, assembling the certificate,
//! and reading account state without trusting any single validator.

use crate::store::write_replacing;
use crate::wire::{AccountView, NodeInfo, Request, Response, read_frame, write_frame};
use borsh::{BorshDeserialize, BorshSerialize};
use orien_crypto::Hash32;
use orien_ledger::close::{self, Decided, Share};
use orien_ledger::{
    Account, AccountId, Certificate, Committee, Confirmation, Genesis, Member, Receipt, Reject,
    SignedOrder, Signer, State, Vote, account_root, authorized,
};
use std::collections::{BTreeMap, HashMap};
use std::fmt;
use std::fs;
use std::io;
use std::net::{TcpStream, ToSocketAddrs};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, RwLock};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

pub(crate) fn connect(address: &str, timeout: Duration) -> io::Result<TcpStream> {
    // A name can resolve to several addresses (IPv6 and IPv4, say). Try each.
    let mut last_error = io::Error::new(io::ErrorKind::NotFound, "address did not resolve");
    let mut connected = None;
    for resolved in address.to_socket_addrs()? {
        match TcpStream::connect_timeout(&resolved, timeout) {
            Ok(stream) => {
                connected = Some(stream);
                break;
            }
            Err(error) => last_error = error,
        }
    }
    let Some(stream) = connected else { return Err(last_error) };
    let configure = || -> io::Result<()> {
        stream.set_nodelay(true)?;
        stream.set_read_timeout(Some(timeout))?;
        stream.set_write_timeout(Some(timeout))
    };
    // macOS reports "invalid argument" here when the peer has already reset
    // the connection. Report it as what it is.
    configure().map_err(|error| io::Error::new(io::ErrorKind::ConnectionReset, error))?;
    Ok(stream)
}

/// One request and its answer over an open connection.
pub(crate) fn exchange(stream: &mut TcpStream, request: &Request) -> io::Result<Response> {
    write_frame(stream, &borsh::to_vec(request)?)?;
    let frame = read_frame(stream)?
        .ok_or_else(|| io::Error::new(io::ErrorKind::UnexpectedEof, "connection closed"))?;
    borsh::from_slice(&frame)
}

/// Sends one request to one validator over a fresh connection.
pub fn rpc(address: &str, request: &Request, timeout: Duration) -> io::Result<Response> {
    exchange(&mut connect(address, timeout)?, request)
}

/// True for errors that mean the connection was cut, not that the validator
/// is down or refused the request.
fn was_dropped(error: &io::Error) -> bool {
    use io::ErrorKind::*;
    matches!(error.kind(), ConnectionReset | ConnectionAborted | BrokenPipe | UnexpectedEof)
}

/// Idle connections kept per validator.
const MAX_IDLE: usize = 64;

/// How far a client has followed the network: the current epoch, and what
/// the checkpoint before it fixed. Each step was checked against the one
/// before, back to the genesis file, so none of it rests on any one
/// validator's word.
#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct Head {
    pub network: Hash32,
    pub epoch: u64,
    /// The checkpoint that closed the epoch before, or the network id.
    pub previous: Hash32,
    /// The root of every account's state as that checkpoint left it. `None`
    /// in the first epoch, whose starting state is the genesis file.
    pub state: Option<Hash32>,
    /// When the current epoch began, in seconds since 1970.
    pub time: u64,
    /// The validators, with the first epoch each set served. A new entry
    /// is added only when the set changes.
    pub validators: Vec<(u64, Vec<Member>)>,
}

impl Head {
    /// Where a client that knows only the genesis file starts.
    pub fn genesis(genesis: &Genesis) -> Head {
        let committee = Committee::new(genesis);
        Head {
            network: *committee.network(),
            epoch: 0,
            previous: *committee.network(),
            state: None,
            time: genesis.start_time,
            validators: vec![(0, committee.members().to_vec())],
        }
    }

    /// Reads a head saved earlier, if it belongs to this network.
    pub fn load(path: &Path, genesis: &Genesis) -> Option<Head> {
        let head: Head = borsh::from_slice(&fs::read(path).ok()?).ok()?;
        (head.network == genesis.id() && !head.validators.is_empty()).then_some(head)
    }

    pub fn save(&self, path: &Path) -> io::Result<()> {
        write_replacing(path, &borsh::to_vec(self)?, false)
    }

    /// The validators that served `epoch`, if this head reaches that far.
    fn members(&self, epoch: u64) -> Option<&Vec<Member>> {
        (epoch <= self.epoch)
            .then(|| self.validators.iter().rev().find(|(from, _)| *from <= epoch).map(|(_, members)| members))
            .flatten()
    }

    /// Takes one step forward, if `decided` is the checkpoint that closes
    /// this head's epoch and `committee` is that epoch's validators.
    fn advance(&mut self, committee: &Committee, decided: &Decided) -> bool {
        let checkpoint = &decided.checkpoint;
        if checkpoint.previous != self.previous || close::verify_decided(committee, decided).is_err() {
            return false;
        }
        self.epoch += 1;
        self.previous = checkpoint.digest();
        self.state = Some(checkpoint.state);
        self.time = checkpoint.time;
        if self.validators.last().is_none_or(|(_, members)| *members != checkpoint.validators) {
            self.validators.push((self.epoch, checkpoint.validators.clone()));
        }
        true
    }
}

pub struct Client {
    /// The validators of the genesis file, for building later sets from.
    origin: Committee,
    /// The accounts as the genesis file sets them up.
    genesis_state: State,
    head: RwLock<Head>,
    /// Set when a validator answers that an epoch is closing or has closed.
    changing: AtomicBool,
    /// Where to reach the validators of the head's epoch. Replaced as the
    /// client follows the checkpoints, which name each validator's address.
    endpoints: RwLock<Vec<String>>,
    timeout: Duration,
    /// Open connections to each validator, by address, reused across
    /// requests. Opening one per request does not scale: closed sockets
    /// linger in the operating system and a busy client runs out of local
    /// ports.
    idle: Mutex<HashMap<String, Vec<TcpStream>>>,
}

/// The addresses of the validators serving the head's epoch.
fn addresses(head: &Head) -> Vec<String> {
    head.members(head.epoch).map_or_else(Vec::new, |members| members.iter().map(|m| m.address.clone()).collect())
}

/// What the validators collectively say about an account.
pub struct AccountStatus {
    /// Each validator's answer, or `None` if it could not be reached.
    pub views: Vec<Option<AccountView>>,
    /// The payment number the account must use next. Raised above the
    /// caller's own record only on proof: the account as a checkpoint left
    /// it, or the confirmed certificate of the payment before.
    pub seq: u64,
    /// The account as the latest checkpoint left it, with proof. `None` if
    /// it held nothing then.
    pub proven: Option<Account>,
    /// The key the next order must be signed with, where a checkpoint
    /// settles it. `None` when the account has paid since the checkpoint,
    /// and its next key is simply the one its last payment named.
    pub signer: Option<Signer>,
    /// The largest amount that validators with a quorum of stake all agree
    /// the account holds, or `None` while too few of them are caught up.
    pub spendable: Option<i128>,
    /// Orders validators have voted for at `seq` that are not final.
    pub locks: Vec<Hash32>,
}

/// A payment its owner signed and validators voted for, but that is not final.
#[derive(Clone, Debug)]
pub struct Unfinished {
    pub signed: SignedOrder,
    pub digest: Hash32,
    /// Stake of the validators that have voted for it.
    pub stake: u64,
}

/// How a payment became final.
// A receipt is over a kilobyte and travels inline; these values are short-lived.
#[allow(clippy::large_enum_variant)]
pub enum Done {
    /// Validators confirmed it, which gives a receipt.
    Receipt(Final),
    /// The checkpoint that closed this epoch took it in before enough
    /// validators had confirmed it.
    InCheckpoint(u64),
}

/// A payment that became final.
pub struct Final {
    /// The proof of it, holding no more votes and confirmations than needed.
    pub receipt: Receipt,
    /// Validators that voted.
    pub votes: usize,
    /// Validators that confirmed recording the certificate.
    pub confirmations: usize,
}

#[derive(Debug)]
pub enum TransferError {
    /// Too few validators voted. Nothing happened. `closing` is set when
    /// that is because the network was closing an epoch.
    NoQuorum { stake: u64, total: u64, refusals: Vec<(usize, String)>, closing: bool },
    /// The payment has its certificate, so no other payment can take its
    /// number, but too few validators have confirmed recording it. It is
    /// not final yet, and asking again later will finish it.
    NotConfirmed { stake: u64, total: u64, refusals: Vec<(usize, String)>, closing: bool },
}

impl TransferError {
    /// True when the payment failed because an epoch was closing.
    pub fn closing(&self) -> bool {
        match self {
            TransferError::NoQuorum { closing, .. } | TransferError::NotConfirmed { closing, .. } => *closing,
        }
    }
}

impl fmt::Display for TransferError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let refusals = match self {
            TransferError::NoQuorum { stake, total, refusals, .. } => {
                write!(f, "not final: votes cover {stake} of {total} stake, more than two thirds are needed")?;
                refusals
            }
            TransferError::NotConfirmed { stake, total, refusals, .. } => {
                write!(
                    f,
                    "not final yet: the payment has enough votes, but confirmations cover {stake} of \
                     {total} stake and more than two thirds are needed"
                )?;
                refusals
            }
        };
        for (validator, reason) in refusals {
            write!(f, "\n  validator {validator}: {reason}")?;
        }
        Ok(())
    }
}

impl std::error::Error for TransferError {}

impl Client {
    /// A client that knows only the genesis file. Call `follow` to bring
    /// it up to the network's current epoch.
    pub fn new(genesis: &Genesis) -> Client {
        Client::resume(genesis, Head::genesis(genesis))
    }

    /// A client that picks up from a head it saved earlier.
    pub fn resume(genesis: &Genesis, head: Head) -> Client {
        let endpoints = addresses(&head);
        Client {
            origin: Committee::new(genesis),
            genesis_state: State::from_genesis(genesis),
            head: RwLock::new(head),
            changing: AtomicBool::new(false),
            endpoints: RwLock::new(endpoints),
            timeout: Duration::from_secs(5),
            idle: Mutex::new(HashMap::new()),
        }
    }

    pub fn head(&self) -> Head {
        self.head.read().unwrap().clone()
    }

    pub fn epoch(&self) -> u64 {
        self.head.read().unwrap().epoch
    }

    /// The validators of the current epoch, as far as this client has
    /// followed the network.
    pub fn committee(&self) -> Committee {
        let head = self.head.read().unwrap();
        let members = head.members(head.epoch).expect("a head lists its own epoch's validators");
        self.origin.with(head.epoch, members.clone())
    }

    /// The validators that served `epoch`, if this client has followed the
    /// network that far.
    pub fn committee_at(&self, epoch: u64) -> Option<Committee> {
        let head = self.head.read().unwrap();
        head.members(epoch).map(|members| self.origin.with(epoch, members.clone()))
    }

    /// Catches up with the network: asks for the checkpoints after this
    /// client's epoch and checks each against the validators of the epoch
    /// it closes. Returns how many epochs it moved forward.
    pub fn follow(&self) -> u64 {
        let mut moved = 0;
        loop {
            let head = self.head();
            let answers = self.ask_all(&Request::Checkpoints { from: head.epoch, max: 64 });
            // Validators may offer different lengths. Take the longest run
            // that checks out.
            let mut best = head.clone();
            for answer in answers {
                let Ok(Response::Checkpoints(checkpoints)) = answer else { continue };
                let mut reached = head.clone();
                for decided in &checkpoints {
                    let members = reached.members(reached.epoch).expect("a head lists its own epoch").clone();
                    if !reached.advance(&self.origin.with(reached.epoch, members), decided) {
                        break;
                    }
                }
                if reached.epoch > best.epoch {
                    best = reached;
                }
            }
            if best.epoch == head.epoch {
                return moved;
            }
            moved += best.epoch - head.epoch;
            *self.endpoints.write().unwrap() = addresses(&best);
            *self.head.write().unwrap() = best;
        }
    }

    /// The account as the latest checkpoint left it, with proof. In the
    /// first epoch that is what the genesis file says.
    pub fn proven(&self, id: &AccountId) -> Option<Account> {
        let head = self.head();
        let Some(root) = head.state else { return self.genesis_state.account(id).cloned() };
        self.ask_all(&Request::Proof(*id)).into_iter().find_map(|answer| match answer {
            Ok(Response::Proof { account: Some((account, path)), .. })
                if account_root(id, &account, &path) == Some(root) =>
            {
                Some(account)
            }
            _ => None,
        })
    }

    /// The current epoch, its seed and its work target, as most of the
    /// validators that answer give them. A miner works against these.
    pub fn work(&self) -> Option<(u64, Hash32, u64)> {
        let mut answers: Vec<(u64, Hash32, u64)> = self
            .ask_all(&Request::Work)
            .into_iter()
            .filter_map(|answer| match answer {
                Ok(Response::Work { epoch, seed, target }) => Some((epoch, seed, target)),
                _ => None,
            })
            .collect();
        answers.sort();
        // The answer given most often; validators in the same epoch agree.
        let mut best: Option<((u64, Hash32, u64), usize)> = None;
        for answer in &answers {
            let count = answers.iter().filter(|other| *other == answer).count();
            if best.is_none_or(|(_, most)| count > most) {
                best = Some((*answer, count));
            }
        }
        best.map(|(answer, _)| answer)
    }

    /// Hands a work share to every validator. Returns how many took it.
    pub fn submit_share(&self, share: Share) -> usize {
        self.ask_all(&Request::SubmitShare(share))
            .into_iter()
            .filter(|answer| matches!(answer, Ok(Response::Share { .. })))
            .count()
    }

    pub fn validators(&self) -> usize {
        self.endpoints.read().unwrap().len()
    }

    /// Where the validators of the current epoch are reached, in their
    /// order in the list.
    pub fn endpoints(&self) -> Vec<String> {
        self.endpoints.read().unwrap().clone()
    }

    /// Sends a request to validator `index`.
    ///
    /// Every request is safe to repeat (a validator answers a repeated order
    /// with the same vote and applies a certificate once), so a request that
    /// fails on a reused connection is simply sent again on a fresh one.
    pub fn call(&self, index: usize, request: &Request) -> io::Result<Response> {
        let address = self.endpoints.read().unwrap().get(index).cloned();
        let Some(address) = address else { return Err(io::Error::other("the validator list changed")) };
        let reused = self.idle.lock().unwrap().get_mut(&address).and_then(Vec::pop);
        if let Some(mut stream) = reused
            && let Ok(response) = exchange(&mut stream, request)
        {
            self.keep(&address, stream);
            return Ok(response);
        }
        // A validator whose accept queue is momentarily full drops new
        // connections. Back off briefly, with jitter, and try again.
        let mut attempt = 0u32;
        loop {
            let result = connect(&address, self.timeout).and_then(|mut stream| {
                let response = exchange(&mut stream, request)?;
                self.keep(&address, stream);
                Ok(response)
            });
            match result {
                Err(error) if attempt < 5 && was_dropped(&error) => {
                    attempt += 1;
                    let base = 10u64 << attempt;
                    let jitter = SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .map_or(0, |now| now.subsec_nanos() as u64 % base);
                    thread::sleep(Duration::from_millis(base + jitter));
                }
                other => return other,
            }
        }
    }

    fn keep(&self, address: &str, stream: TcpStream) {
        let mut all = self.idle.lock().unwrap();
        let idle = all.entry(address.to_string()).or_default();
        if idle.len() < MAX_IDLE {
            idle.push(stream);
        }
    }

    /// Sends the same request to every validator at once.
    fn ask_all(&self, request: &Request) -> Vec<io::Result<Response>> {
        thread::scope(|scope| {
            let workers: Vec<_> = (0..self.validators())
                .map(|index| scope.spawn(move || self.call(index, request)))
                .collect();
            workers
                .into_iter()
                .map(|worker| worker.join().unwrap_or_else(|_| Err(io::Error::other("request failed"))))
                .collect()
        })
    }

    pub fn info(&self) -> Vec<io::Result<NodeInfo>> {
        self.ask_all(&Request::Info)
            .into_iter()
            .map(|response| match response? {
                Response::Info(info) => Ok(info),
                other => Err(io::Error::other(format!("unexpected answer: {other:?}"))),
            })
            .collect()
    }

    /// Each validator's fingerprint of all accounts, or `None` where it
    /// could not be had. Validators that have applied the same payments
    /// report the same value.
    pub fn roots(&self) -> Vec<Option<Hash32>> {
        self.ask_all(&Request::Root)
            .into_iter()
            .map(|response| match response {
                Ok(Response::Root(root)) => Some(root),
                _ => None,
            })
            .collect()
    }

    /// Each validator's view of an account, or `None` if it did not answer.
    fn views(&self, id: &AccountId) -> Vec<Option<AccountView>> {
        self.ask_all(&Request::Account(*id))
            .into_iter()
            .map(|response| match response {
                Ok(Response::Account(Some(view))) => Some(view),
                // An account a validator has never seen is an empty one.
                Ok(Response::Account(None)) => {
                    Some(AccountView {
                        balance: 0,
                        seq: 0,
                        signer: Signer::First,
                        spare: Hash32::default(),
                        lock: None,
                    })
                }
                _ => None,
            })
            .collect()
    }

    /// Reads an account from every validator. `known_seq` is the payment
    /// number the caller already knows the account has reached (0 if none).
    pub fn account(&self, id: &AccountId, known_seq: u64) -> AccountStatus {
        self.follow();
        // What the latest checkpoint settled about the account.
        let proven = self.proven(id);
        let settled = proven.as_ref().map_or(0, |account| account.seq);
        let known_seq = known_seq.max(settled);
        let mut views = self.views(id);

        // A validator claiming a higher payment number must back it with the
        // certificate of the payment before. Otherwise a lying validator
        // could trick a wallet into signing with a key meant for later.
        let mut seq = known_seq;
        let mut claims: Vec<(u64, usize)> = views
            .iter()
            .enumerate()
            .filter_map(|(index, view)| view.as_ref().map(|view| (view.seq, index)))
            .filter(|(claimed, _)| *claimed > known_seq)
            .collect();
        claims.sort_unstable_by_key(|claim| std::cmp::Reverse(*claim));
        for (claimed, index) in claims {
            if claimed <= seq {
                break;
            }
            if self.proves_seq(index, id, claimed) {
                seq = claimed;
            }
        }
        // Proving a number hands the certificate to every validator, which
        // brings lagging ones up to date. Look again, so that the balance
        // reported is the one they agree on now.
        if seq > known_seq {
            views = self.views(id);
        }

        let committee = self.committee();
        let mut current: Vec<(i128, u64)> = Vec::new();
        let mut locks: Vec<Hash32> = Vec::new();
        for (index, view) in views.iter().enumerate() {
            let Some(view) = view else { continue };
            if view.seq != seq {
                continue;
            }
            current.push((view.balance, committee.stake_of(index as u16)));
            if let Some(lock) = view.lock
                && !locks.contains(&lock)
            {
                locks.push(lock);
            }
        }
        current.sort_unstable_by_key(|entry| std::cmp::Reverse(entry.0));
        let mut stake = 0u64;
        let mut spendable = None;
        for (balance, weight) in current {
            stake += weight;
            if committee.has_quorum(stake) {
                spendable = Some(balance);
                break;
            }
        }
        // Only a checkpoint can change which kind of key an account must
        // use. If the account has not paid since, the checkpoint's word on
        // the key stands.
        let signer = match &proven {
            Some(account) if account.seq == seq => Some(account.signer.clone()),
            None if seq == 0 => Some(Signer::First),
            _ => None,
        };
        AccountStatus { views, seq, proven, signer, spendable, locks }
    }

    /// Whether validator `index` can back its claim that `id` has reached
    /// payment number `claimed`. A certificate for the payment before is not
    /// enough, since a certificate alone is not final. Validators holding
    /// more than two thirds of the stake must confirm it. They can, because
    /// it is the account's latest payment.
    fn proves_seq(&self, index: usize, id: &AccountId, claimed: u64) -> bool {
        let request = Request::GetCertificate { account: *id, seq: claimed - 1 };
        let Ok(Response::Certificate(Some(certificate))) = self.call(index, &request) else {
            return false;
        };
        certificate.order.from == *id
            && certificate.order.seq == claimed - 1
            && self.committee().verify_certificate(&certificate).is_ok()
            && self.finish(certificate, 0).is_ok()
    }

    /// A certificate from the current epoch for this payment number.
    pub fn certificate(&self, id: &AccountId, seq: u64) -> Option<Certificate> {
        let request = Request::GetCertificate { account: *id, seq };
        let committee = self.committee();
        (0..self.validators()).find_map(|index| match self.call(index, &request) {
            Ok(Response::Certificate(Some(certificate)))
                if certificate.order.from == *id
                    && certificate.order.seq == seq
                    && committee.verify_certificate(&certificate).is_ok() =>
            {
                Some(certificate)
            }
            _ => None,
        })
    }

    /// The signed orders behind the locks in `status`, most votes first.
    ///
    /// Validators keep every order they vote for until its payment is final
    /// and give it to anyone who asks. A payment that was signed but never
    /// finished can therefore be finished from anywhere: by another copy of
    /// the wallet that signed it, say.
    ///
    /// Each order returned is correctly signed with the key the validator
    /// holding it reports for the account. A wallet must still check the
    /// order against its own key before relying on it.
    pub fn unfinished(&self, id: &AccountId, status: &AccountStatus) -> Vec<Unfinished> {
        let mut found: Vec<Unfinished> = Vec::new();
        let committee = self.committee();
        let answers = self.ask_all(&Request::Pending(*id));
        for (index, (view, answer)) in status.views.iter().zip(answers).enumerate() {
            let (Some(view), Ok(Response::Pending(Some(signed)))) = (view, answer) else { continue };
            let digest = signed.order.digest(committee.network());
            let genuine = signed.order.from == *id
                && signed.order.seq == status.seq
                && view.seq == status.seq
                && signed
                    .signature
                    .signed_by(&digest)
                    .is_some_and(|by| authorized(id, &view.signer, &view.spare, &signed.order, &by));
            if !genuine {
                continue;
            }
            let stake = committee.stake_of(index as u16);
            match found.iter_mut().find(|known| known.digest == digest) {
                Some(known) => known.stake += stake,
                None => found.push(Unfinished { signed, digest, stake }),
            }
        }
        found.sort_unstable_by_key(|payment| (std::cmp::Reverse(payment.stake), payment.digest.0));
        found
    }

    /// Whether `one` could still gather a quorum, given the votes already
    /// cast for the other unfinished payments under the same number.
    pub fn can_still_finish(&self, all: &[Unfinished], one: &Unfinished) -> bool {
        let elsewhere: u64 =
            all.iter().filter(|other| other.digest != one.digest).map(|other| other.stake).sum();
        let committee = self.committee();
        committee.has_quorum(committee.total_stake().saturating_sub(elsewhere))
    }

    /// Brings validator `behind` up to date on `account`'s payments numbered
    /// `from..to`, using certificates held by the other validators.
    fn catch_up(&self, behind: usize, account: &AccountId, from: u64, to: u64) -> bool {
        if to.saturating_sub(from) > 1024 {
            return false; // Far behind: leave it to validator-to-validator sync.
        }
        for seq in from..to {
            let Some(certificate) = self.certificate(account, seq) else { return false };
            match self.call(behind, &Request::SubmitCertificate(certificate)) {
                Ok(Response::Applied { .. }) => {}
                _ => return false,
            }
        }
        true
    }

    /// Sends `request` to all validators. Any that turn out to be behind on
    /// this account are caught up and asked once more.
    fn ask_all_synced(&self, request: &Request, account: &AccountId, seq: u64) -> Vec<io::Result<Response>> {
        let mut responses = self.ask_all(request);
        for (index, response) in responses.iter_mut().enumerate() {
            if let Ok(Response::Rejected(Reject::FutureSeq { current })) = response
                && self.catch_up(index, account, *current, seq)
            {
                *response = self.call(index, request);
            }
        }
        responses
    }

    /// Collects votes for an order without finishing the payment.
    /// Notes that a validator is closing its epoch or has moved to another.
    fn pausing(&self, index: usize, refusals: &mut Vec<(usize, String)>, epoch: Option<u64>) {
        self.changing.store(true, Ordering::Relaxed);
        refusals.push((
            index,
            match epoch {
                Some(epoch) => format!("is in epoch {epoch}"),
                None => "is closing the epoch and takes no payments for a few seconds".into(),
            },
        ));
    }

    pub fn collect_votes(&self, signed: &SignedOrder) -> (BTreeMap<u16, Vote>, Vec<(usize, String)>) {
        let committee = self.committee();
        let digest = signed.order.digest(committee.network());
        let request = Request::SubmitOrder(signed.clone());
        let mut votes = BTreeMap::new();
        let mut refusals = Vec::new();
        let responses = self.ask_all_synced(&request, &signed.order.from, signed.order.seq);
        for (index, response) in responses.into_iter().enumerate() {
            match response {
                Ok(Response::Vote(vote)) => {
                    // A vote names its validator by place in this epoch's
                    // list, which need not be its place among the addresses
                    // asked. Its signature is checked against that
                    // validator's key, so it cannot be passed off.
                    if committee.verify_vote(&digest, &vote).is_ok() {
                        votes.insert(vote.validator, vote);
                    } else {
                        refusals.push((index, "returned a vote that does not verify".into()));
                    }
                }
                Ok(Response::Closing) => self.pausing(index, &mut refusals, None),
                Ok(Response::Epoch(epoch)) => self.pausing(index, &mut refusals, Some(epoch)),
                Ok(Response::Rejected(reject)) => refusals.push((index, reject.to_string())),
                Ok(Response::Invalid(reason)) | Ok(Response::Unavailable(reason)) => {
                    refusals.push((index, reason))
                }
                Ok(other) => refusals.push((index, format!("unexpected answer: {other:?}"))),
                Err(error) => refusals.push((index, format!("unreachable ({error})"))),
            }
        }
        (votes, refusals)
    }

    /// Hands a certificate to every validator and collects the
    /// confirmations they sign for it.
    pub fn confirm(&self, certificate: &Certificate) -> (BTreeMap<u16, Confirmation>, Vec<(usize, String)>) {
        let order = &certificate.order;
        let committee = self.committee();
        let digest = order.digest(committee.network());
        let request = Request::Confirm(certificate.clone());
        let mut confirmations = BTreeMap::new();
        let mut refusals = Vec::new();
        let responses = self.ask_all_synced(&request, &order.from, order.seq);
        for (index, response) in responses.into_iter().enumerate() {
            match response {
                Ok(Response::Confirmed(confirmation)) => {
                    if committee.verify_confirmation(&digest, &confirmation).is_ok() {
                        confirmations.insert(confirmation.validator, confirmation);
                    } else {
                        refusals.push((index, "returned a confirmation that does not verify".into()));
                    }
                }
                Ok(Response::Applied { .. }) => {
                    refusals.push((index, "recorded it, but no longer confirms a payment this old".into()))
                }
                Ok(Response::Closing) => self.pausing(index, &mut refusals, None),
                Ok(Response::Epoch(epoch)) => self.pausing(index, &mut refusals, Some(epoch)),
                Ok(Response::Rejected(reject)) => refusals.push((index, reject.to_string())),
                Ok(Response::Invalid(reason)) | Ok(Response::Unavailable(reason)) => {
                    refusals.push((index, reason))
                }
                Ok(other) => refusals.push((index, format!("unexpected answer: {other:?}"))),
                Err(error) => refusals.push((index, format!("unreachable ({error})"))),
            }
        }
        (confirmations, refusals)
    }

    /// The smallest set of signatures, in validator order, whose stake is
    /// more than two thirds. Fewer signatures, smaller proof.
    fn just_enough(&self, signatures: BTreeMap<u16, Vote>) -> Vec<Vote> {
        let committee = self.committee();
        let mut chosen = Vec::new();
        let mut covered = 0u64;
        for (validator, signature) in signatures {
            if committee.has_quorum(covered) {
                break;
            }
            covered += committee.stake_of(validator);
            chosen.push(signature);
        }
        chosen
    }

    fn stake(&self, signatures: &BTreeMap<u16, Vote>) -> u64 {
        let committee = self.committee();
        signatures.keys().map(|validator| committee.stake_of(*validator)).sum()
    }

    /// Gathers votes from more than two thirds of the stake into a
    /// certificate. Returns it with the number of validators that voted.
    fn certify(&self, signed: &SignedOrder) -> Result<(Certificate, usize), TransferError> {
        let committee = self.committee();
        let (votes, refusals) = self.collect_votes(signed);
        let stake = self.stake(&votes);
        if !committee.has_quorum(stake) {
            // Validators that have already applied this payment will not
            // vote for it again. If its certificate is out there, all the
            // payment lacks is confirmations.
            let order = &signed.order;
            if let Some(existing) = self.certificate(&order.from, order.seq)
                && existing.order == *order
            {
                return Ok((existing, votes.len()));
            }
            let closing = self.changing.load(Ordering::Relaxed);
            return Err(TransferError::NoQuorum { stake, total: committee.total_stake(), refusals, closing });
        }
        let voters = votes.len();
        let certificate = Certificate::new(committee.epoch(), signed, self.just_enough(votes));
        Ok((certificate, voters))
    }

    /// Takes a certificate the rest of the way: gathers confirmations from
    /// more than two thirds of the stake.
    pub fn finish(&self, certificate: Certificate, votes: usize) -> Result<Final, TransferError> {
        let (confirmations, refusals) = self.confirm(&certificate);
        let stake = self.stake(&confirmations);
        let committee = self.committee();
        if !committee.has_quorum(stake) {
            return Err(TransferError::NotConfirmed {
                stake,
                total: committee.total_stake(),
                refusals,
                closing: self.changing.load(Ordering::Relaxed),
            });
        }
        let confirmed = confirmations.len();
        let receipt = Receipt { certificate, confirmations: self.just_enough(confirmations) };
        Ok(Final { receipt, votes, confirmations: confirmed })
    }

    /// Makes a payment final. First validators holding more than two thirds
    /// of the stake vote for the order, which gives it a certificate. Then
    /// as many confirm that they have recorded the certificate.
    pub fn transfer(&self, signed: &SignedOrder) -> Result<Final, TransferError> {
        self.changing.store(false, Ordering::Relaxed);
        let (certificate, votes) = self.certify(signed)?;
        self.finish(certificate, votes)
    }

    /// Makes a payment final, waiting out a checkpoint if one gets in the
    /// way. Payments pause for a few seconds while an epoch closes; this
    /// keeps trying for up to `patience`.
    ///
    /// The signed order is the same throughout. Votes do not carry over
    /// from one epoch to the next, but the validators' promises do, so the
    /// order simply gathers its votes again.
    pub fn pay(&self, signed: &SignedOrder, patience: Duration) -> Result<Done, TransferError> {
        let deadline = Instant::now() + patience;
        let order = &signed.order;
        loop {
            let epoch = self.epoch();
            self.changing.store(false, Ordering::Relaxed);
            let (certified, error) = match self.certify(signed) {
                Ok((certificate, votes)) => match self.finish(certificate, votes) {
                    Ok(done) => return Ok(Done::Receipt(done)),
                    Err(error) => (true, error),
                },
                Err(error) => (false, error),
            };
            let paused = self.changing.load(Ordering::Relaxed);
            // Wait for the checkpoint, if one is being made.
            while self.follow() == 0 && self.epoch() == epoch && paused && Instant::now() < deadline {
                thread::sleep(Duration::from_millis(100));
            }
            if self.epoch() == epoch || Instant::now() >= deadline {
                return Err(error);
            }
            // The payment had its certificate in the epoch that just
            // closed. If the checkpoint moved the account past this payment
            // number, this is the payment it took in: no other order under
            // the number could have had a certificate in the same epoch.
            if certified
                && self.epoch() == epoch + 1
                && self.proven(&order.from).is_some_and(|account| account.seq > order.seq)
            {
                return Ok(Done::InCheckpoint(epoch));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use orien_crypto::keytree::KeyTree;
    use orien_crypto::merkle;
    use orien_ledger::ValidatorInfo;
    use orien_ledger::close::{Ballot, Checkpoint};

    #[test]
    fn a_head_moves_forward_only_on_a_checkpoint_that_checks_out() {
        let trees: Vec<KeyTree> = (0..4).map(|v| KeyTree::generate([70 + v as u8; 32], 4)).collect();
        let validators = trees
            .iter()
            .map(|tree| ValidatorInfo { name: String::new(), root: tree.root(), height: 4, stake: 1, address: String::new() })
            .collect();
        let genesis = Genesis::new("head-test", validators, Vec::new());
        let origin = Committee::new(&genesis);
        let network = *origin.network();
        // Signs a checkpoint with these validators' keys, as its precommits.
        let decide = |committee: &Committee, leaf: u32, checkpoint: &Checkpoint, signers: &[usize]| {
            let item = Ballot { precommit: true, round: 0, id: Some(checkpoint.digest()) }.item();
            let batch = merkle::Tree::build(&[item]);
            let message = committee.batch_message(&batch.root());
            let precommits = signers
                .iter()
                .map(|v| Vote {
                    validator: *v as u16,
                    batch_path: batch.path(0),
                    signature: trees[*v].sign_with_leaf(leaf, message.as_bytes()),
                })
                .collect();
            Decided { checkpoint: checkpoint.clone(), round: 0, precommits }
        };
        let mut rekeyed = origin.members().to_vec();
        rekeyed[3].root = Hash32([9; 32]);
        let checkpoint = Checkpoint {
            epoch: 0,
            previous: network,
            time: 50,
            state: Hash32([3; 32]),
            payments: 0,
            merged: Vec::new(),
            validators: rekeyed.clone(),
            released: 0,
            target: u64::MAX,
        };

        let start = Head::genesis(&genesis);
        let refused = |decided: &Decided| !start.clone().advance(&origin, decided);
        assert!(refused(&decide(&origin, 0, &checkpoint, &[0, 1])), "two of four validators are not enough");
        let elsewhere = Checkpoint { previous: Hash32([1; 32]), ..checkpoint.clone() };
        assert!(refused(&decide(&origin, 1, &elsewhere, &[0, 1, 2])), "it does not follow what this head knows");
        let later = Checkpoint { epoch: 1, ..checkpoint.clone() };
        assert!(refused(&decide(&origin, 2, &later, &[0, 1, 2])), "it closes a different epoch");
        let mut tampered = decide(&origin, 3, &checkpoint, &[0, 1, 2]);
        tampered.checkpoint.state = Hash32([4; 32]);
        assert!(refused(&tampered), "it was changed after it was signed");

        let mut head = start.clone();
        assert!(head.advance(&origin, &decide(&origin, 4, &checkpoint, &[0, 1, 2])));
        assert_eq!((head.epoch, head.previous, head.state, head.time), (1, checkpoint.digest(), Some(Hash32([3; 32])), 50));
        // It remembers who the validators were in each epoch.
        assert_eq!(head.members(0), Some(&origin.members().to_vec()));
        assert_eq!(head.members(1), Some(&rekeyed));
        assert_eq!(head.members(2), None);
        // The same checkpoint cannot be used for a second step.
        assert!(!head.clone().advance(&origin, &decide(&origin, 5, &checkpoint, &[0, 1, 2])));

        // The next step is signed by the next epoch's validators. Validator
        // 4 changed its key, so the other three sign.
        let next = origin.with(1, rekeyed.clone());
        let second = Checkpoint { epoch: 1, previous: checkpoint.digest(), time: 60, ..checkpoint.clone() };
        assert!(!head.clone().advance(&origin, &decide(&origin, 6, &second, &[0, 1, 2])), "signed for the wrong epoch");
        assert!(head.advance(&next, &decide(&next, 7, &second, &[0, 1, 2])));
        assert_eq!(head.epoch, 2);
        assert_eq!(head.validators.len(), 2, "the validators did not change again");
    }
}
