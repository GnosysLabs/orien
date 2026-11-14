//! Closing an epoch, as one validator does it.
//!
//! The rules are in `orien_ledger::close` and `orien_ledger::agree`, and
//! neither does any input or output. This is what joins them to a running
//! validator: it signs what the validator says, checks what it hears,
//! gathers the other validators' pinned logs, judges proposed checkpoints
//! and keeps time.
//!
//! Everything it hears and says goes into a journal first. The one thing a
//! validator must never do is say two different things in the same round,
//! and a validator that forgot what it said could. So the journal is forced
//! to disk before anything this validator signs leaves it, and after a
//! restart the journal is played back, which puts the validator exactly
//! where it was.
//!
//! It touches the rest of the validator only through `Env`, which is how the
//! tests run several of these against each other with no network.

use crate::store::Journal;
use borsh::{BorshDeserialize, BorshSerialize};
use orien_crypto::Hash32;
use orien_ledger::agree::{self, Agreement, Output, Timeout, Verdict};
use orien_ledger::close::{self, Ballot, Checkpoint, Decided, Invalid, Message, Pin, Proposal, Signed, Statement};
use orien_ledger::{Committee, Order, State, Vote};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::io;
use std::path::Path;
use std::time::{Duration, Instant};

/// How long a validator waits for things while closing.
#[derive(Clone, Copy, Debug)]
pub struct Pace {
    /// Every wait is a multiple of this. Later rounds wait longer.
    pub unit: Duration,
    /// How many seconds a proposed checkpoint's time may differ from this
    /// validator's clock.
    pub clock_tolerance: u64,
}

/// What the closer needs from the validator around it.
pub struct Env<'a> {
    /// The validators of the epoch being closed.
    pub committee: &'a Committee,
    /// The state the epoch started from.
    pub start: &'a State,
    /// The checkpoint before, or the network id for the first epoch.
    pub previous: Hash32,
    /// The time of that checkpoint, or of the network's start.
    pub previous_time: u64,
    pub now: Instant,
    /// This validator's clock, in seconds since 1970.
    pub unix: u64,
    /// Signs these items as one batch, using one one-time key. `None` when
    /// the validator has no key left.
    pub sign: &'a mut dyn FnMut(&[Hash32]) -> Option<Vec<Vote>>,
    /// The order behind a payment this validator has applied in this epoch.
    pub order: &'a dyn Fn(&Hash32) -> Option<&'a Order>,
}

/// A pinned log this validator still has to fetch.
#[derive(Clone, Debug)]
pub struct Wanted {
    pub statement: Hash32,
    /// The validator whose log it is.
    pub validator: u16,
    pub pin: Pin,
    /// How long it has been wanted.
    pub age: Duration,
}

#[derive(BorshSerialize, BorshDeserialize)]
enum Event {
    /// A statement, ballot or decision from another validator.
    Heard(Message),
    /// A proposal, and what judging it found.
    Judged { proposal: Signed<Proposal>, valid: bool, timely: bool },
    /// The first round began.
    Started,
    /// The checkpoint this validator put forward when its turn came.
    Proposed { round: u32, checkpoint: Checkpoint },
    Timeout { step: u8, round: u32 },
    /// Something this validator signed.
    Sent(Message),
}

fn timeout_parts(timeout: Timeout) -> (u8, u32) {
    match timeout {
        Timeout::Propose(round) => (0, round),
        Timeout::Prevote(round) => (1, round),
        Timeout::Precommit(round) => (2, round),
    }
}

fn timeout_from(step: u8, round: u32) -> Timeout {
    match step {
        0 => Timeout::Propose(round),
        1 => Timeout::Prevote(round),
        _ => Timeout::Precommit(round),
    }
}

/// Something the agreement has decided this validator should say, not yet
/// signed.
enum Owed {
    Ballot(Ballot),
    Proposal { round: u32, checkpoint: Checkpoint, valid_round: Option<u32>, backers: Vec<u16> },
}

pub struct Closer {
    me: u16,
    /// Set when this validator is not one of the epoch's validators. It
    /// says nothing and listens only for the decided checkpoint.
    watching: bool,
    epoch: u64,
    network: Hash32,
    weights: Vec<u64>,
    total: u64,
    pace: Pace,
    journal: Journal,
    /// True while the journal is being played back after a restart. Nothing
    /// is signed, sent or written then.
    replaying: bool,
    agreement: Agreement<Checkpoint>,
    started: bool,
    /// Every statement seen, by digest.
    statements: HashMap<Hash32, Signed<Statement>>,
    /// The first statement seen from each validator.
    first: BTreeMap<u16, Hash32>,
    /// This validator's own statement. Once it exists the validator is
    /// frozen: it votes on no payment and confirms none in this epoch.
    mine: Option<Hash32>,
    /// Pinned logs fetched so far, by statement. Each matches its pin.
    logs: HashMap<Hash32, Vec<Hash32>>,
    /// Statements whose whole pinned log this validator holds as payments.
    complete: HashSet<Hash32>,
    asked: HashMap<Hash32, Instant>,
    /// The first ballot from each validator, per step and round.
    ballots: HashMap<(bool, u32, u16), Signed<Ballot>>,
    /// Rounds for which a proposal has been taken. Only the first counts.
    seen: HashSet<u32>,
    /// Proposals that cannot be judged until more logs are gathered, each
    /// with the validators whose prevotes came with it.
    waiting: Vec<(Signed<Proposal>, Vec<u16>)>,
    /// Checkpoints judged so far, and whether they follow the rules.
    valid: HashMap<Hash32, bool>,
    /// The state each valid checkpoint leads to.
    ends: HashMap<Hash32, State>,
    timers: Vec<(Instant, Timeout)>,
    /// The round this validator owes a proposal for, if any.
    need_value: Option<u32>,
    /// Since when enough logs have been at hand to propose.
    ready_since: Option<Instant>,
    /// The agreement's result, until the proof of it is put together.
    outcome: Option<(u32, Checkpoint, Vec<u16>)>,
    decided: Option<Decided>,
    announced: bool,
    /// A decided checkpoint failed this validator's own check. That takes
    /// more than a third of the stake acting dishonestly.
    unsound: bool,
    /// Every message accepted, in order. Other validators read this, which
    /// is how a message shown to one validator reaches the rest.
    outbox: Vec<Message>,
    /// Left over from playing back the journal: things this validator had
    /// decided to say whose signed form never reached the journal, and
    /// waits that had not run out.
    owed: Vec<Owed>,
    armed: Vec<Timeout>,
}

impl Closer {
    /// Opens the closer for the committee's epoch, playing back whatever
    /// the journal at `path` holds. `seed` decides whose turn it is to
    /// propose.
    /// `me` is this validator's place among the epoch's validators, or
    /// `None` if it is not one of them.
    pub fn open(
        path: &Path,
        me: Option<u16>,
        committee: &Committee,
        seed: Hash32,
        pace: Pace,
        durable: bool,
    ) -> io::Result<Closer> {
        let (journal, records) = Journal::open(path, durable)?;
        let weights: Vec<u64> = committee.members().iter().map(|member| member.stake).collect();
        let watching = me.is_none();
        let me = me.unwrap_or(u16::MAX);
        let mut closer = Closer {
            me,
            watching,
            epoch: committee.epoch(),
            network: *committee.network(),
            total: weights.iter().sum(),
            agreement: Agreement::new(me, weights.clone(), seed),
            weights,
            pace,
            journal,
            replaying: true,
            started: false,
            statements: HashMap::new(),
            first: BTreeMap::new(),
            mine: None,
            logs: HashMap::new(),
            complete: HashSet::new(),
            asked: HashMap::new(),
            ballots: HashMap::new(),
            seen: HashSet::new(),
            waiting: Vec::new(),
            valid: HashMap::new(),
            ends: HashMap::new(),
            timers: Vec::new(),
            need_value: None,
            ready_since: None,
            outcome: None,
            decided: None,
            announced: false,
            unsound: false,
            outbox: Vec::new(),
            owed: Vec::new(),
            armed: Vec::new(),
        };
        for record in records {
            let Ok(event) = borsh::from_slice::<Event>(&record) else { break };
            closer.replay(event);
        }
        closer.replaying = false;
        Ok(closer)
    }

