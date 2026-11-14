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
use crate::genesis::{Committee, Member, VerifyError};
use crate::state::{Applied, Checked, State};
use crate::types::{AccountId, Certificate, Order, SignedOrder, Vote};
use borsh::{BorshDeserialize, BorshSerialize};
use orien_crypto::{Hash32, Tag, hash, keytree};
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

/// A key tree a validator will sign with from the next epoch on.
#[derive(Clone, Copy, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct NextKey {
    pub root: Hash32,
    pub height: u8,
}

/// What a validator signs when it stops work on an epoch. It makes one per
/// epoch and never votes or confirms anything in that epoch afterwards.
#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct Statement {
    pub epoch: u64,
    pub validator: u16,
    /// Every payment it had applied when it stopped.
    pub log: Pin,
    /// Pairs of orders showing that an owner signed twice with one key.
    pub evidence: Vec<(SignedOrder, SignedOrder)>,
    /// Set when the validator's key tree is running low. The new tree
    /// replaces the old one when this epoch closes.
    pub next_key: Option<NextKey>,
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
    /// The validators of the next epoch. Anyone who knows this epoch's
    /// validators and holds this checkpoint knows the next epoch's, which
    /// is how a newcomer follows the network forward from its genesis file.
    pub validators: Vec<Member>,
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
    /// The validators named for the next epoch are not the ones the merged
    /// statements lead to.
    WrongValidators,
}

/// The key tree that `validator`'s merged statement names to replace its
/// own, if it names a usable one. A key another validator already holds,
/// or a tree too tall to build, is passed over: the validator keeps the key
/// it has.
fn replacement(committee: &Committee, statements: &[&Statement], validator: u16) -> Option<NextKey> {
    let statement = statements.iter().find(|s| s.validator == validator)?;
    let key = statement.next_key?;
    let usable = key.height <= keytree::MAX_HEIGHT && committee.members().iter().all(|m| m.root != key.root);
    usable.then_some(key)
}

/// The validators of the next epoch. A fixed result of this epoch's
/// validators, the statements merged and the state the epoch ends in, so
/// every validator computes the same list.
///
/// The launch seats come first, each with the key tree its statement names
/// swapped in. After them come the accounts with the most stake, if at
/// least the minimum, largest first and ties by address, until the limit is
/// reached. A staked validator's new key tree is already in the state (see
/// `outcome`). A key tree that a validator placed earlier holds is passed
/// over, and so is its account.
///
/// The seats fade as real stake arrives. Together they weigh what their
/// stakes in the genesis file add up to, less the stake of the validators
/// chosen, shared in the genesis file's proportions. Once the real stake
/// reaches that sum the seats leave the set, and they never come back.
pub fn next_validators(committee: &Committee, statements: &[&Statement], end: &State) -> Vec<Member> {
    let rules = committee.staking();
    let mut seats = Vec::new();
    for (index, member) in committee.members().iter().enumerate() {
        if member.account.is_some() {
            continue;
        }
        let mut seat = member.clone();
        if let Some(key) = replacement(committee, statements, index as u16) {
            seat.root = key.root;
            seat.height = key.height;
        }
        seats.push(seat);
    }
    let mut stakers = end.stakers(rules.min_stake);
    stakers.sort_by(|a, b| b.2.cmp(&a.2).then(a.0.cmp(&b.0)));
    let choose = |mut members: Vec<Member>| {
        for (account, validator, stake) in &stakers {
            if members.len() >= rules.max_validators as usize {
                break;
            }
            if !validator.is_usable() || members.iter().any(|m: &Member| m.root == validator.root) {
                continue;
            }
            members.push(Member {
                root: validator.root,
                height: validator.height,
                stake: *stake,
                address: validator.address.clone(),
                account: Some(*account),
            });
        }
        members
    };
    let seat_count = seats.len();
    let with_seats = choose(seats);
    let real: u128 = with_seats[seat_count..].iter().map(|m| m.stake as u128).sum();
    let seat_total: u128 = committee.seats().iter().map(|share| *share as u128).sum();
    if seat_count == 0 || real >= seat_total {
        return choose(Vec::new());
    }
    let remaining = seat_total - real;
    let mut members = with_seats;
    for (seat, share) in members[..seat_count].iter_mut().zip(committee.seats()) {
        // Never zero while the seats remain: a seat with no weight could
        // not take part.
        seat.stake = ((remaining * *share as u128 / seat_total) as u64).max(1);
    }
    members
}

