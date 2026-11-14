//! One decision among validators.
//!
//! Payments need no agreement. Closing an epoch does: every validator must
//! end up with the same checkpoint. This is the procedure that gets them
//! there, and it is deliberately not new. It is the Tendermint algorithm
//! (Buchman, Kwon and Milosevic, "The latest gossip on BFT consensus",
//! 2018), followed rule for rule.
//!
//! Validators work in rounds. In each round one of them, chosen in
//! proportion to stake, proposes a value. The others prevote for it or for
//! nothing, and then precommit to it or to nothing. A value with precommits
//! from more than two thirds of the stake is decided. A validator that
//! precommits to a value is locked on it: in later rounds it will not
//! prevote for anything else unless it sees that more than two thirds have
//! since prevoted for something newer. That lock is what makes two
//! different decisions impossible while fewer than a third are dishonest.
//!
//! Honest validators send every message to everyone. A dishonest one may
//! tell different validators different things, or tell only one of them.
//! So two kinds of message carry their own proof, and are judged on that
//! proof alone, whatever else the receiver has or has not seen:
//!
//! - a value put forward again names the validators whose earlier prevotes
//!   stand behind it;
//! - a decision names the validators whose precommits made it.
//!
//! Without this, one vote shown to one validator could leave it locked on
//! a value the others can never be persuaded of, and nothing would ever be
//! decided.
//!
//! This is pure logic. It does no input or output, keeps no clock and checks
//! no signatures. The caller hands it messages already known to be genuine,
//! tells it when a timeout it asked for has passed, and carries out what it
//! returns. The caller also judges each proposed value, and that judgement
//! must be one every honest validator would reach: a validator that rejects
//! a value for reasons only it knows can leave the others locked on
//! something that can never be decided.

use orien_crypto::{Hash32, Tag, hash};
use std::collections::BTreeMap;

/// Something validators can agree on.
pub trait Value: Clone {
    /// A digest that stands for the value in votes.
    fn id(&self) -> Hash32;
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Message<V> {
    /// A round's proposer puts a value forward. `valid_round` is set when
    /// the value is not new: it names an earlier round in which more than
    /// two thirds prevoted for it, and `backers` are the validators whose
    /// prevotes from that round come with the proposal as proof.
    Proposal { round: u32, value: V, valid_round: Option<u32>, backers: Vec<u16> },
    /// A vote for the value with this id, or for nothing.
    Prevote { round: u32, id: Option<Hash32> },
    Precommit { round: u32, id: Option<Hash32> },
    /// A decision. `backers` are the validators whose precommits for the
    /// value in `round` come with it as proof.
    Commit { round: u32, value: V, backers: Vec<u16> },
}

/// A wait the caller is asked to time. Later rounds should wait longer, so
/// that slow networks eventually get a round long enough to finish in.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Timeout {
    Propose(u32),
    Prevote(u32),
    Precommit(u32),
}

/// What the caller must do next.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Output<V> {
    /// Send this to every other validator. Where the message names
    /// backers, their votes go with it.
    Broadcast(Message<V>),
    /// Call `on_timeout` with this once its time has passed.
    Schedule(Timeout),
    /// It is this validator's turn. Call `propose` with a value for this round.
    NeedValue { round: u32 },
    /// Agreement is reached. The caller keeps the precommits of `backers`
    /// as the proof.
    Decided { value: V, round: u32, backers: Vec<u16> },
}

/// What a validator found when it checked a proposed value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Verdict {
    /// The value follows the rules. Every honest validator must reach the
    /// same answer here.
    pub valid: bool,
    /// The value's time is close to this validator's clock. Clocks differ,
    /// so validators may disagree. It is asked only of a value proposed
    /// for the first time, never of one that already has two thirds of
    /// the prevotes behind it.
    pub timely: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Step {
    Propose,
    Prevote,
    Precommit,
}

struct Proposed<V> {
    value: V,
    valid_round: Option<u32>,
    /// The proposal came with prevotes from more than two thirds for this
    /// value in `valid_round`.
    backed: bool,
    verdict: Verdict,
}

#[derive(Default)]
struct Round {
    prevotes: BTreeMap<u16, Option<Hash32>>,
    precommits: BTreeMap<u16, Option<Hash32>>,
    prevote_wait_asked: bool,
    precommit_wait_asked: bool,
    /// The rule that locks on a value with two thirds of the prevotes has run.
    locked_in: bool,
}