    pub fn epoch(&self) -> u64 {
        self.epoch
    }

    /// True once this validator has made its closing statement.
    pub fn frozen(&self) -> bool {
        self.mine.is_some()
    }

    /// The log this validator pinned when it froze.
    pub fn own_pin(&self) -> Option<Pin> {
        self.mine.and_then(|digest| self.statements.get(&digest)).map(|signed| signed.body.log)
    }

    /// The stake of the validators a closing statement has been seen from.
    pub fn statement_stake(&self) -> u64 {
        self.first.keys().map(|validator| self.weights[*validator as usize]).sum()
    }

    pub fn total_stake(&self) -> u64 {
        self.total
    }

    pub fn decided(&self) -> Option<&Decided> {
        self.decided.as_ref()
    }

    /// The state the decided checkpoint leads to, once this validator has
    /// worked it out. Hands it over once.
    pub fn take_end(&mut self) -> Option<State> {
        let digest = self.decided.as_ref()?.checkpoint.digest();
        self.ends.remove(&digest)
    }

    /// Messages accepted so far, from position `from`.
    pub fn messages(&self, from: usize, max: usize) -> (&[Message], usize) {
        let end = self.outbox.len().min(from.saturating_add(max));
        (self.outbox.get(from..end).unwrap_or(&[]), self.outbox.len())
    }

    /// Part of a pinned log this validator has fetched, or pinned itself.
    pub fn log(&self, statement: &Hash32, from: usize, max: usize) -> Option<&[Hash32]> {
        let log = self.logs.get(statement)?;
        log.get(from.min(log.len())..log.len().min(from.saturating_add(max)))
    }

    /// When the next wait runs out.
    pub fn next_wake(&self) -> Option<Instant> {
        self.timers.iter().map(|(at, _)| *at).min()
    }

    /// True while there is closing work that cannot wait for a timer.
    pub fn busy(&self) -> bool {
        self.frozen() && self.decided.is_none()
    }

    fn quorum(&self, stake: u64) -> bool {
        3 * stake as u128 > 2 * self.total as u128
    }

