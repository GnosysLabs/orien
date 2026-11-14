//! Closing an epoch: what a checkpoint is, and when one is acceptable.
//!
//! Validators apply payments in whatever order they hear of them, and when
//! an epoch ends each has applied a slightly different set. A checkpoint
//! settles which payments the epoch holds, for everyone.
//!
//! Each validator first stops and signs a statement that pins what it had
//! applied. A proposer gathers statements from validators holding more than
//! two thirds of the stake and builds a checkpoint from them. The epoch then
//! holds every payment in any of those validators' pinned logs. That is
//! what keeps a final payment from being lost: a final payment was
//! confirmed by more than two thirds, any two such groups share an honest
//! validator, and that validator's pinned log contains the payment.
//!
//! Whether a checkpoint is acceptable is a test that gives every validator
//! the same answer, because it depends only on the checkpoint and on signed
//! data anyone can fetch. A validator may not reject one because of
//! something only it knows. If validators could, two of them might lock on
//! a checkpoint a third will never accept, and the network would stop.
//!
//! Unfinished payments are not part of the agreement at all. A validator
//! keeps its own promises across the checkpoint. The one exception is an
//! account whose owner signed twice: the statements carry the two signed
//! orders, and the checkpoint cancels that number and moves the account to
//! a spare key.

use crate::agree::Value;
use crate::genesis::Committee;
use crate::state::{Applied, Checked, State};
use crate::types::{AccountId, Certificate, SignedOrder};
use borsh::{BorshDeserialize, BorshSerialize};
use orien_crypto::{Hash32, Tag, hash};
use std::collections::{BTreeMap, BTreeSet};

/// A commitment to the payments a validator has applied in an epoch, in the
/// order it applied them. Once signed, the list cannot be changed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct Pin {
    pub length: u64,
    pub hash: Hash32,
}

impl Pin {
    /// The pin of an empty log.
    pub fn start(epoch: u64) -> Pin {
        Pin { length: 0, hash: hash(Tag::Pin, &[&epoch.to_le_bytes()]) }
    }

    /// Extends the pin by one applied order.
    pub fn add(&mut self, order_digest: &Hash32) {
        self.hash = hash(Tag::Pin, &[self.hash.as_bytes(), order_digest.as_bytes()]);
        self.length += 1;
    }

    pub fn of(epoch: u64, order_digests: &[Hash32]) -> Pin {
        let mut pin = Pin::start(epoch);
        for digest in order_digests {
            pin.add(digest);
        }
        pin
    }
}

/// What a validator signs when it stops work on an epoch. It makes one per
/// epoch and never applies or confirms anything in that epoch afterwards.
#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct Statement {
    pub epoch: u64,
    pub validator: u16,
    /// Every payment it had applied when it stopped.
    pub log: Pin,
    /// Pairs of orders showing that an owner signed twice with one key.
    pub evidence: Vec<(SignedOrder, SignedOrder)>,
}

impl Statement {
    pub fn digest(&self, network: &Hash32) -> Hash32 {
        let bytes = borsh::to_vec(self).expect("writing to memory cannot fail");
        hash(Tag::Statement, &[network.as_bytes(), &bytes])
    }
}

/// What validators agree on to close an epoch.
///
/// It is small. It does not list the payments: validators hold those
/// already, and the state root pins the result of applying them.
#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct Checkpoint {
    pub epoch: u64,
    /// The checkpoint before this one, or the network id for the first.
    /// Checkpoints form a chain back to the genesis file.
    pub previous: Hash32,
    /// Seconds since 1970 by the proposer's clock.
    pub time: u64,
    /// The root of every account's state once the epoch is closed.
    pub state: Hash32,
    /// How many payments the epoch holds.
    pub payments: u64,
    /// The statements it was built from, by validator, in validator order.
    pub merged: Vec<(u16, Hash32)>,
}

impl Checkpoint {
    pub fn digest(&self) -> Hash32 {
        let bytes = borsh::to_vec(self).expect("writing to memory cannot fail");
        hash(Tag::Checkpoint, &[&bytes])
    }
}

impl Value for Checkpoint {
    fn id(&self) -> Hash32 {
        self.digest()
    }
}

/// Why something offered as a checkpoint is not one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Invalid {
    WrongEpoch,
    /// It does not follow the checkpoint this validator holds as the latest.
    WrongPrevious,
    /// The statements named do not cover more than two thirds of the stake,
    /// or are repeated or out of order.
    TooFewStatements,
    /// A statement named is not one this validator has from that validator.
    UnknownStatement(u16),
    /// The pinned log of a validator whose statement is named is not at
    /// hand, or is not the log its statement pins.
    MissingLog(u16),
    /// A payment in a named validator's pinned log is left out.
    MissingPayment(u16),
    /// A payment's votes or owner's signature do not check out.
    BadPayment(AccountId, u64),
    /// The payments skip one of an account's numbers, or use one twice.
    OutOfSequence(AccountId, u64),
    /// An account would end the epoch having paid out more than it holds.
    Overdrawn(AccountId),
    WrongCount,
    WrongState,
}

