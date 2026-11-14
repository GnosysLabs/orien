//! The client side: asking validators to vote, assembling the certificate,
//! and reading account state without trusting any single validator.

use crate::wire::{AccountView, NodeInfo, Request, Response, read_frame, write_frame};
use orien_crypto::{Hash32, ots};
use orien_ledger::{AccountId, Certificate, Committee, Genesis, Reject, SignedOrder, Vote};
use std::collections::BTreeMap;
use std::fmt;
use std::io;
use std::net::{TcpStream, ToSocketAddrs};
use std::sync::Mutex;
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

fn connect(address: &str, timeout: Duration) -> io::Result<TcpStream> {
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
fn exchange(stream: &mut TcpStream, request: &Request) -> io::Result<Response> {
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

pub struct Client {
    committee: Committee,
    endpoints: Vec<String>,
    timeout: Duration,
    /// Open connections to each validator, reused across requests. Opening
    /// one per request does not scale: closed sockets linger in the
    /// operating system and a busy client runs out of local ports.
    idle: Vec<Mutex<Vec<TcpStream>>>,
}

/// What the validators collectively say about an account.
pub struct AccountStatus {
    /// Each validator's answer, or `None` if it could not be reached.
    pub views: Vec<Option<AccountView>>,
    /// The payment number the account must use next. Raised above the
    /// caller's own record only on proof: the certificate of the payment
    /// before it.
    pub seq: u64,
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

pub struct Receipt {
    pub certificate: Certificate,
    /// Validators that voted.
    pub votes: usize,
    /// Validators that confirmed applying the certificate.
    pub acknowledged: usize,
}

#[derive(Debug)]
pub enum TransferError {
    NoQuorum { stake: u64, total: u64, refusals: Vec<(usize, String)> },
}

impl fmt::Display for TransferError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TransferError::NoQuorum { stake, total, refusals } => {
                write!(f, "not final: votes cover {stake} of {total} stake, more than two thirds are needed")?;
                for (validator, reason) in refusals {
                    write!(f, "\n  validator {validator}: {reason}")?;
                }
                Ok(())
            }
        }
    }
}

impl std::error::Error for TransferError {}

impl Client {
    pub fn new(genesis: &Genesis) -> Client {
        Client {
            committee: Committee::new(genesis),
            endpoints: genesis.validators.iter().map(|v| v.address.clone()).collect(),
            timeout: Duration::from_secs(5),
            idle: genesis.validators.iter().map(|_| Mutex::new(Vec::new())).collect(),
        }
    }

    pub fn committee(&self) -> &Committee {
        &self.committee
    }

    pub fn validators(&self) -> usize {
        self.endpoints.len()
    }