/// The payments offered for an epoch.
#[derive(Clone, Copy)]
enum Offered<'a> {
    /// With their votes and owners' signatures, all still to be checked.
    Certificates(&'a [Certificate]),
    /// Orders whose certificates the caller checked when it applied them.
    Checked(&'a [&'a Order]),
}

impl<'a> Offered<'a> {
    fn len(&self) -> usize {
        match self {
            Offered::Certificates(payments) => payments.len(),
            Offered::Checked(orders) => orders.len(),
        }
    }

    /// Each order, with its certificate where that still needs checking.
    fn orders(&self) -> Vec<(&'a Order, Option<&'a Certificate>)> {
        match self {
            Offered::Certificates(payments) => payments.iter().map(|p| (&p.order, Some(p))).collect(),
            Offered::Checked(orders) => orders.iter().map(|order| (*order, None)).collect(),
        }
    }
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
    outcome_of(committee, start, Offered::Certificates(payments), statements)
}

/// `outcome`, for a validator that holds the payments already. It checked
/// each certificate and each owner's signature when it applied them, and
/// does not check them a second time.
pub fn outcome_checked(
    committee: &Committee,
    start: &State,
    orders: &[&Order],
    statements: &[&Statement],
) -> Result<State, Invalid> {
    outcome_of(committee, start, Offered::Checked(orders), statements)
}