pub struct Agreement<V> {
    me: u16,
    weights: Vec<u64>,
    total: u64,
    /// Decides whose turn it is to propose. Taken from the previous
    /// checkpoint, so nobody can arrange the order of turns in advance.
    seed: Hash32,
    round: u32,
    step: Step,
    started: bool,
    /// The value this validator precommitted to, and the round it did so.
    locked: Option<(u32, V)>,
    /// The newest value seen with two thirds of the prevotes.
    valid: Option<(u32, V)>,
    /// The first proposal seen from each round's proposer.
    proposals: BTreeMap<u32, Proposed<V>>,
    rounds: BTreeMap<u32, Round>,
    decided: Option<V>,
}

impl<V: Value> Agreement<V> {
    /// `weights` holds each validator's stake, by index. `me` is this one.
    pub fn new(me: u16, weights: Vec<u64>, seed: Hash32) -> Agreement<V> {
        let total = weights.iter().sum();
        Agreement {
            me,
            weights,
            total,
            seed,
            round: 0,
            step: Step::Propose,
            started: false,
            locked: None,
            valid: None,
            proposals: BTreeMap::new(),
            rounds: BTreeMap::new(),
            decided: None,
        }
    }

    pub fn round(&self) -> u32 {
        self.round
    }

    pub fn decided(&self) -> Option<&V> {
        self.decided.as_ref()
    }

    /// What this validator itself prevoted, or precommitted, in `round`.
    pub fn voted(&self, round: u32, precommit: bool) -> Option<Option<Hash32>> {
        let votes = self.rounds.get(&round)?;
        if precommit { votes.precommits.get(&self.me).copied() } else { votes.prevotes.get(&self.me).copied() }
    }

    /// The value this validator is locked on, and the round it locked in.
    pub fn locked(&self) -> Option<(u32, &V)> {
        self.locked.as_ref().map(|(round, value)| (*round, value))
    }

    /// Whose turn it is to propose in `round`: a validator picked in
    /// proportion to stake, the same pick on every validator.
    pub fn proposer(&self, round: u32) -> u16 {
        let draw = hash(Tag::Proposer, &[self.seed.as_bytes(), &round.to_le_bytes()]);
        let mut point = u128::from_le_bytes(draw.0[..16].try_into().unwrap()) % self.total.max(1) as u128;
        for (index, weight) in self.weights.iter().enumerate() {
            if point < *weight as u128 {
                return index as u16;
            }
            point -= *weight as u128;
        }
        0
    }

    /// Begins the first round.
    pub fn start(&mut self) -> Vec<Output<V>> {
        let mut out = Vec::new();
        if !self.started {
            self.started = true;
            self.start_round(0, &mut out);
            self.settle(&mut out);
        }
        out
    }

    /// Supplies the value asked for with `NeedValue`. Ignored if the round
    /// has moved on in the meantime.
    pub fn propose(&mut self, round: u32, value: V) -> Vec<Output<V>> {
        let mut out = Vec::new();
        let mine = self.decided.is_none()
            && round == self.round
            && self.step == Step::Propose
            && self.proposer(round) == self.me
            && !self.proposals.contains_key(&round);
        if mine {
            self.send_proposal(round, value, None, Vec::new(), &mut out);
            self.settle(&mut out);
        }
        out
    }

    /// A proposal made by `round`'s proposer, with what checking its value
    /// found. The caller has checked that each backer's prevote for this
    /// value in `valid_round` really came with it. Only the first proposal
    /// seen for a round counts.
    pub fn on_proposal(
        &mut self,
        round: u32,
        value: V,
        valid_round: Option<u32>,
        backers: &[u16],
        verdict: Verdict,
    ) -> Vec<Output<V>> {
        let mut out = Vec::new();
        let backed = valid_round.is_some() && self.most(self.weight_of(backers));
        self.proposals.entry(round).or_insert(Proposed { value, valid_round, backed, verdict });
        self.settle(&mut out);
        out
    }

    /// A decision another validator reached, with what checking its value
    /// found. The caller has checked that each backer's precommit for this
    /// value in `round` really came with it.
    pub fn on_commit(&mut self, round: u32, value: V, backers: &[u16], verdict: Verdict) -> Vec<Output<V>> {
        let mut out = Vec::new();
        if self.decided.is_none() && verdict.valid && self.most(self.weight_of(backers)) {
            self.decided = Some(value.clone());
            // Pass the proof on. Whoever sent it may have sent it only here,
            // and a validator that has decided takes no further part.
            let backers = backers.to_vec();
            out.push(Output::Broadcast(Message::Commit { round, value: value.clone(), backers: backers.clone() }));
            out.push(Output::Decided { value, round, backers });
        }
        out
    }

    /// Only a validator's first prevote in a round counts.
    pub fn on_prevote(&mut self, from: u16, round: u32, id: Option<Hash32>) -> Vec<Output<V>> {
        let mut out = Vec::new();
        if (from as usize) < self.weights.len() {
            self.rounds.entry(round).or_default().prevotes.entry(from).or_insert(id);
            self.settle(&mut out);
        }
        out
    }