    fn weight_of<'a>(&self, validators: impl Iterator<Item = &'a u16>) -> u64 {
        validators.filter_map(|validator| self.weights.get(*validator as usize)).sum()
    }

    fn wait(&self, timeout: Timeout) -> Duration {
        match timeout {
            // Long enough for the proposer to gather the pinned logs.
            Timeout::Propose(round) => self.pace.unit * (6 + 2 * round.min(30)),
            Timeout::Prevote(round) | Timeout::Precommit(round) => self.pace.unit * (1 + round.min(30)),
        }
    }

    fn record(&mut self, event: &Event) {
        if !self.replaying {
            let bytes = borsh::to_vec(event).expect("writing to memory cannot fail");
            self.journal.append(&bytes).expect("cannot record a closing message; stopping");
        }
    }

    fn take_statement(&mut self, digest: Hash32, signed: Signed<Statement>) {
        self.first.entry(signed.body.validator).or_insert(digest);
        // An empty log needs no fetching.
        if signed.body.log == Pin::start(self.epoch) {
            self.logs.insert(digest, Vec::new());
            self.complete.insert(digest);
        }
        self.outbox.push(Message::Statement(signed.clone()));
        self.statements.insert(digest, signed);
    }

    /// Stops this validator's work on the epoch: signs its statement, which
    /// pins `own_log`, the payments it has applied.
    pub fn freeze(&mut self, statement: Statement, own_log: Vec<Hash32>, env: &mut Env) -> Vec<Message> {
        let mut out = Vec::new();
        if self.frozen() || self.watching {
            return out;
        }
        let digest = statement.digest(&self.network);
        let Some(proof) = (env.sign)(&[digest]).and_then(|mut proofs| proofs.pop()) else {
            return out;
        };
        let signed = Signed { body: statement, proof };
        // On disk before it is sent: from here on this validator must not
        // confirm any payment its statement does not pin.
        self.record(&Event::Sent(Message::Statement(signed.clone())));
        self.journal.sync().expect("cannot record the closing statement; stopping");
        self.take_statement(digest, signed.clone());
        self.mine = Some(digest);
        self.logs.insert(digest, own_log);
        self.complete.insert(digest);
        out.push(Message::Statement(signed));
        self.maybe_start(env, &mut out);
        out
    }

    /// After a restart: hands back the log this validator pinned before it
    /// stopped. Returns false if it is not that log.
    pub fn restore_own_log(&mut self, own_log: Vec<Hash32>) -> bool {
        let (Some(digest), Some(pin)) = (self.mine, self.own_pin()) else { return true };
        if Pin::of(self.epoch, &own_log) != pin {
            return false;
        }
        self.logs.insert(digest, own_log);
        self.complete.insert(digest);
        true
    }

    /// A message from another validator. Returns what this validator says
    /// in answer, for sending to everyone.
    pub fn on_message(&mut self, message: Message, env: &mut Env) -> Vec<Message> {
        let mut out = Vec::new();
        if self.watching && !matches!(message, Message::Decided(_)) {
            return out;
        }
        match message {
            Message::Statement(signed) => {
                let digest = signed.body.digest(&self.network);
                if self.statements.contains_key(&digest) || !close::verify_statement(env.committee, &signed) {
                    return out;
                }
                self.record(&Event::Heard(Message::Statement(signed.clone())));
                self.take_statement(digest, signed);
                self.maybe_start(env, &mut out);
            }
            Message::Ballot(signed) => {
                let ballot = signed.body;
                let from = signed.proof.validator;
                let key = (ballot.precommit, ballot.round, from);
                if self.ballots.contains_key(&key) || !close::verify_ballot(env.committee, &signed) {
                    return out;
                }
                self.record(&Event::Heard(Message::Ballot(signed.clone())));
                let outputs = self.hear_ballot(signed);
                self.perform(outputs, Some(env), &mut out);
            }
            Message::Proposal(signed) => {
                self.hear_proposal(signed, env);
                self.judge(env, &mut out);
            }
            Message::Decided(decided) => {
                if self.decided.is_some() || close::verify_decided(env.committee, &decided).is_err() {
                    return out;
                }
                self.record(&Event::Heard(Message::Decided(decided.clone())));
                let outputs = self.hear_decided(decided);
                self.perform(outputs, Some(env), &mut out);
            }
        }
        out
    }

    fn hear_ballot(&mut self, signed: Signed<Ballot>) -> Vec<Output<Checkpoint>> {
        let (ballot, from) = (signed.body, signed.proof.validator);
        self.outbox.push(Message::Ballot(signed.clone()));
        self.ballots.insert((ballot.precommit, ballot.round, from), signed);
        match ballot.precommit {
            true => self.agreement.on_precommit(from, ballot.round, ballot.id),
            false => self.agreement.on_prevote(from, ballot.round, ballot.id),
        }
    }

    fn hear_decided(&mut self, decided: Decided) -> Vec<Output<Checkpoint>> {
        let backers: Vec<u16> = decided.precommits.iter().map(|precommit| precommit.validator).collect();
        let (round, checkpoint) = (decided.round, decided.checkpoint.clone());
        self.decided = Some(decided);
        // Precommits from more than two thirds of the stake mean an honest
        // validator found the checkpoint valid, and validity is the same
        // test for everyone.
        self.agreement.on_commit(round, checkpoint, &backers, Verdict { valid: true, timely: true })
    }

    /// Takes a proposal if it is well formed and the first for its round.
    /// It is judged later, once the logs it rests on are gathered.
    fn hear_proposal(&mut self, signed: Signed<Proposal>, env: &Env) {
        let proposal = &signed.body;
        let round = proposal.round;
        if self.seen.contains(&round)
            || signed.proof.validator != self.agreement.proposer(round)
            || !close::verify_proposal(env.committee, &signed)
        {
            return;
        }
        let mut carried = Vec::new();
        for statement in &proposal.statements {
            let digest = statement.body.digest(&self.network);
            if self.statements.contains_key(&digest) || close::verify_statement(env.committee, statement) {
                carried.push((digest, statement.clone()));
            }
        }
        // A proposal must bring every statement it is built from, and the
        // prevotes it claims. One that does not is passed over, so that a
        // complete copy can still be taken.
        let known = |digest: &Hash32| {
            self.statements.contains_key(digest) || carried.iter().any(|(carried, _)| carried == digest)
        };
        if !proposal.checkpoint.merged.iter().all(|(_, digest)| known(digest)) {
            return;
        }
        let backers = match proposal.valid_round {
            None => Vec::new(),
            Some(earlier) => {
                let ballot = Ballot { precommit: false, round: earlier, id: Some(proposal.checkpoint.digest()) };
                match close::voters(env.committee, &ballot, &proposal.backers) {
                    Some(backers) if self.quorum(self.weight_of(backers.iter())) => backers,
                    _ => return,
                }
            }
        };
        for (digest, statement) in carried {
            if !self.statements.contains_key(&digest) {
                self.take_statement(digest, statement);
            }
        }
        self.seen.insert(round);
        self.outbox.push(Message::Proposal(signed.clone()));
        self.waiting.push((signed, backers));
    }

    /// The first round starts once statements from more than two thirds of
    /// the stake are in. Validators' clocks differ, but that moment is
    /// nearly the same for all of them.
    fn maybe_start(&mut self, env: &mut Env, out: &mut Vec<Message>) {
        if self.started || !self.frozen() || !self.quorum(self.statement_stake()) {
            return;
        }
        self.record(&Event::Started);
        self.started = true;
        let outputs = self.agreement.start();
        self.perform(outputs, Some(env), out);
    }

    /// Carries out what the agreement asks for. With no `env` the journal
    /// is being played back, and what would be signed is only noted.
    fn perform(&mut self, outputs: Vec<Output<Checkpoint>>, env: Option<&mut Env>, out: &mut Vec<Message>) {
        let mut owed = Vec::new();
        let now = env.as_ref().map(|env| env.now);
        for output in outputs {
            match output {
                Output::Broadcast(agree::Message::Prevote { round, id }) => {
                    owed.push(Owed::Ballot(Ballot { precommit: false, round, id }))
                }
                Output::Broadcast(agree::Message::Precommit { round, id }) => {
                    owed.push(Owed::Ballot(Ballot { precommit: true, round, id }))
                }
                Output::Broadcast(agree::Message::Proposal { round, value, valid_round, backers }) => {
                    owed.push(Owed::Proposal { round, checkpoint: value, valid_round, backers })
                }
                Output::Broadcast(agree::Message::Commit { round, value, backers })
                | Output::Decided { value, round, backers } => {
                    if self.outcome.is_none() {
                        self.outcome = Some((round, value, backers));
                    }
                }
                Output::Schedule(timeout) => match now {
                    Some(now) => self.timers.push((now + self.wait(timeout), timeout)),
                    None => self.armed.push(timeout),
                },
                Output::NeedValue { round } => {
                    self.need_value = Some(round);
                    self.ready_since = None;
                }
            }
        }
        match env {
            Some(env) => {
                self.send(owed, env, out);
                self.announce(out);
            }
            None => self.owed.extend(owed),
        }
    }

    /// Signs what this validator owes, records it, and hands it over for
    /// sending. One one-time key covers everything signed in one go.
    fn send(&mut self, owed: Vec<Owed>, env: &mut Env, out: &mut Vec<Message>) {
        if owed.is_empty() {
            return;
        }
        enum Body {
            Ballot(Ballot),
            Proposal(Proposal),
        }
        let bodies: Vec<Body> = owed
            .into_iter()
            .map(|owed| match owed {
                Owed::Ballot(ballot) => Body::Ballot(ballot),
                Owed::Proposal { round, checkpoint, valid_round, backers } => {
                    Body::Proposal(self.proposal(round, checkpoint, valid_round, &backers))
                }
            })
            .collect();
        let items: Vec<Hash32> = bodies
            .iter()
            .map(|body| match body {
                Body::Ballot(ballot) => ballot.item(),
                Body::Proposal(proposal) => proposal.item(),
            })
            .collect();
        let Some(proofs) = (env.sign)(&items) else { return };
        for (body, proof) in bodies.into_iter().zip(proofs) {
            let message = match body {
                Body::Ballot(ballot) => {
                    let signed = Signed { body: ballot, proof };
                    self.ballots.insert((ballot.precommit, ballot.round, self.me), signed.clone());
                    Message::Ballot(signed)
                }
                Body::Proposal(proposal) => {
                    self.seen.insert(proposal.round);
                    Message::Proposal(Signed { body: proposal, proof })
                }
            };
            self.record(&Event::Sent(message.clone()));
            self.outbox.push(message.clone());
            out.push(message);
        }
        // Everything that led to these messages, and the messages, must be
        // on disk before they leave. Otherwise a restart could bring this
        // validator back to before it sent them, free to send others.
        self.journal.sync().expect("cannot record a closing message; stopping");
    }

    /// The proposal that goes with a checkpoint: the statements it is built
    /// from and, when it is put forward again, the prevotes behind it.
    fn proposal(&self, round: u32, checkpoint: Checkpoint, valid_round: Option<u32>, backers: &[u16]) -> Proposal {
        let statements =
            checkpoint.merged.iter().filter_map(|(_, digest)| self.statements.get(digest).cloned()).collect();
        let prevotes = match valid_round {
            Some(earlier) => backers
                .iter()
                .filter_map(|backer| self.ballots.get(&(false, earlier, *backer)))
                .map(|signed| signed.proof.clone())
                .collect(),
            None => Vec::new(),
        };
        Proposal { round, checkpoint, valid_round, backers: prevotes, statements }
    }

    /// Puts together the proof of the decision and passes it on.
    fn announce(&mut self, out: &mut Vec<Message>) {
        if self.decided.is_none()
            && let Some((round, checkpoint, backers)) = &self.outcome
        {
            let precommits: Vec<Vote> = backers
                .iter()
                .filter_map(|backer| self.ballots.get(&(true, *round, *backer)))
                .map(|signed| signed.proof.clone())
                .collect();
            if precommits.len() == backers.len() {
                self.decided = Some(Decided { checkpoint: checkpoint.clone(), round: *round, precommits });
            }
        }
        if !self.announced
            && let Some(decided) = &self.decided
        {
            self.announced = true;
            self.outbox.push(Message::Decided(decided.clone()));
            out.push(Message::Decided(decided.clone()));
        }
    }

    /// The payments an epoch holds if it is closed from these statements:
    /// everything in any of their pinned logs.
    fn union<'a>(&self, merged: &[(u16, Hash32)], env: &Env<'a>) -> Result<Vec<&'a Order>, Invalid> {
        let mut seen = HashSet::new();
        let mut orders = Vec::new();
        for (validator, statement) in merged {
            for digest in self.logs.get(statement).ok_or(Invalid::MissingLog(*validator))? {
                if seen.insert(*digest) {
                    orders.push((env.order)(digest).ok_or(Invalid::MissingPayment(*validator))?);
                }
            }
        }
        Ok(orders)
    }

    /// Checks a checkpoint against the rules. Every validator holding the
    /// same statements and logs gets the same answer.
    fn evaluate(&self, checkpoint: &Checkpoint, env: &Env) -> Result<State, Invalid> {
        let named: BTreeMap<u16, Hash32> = checkpoint.merged.iter().copied().collect();
        let statement =
            |validator: u16| named.get(&validator).and_then(|d| self.statements.get(d)).map(|signed| &signed.body);
        let log = |validator: u16| named.get(&validator).and_then(|d| self.logs.get(d)).map(Vec::as_slice);
        let orders = self.union(&checkpoint.merged, env)?;
        if checkpoint.time < env.previous_time {
            return Err(Invalid::WrongPrevious);
        }
        close::validate_checked(env.committee, &env.previous, env.start, checkpoint, &statement, &log, &orders)
    }

    /// Judges every waiting proposal whose logs are all at hand.
    fn judge(&mut self, env: &mut Env, out: &mut Vec<Message>) {
        let mut index = 0;
        while index < self.waiting.len() {
            let merged = &self.waiting[index].0.body.checkpoint.merged;
            if !merged.iter().all(|(_, digest)| self.complete.contains(digest)) {
                index += 1;
                continue;
            }
            let (signed, backers) = self.waiting.remove(index);
            let proposal = &signed.body;
            let digest = proposal.checkpoint.digest();
            let valid = match self.valid.get(&digest) {
                Some(valid) => *valid,
                None => match self.evaluate(&proposal.checkpoint, env) {
                    Ok(end) => {
                        self.ends.insert(digest, end);
                        true
                    }
                    Err(_) => false,
                },
            };
            let timely = proposal.checkpoint.time.abs_diff(env.unix) <= self.pace.clock_tolerance;
            self.valid.insert(digest, valid);
            self.record(&Event::Judged { proposal: signed.clone(), valid, timely });
            let outputs = self.agreement.on_proposal(
                proposal.round,
                proposal.checkpoint.clone(),
                proposal.valid_round,
                &backers,
                Verdict { valid, timely },
            );
            self.perform(outputs, Some(env), out);
        }
    }

    /// Puts a checkpoint forward when it is this validator's turn and the
    /// pinned logs of enough validators are at hand.
    fn propose(&mut self, env: &mut Env, out: &mut Vec<Message>) {
        let Some(round) = self.need_value else { return };
        if self.decided.is_some() || round != self.agreement.round() {
            self.need_value = None;
            return;
        }
        let merged: Vec<(u16, Hash32)> = self
            .first
            .iter()
            .filter(|(_, digest)| self.complete.contains(*digest))
            .map(|(validator, digest)| (*validator, *digest))
            .collect();
        if !self.quorum(self.weight_of(merged.iter().map(|(validator, _)| validator))) {
            return;
        }
        // Enough is not all. Wait a moment for the rest, so that payments
        // only a slow validator recorded are not put off to the next epoch.
        let since = *self.ready_since.get_or_insert(env.now);
        if merged.len() < self.weights.len() && env.now.duration_since(since) < self.pace.unit {
            return;
        }
        self.need_value = None;
        let statements: Vec<&Statement> = merged.iter().map(|(_, digest)| &self.statements[digest].body).collect();
        let Ok(orders) = self.union(&merged, env) else { return };
        let time = env.unix.max(env.previous_time);
        let Ok((checkpoint, end)) =
            close::propose_checked(env.committee, env.previous, time, env.start, &orders, &statements)
        else {
            return;
        };
        let digest = checkpoint.digest();
        self.record(&Event::Proposed { round, checkpoint: checkpoint.clone() });
        self.valid.insert(digest, true);
        self.ends.insert(digest, end);
        let outputs = self.agreement.propose(round, checkpoint);
        self.perform(outputs, Some(env), out);
    }

    /// Works out the state the decided checkpoint leads to, if that was not
    /// done when it was proposed and the logs are at hand.
    fn work_out_end(&mut self, env: &Env) {
        let Some(decided) = &self.decided else { return };
        let checkpoint = decided.checkpoint.clone();
        let digest = checkpoint.digest();
        let ready = checkpoint.merged.iter().all(|(_, statement)| self.complete.contains(statement));
        if self.unsound || self.ends.contains_key(&digest) || !ready {
            return;
        }
        match self.evaluate(&checkpoint, env) {
            Ok(end) => {
                self.ends.insert(digest, end);
            }
            Err(reason) => {
                self.unsound = true;
                eprintln!("a decided checkpoint fails this validator's check ({reason:?}); not following it");
            }
        }
    }

    /// Does whatever has come due. Call it often.
    pub fn tick(&mut self, env: &mut Env) -> Vec<Message> {
        let mut out = Vec::new();
        // Left over from playing back the journal after a restart.
        if !self.owed.is_empty() || !self.armed.is_empty() {
            let owed = std::mem::take(&mut self.owed);
            self.send(owed, env, &mut out);
            for timeout in std::mem::take(&mut self.armed) {
                self.timers.push((env.now + self.wait(timeout), timeout));
            }
        }
        self.announce(&mut out);
        while let Some(index) = self.timers.iter().position(|(at, _)| *at <= env.now) {
            let (_, timeout) = self.timers.remove(index);
            let (step, round) = timeout_parts(timeout);
            self.record(&Event::Timeout { step, round });
            let outputs = self.agreement.on_timeout(timeout);
            self.perform(outputs, Some(env), &mut out);
        }
        // Logs whose payments have all arrived since last time.
        let arrived: Vec<Hash32> = self
            .logs
            .iter()
            .filter(|(statement, log)| {
                !self.complete.contains(*statement) && log.iter().all(|digest| (env.order)(digest).is_some())
            })
            .map(|(statement, _)| *statement)
            .collect();
        self.complete.extend(arrived);
        self.maybe_start(env, &mut out);
        self.judge(env, &mut out);
        self.propose(env, &mut out);
        self.work_out_end(env);
        out
    }

    /// The pinned logs still to be fetched from other validators.
    pub fn wanted(&mut self, now: Instant) -> Vec<Wanted> {
        let settled = self.decided.as_ref().is_some_and(|d| self.ends.contains_key(&d.checkpoint.digest()));
        if settled || self.unsound {
            return Vec::new();
        }
        let mut needed: Vec<Hash32> = self.first.values().copied().collect();
        let named = self.waiting.iter().map(|(proposal, _)| &proposal.body.checkpoint);
        for checkpoint in named.chain(self.decided.as_ref().map(|decided| &decided.checkpoint)) {
            needed.extend(checkpoint.merged.iter().map(|(_, statement)| *statement));
        }
        needed.sort_unstable();
        needed.dedup();
        let mut wanted = Vec::new();
        for statement in needed {
            let Some(signed) = self.statements.get(&statement) else { continue };
            if self.complete.contains(&statement) {
                continue;
            }
            let since = *self.asked.entry(statement).or_insert(now);
            wanted.push(Wanted {
                statement,
                validator: signed.body.validator,
                pin: signed.body.log,
                age: now.duration_since(since),
            });
        }
        wanted
    }

    /// A pinned log fetched from another validator. Returns the payments
    /// in it that this validator does not hold, or `None` if it is not the
    /// log the statement pins.
    pub fn on_log(&mut self, statement: Hash32, log: Vec<Hash32>, env: &Env) -> Option<Vec<Hash32>> {
        let pin = self.statements.get(&statement)?.body.log;
        if self.complete.contains(&statement) {
            return Some(Vec::new());
        }
        if !self.logs.contains_key(&statement) {
            if Pin::of(self.epoch, &log) != pin {
                return None;
            }
            self.logs.insert(statement, log);
        }
        let missing = self.logs[&statement].iter().filter(|digest| (env.order)(digest).is_none()).copied().collect();
        Some(missing)
    }

    /// Plays one journal record back into memory.
    fn replay(&mut self, event: Event) {
        let mut nothing = Vec::new();
        let outputs = match event {
            Event::Heard(Message::Statement(signed)) => {
                let digest = signed.body.digest(&self.network);
                self.take_statement(digest, signed);
                return;
            }
            Event::Heard(Message::Ballot(signed)) => self.hear_ballot(signed),
            Event::Heard(Message::Decided(decided)) => self.hear_decided(decided),
            Event::Heard(Message::Proposal(_)) => return,
            Event::Judged { proposal: signed, valid, timely } => {
                let proposal = &signed.body;
                for statement in &proposal.statements {
                    let digest = statement.body.digest(&self.network);
                    if !self.statements.contains_key(&digest) {
                        self.take_statement(digest, statement.clone());
                    }
                }
                self.seen.insert(proposal.round);
                self.outbox.push(Message::Proposal(signed.clone()));
                self.valid.insert(proposal.checkpoint.digest(), valid);
                let backers: Vec<u16> = proposal.backers.iter().map(|prevote| prevote.validator).collect();
                self.agreement.on_proposal(
                    proposal.round,
                    proposal.checkpoint.clone(),
                    proposal.valid_round,
                    &backers,
                    Verdict { valid, timely },
                )
            }
            Event::Started => {
                self.started = true;
                self.agreement.start()
            }
            Event::Proposed { round, checkpoint } => {
                self.valid.insert(checkpoint.digest(), true);
                self.need_value = None;
                self.agreement.propose(round, checkpoint)
            }
            Event::Timeout { step, round } => {
                let timeout = timeout_from(step, round);
                if let Some(index) = self.armed.iter().position(|armed| *armed == timeout) {
                    self.armed.remove(index);
                }
                self.agreement.on_timeout(timeout)
            }
            Event::Sent(message) => {
                match &message {
                    Message::Statement(signed) => {
                        let digest = signed.body.digest(&self.network);
                        self.take_statement(digest, signed.clone());
                        self.mine = Some(digest);
                        return;
                    }
                    Message::Ballot(signed) => {
                        let ballot = signed.body;
                        let owed = |owed: &Owed| matches!(owed, Owed::Ballot(other) if *other == ballot);
                        if let Some(index) = self.owed.iter().position(owed) {
                            self.owed.remove(index);
                        }
                        self.ballots.insert((ballot.precommit, ballot.round, self.me), signed.clone());
                    }
                    Message::Proposal(signed) => {
                        let (round, digest) = (signed.body.round, signed.body.checkpoint.digest());
                        let owed = |owed: &Owed| {
                            matches!(owed, Owed::Proposal { round: r, checkpoint, .. }
                                if *r == round && checkpoint.digest() == digest)
                        };
                        if let Some(index) = self.owed.iter().position(owed) {
                            self.owed.remove(index);
                        }
                        self.seen.insert(round);
                    }
                    Message::Decided(_) => {}
                }
                self.outbox.push(message);
                return;
            }
        };
        self.perform(outputs, None, &mut nothing);
    }
}