fn outcome_of(
    committee: &Committee,
    start: &State,
    offered: Offered,
    statements: &[&Statement],
) -> Result<State, Invalid> {
    let network = committee.network();
    let mut state = start.clone();
    // An account's payments must go in number order. Across accounts the
    // order makes no difference.
    let mut ordered = offered.orders();
    ordered.sort_by_key(|(order, _)| (order.from, order.seq));
    for (order, certificate) in ordered {
        if let Some(payment) = certificate {
            let bad = Invalid::BadPayment(order.from, order.seq);
            committee.verify_certificate(payment).map_err(|_| bad.clone())?;
            let signed_by = payment.signature.signed_by(&order.digest(network)).ok_or(bad.clone())?;
            if !order.stake_proof_holds(network) {
                return Err(bad);
            }
            if state.seq(&order.from) == order.seq && !state.authorizes(order, &signed_by) {
                return Err(bad);
            }
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
    // A staked validator that announced a new key tree signs with it from
    // the next epoch. Its stake names its key, so the change goes there.
    for statement in statements {
        let staker = committee.members().get(statement.validator as usize).and_then(|m| m.account);
        if let (Some(account), Some(key)) = (staker, replacement(committee, statements, statement.validator)) {
            state.rekey_validator(&account, key.root, key.height);
        }
    }
    state.settle_stakes(committee.epoch(), committee.staking().unstake_epochs);
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
    propose_of(committee, previous, time, start, Offered::Certificates(payments), statements)
}

/// `propose`, for a validator that checked the payments when it applied them.
pub fn propose_checked(
    committee: &Committee,
    previous: Hash32,
    time: u64,
    start: &State,
    orders: &[&Order],
    statements: &[&Statement],
) -> Result<(Checkpoint, State), Invalid> {
    propose_of(committee, previous, time, start, Offered::Checked(orders), statements)
}

fn propose_of(
    committee: &Committee,
    previous: Hash32,
    time: u64,
    start: &State,
    offered: Offered,
    statements: &[&Statement],
) -> Result<(Checkpoint, State), Invalid> {
    if !enough(committee, statements.iter().map(|s| s.validator)) {
        return Err(Invalid::TooFewStatements);
    }
    let end = outcome_of(committee, start, offered, statements)?;
    let checkpoint = Checkpoint {
        epoch: committee.epoch(),
        previous,
        time,
        state: end.root(),
        payments: offered.len() as u64,
        merged: statements.iter().map(|s| (s.validator, s.digest(committee.network()))).collect(),
        validators: next_validators(committee, statements, &end),
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
    let logs = |validator: u16| gathered.logs.get(&validator).map(Vec::as_slice);
    let statements = |validator: u16| gathered.statements.get(&validator);
    validate_of(committee, previous, start, checkpoint, &statements, &logs, Offered::Certificates(gathered.payments))
}

/// `validate`, for a validator that checked the payments when it applied
/// them. `statement` and `log` give what it has gathered from a validator:
/// the statement, and the orders in that statement's pinned log.
pub fn validate_checked<'a>(
    committee: &Committee,
    previous: &Hash32,
    start: &State,
    checkpoint: &Checkpoint,
    statement: &dyn Fn(u16) -> Option<&'a Statement>,
    log: &dyn Fn(u16) -> Option<&'a [Hash32]>,
    orders: &[&Order],
) -> Result<State, Invalid> {
    validate_of(committee, previous, start, checkpoint, statement, log, Offered::Checked(orders))
}

fn validate_of<'a>(
    committee: &Committee,
    previous: &Hash32,
    start: &State,
    checkpoint: &Checkpoint,
    statement_of: &dyn Fn(u16) -> Option<&'a Statement>,
    log_of: &dyn Fn(u16) -> Option<&'a [Hash32]>,
    offered: Offered,
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
    let held: BTreeSet<Hash32> = offered.orders().iter().map(|(order, _)| order.digest(network)).collect();
    let mut merged = Vec::with_capacity(checkpoint.merged.len());
    for (validator, digest) in &checkpoint.merged {
        let statement = statement_of(*validator)
            .filter(|s| s.validator == *validator && s.epoch == checkpoint.epoch && s.digest(network) == *digest)
            .ok_or(Invalid::UnknownStatement(*validator))?;
        // Everything this validator had applied when it stopped must be in.
        let log = log_of(*validator)
            .filter(|log| Pin::of(checkpoint.epoch, log) == statement.log)
            .ok_or(Invalid::MissingLog(*validator))?;
        if !log.iter().all(|order| held.contains(order)) {
            return Err(Invalid::MissingPayment(*validator));
        }
        merged.push(statement);
    }
    if checkpoint.payments != offered.len() as u64 {
        return Err(Invalid::WrongCount);
    }
    let end = outcome_of(committee, start, offered, &merged)?;
    if end.root() != checkpoint.state {
        return Err(Invalid::WrongState);
    }
    if checkpoint.validators != next_validators(committee, &merged, &end) {
        return Err(Invalid::WrongValidators);
    }
    Ok(end)
}

/// Something a validator signed. The signature covers a batch, and the
/// path inside `proof` ties this item to it.
#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct Signed<T> {
    pub body: T,
    pub proof: Vote,
}

/// A vote in the agreement on a checkpoint: for the checkpoint with this
/// digest, or for nothing.
#[derive(Clone, Copy, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct Ballot {
    /// False for a prevote, true for a precommit.
    pub precommit: bool,
    pub round: u32,
    pub id: Option<Hash32>,
}

impl Ballot {
    /// What a validator signs to cast it. The epoch and the network are
    /// part of every batch a validator signs, so a ballot counts only in
    /// the epoch it was cast in.
    pub fn item(&self) -> Hash32 {
        let flags = [self.precommit as u8, self.id.is_some() as u8];
        hash(Tag::Ballot, &[&flags, &self.round.to_le_bytes(), self.id.unwrap_or_default().as_bytes()])
    }
}