    /// Only a validator's first precommit in a round counts.
    pub fn on_precommit(&mut self, from: u16, round: u32, id: Option<Hash32>) -> Vec<Output<V>> {
        let mut out = Vec::new();
        if (from as usize) < self.weights.len() {
            self.rounds.entry(round).or_default().precommits.entry(from).or_insert(id);
            self.settle(&mut out);
        }
        out
    }

    pub fn on_timeout(&mut self, timeout: Timeout) -> Vec<Output<V>> {
        let mut out = Vec::new();
        if self.decided.is_some() || !self.started {
            return out;
        }
        match timeout {
            // No acceptable proposal came in time: vote for nothing.
            Timeout::Propose(round) if round == self.round && self.step == Step::Propose => {
                self.prevote(None, &mut out);
            }
            // The prevotes did not settle on anything: precommit to nothing.
            Timeout::Prevote(round) if round == self.round && self.step == Step::Prevote => {
                self.precommit(None, &mut out);
            }
            // The round produced no decision: try the next.
            Timeout::Precommit(round) if round == self.round => self.start_round(round + 1, &mut out),
            _ => {}
        }
        self.settle(&mut out);
        out
    }

    fn start_round(&mut self, round: u32, out: &mut Vec<Output<V>>) {
        self.round = round;
        self.step = Step::Propose;
        out.push(Output::Schedule(Timeout::Propose(round)));
        if self.proposer(round) == self.me {
            match self.valid.clone() {
                // A value that already gathered two thirds of the prevotes
                // is put forward again, not replaced.
                Some((valid_round, value)) => {
                    let backers = self.voters(valid_round, &Some(value.id()), |r| &r.prevotes);
                    self.send_proposal(round, value, Some(valid_round), backers, out)
                }
                None => out.push(Output::NeedValue { round }),
            }
        }
    }

    fn send_proposal(
        &mut self,
        round: u32,
        value: V,
        valid_round: Option<u32>,
        backers: Vec<u16>,
        out: &mut Vec<Output<V>>,
    ) {
        let verdict = Verdict { valid: true, timely: true };
        let proposed = Proposed { value: value.clone(), valid_round, backed: valid_round.is_some(), verdict };
        self.proposals.insert(round, proposed);
        out.push(Output::Broadcast(Message::Proposal { round, value, valid_round, backers }));
    }

    /// The validators this one has seen vote for exactly `id` in `round`.
    fn voters(
        &self,
        round: u32,
        id: &Option<Hash32>,
        votes: impl Fn(&Round) -> &BTreeMap<u16, Option<Hash32>>,
    ) -> Vec<u16> {
        let seen = self.rounds.get(&round).map(|r| votes(r).clone()).unwrap_or_default();
        seen.into_iter().filter(|(_, vote)| vote == id).map(|(from, _)| from).collect()
    }

    /// The combined stake of the validators listed, each counted once.
    fn weight_of(&self, validators: &[u16]) -> u64 {
        let mut distinct: Vec<u16> = validators.to_vec();
        distinct.sort_unstable();
        distinct.dedup();
        distinct.iter().filter_map(|v| self.weights.get(*v as usize)).sum()
    }

    fn prevote(&mut self, id: Option<Hash32>, out: &mut Vec<Output<V>>) {
        self.rounds.entry(self.round).or_default().prevotes.entry(self.me).or_insert(id);
        self.step = Step::Prevote;
        out.push(Output::Broadcast(Message::Prevote { round: self.round, id }));
    }

    fn precommit(&mut self, id: Option<Hash32>, out: &mut Vec<Output<V>>) {
        self.rounds.entry(self.round).or_default().precommits.entry(self.me).or_insert(id);
        self.step = Step::Precommit;
        out.push(Output::Broadcast(Message::Precommit { round: self.round, id }));
    }

    fn weight(&self, votes: &BTreeMap<u16, Option<Hash32>>, counts: impl Fn(&Option<Hash32>) -> bool) -> u64 {
        votes.iter().filter(|(_, vote)| counts(vote)).map(|(from, _)| self.weights[*from as usize]).sum()
    }

    /// More than two thirds of the stake.
    fn most(&self, weight: u64) -> bool {
        3 * weight as u128 > 2 * self.total as u128
    }

    /// More than two thirds of the stake prevoted for exactly this in `round`.
    fn prevoted(&self, round: u32, id: &Option<Hash32>) -> bool {
        self.rounds.get(&round).is_some_and(|r| self.most(self.weight(&r.prevotes, |vote| vote == id)))
    }