/// The state an epoch ends in. A fixed result of the state it began in, the
/// payments it holds and the statements it was built from, so that every
/// validator computes the same one.
///
/// Payments are applied first. Then each pair of orders offered as evidence
/// is tried, statement by statement. A pair that is real evidence against
/// the account as it now stands cancels the account's current number. A
/// pair that is not, because one of its orders became final or the account
/// was already cancelled, changes nothing.
pub fn outcome(
    committee: &Committee,
    start: &State,
    payments: &[Certificate],
    statements: &[&Statement],
) -> Result<State, Invalid> {
    let network = committee.network();
    let mut state = start.clone();
    // An account's payments must go in number order. Across accounts the
    // order makes no difference.
    let mut ordered: Vec<&Certificate> = payments.iter().collect();
    ordered.sort_by_key(|payment| (payment.order.from, payment.order.seq));
    for payment in ordered {
        let order = &payment.order;
        let bad = Invalid::BadPayment(order.from, order.seq);
        committee.verify_certificate(payment).map_err(|_| bad.clone())?;
        let signed_by = payment.signature.signed_by(&order.digest(network)).ok_or(bad.clone())?;
        if state.seq(&order.from) == order.seq && !state.authorizes(order, &signed_by) {
            return Err(bad);
        }
        match state.apply(order) {
            Ok(Applied::New) => {}
            _ => return Err(Invalid::OutOfSequence(order.from, order.seq)),
        }
    }
    if let Some(account) = state.overdrawn() {
        return Err(Invalid::Overdrawn(account));
    }
    for statement in statements {
        for (one, other) in &statement.evidence {
            let checked = |signed: &'_ SignedOrder| {
                let digest = signed.order.digest(network);
                signed.signature.signed_by(&digest).map(|by| (digest, by))
            };
            if let (Some((one_digest, one_by)), Some((other_digest, other_by))) = (checked(one), checked(other)) {
                let one = Checked { order: &one.order, digest: one_digest, by: one_by };
                let other = Checked { order: &other.order, digest: other_digest, by: other_by };
                // Stale or false evidence is simply not acted on.
                let _ = state.cancel(&one, &other);
            }
        }
    }
    Ok(state)
}

/// Checks that the statements named cover more than two thirds of the
/// stake, each validator once and in order.
fn enough(committee: &Committee, validators: impl Iterator<Item = u16>) -> bool {
    let mut stake = 0u64;
    let mut previous: Option<u16> = None;
    for validator in validators {
        if previous.is_some_and(|p| validator <= p) || committee.stake_of(validator) == 0 {
            return false;
        }
        previous = Some(validator);
        stake += committee.stake_of(validator);
    }
    committee.has_quorum(stake)
}

/// Builds the checkpoint a proposer puts forward, and the state it leads
/// to. `payments` is everything the proposer has applied this epoch, which
/// must take in the pinned log of every statement it names.
pub fn propose(
    committee: &Committee,
    previous: Hash32,
    time: u64,
    start: &State,
    payments: &[Certificate],
    statements: &[&Statement],
) -> Result<(Checkpoint, State), Invalid> {
    if !enough(committee, statements.iter().map(|s| s.validator)) {
        return Err(Invalid::TooFewStatements);
    }
    let end = outcome(committee, start, payments, statements)?;
    let checkpoint = Checkpoint {
        epoch: committee.epoch(),
        previous,
        time,
        state: end.root(),
        payments: payments.len() as u64,
        merged: statements.iter().map(|s| (s.validator, s.digest(committee.network()))).collect(),
    };
    Ok((checkpoint, end))
}

/// What a validator has gathered to judge a proposed checkpoint with. All
/// of it is signed or pinned by someone, and any validator can fetch it.
pub struct Gathered<'a> {
    /// Statements received, by validator, signatures already checked.
    pub statements: &'a BTreeMap<u16, Statement>,
    /// The orders in each validator's pinned log, as digests, in its order.
    pub logs: &'a BTreeMap<u16, Vec<Hash32>>,
    /// The payments the proposer says the epoch holds.
    pub payments: &'a [Certificate],
}

/// Judges a proposed checkpoint. Returns the state it leads to.
///
/// Every validator given the same checkpoint and the same gathered data
/// reaches the same verdict. The checkpoint's time is not judged here: that
/// is the one test that depends on the validator's own clock, and it is
/// asked only of a checkpoint proposed for the first time.
pub fn validate(
    committee: &Committee,
    previous: &Hash32,
    start: &State,
    checkpoint: &Checkpoint,
    gathered: &Gathered,
) -> Result<State, Invalid> {
    let network = committee.network();
    if checkpoint.epoch != committee.epoch() {
        return Err(Invalid::WrongEpoch);
    }
    if checkpoint.previous != *previous {
        return Err(Invalid::WrongPrevious);
    }
    if !enough(committee, checkpoint.merged.iter().map(|(validator, _)| *validator)) {
        return Err(Invalid::TooFewStatements);
    }
    let offered: BTreeSet<Hash32> = gathered.payments.iter().map(|p| p.order.digest(network)).collect();
    let mut merged = Vec::with_capacity(checkpoint.merged.len());
    for (validator, digest) in &checkpoint.merged {
        let statement = gathered
            .statements
            .get(validator)
            .filter(|s| s.validator == *validator && s.epoch == checkpoint.epoch && s.digest(network) == *digest)
            .ok_or(Invalid::UnknownStatement(*validator))?;
        // Everything this validator had applied when it stopped must be in.
        let log = gathered
            .logs
            .get(validator)
            .filter(|log| Pin::of(checkpoint.epoch, log) == statement.log)
            .ok_or(Invalid::MissingLog(*validator))?;
        if !log.iter().all(|order| offered.contains(order)) {
            return Err(Invalid::MissingPayment(*validator));
        }
        merged.push(statement);
    }
    if checkpoint.payments != gathered.payments.len() as u64 {
        return Err(Invalid::WrongCount);
    }
    let end = outcome(committee, start, gathered.payments, &merged)?;
    if end.root() != checkpoint.state {
        return Err(Invalid::WrongState);
    }
    Ok(end)
}