    /// Sends a request to validator `index`.
    ///
    /// Every request is safe to repeat (a validator answers a repeated order
    /// with the same vote and applies a certificate once), so a request that
    /// fails on a reused connection is simply sent again on a fresh one.
    pub fn call(&self, index: usize, request: &Request) -> io::Result<Response> {
        let reused = self.idle[index].lock().unwrap().pop();
        if let Some(mut stream) = reused
            && let Ok(response) = exchange(&mut stream, request)
        {
            self.keep(index, stream);
            return Ok(response);
        }
        // A validator whose accept queue is momentarily full drops new
        // connections. Back off briefly, with jitter, and try again.
        let mut attempt = 0u32;
        loop {
            let result = connect(&self.endpoints[index], self.timeout).and_then(|mut stream| {
                let response = exchange(&mut stream, request)?;
                self.keep(index, stream);
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

    fn keep(&self, index: usize, stream: TcpStream) {
        let mut idle = self.idle[index].lock().unwrap();
        if idle.len() < MAX_IDLE {
            idle.push(stream);
        }
    }

    /// Sends the same request to every validator at once.
    fn ask_all(&self, request: &Request) -> Vec<io::Result<Response>> {
        thread::scope(|scope| {
            let workers: Vec<_> = (0..self.endpoints.len())
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

    /// Reads an account from every validator. `known_seq` is the payment
    /// number the caller already knows the account has reached (0 if none).
    pub fn account(&self, id: &AccountId, known_seq: u64) -> AccountStatus {
        let views: Vec<Option<AccountView>> = self
            .ask_all(&Request::Account(*id))
            .into_iter()
            .map(|response| match response {
                Ok(Response::Account(Some(view))) => Some(view),
                // An account a validator has never seen is an empty one.
                Ok(Response::Account(None)) => {
                    Some(AccountView { balance: 0, seq: 0, key: *id, lock: None })
                }
                _ => None,
            })
            .collect();

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

        let mut current: Vec<(i128, u64)> = Vec::new();
        let mut locks: Vec<Hash32> = Vec::new();
        for (index, view) in views.iter().enumerate() {
            let Some(view) = view else { continue };
            if view.seq != seq {
                continue;
            }
            current.push((view.balance, self.committee.stake_of(index as u16)));
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
            if self.committee.has_quorum(stake) {
                spendable = Some(balance);
                break;
            }
        }
        AccountStatus { views, seq, spendable, locks }
    }

    fn proves_seq(&self, index: usize, id: &AccountId, claimed: u64) -> bool {
        let request = Request::GetCertificate { account: *id, seq: claimed - 1 };
        match self.call(index, &request) {
            Ok(Response::Certificate(Some(certificate))) => {
                certificate.order.from == *id
                    && certificate.order.seq == claimed - 1
                    && self.committee.verify_certificate(&certificate).is_ok()
            }
            _ => false,
        }
    }

    pub fn certificate(&self, id: &AccountId, seq: u64) -> Option<Certificate> {
        let request = Request::GetCertificate { account: *id, seq };
        (0..self.endpoints.len()).find_map(|index| match self.call(index, &request) {
            Ok(Response::Certificate(Some(certificate)))
                if certificate.order.from == *id
                    && certificate.order.seq == seq
                    && self.committee.verify_certificate(&certificate).is_ok() =>
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
        let answers = self.ask_all(&Request::Pending(*id));
        for (index, (view, answer)) in status.views.iter().zip(answers).enumerate() {
            let (Some(view), Ok(Response::Pending(Some(signed)))) = (view, answer) else { continue };
            let digest = signed.order.digest(self.committee.network());
            let genuine = signed.order.from == *id
                && signed.order.seq == status.seq
                && view.seq == status.seq
                && ots::recover(&signed.signature, digest.as_bytes()) == view.key;
            if !genuine {
                continue;
            }
            let stake = self.committee.stake_of(index as u16);
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
        self.committee.has_quorum(self.committee.total_stake().saturating_sub(elsewhere))
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
    pub fn collect_votes(&self, signed: &SignedOrder) -> (BTreeMap<u16, Vote>, Vec<(usize, String)>) {
        let digest = signed.order.digest(self.committee.network());
        let request = Request::SubmitOrder(signed.clone());
        let mut votes = BTreeMap::new();
        let mut refusals = Vec::new();
        let responses = self.ask_all_synced(&request, &signed.order.from, signed.order.seq);
        for (index, response) in responses.into_iter().enumerate() {
            match response {
                Ok(Response::Vote(vote)) => {
                    if vote.validator as usize == index
                        && self.committee.verify_vote(&digest, &vote).is_ok()
                    {
                        votes.insert(vote.validator, vote);
                    } else {
                        refusals.push((index, "returned a vote that does not verify".into()));
                    }
                }
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

    /// Hands a certificate to every validator. Returns how many applied it.
    pub fn broadcast(&self, certificate: &Certificate) -> usize {
        let request = Request::SubmitCertificate(certificate.clone());
        self.ask_all_synced(&request, &certificate.order.from, certificate.order.seq)
            .into_iter()
            .filter(|response| matches!(response, Ok(Response::Applied { .. })))
            .count()
    }

    /// Makes a payment final: gathers votes from more than two thirds of the
    /// stake, then tells every validator.
    pub fn transfer(&self, signed: &SignedOrder) -> Result<Receipt, TransferError> {
        let (votes, refusals) = self.collect_votes(signed);
        let stake: u64 = votes.keys().map(|v| self.committee.stake_of(*v)).sum();
        if !self.committee.has_quorum(stake) {
            return Err(TransferError::NoQuorum {
                stake,
                total: self.committee.total_stake(),
                refusals,
            });
        }
        // A certificate needs just enough votes. Fewer votes, smaller proof.
        let voters = votes.len();
        let mut chosen = Vec::new();
        let mut covered = 0u64;
        for (validator, vote) in votes {
            if self.committee.has_quorum(covered) {
                break;
            }
            covered += self.committee.stake_of(validator);
            chosen.push(vote);
        }
        let certificate = Certificate { order: signed.order.clone(), votes: chosen };
        let acknowledged = self.broadcast(&certificate);
        Ok(Receipt { certificate, votes: voters, acknowledged })
    }
}