/// A checkpoint put forward by a round's proposer, with what the others
/// need in order to judge it.
#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct Proposal {
    pub round: u32,
    pub checkpoint: Checkpoint,
    /// Set when the checkpoint is not new: an earlier round in which more
    /// than two thirds prevoted for it.
    pub valid_round: Option<u32>,
    /// Those prevotes. The proposer's signature does not cover them: each
    /// carries its own.
    pub backers: Vec<Vote>,
    /// The statements the checkpoint was built from. Each carries its own
    /// signature too.
    pub statements: Vec<Signed<Statement>>,
}

impl Proposal {
    /// What the proposer signs.
    pub fn item(&self) -> Hash32 {
        let earlier = [self.valid_round.is_some() as u8];
        hash(
            Tag::Proposal,
            &[
                &self.round.to_le_bytes(),
                &earlier,
                &self.valid_round.unwrap_or(0).to_le_bytes(),
                self.checkpoint.digest().as_bytes(),
            ],
        )
    }
}

/// A checkpoint with the precommits that decided it, from validators
/// holding more than two thirds of the stake. Anyone who knows the epoch's
/// validators can check it.
#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct Decided {
    pub checkpoint: Checkpoint,
    pub round: u32,
    /// In validator order.
    pub precommits: Vec<Vote>,
}

/// What validators send each other to close an epoch.
#[allow(clippy::large_enum_variant)]
#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub enum Message {
    Statement(Signed<Statement>),
    Proposal(Signed<Proposal>),
    Ballot(Signed<Ballot>),
    Decided(Decided),
}

/// Whether this statement was signed by the validator it names, in the
/// committee's epoch.
pub fn verify_statement(committee: &Committee, signed: &Signed<Statement>) -> bool {
    let statement = &signed.body;
    statement.epoch == committee.epoch()
        && statement.validator == signed.proof.validator
        && committee.verify_item(&statement.digest(committee.network()), &signed.proof).is_ok()
}

pub fn verify_ballot(committee: &Committee, signed: &Signed<Ballot>) -> bool {
    committee.verify_item(&signed.body.item(), &signed.proof).is_ok()
}

/// Whether the proposer's own signature on a proposal is good. The prevotes
/// and statements it carries are checked separately.
pub fn verify_proposal(committee: &Committee, signed: &Signed<Proposal>) -> bool {
    signed.body.checkpoint.epoch == committee.epoch()
        && committee.verify_item(&signed.body.item(), &signed.proof).is_ok()
}

/// The validators who cast `ballot`, going by these signatures. `None` if
/// any signature is bad or any validator appears twice.
pub fn voters(committee: &Committee, ballot: &Ballot, signatures: &[Vote]) -> Option<Vec<u16>> {
    let item = ballot.item();
    let mut seen = BTreeSet::new();
    for signature in signatures {
        committee.verify_item(&item, signature).ok()?;
        if !seen.insert(signature.validator) {
            return None;
        }
    }
    Some(seen.into_iter().collect())
}

/// Checks that a checkpoint was decided: that validators of this epoch
/// holding more than two thirds of the stake precommitted to it.
pub fn verify_decided(committee: &Committee, decided: &Decided) -> Result<(), VerifyError> {
    let checkpoint = &decided.checkpoint;
    if checkpoint.epoch != committee.epoch() {
        return Err(VerifyError::WrongEpoch { found: checkpoint.epoch, current: committee.epoch() });
    }
    let item = Ballot { precommit: true, round: decided.round, id: Some(checkpoint.digest()) }.item();
    let mut stake = 0u64;
    let mut previous: Option<u16> = None;
    for precommit in &decided.precommits {
        if previous.is_some_and(|p| precommit.validator <= p) {
            return Err(VerifyError::VotesOutOfOrder);
        }
        previous = Some(precommit.validator);
        stake += committee.verify_item(&item, precommit)?;
    }
    if !committee.has_quorum(stake) {
        return Err(VerifyError::NoQuorum { stake, total: committee.total_stake() });
    }
    Ok(())
}