    fn precommitted(&self, round: u32, id: &Option<Hash32>) -> bool {
        self.rounds.get(&round).is_some_and(|r| self.most(self.weight(&r.precommits, |vote| vote == id)))
    }

    /// Applies the rules until none of them has anything left to do.
    fn settle(&mut self, out: &mut Vec<Output<V>>) {
        while self.started && self.decided.is_none() && self.apply_one_rule(out) {}
    }

    fn apply_one_rule(&mut self, out: &mut Vec<Output<V>>) -> bool {
        // A value with precommits from more than two thirds, in any round,
        // is decided.
        let decision = self.proposals.iter().find_map(|(round, proposed)| {
            let agreed = proposed.verdict.valid && self.precommitted(*round, &Some(proposed.value.id()));
            agreed.then(|| (*round, proposed.value.clone()))
        });
        if let Some((round, value)) = decision {
            let backers = self.voters(round, &Some(value.id()), |r| &r.precommits);
            self.decided = Some(value.clone());
            out.push(Output::Broadcast(Message::Commit { round, value: value.clone(), backers: backers.clone() }));
            out.push(Output::Decided { value, round, backers });
            return true;
        }

        // Validators holding more than a third are already in a later
        // round: join them, since this round can no longer finish.
        let later = self.rounds.iter().rev().find_map(|(round, votes)| {
            let mut present: BTreeMap<u16, Option<Hash32>> = votes.prevotes.clone();
            present.extend(votes.precommits.iter().map(|(from, vote)| (*from, *vote)));
            (*round > self.round && 3 * self.weight(&present, |_| true) as u128 > self.total as u128)
                .then_some(*round)
        });
        if let Some(round) = later {
            self.start_round(round, out);
            return true;
        }

        let round = self.round;
        let proposal =
            self.proposals.get(&round).map(|p| (p.value.clone(), p.valid_round, p.backed, p.verdict));

        // Prevote on this round's proposal.
        if self.step == Step::Propose
            && let Some((value, valid_round, backed, verdict)) = &proposal
        {
            let id = value.id();
            let accept = match valid_round {
                // A new value: it must be valid and timely, and this
                // validator must not be locked on something else.
                None => Some(
                    verdict.valid
                        && verdict.timely
                        && self.locked.as_ref().is_none_or(|(_, locked)| locked.id() == id),
                ),
                // A value put forward again: it needs the two thirds of
                // prevotes it claims, either brought along as proof or seen
                // here. Then accept it unless locked on a different value
                // more recently than that.
                Some(earlier) if *earlier < round => (*backed || self.prevoted(*earlier, &Some(id))).then(|| {
                    verdict.valid
                        && self
                            .locked
                            .as_ref()
                            .is_none_or(|(locked_round, locked)| locked_round <= earlier || locked.id() == id)
                }),
                Some(_) => Some(false),
            };
            if let Some(accept) = accept {
                self.prevote(accept.then_some(id), out);
                return true;
            }
        }

        // Two thirds prevoted for this round's proposal: lock on it and
        // precommit.
        let locked_in = self.rounds.get(&round).is_some_and(|r| r.locked_in);
        if self.step != Step::Propose
            && !locked_in
            && let Some((value, _, _, verdict)) = &proposal
            && verdict.valid
            && self.prevoted(round, &Some(value.id()))
        {
            self.rounds.entry(round).or_default().locked_in = true;
            if self.step == Step::Prevote {
                self.locked = Some((round, value.clone()));
                self.precommit(Some(value.id()), out);
            }
            self.valid = Some((round, value.clone()));
            return true;
        }

        // Two thirds prevoted for nothing: precommit to nothing.
        if self.step == Step::Prevote && self.prevoted(round, &None) {
            self.precommit(None, out);
            return true;
        }

        // Two thirds have prevoted, though not for one thing: give the
        // stragglers a moment, then move on.
        let votes = self.rounds.entry(round).or_default();
        let (prevotes, precommits) = (votes.prevotes.clone(), votes.precommits.clone());
        let (prevote_wait_asked, precommit_wait_asked) = (votes.prevote_wait_asked, votes.precommit_wait_asked);
        if self.step == Step::Prevote && !prevote_wait_asked && self.most(self.weight(&prevotes, |_| true)) {
            self.rounds.entry(round).or_default().prevote_wait_asked = true;
            out.push(Output::Schedule(Timeout::Prevote(round)));
            return true;
        }
        if !precommit_wait_asked && self.most(self.weight(&precommits, |_| true)) {
            self.rounds.entry(round).or_default().precommit_wait_asked = true;
            out.push(Output::Schedule(Timeout::Precommit(round)));
            return true;
        }
        false
    }
}