#[cfg(test)]
mod tests {
    //! Several closers run against each other in memory. Messages arrive in
    //! random order, each validator starts with different payments, and any
    //! one of them can be stopped and reopened from its journal at any step.

    use super::*;
    use orien_crypto::keytree::KeyTree;
    use orien_crypto::merkle;
    use orien_ledger::{Allocation, Genesis, Kind, ValidatorInfo};
    use std::collections::VecDeque;
    use std::path::PathBuf;

    const PACE: Pace = Pace { unit: Duration::from_millis(50), clock_tolerance: 30 };

    struct Node {
        closer: Option<Closer>,
        tree: KeyTree,
        next_leaf: u32,
        /// Payments applied, in order, and where each is by digest.
        orders: Vec<Order>,
        digests: Vec<Hash32>,
        held: HashMap<Hash32, usize>,
        /// How many of them the closing statement pinned.
        pinned: usize,
        /// How far into each other validator's messages this one has read.
        read: Vec<usize>,
    }

    struct Sim {
        dir: PathBuf,
        committee: Committee,
        network: Hash32,
        start: State,
        nodes: Vec<Node>,
        base: Instant,
        clock: Duration,
        queue: VecDeque<(usize, Message)>,
        /// Everything any validator signed and sent, with who sent it.
        said: Vec<(usize, Message)>,
        /// Check after every step that what was sent is in the journal.
        audit: bool,
        steps: usize,
        random: u64,
    }

    impl Drop for Sim {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }

    impl Sim {
        /// `validators` validators, each holding its own share of `payments`
        /// payments when the epoch ends.
        fn new(name: &str, validators: usize, payments: u8, seed: u64) -> Sim {
            let dir = std::env::temp_dir().join(format!("orien-closer-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            let trees: Vec<KeyTree> = (0..validators).map(|v| KeyTree::generate([40 + v as u8; 32], 6)).collect();
            let account = |n: u8| Hash32([n; 32]);
            let genesis = Genesis::new(
                name,
                trees
                    .iter()
                    .enumerate()
                    .map(|(v, tree)| ValidatorInfo {
                        name: format!("v{v}"),
                        root: tree.root(),
                        height: 6,
                        stake: 1,
                        address: String::new(),
                    })
                    .collect(),
                (1..=payments).map(|n| Allocation { account: account(n), amount: 100 }).collect(),
            );
            let committee = Committee::new(&genesis);
            let network = *committee.network();
            let mut sim = Sim {
                dir,
                network,
                start: State::from_genesis(&genesis),
                nodes: Vec::new(),
                base: Instant::now(),
                clock: Duration::ZERO,
                queue: VecDeque::new(),
                said: Vec::new(),
                audit: false,
                steps: 0,
                random: seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1,
                committee,
            };
            // Each account makes one payment, and each validator has heard
            // of some of them.
            let pool: Vec<Order> = (1..=payments)
                .map(|n| Order {
                    kind: Kind::Payment,
                    from: account(n),
                    seq: 0,
                    to: account(200),
                    amount: n as u64,
                    max_fee: 0,
                    next_key: account(n),
                    spare: account(n),
                })
                .collect();
            for (v, tree) in trees.into_iter().enumerate() {
                let mut node = Node {
                    closer: None,
                    tree,
                    next_leaf: 0,
                    orders: Vec::new(),
                    digests: Vec::new(),
                    held: HashMap::new(),
                    pinned: 0,
                    read: vec![0; validators],
                };
                for order in &pool {
                    if sim.chance(2) {
                        Sim::apply(&mut node, order, &network);
                    }
                }
                node.closer = Some(sim.open(v));
                sim.nodes.push(node);
            }
            sim
        }

        fn chance(&mut self, one_in: u64) -> bool {
            self.pick(one_in as usize) == 0
        }

        fn pick(&mut self, below: usize) -> usize {
            self.random ^= self.random << 13;
            self.random ^= self.random >> 7;
            self.random ^= self.random << 17;
            (self.random % below.max(1) as u64) as usize
        }

        fn apply(node: &mut Node, order: &Order, network: &Hash32) {
            let digest = order.digest(network);
            if !node.held.contains_key(&digest) {
                node.held.insert(digest, node.orders.len());
                node.orders.push(order.clone());
                node.digests.push(digest);
            }
        }

        fn journal(&self, v: usize) -> PathBuf {
            self.dir.join(format!("close-{v}.log"))
        }

        fn open(&self, v: usize) -> Closer {
            Closer::open(&self.journal(v), Some(v as u16), &self.committee, self.network, PACE, false).unwrap()
        }

        /// Runs one step of validator `v`'s closer and passes on what it says.
        fn step(&mut self, v: usize, step: impl FnOnce(&mut Closer, &mut Env) -> Vec<Message>) {
            let Node { closer, tree, next_leaf, orders, held, .. } = &mut self.nodes[v];
            let (committee, tree): (&Committee, &KeyTree) = (&self.committee, tree);
            let mut sign = |items: &[Hash32]| {
                let batch = merkle::Tree::build(items);
                let signature = tree.sign_with_leaf(*next_leaf, committee.batch_message(&batch.root()).as_bytes());
                *next_leaf += 1;
                let votes = (0..items.len())
                    .map(|place| Vote { validator: v as u16, batch_path: batch.path(place), signature: signature.clone() });
                Some(votes.collect())
            };
            let (orders, held): (&Vec<Order>, &HashMap<Hash32, usize>) = (orders, held);
            let order = |digest: &Hash32| held.get(digest).map(|place| &orders[*place]);
            let mut env = Env {
                committee,
                start: &self.start,
                previous: self.network,
                previous_time: 0,
                now: self.base + self.clock,
                unix: 1_000,
                sign: &mut sign,
                order: &order,
            };
            let out = step(closer.as_mut().unwrap(), &mut env);
            self.steps += 1;
            if self.audit {
                self.assert_recorded(v, &out);
            }
            for message in out {
                self.said.push((v, message.clone()));
                for to in (0..self.nodes.len()).filter(|to| *to != v) {
                    self.queue.push_back((to, message.clone()));
                }
            }
        }

        /// Everything `v` has just handed over for sending is in its journal.
        fn assert_recorded(&self, v: usize, out: &[Message]) {
            let (_, records) = Journal::open(&self.journal(v), false).unwrap();
            let sent: Vec<Message> = records
                .iter()
                .filter_map(|record| match borsh::from_slice::<Event>(record) {
                    Ok(Event::Sent(message)) => Some(message),
                    _ => None,
                })
                .collect();
            for message in out {
                if !matches!(message, Message::Decided(_)) {
                    assert!(sent.contains(message), "validator {v} sent something it had not recorded");
                }
            }
        }

        /// Validator `v`'s signature over `item`, made outside its closer.
        fn sign(&mut self, v: usize, item: Hash32) -> Vote {
            let node = &mut self.nodes[v];
            let batch = merkle::Tree::build(&[item]);
            let message = self.committee.batch_message(&batch.root());
            let signature = node.tree.sign_with_leaf(node.next_leaf, message.as_bytes());
            node.next_leaf += 1;
            Vote { validator: v as u16, batch_path: batch.path(0), signature }
        }

        /// Offers validator 0 a message. Returns whether it took it.
        fn offer(&mut self, message: Message) -> bool {
            let taken = |sim: &Sim| sim.nodes[0].closer.as_ref().unwrap().messages(0, 0).1;
            let before = taken(self);
            self.step(0, |closer, env| closer.on_message(message, env));
            taken(self) > before
        }

        fn statement_of(&self, v: usize) -> Signed<Statement> {
            let found = self.said.iter().find_map(|(from, message)| match message {
                Message::Statement(signed) if *from == v => Some(signed.clone()),
                _ => None,
            });
            found.unwrap()
        }

        fn freeze(&mut self, v: usize) {
            let statement = Statement {
                epoch: 0,
                validator: v as u16,
                log: Pin::of(0, &self.nodes[v].digests),
                evidence: Vec::new(),
                next_key: None,
                cheats: Vec::new(),
            };
            self.nodes[v].pinned = self.nodes[v].digests.len();
            let own = self.nodes[v].digests.clone();
            self.step(v, |closer, env| closer.freeze(statement, own, env));
        }

        /// What a closer must not forget across a restart: whether it has
        /// started, what it voted in every round, what it is locked on, and
        /// what it has proposed and decided.
        fn memory(closer: &Closer) -> String {
            let agreement = &closer.agreement;
            let votes: Vec<_> =
                (0..=agreement.round()).map(|round| (agreement.voted(round, false), agreement.voted(round, true))).collect();
            let locked = agreement.locked().map(|(round, value)| (round, value.digest()));
            let outcome = closer.outcome.as_ref().map(|(_, checkpoint, _)| checkpoint.digest());
            let decided = closer.decided.as_ref().map(|decided| decided.checkpoint.digest()).or(outcome);
            // A proposal owed for a round that is over means nothing.
            let owes = closer.need_value.filter(|round| *round == agreement.round() && decided.is_none());
            // A proposal taken in but not yet judged is not part of this. It
            // has had no effect, and the validator hears it again.
            format!("{:?}", (closer.started, closer.mine, agreement.round(), locked, votes, owes, decided))
        }

        /// Stops validator `v` and brings it back from its journal. It must
        /// come back knowing everything it knew.
        fn restart(&mut self, v: usize) {
            let before = Sim::memory(self.nodes[v].closer.as_ref().unwrap());
            self.nodes[v].closer = None;
            let mut closer = self.open(v);
            assert_eq!(Sim::memory(&closer), before, "validator {v} came back from a restart knowing something else");
            let pinned = self.nodes[v].pinned;
            assert!(closer.restore_own_log(self.nodes[v].digests[..pinned].to_vec()));
            self.nodes[v].closer = Some(closer);
            // Its list of messages was rebuilt, so the others read it again,
            // and it reads theirs again from the start, as a restarted
            // validator does.
            for node in &mut self.nodes {
                node.read[v] = 0;
            }
            self.nodes[v].read.fill(0);
        }

        fn decided(&self, v: usize) -> Option<Hash32> {
            self.nodes[v].closer.as_ref().unwrap().decided().map(|decided| decided.checkpoint.digest())
        }

        /// One round of what the validators' background threads do: read
        /// each other's messages, and fetch the logs they are missing.
        fn exchange(&mut self) {
            let count = self.nodes.len();
            for v in 0..count {
                for peer in (0..count).filter(|peer| *peer != v) {
                    let from = self.nodes[v].read[peer];
                    let (heard, total) = {
                        let (messages, total) = self.nodes[peer].closer.as_ref().unwrap().messages(from, 1_000);
                        (messages.to_vec(), total)
                    };
                    self.nodes[v].read[peer] = total;
                    for message in heard {
                        self.step(v, |closer, env| closer.on_message(message, env));
                    }
                }
                let now = self.base + self.clock;
                for wanted in self.nodes[v].closer.as_mut().unwrap().wanted(now) {
                    let owner = wanted.validator as usize;
                    let log = self.nodes[owner].closer.as_ref().unwrap().log(&wanted.statement, 0, usize::MAX);
                    let Some(log) = log.map(<[Hash32]>::to_vec) else { continue };
                    let mut missing = Vec::new();
                    self.step(v, |closer, env| {
                        missing = closer.on_log(wanted.statement, log, env).unwrap_or_default();
                        Vec::new()
                    });
                    for digest in missing {
                        let place = self.nodes[owner].held[&digest];
                        let order = self.nodes[owner].orders[place].clone();
                        Sim::apply(&mut self.nodes[v], &order, &self.network);
                    }
                }
            }
        }

        /// Runs until every validator has decided. `at` is called before
        /// every step with the number of steps taken so far.
        fn run(&mut self, mut at: impl FnMut(&mut Sim)) {
            for v in 0..self.nodes.len() {
                at(self);
                self.freeze(v);
            }
            for _ in 0..20_000 {
                if (0..self.nodes.len()).all(|v| self.decided(v).is_some()) {
                    return;
                }
                at(self);
                if !self.queue.is_empty() && !self.chance(8) {
                    let place = self.pick(self.queue.len());
                    let (to, message) = self.queue.remove(place).unwrap();
                    self.step(to, |closer, env| closer.on_message(message, env));
                } else {
                    self.clock += Duration::from_millis(10);
                    self.exchange();
                    for v in 0..self.nodes.len() {
                        self.step(v, |closer, env| closer.tick(env));
                    }
                }
            }
            panic!("the validators never agreed on a checkpoint");
        }

        /// No validator said two different things where it may say one.
        fn assert_consistent(&self) {
            let mut statements: HashMap<usize, Hash32> = HashMap::new();
            let mut ballots: HashMap<(usize, bool, u32), Option<Hash32>> = HashMap::new();
            let mut proposals: HashMap<(usize, u32), Hash32> = HashMap::new();
            for (v, message) in &self.said {
                match message {
                    Message::Statement(signed) => {
                        let digest = signed.body.digest(&self.network);
                        assert_eq!(*statements.entry(*v).or_insert(digest), digest, "validator {v} made two statements");
                    }
                    Message::Ballot(signed) => {
                        let ballot = signed.body;
                        let first = *ballots.entry((*v, ballot.precommit, ballot.round)).or_insert(ballot.id);
                        assert_eq!(first, ballot.id, "validator {v} voted twice in round {}", ballot.round);
                    }
                    Message::Proposal(signed) => {
                        let digest = signed.body.checkpoint.digest();
                        let first = *proposals.entry((*v, signed.body.round)).or_insert(digest);
                        assert_eq!(first, digest, "validator {v} proposed twice in round {}", signed.body.round);
                    }
                    Message::Decided(_) => {}
                }
            }
        }

        /// Everyone decided the same checkpoint, it holds every payment
        /// any validator had recorded, and each can work out its state.
        fn assert_agreed(&mut self) -> Checkpoint {
            let checkpoint = self.nodes[0].closer.as_ref().unwrap().decided().unwrap().checkpoint.clone();
            for v in 0..self.nodes.len() {
                assert_eq!(self.decided(v), Some(checkpoint.digest()), "validator {v} decided something else");
                let decided = self.nodes[v].closer.as_ref().unwrap().decided().unwrap().clone();
                assert!(close::verify_decided(&self.committee, &decided).is_ok());
            }
            // Give the validators that learned the decision late a moment
            // to gather what they need to work out the state.
            for _ in 0..50 {
                self.clock += Duration::from_millis(10);
                self.exchange();
                for v in 0..self.nodes.len() {
                    self.step(v, |closer, env| closer.tick(env));
                }
            }
            for v in 0..self.nodes.len() {
                let end = self.nodes[v].closer.as_mut().unwrap().take_end();
                assert_eq!(end.map(|end| end.root()), Some(checkpoint.state), "validator {v} has no end state");
            }
            checkpoint
        }
    }

    #[test]
    fn validators_with_different_payments_close_on_one_checkpoint() {
        for seed in 1..=20 {
            let mut sim = Sim::new("agree", 4, 12, seed);
            let recorded: HashSet<Hash32> = sim.nodes.iter().flat_map(|node| node.digests.clone()).collect();
            sim.run(|_| {});
            sim.assert_consistent();
            let checkpoint = sim.assert_agreed();
            // Every statement was merged, so nothing anyone recorded is left out.
            if checkpoint.merged.len() == 4 {
                assert_eq!(checkpoint.payments as usize, recorded.len(), "seed {seed}");
            }
            assert!(checkpoint.payments as usize <= recorded.len());
        }
    }

    #[test]
    fn messages_that_do_not_check_out_are_ignored() {
        let mut sim = Sim::new("forged", 4, 4, 5);
        for v in 0..4 {
            sim.freeze(v);
        }
        // Nothing has been delivered: validator 0 knows only its own statement.
        let (one, two) = (sim.statement_of(1), sim.statement_of(2));
        let stolen = Signed { body: one.body.clone(), proof: two.proof.clone() };
        assert!(!sim.offer(Message::Statement(stolen)), "a statement with another validator's signature");
        let mut altered = one.clone();
        altered.body.log.length += 1;
        assert!(!sim.offer(Message::Statement(altered)), "a statement changed after signing");
        assert!(sim.offer(Message::Statement(one.clone())));
        assert!(!sim.offer(Message::Statement(one)), "the same statement a second time");

        // A log is taken only if it is the one the statement pins.
        let owner = (1..4).find(|v| !sim.nodes[*v].digests.is_empty()).expect("someone recorded a payment");
        let statement = sim.statement_of(owner);
        let pinned = statement.body.digest(&sim.network);
        sim.offer(Message::Statement(statement));
        let genuine = sim.nodes[owner].digests.clone();
        let mut longer = genuine.clone();
        longer.push(Hash32([3; 32]));
        sim.step(0, |closer, env| {
            assert_eq!(closer.on_log(pinned, longer, env), None, "a log that is not the pinned one was taken");
            assert_eq!(closer.on_log(pinned, Vec::new(), env), None, "an empty log was taken for a pinned one");
            assert!(closer.on_log(pinned, genuine, env).is_some());
            Vec::new()
        });

        let ballot = Ballot { precommit: false, round: 0, id: None };
        let signed = Signed { body: ballot, proof: sim.sign(1, ballot.item()) };
        let relabelled = Signed { body: Ballot { precommit: true, ..ballot }, proof: signed.proof.clone() };
        assert!(!sim.offer(Message::Ballot(relabelled)), "a prevote passed off as a precommit");
        assert!(sim.offer(Message::Ballot(signed)));
        let second = Ballot { id: Some(Hash32([4; 32])), ..ballot };
        let signed = Signed { body: second, proof: sim.sign(1, second.item()) };
        assert!(!sim.offer(Message::Ballot(signed)), "a second prevote from one validator in one round");

        // Proposals. Validator 0 is the one judging, so pick rounds that are
        // someone else's turn.
        let proposer = |sim: &Sim, round: u32| sim.nodes[0].closer.as_ref().unwrap().agreement.proposer(round) as usize;
        let checkpoint = Checkpoint {
            epoch: 0,
            previous: sim.network,
            time: 1_000,
            state: sim.start.root(),
            payments: 0,
            merged: Vec::new(),
            validators: sim.committee.members().to_vec(),
        };
        let make = |sim: &mut Sim, signer: usize, proposal: Proposal| {
            Message::Proposal(Signed { proof: sim.sign(signer, proposal.item()), body: proposal })
        };
        let round = (1..).find(|round| proposer(&sim, *round) != 0).unwrap();
        let turn = proposer(&sim, round);
        let other = (1..4).find(|v| *v != turn).unwrap();
        let plain = Proposal {
            round,
            checkpoint: checkpoint.clone(),
            valid_round: None,
            backers: Vec::new(),
            statements: Vec::new(),
        };
        let message = make(&mut sim, other, plain.clone());
        assert!(!sim.offer(message), "a proposal from a validator whose turn it is not");
        let unknown = Checkpoint { merged: vec![(1, Hash32([7; 32]))], ..checkpoint.clone() };
        let message = make(&mut sim, turn, Proposal { checkpoint: unknown, ..plain.clone() });
        assert!(!sim.offer(message), "a proposal that does not bring the statements it is built from");
        let prevote = Ballot { precommit: false, round: 0, id: Some(checkpoint.digest()) };
        let thin = vec![sim.sign(1, prevote.item()), sim.sign(2, prevote.item())];
        let message = make(&mut sim, turn, Proposal { valid_round: Some(0), backers: thin, ..plain.clone() });
        assert!(!sim.offer(message), "a checkpoint put forward again with too few prevotes behind it");
        let message = make(&mut sim, turn, plain.clone());
        assert!(sim.offer(message));
        let later = Checkpoint { time: 1_001, ..checkpoint.clone() };
        let message = make(&mut sim, turn, Proposal { checkpoint: later, ..plain });
        assert!(!sim.offer(message), "a second proposal for one round");

        // A decision needs precommits from more than two thirds of the stake.
        let precommit = Ballot { precommit: true, round: 0, id: Some(checkpoint.digest()) };
        let precommits: Vec<Vote> = (1..4).map(|v| sim.sign(v, precommit.item())).collect();
        let thin = Decided { checkpoint: checkpoint.clone(), round: 0, precommits: precommits[..2].to_vec() };
        assert!(!sim.offer(Message::Decided(thin)));
        assert_eq!(sim.decided(0), None);
        assert!(sim.offer(Message::Decided(Decided { checkpoint: checkpoint.clone(), round: 0, precommits })));
        assert_eq!(sim.decided(0), Some(checkpoint.digest()));
    }

    #[test]
    fn what_a_validator_sends_is_in_its_journal_first() {
        let mut sim = Sim::new("recorded", 4, 6, 3);
        sim.audit = true;
        sim.run(|_| {});
        sim.assert_agreed();
    }

    #[test]
    fn a_validator_restarted_at_any_moment_says_nothing_different() {
        // How many steps a run takes, so that every one of them is tried.
        let mut plain = Sim::new("restart-plain", 4, 6, 7);
        plain.run(|_| {});
        let steps = plain.steps;
        assert!(steps > 30, "the run was too short to mean anything: {steps} steps");

        for crash_at in 0..steps {
            let victim = crash_at % 4;
            let mut sim = Sim::new("restart", 4, 6, 7);
            sim.run(|sim| {
                // Only once it has a statement is there anything to forget.
                if sim.steps == crash_at && sim.nodes[victim].closer.as_ref().unwrap().frozen() {
                    sim.restart(victim);
                }
            });
            sim.assert_consistent();
            sim.assert_agreed();
        }
    }

    #[test]
    fn a_restart_remembers_a_wait_that_ran_out() {
        let mut sim = Sim::new("timeout", 4, 0, 9);
        // The validator whose turn it is to propose never shows up.
        let silent = sim.nodes[0].closer.as_ref().unwrap().agreement.proposer(0) as usize;
        let awake: Vec<usize> = (0..4).filter(|v| *v != silent).collect();
        for v in &awake {
            sim.freeze(*v);
        }
        while let Some((to, message)) = sim.queue.pop_front() {
            if to != silent {
                sim.step(to, |closer, env| closer.on_message(message, env));
            }
        }
        let v = awake[0];
        assert!(sim.nodes[v].closer.as_ref().unwrap().started);
        let prevotes = |sim: &Sim| {
            let cast = |(from, message): &&(usize, Message)| {
                *from == v && matches!(message, Message::Ballot(signed) if !signed.body.precommit && signed.body.round == 0)
            };
            sim.said.iter().filter(cast).count()
        };
        assert_eq!(prevotes(&sim), 0);
        // The wait for a proposal runs out, and it votes for nothing.
        sim.clock += Duration::from_secs(2);
        sim.step(v, |closer, env| closer.tick(env));
        assert_eq!(prevotes(&sim), 1);
        // It comes back from a restart knowing that it has voted.
        sim.restart(v);
        sim.clock += Duration::from_secs(2);
        sim.step(v, |closer, env| closer.tick(env));
        assert_eq!(prevotes(&sim), 1, "it voted a second time in one round");
        sim.assert_consistent();
    }

    #[test]
    fn a_validator_restarted_again_and_again_still_closes() {
        for seed in 1..=10 {
            let mut sim = Sim::new("restarts", 4, 8, seed);
            sim.run(|sim| {
                let victim = sim.pick(4);
                if sim.chance(15) && sim.nodes[victim].closer.as_ref().unwrap().frozen() {
                    sim.restart(victim);
                }
            });
            sim.assert_consistent();
            sim.assert_agreed();
        }
    }

    #[test]
    fn a_close_goes_through_with_one_validator_silent() {
        for seed in 1..=10 {
            let mut sim = Sim::new("silent", 4, 8, seed);
            // Validator 3 never takes part: it is never frozen and hears nothing.
            for v in 0..3 {
                sim.freeze(v);
            }
            for _ in 0..20_000 {
                if (0..3).all(|v| sim.decided(v).is_some()) {
                    break;
                }
                sim.queue.retain(|(to, _)| *to != 3);
                // Rounds whose turn falls to the silent validator time out,
                // and a restart must not forget that either.
                let victim = sim.pick(3);
                if sim.chance(12) && sim.decided(victim).is_none() {
                    sim.restart(victim);
                }
                if let Some((to, message)) = sim.queue.pop_front() {
                    sim.step(to, |closer, env| closer.on_message(message, env));
                } else {
                    sim.clock += Duration::from_millis(10);
                    let count = sim.nodes.len();
                    for v in 0..3 {
                        for peer in (0..count).filter(|peer| *peer != v && *peer != 3) {
                            let from = sim.nodes[v].read[peer];
                            let (heard, total) = {
                                let (messages, total) = sim.nodes[peer].closer.as_ref().unwrap().messages(from, 1_000);
                                (messages.to_vec(), total)
                            };
                            sim.nodes[v].read[peer] = total;
                            for message in heard {
                                sim.step(v, |closer, env| closer.on_message(message, env));
                            }
                        }
                        let now = sim.base + sim.clock;
                        for wanted in sim.nodes[v].closer.as_mut().unwrap().wanted(now) {
                            let owner = wanted.validator as usize;
                            let log = sim.nodes[owner].closer.as_ref().unwrap().log(&wanted.statement, 0, usize::MAX);
                            let Some(log) = log.map(<[Hash32]>::to_vec) else { continue };
                            let mut missing = Vec::new();
                            sim.step(v, |closer, env| {
                                missing = closer.on_log(wanted.statement, log, env).unwrap_or_default();
                                Vec::new()
                            });
                            for digest in missing {
                                let place = sim.nodes[owner].held[&digest];
                                let order = sim.nodes[owner].orders[place].clone();
                                Sim::apply(&mut sim.nodes[v], &order, &sim.network);
                            }
                        }
                        sim.step(v, |closer, env| closer.tick(env));
                    }
                }
            }
            let first = sim.decided(0).expect("three of four validators are enough");
            assert!((0..3).all(|v| sim.decided(v) == Some(first)), "seed {seed}");
            sim.assert_consistent();
            // The silent one learns the outcome from the proof alone.
            let decided = sim.nodes[0].closer.as_ref().unwrap().decided().unwrap().clone();
            sim.step(3, |closer, env| closer.on_message(Message::Decided(decided), env));
            assert_eq!(sim.decided(3), Some(first));
        }
    }
}
