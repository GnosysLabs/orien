//! Agreement under bad conditions: messages delivered in any order, timeouts
//! that fire early, validators that are silent, and one that lies.

use orien_crypto::Hash32;
use orien_ledger::agree::{Agreement, Message, Output, Timeout, Value, Verdict};
use std::collections::BTreeSet;

/// A value the liar proposes that breaks the rules.
const RULE_BREAKER: Val = Val(5_000_000);

/// A value is a number. Numbers of a million and above break the rules,
/// which every validator can see.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Val(u64);

impl Value for Val {
    fn id(&self) -> Hash32 {
        let mut id = [0u8; 32];
        id[..8].copy_from_slice(&self.0.to_le_bytes());
        Hash32(id)
    }
}

fn follows_the_rules(value: &Val) -> bool {
    value.0 < 1_000_000
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Role {
    Honest,
    /// Never sends and never hears anything.
    Silent,
    /// Sends whatever the simulation makes up for it.
    Lying,
}

struct Sim {
    nodes: Vec<Agreement<Val>>,
    roles: Vec<Role>,
    /// Messages on their way: recipient, sender, message.
    queue: Vec<(usize, usize, Message<Val>)>,
    timeouts: Vec<(usize, Timeout)>,
    decided: Vec<Option<(Val, u32)>>,
    /// Every value an honest proposer put forward, by round.
    proposed: Vec<Val>,
    rng: u64,
    steps: usize,
    /// Until this many steps have passed the network misbehaves: timeouts
    /// fire while messages are still in flight and the liar keeps talking.
    /// After it, messages arrive before timeouts, as on a network that has
    /// settled down.
    chaos_until: usize,
    /// Validators whose clocks reject every value proposed in round 0.
    slow_clocks: Vec<usize>,
    /// How many times a validator began a round while locked on a value.
    locks_carried: usize,
    /// Proposals honest validators have made, with their rounds.
    honest_proposals: Vec<(u32, Val)>,
    /// Proposals the liar has already prevoted for.
    backed: BTreeSet<(u32, Hash32)>,
    /// Precommits the liar has already slipped to one validator alone.
    tipped: BTreeSet<(u32, Hash32)>,
    /// The test moves every message by hand; the liar does nothing unasked.
    scripted: bool,
}

impl Sim {
    fn new(roles: &[Role], seed: u64) -> Sim {
        let weights = vec![1u64; roles.len()];
        let mut id = [0u8; 32];
        id[..8].copy_from_slice(&seed.to_le_bytes());
        Sim {
            nodes: (0..roles.len()).map(|i| Agreement::new(i as u16, weights.clone(), Hash32(id))).collect(),
            roles: roles.to_vec(),
            queue: Vec::new(),
            timeouts: Vec::new(),
            decided: vec![None; roles.len()],
            proposed: Vec::new(),
            rng: seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1,
            steps: 0,
            chaos_until: 400,
            slow_clocks: Vec::new(),
            locks_carried: 0,
            honest_proposals: Vec::new(),
            backed: BTreeSet::new(),
            tipped: BTreeSet::new(),
            scripted: false,
        }
    }

    fn random(&mut self, below: usize) -> usize {
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 7;
        self.rng ^= self.rng << 17;
        (self.rng % below.max(1) as u64) as usize
    }

    fn honest(&self) -> impl Iterator<Item = usize> + '_ {
        (0..self.nodes.len()).filter(|i| self.roles[*i] == Role::Honest)
    }

    fn carry_out(&mut self, node: usize, outputs: Vec<Output<Val>>) {
        for output in outputs {
            match output {
                Output::Broadcast(message) => {
                    match &message {
                        Message::Proposal { round, value, .. } => {
                            if !self.honest_proposals.contains(&(*round, value.clone())) {
                                self.honest_proposals.push((*round, value.clone()));
                            }
                        }
                        // An honest validator never votes for a value that
                        // breaks the rules, whatever anyone else does.
                        Message::Prevote { id, .. } | Message::Precommit { id, .. } => {
                            assert_ne!(*id, Some(RULE_BREAKER.id()), "validator {node} voted for a rule-breaking value");
                        }
                        Message::Commit { .. } => {}
                    }
                    for to in 0..self.nodes.len() {
                        if to != node && self.roles[to] == Role::Honest {
                            self.queue.push((to, node, message.clone()));
                        }
                    }
                }
                Output::Schedule(timeout) => self.timeouts.push((node, timeout)),
                Output::NeedValue { round } => {
                    // Each proposal an honest validator makes up is a new, valid value.
                    let value = Val(1 + self.proposed.len() as u64);
                    self.proposed.push(value.clone());
                    let outputs = self.nodes[node].propose(round, value);
                    self.carry_out(node, outputs);
                }
                Output::Decided { value, round, .. } => self.decided[node] = Some((value, round)),
            }
        }
    }

    fn start(&mut self) {
        for node in self.honest().collect::<Vec<_>>() {
            let outputs = self.nodes[node].start();
            self.carry_out(node, outputs);
        }
        if !self.scripted
            && let Some(liar) = (0..self.roles.len()).find(|i| self.roles[*i] == Role::Lying)
        {
            self.prepare_lies(liar);
        }
    }

    /// Delivers every waiting message `wanted` picks, and any it picks
    /// among those sent in response.
    fn deliver_where(&mut self, wanted: impl Fn(usize, usize, &Message<Val>) -> bool) {
        while let Some(index) = self.queue.iter().position(|(to, from, message)| wanted(*to, *from, message)) {
            let (to, from, message) = self.queue.remove(index);
            self.deliver(to, from, message);
        }
    }

    fn fire(&mut self, node: usize, timeout: Timeout) {
        let index = self.timeouts.iter().position(|pending| *pending == (node, timeout));
        self.timeouts.remove(index.unwrap_or_else(|| panic!("validator {node} is not waiting on {timeout:?}")));
        let outputs = self.nodes[node].on_timeout(timeout);
        self.carry_out(node, outputs);
    }

    fn say(&mut self, liar: usize, to: &[usize], message: Message<Val>) {
        for to in to {
            self.queue.push((*to, liar, message.clone()));
        }
    }

    /// What the liar has ready for each round in which it is its turn to
    /// propose. In turn it tries the three things a dishonest proposer can:
    /// two different values for two halves of the network, with votes for
    /// each sent to the half that saw it; a value that breaks the rules;
    /// and a new value passed off as one that earlier prevotes stand behind.
    fn prepare_lies(&mut self, liar: usize) {
        let honest: Vec<usize> = self.honest().collect();
        let (left, right) = honest.split_at(honest.len() / 2);
        let (left, right) = (left.to_vec(), right.to_vec());
        let mut everyone: Vec<u16> = (0..self.nodes.len() as u16).collect();
        everyone.extend([liar as u16; 5]);
        let mut turn = 0;
        for round in 0..12u32 {
            if self.nodes[honest[0]].proposer(round) as usize != liar {
                continue;
            }
            let (one, other) = (Val(800_000 + 2 * round as u64), Val(800_001 + 2 * round as u64));
            let vote = |value: &Val| {
                let id = Some(value.id());
                [Message::Prevote { round, id }, Message::Precommit { round, id }]
            };
            let proposal = |value: &Val, valid_round: Option<u32>, backers: &[u16]| Message::Proposal {
                round,
                value: value.clone(),
                valid_round,
                backers: backers.to_vec(),
            };
            match turn % 3 {
                0 => {
                    for (half, value) in [(&left, &one), (&right, &other)] {
                        self.say(liar, half, proposal(value, None, &[]));
                        for message in vote(value) {
                            self.say(liar, half, message);
                        }
                    }
                }
                1 => {
                    self.say(liar, &honest, proposal(&RULE_BREAKER, None, &[]));
                    for message in vote(&RULE_BREAKER) {
                        self.say(liar, &honest, message);
                    }
                }
                _ if round > 0 => {
                    self.say(liar, &honest, proposal(&one, Some(round - 1), &everyone));
                    for message in vote(&one) {
                        self.say(liar, &honest, message);
                    }
                }
                _ => {}
            }
            turn += 1;
        }
    }

    /// The liar's standing tactics, which it tries at every step.
    ///
    /// It prevotes for whatever honest validators propose, so that values
    /// gather enough prevotes to lock validators on them. And whenever a
    /// value is one precommit short of a decision, it gives that precommit
    /// to a single validator and to nobody else. That validator decides
    /// alone, while word of its decision is still held up in the network,
    /// and the liar goes on to back other values with the rest. If the
    /// locks were not respected, the rest would then decide differently.
    fn scheme(&mut self, liar: usize) {
        let honest: Vec<usize> = self.honest().collect();
        for (round, value) in self.honest_proposals.clone() {
            let id = Some(value.id());
            if self.backed.insert((round, value.id())) {
                self.say(liar, &honest, Message::Prevote { round, id });
            }
            let backers: Vec<usize> = honest
                .iter()
                .copied()
                .filter(|node| self.nodes[*node].voted(round, true) == Some(id))
                .collect();
            let (with_liar, alone) = (3 * (backers.len() + 1) > 2 * self.nodes.len(), 3 * backers.len() > 2 * self.nodes.len());
            // It waits until the round is over for everyone else, so that
            // nobody but its chosen validator can decide in it.
            let round_over = honest
                .iter()
                .all(|node| self.nodes[*node].voted(round, true).is_some() || self.nodes[*node].round() > round);
            if with_liar && !alone && round_over && self.tipped.insert((round, value.id())) {
                self.say(liar, &backers[..1], Message::Precommit { round, id });
            }
        }
    }

    fn deliver(&mut self, to: usize, from: usize, message: Message<Val>) {
        let outputs = match message {
            Message::Proposal { round, value, valid_round, backers } => {
                // Only the round's own proposer may propose in it.
                if self.nodes[to].proposer(round) as usize != from {
                    return;
                }
                let timely = !(round == 0 && self.slow_clocks.contains(&to));
                let verdict = Verdict { valid: follows_the_rules(&value), timely };
                let backers = self.genuine(&backers, valid_round, &value, false);
                self.nodes[to].on_proposal(round, value, valid_round, &backers, verdict)
            }
            Message::Commit { round, value, backers } => {
                let verdict = Verdict { valid: follows_the_rules(&value), timely: true };
                let backers = self.genuine(&backers, Some(round), &value, true);
                self.nodes[to].on_commit(round, value, &backers, verdict)
            }
            Message::Prevote { round, id } => self.nodes[to].on_prevote(from as u16, round, id),
            Message::Precommit { round, id } => self.nodes[to].on_precommit(from as u16, round, id),
        };
        let before = self.nodes[to].round();
        self.carry_out(to, outputs);
        if self.nodes[to].round() > before && self.nodes[to].locked().is_some() {
            self.locks_carried += 1;
        }
    }

    /// Stands in for checking signatures: of the validators a message names
    /// as backers, keeps those that really cast that vote. An honest
    /// validator's vote is whatever its own record shows. The liar signs
    /// anything, so its name always stands.
    fn genuine(&self, backers: &[u16], round: Option<u32>, value: &Val, precommit: bool) -> Vec<u16> {
        let Some(round) = round else { return Vec::new() };
        let id = Some(value.id());
        backers
            .iter()
            .copied()
            .filter(|backer| match self.roles.get(*backer as usize) {
                Some(Role::Lying) => true,
                Some(Role::Honest) => self.nodes[*backer as usize].voted(round, precommit) == Some(id),
                _ => false,
            })
            .collect()
    }

    /// The liar says something made up to a random part of the network:
    /// proposals where it is its turn, votes for values real and invented,
    /// and decisions it claims others backed.
    fn lie(&mut self, liar: usize) {
        let round = self.random(4) as u32;
        let invented = Val([900_000, 900_001, 5_000_000][self.random(3)]);
        let value = match self.random(3) {
            0 if !self.proposed.is_empty() => {
                let pick = self.random(self.proposed.len());
                self.proposed[pick].clone()
            }
            _ => invented,
        };
        let id = [None, Some(value.id())][self.random(2)];
        // Whatever backers it names, only real votes survive the check. It
        // names itself several times over, in case that is counted twice.
        let mut everyone: Vec<u16> = (0..self.nodes.len() as u16).collect();
        everyone.extend([liar as u16; 5]);
        let message = match self.random(4) {
            0 => {
                let valid_round = [None, Some(self.random(3) as u32)][self.random(2)];
                Message::Proposal { round, value, valid_round, backers: everyone }
            }
            1 => Message::Prevote { round, id },
            2 => Message::Precommit { round, id },
            _ => Message::Commit { round, value, backers: everyone },
        };
        for to in self.honest().collect::<Vec<_>>() {
            if self.random(2) == 0 {
                self.queue.push((to, liar, message.clone()));
            }
        }
    }

    /// Runs until every honest validator has decided, nothing more can
    /// happen, or the step limit is reached.
    fn run(&mut self, limit: usize) {
        while self.steps < limit && self.honest().any(|i| self.decided[i].is_none()) {
            self.steps += 1;
            let chaos = self.steps < self.chaos_until;
            if chaos && let Some(liar) = (0..self.roles.len()).find(|i| self.roles[*i] == Role::Lying) {
                self.scheme(liar);
                if self.random(3) == 0 {
                    self.lie(liar);
                }
            }
            let early_timeout = chaos && !self.timeouts.is_empty() && self.random(6) == 0;
            // While the network misbehaves, news of a decision is held up.
            let deliverable: Vec<usize> = (0..self.queue.len())
                .filter(|index| !(chaos && matches!(self.queue[*index].2, Message::Commit { .. })))
                .collect();
            if !deliverable.is_empty() && !early_timeout {
                let pick = deliverable[self.random(deliverable.len())];
                let (to, from, message) = self.queue.swap_remove(pick);
                self.deliver(to, from, message);
            } else if !self.timeouts.is_empty() {
                let pick = self.random(self.timeouts.len());
                let (node, timeout) = self.timeouts.swap_remove(pick);
                let before = self.nodes[node].round();
                let outputs = self.nodes[node].on_timeout(timeout);
                self.carry_out(node, outputs);
                if self.nodes[node].round() > before && self.nodes[node].locked().is_some() {
                    self.locks_carried += 1;
                }
            } else if !chaos {
                return;
            }
        }
    }

    /// The one value every honest validator decided. Panics if any two of
    /// them differ, or if what they decided breaks the rules.
    fn agreed(&self, context: &str) -> Option<(Val, u32)> {
        let mut decisions = self.honest().filter_map(|i| self.decided[i].clone());
        let first = decisions.next()?;
        for other in decisions {
            assert_eq!(other.0, first.0, "{context}: honest validators decided differently");
        }
        assert!(follows_the_rules(&first.0), "{context}: a value that breaks the rules was decided");
        Some(first)
    }

    fn all_decided(&self) -> bool {
        self.honest().all(|i| self.decided[i].is_some())
    }
}

fn roles(honest: usize, silent: usize, lying: usize) -> Vec<Role> {
    let mut roles = vec![Role::Honest; honest];
    roles.extend(vec![Role::Silent; silent]);
    roles.extend(vec![Role::Lying; lying]);
    roles
}

/// Moves the faulty validators around, so that over many seeds they sit in
/// every position, including first proposer.
fn rotated(mut roles: Vec<Role>, by: u64) -> Vec<Role> {
    let by = by as usize % roles.len();
    roles.rotate_left(by);
    roles
}

#[test]
fn honest_validators_decide_one_valid_value() {
    for size in [1, 4, 7] {
        for seed in 0..300 {
            let mut sim = Sim::new(&roles(size, 0, 0), seed);
            sim.start();
            sim.run(20_000);
            let context = format!("{size} validators, seed {seed}");
            assert!(sim.all_decided(), "{context}: not everyone decided");
            sim.agreed(&context).unwrap();
        }
    }
}

#[test]
fn on_a_quiet_network_the_first_round_decides() {
    for seed in 0..50 {
        let mut sim = Sim::new(&roles(4, 0, 0), seed);
        sim.chaos_until = 0;
        sim.start();
        sim.run(20_000);
        let (value, round) = sim.agreed("quiet").unwrap();
        assert_eq!((value, round), (Val(1), 0), "seed {seed}");
        assert!(sim.all_decided());
    }
}

#[test]
fn silent_validators_within_the_limit_cannot_stop_a_decision() {
    // One of four may be silent, and two of seven.
    for (honest, silent) in [(3, 1), (5, 2)] {
        let mut later_rounds = 0;
        for seed in 0..300 {
            let mut sim = Sim::new(&rotated(roles(honest, silent, 0), seed), seed);
            sim.start();
            sim.run(40_000);
            let context = format!("{honest} honest and {silent} silent, seed {seed}");
            assert!(sim.all_decided(), "{context}: not everyone decided");
            later_rounds += (sim.agreed(&context).unwrap().1 > 0) as usize;
        }
        assert!(later_rounds > 20, "the silent validators were never the proposer");
    }
}

#[test]
fn too_many_silent_validators_stop_it_without_a_wrong_decision() {
    for seed in 0..100 {
        let mut sim = Sim::new(&rotated(roles(2, 2, 0), seed), seed);
        sim.start();
        sim.run(5_000);
        assert!(sim.honest().all(|i| sim.decided[i].is_none()), "seed {seed}: decided without two thirds");
    }
}

#[test]
fn a_lying_validator_cannot_split_or_spoil_the_decision() {
    // The liar proposes different values to different validators, proposes
    // values that break the rules, and votes every way at once.
    let (mut later_rounds, mut locks_carried, mut tipped) = (0, 0, 0);
    for (honest, silent, lying) in [(3, 0, 1), (6, 0, 1), (5, 1, 1)] {
        for seed in 0..400 {
            let mut sim = Sim::new(&rotated(roles(honest, silent, lying), seed), seed);
            sim.chaos_until = 900;
            sim.start();
            sim.run(60_000);
            let context = format!("{honest} honest, {silent} silent, {lying} lying, seed {seed}");
            assert!(sim.all_decided(), "{context}: not everyone decided");
            // The liar may get its own value decided when it is its turn to
            // propose, as long as the value follows the rules, which
            // `agreed` checks.
            let (_, round) = sim.agreed(&context).unwrap();
            later_rounds += (round > 0) as usize;
            locks_carried += sim.locks_carried;
            tipped += sim.tipped.len();
        }
    }
    // The runs must have reached the hard cases: rounds beyond the first,
    // validators carrying a lock from one round into the next, and the liar
    // handing one validator alone the last precommit a decision needed.
    assert!(later_rounds > 250, "only {later_rounds} runs went past the first round");
    assert!(locks_carried > 100, "locks were carried into a later round only {locks_carried} times");
    assert!(tipped > 100, "the liar tipped a validator into deciding alone only {tipped} times");
}

#[test]
fn a_value_that_some_clocks_reject_gives_way_to_a_later_round() {
    for seed in 0..100 {
        let mut sim = Sim::new(&roles(4, 0, 0), seed);
        sim.chaos_until = 0;
        // Two of four find the first proposal's time too far from their own
        // clocks. That is too many for it to pass, and too few to stop the
        // network: the next round proposes afresh and everyone accepts. The
        // proposer is not one of the two: it set the time by its own clock.
        let proposer = sim.nodes[0].proposer(0) as usize;
        sim.slow_clocks = (0..4).filter(|node| *node != proposer).take(2).collect();
        sim.start();
        sim.run(40_000);
        assert!(sim.all_decided(), "seed {seed}");
        let (_, round) = sim.agreed("clocks").unwrap();
        assert!(round > 0, "seed {seed}: decided in round 0 against two clocks");
    }
}

#[test]
fn the_turn_to_propose_follows_stake() {
    let weights = vec![1u64, 1, 2, 4];
    let agreement: Agreement<Val> = Agreement::new(0, weights.clone(), Hash32([3; 32]));
    let mut turns = [0u32; 4];
    for round in 0..8000 {
        turns[agreement.proposer(round) as usize] += 1;
    }
    for (index, weight) in weights.iter().enumerate() {
        let expected = 1000 * *weight as u32;
        assert!(turns[index].abs_diff(expected) < 150, "validator {index} had {} turns", turns[index]);
    }
    // Every validator computes the same turn, and a different seed gives a
    // different order.
    let other: Agreement<Val> = Agreement::new(3, weights.clone(), Hash32([3; 32]));
    assert!((0..100).all(|round| agreement.proposer(round) == other.proposer(round)));
    let reseeded: Agreement<Val> = Agreement::new(0, weights, Hash32([4; 32]));
    assert!((0..100).any(|round| agreement.proposer(round) != reseeded.proposer(round)));
}

#[test]
fn a_validator_locked_later_refuses_an_older_value_put_forward_again() {
    // Four validators: `first` proposes in round 1, `old` in round 2, the
    // liar in none of the first three rounds, and `other` is the fourth.
    let seed_bytes = |seed: u64| {
        let mut id = [0u8; 32];
        id[..8].copy_from_slice(&seed.to_le_bytes());
        Hash32(id)
    };
    let (seed, turns) = (0u64..)
        .find_map(|seed| {
            let probe: Agreement<Val> = Agreement::new(0, vec![1; 4], seed_bytes(seed));
            let turns = [probe.proposer(0), probe.proposer(1), probe.proposer(2)];
            (turns[1] != turns[2]).then_some((seed, turns))
        })
        .unwrap();
    let (first, old) = (turns[1] as usize, turns[2] as usize);
    let liar = (0..4).find(|i| !turns.contains(&(*i as u16))).unwrap();
    let other = (0..4).find(|i| ![first, old, liar].contains(i)).unwrap();
    let mut roles = vec![Role::Honest; 4];
    roles[liar] = Role::Lying;
    let mut sim = Sim::new(&roles, seed);
    sim.chaos_until = 0;
    sim.scripted = true;
    sim.start();
    let prevote = |round, value: Option<&Val>| Message::Prevote { round, id: value.map(Value::id) };
    let precommit = |round, value: Option<&Val>| Message::Precommit { round, id: value.map(Value::id) };

    // Round 0. Everyone prevotes for the proposal, the early value. The
    // liar tells `old` it does too and tells the others it does not, and
    // `old`'s own prevote is held up. So only `old` sees two thirds behind
    // the early value and locks on it. The other two give up on the round.
    sim.deliver_where(|_, _, m| matches!(m, Message::Proposal { round: 0, .. }));
    let early = Val(1);
    sim.say(liar, &[old], prevote(0, Some(&early)));
    sim.say(liar, &[first, other], prevote(0, None));
    sim.deliver_where(|to, _, m| to == old && matches!(m, Message::Prevote { round: 0, .. }));
    sim.deliver_where(|to, from, m| to != old && from != old && matches!(m, Message::Prevote { round: 0, .. }));
    assert_eq!(sim.nodes[old].locked().map(|(round, value)| (round, value.clone())), Some((0, early.clone())));
    sim.fire(first, Timeout::Prevote(0));
    sim.fire(other, Timeout::Prevote(0));
    sim.deliver_where(|_, _, m| matches!(m, Message::Precommit { round: 0, .. }));
    for node in [first, other, old] {
        sim.fire(node, Timeout::Precommit(0));
    }

    // Round 1. `first` proposes a new value, the later one. `old` is locked
    // and refuses it. With the liar's prevote the other two lock on it, and
    // the liar gives its precommit to `first` alone, which decides. News of
    // that decision is held up.
    sim.deliver_where(|_, _, m| matches!(m, Message::Proposal { round: 1, .. }));
    let later = Val(2);
    assert_eq!(sim.nodes[old].voted(1, false), Some(None), "locked on the early value, it votes for nothing");
    sim.say(liar, &[first, other], prevote(1, Some(&later)));
    sim.deliver_where(|_, _, m| matches!(m, Message::Prevote { round: 1, .. }));
    sim.fire(old, Timeout::Prevote(1));
    sim.say(liar, &[first], precommit(1, Some(&later)));
    sim.deliver_where(|_, _, m| matches!(m, Message::Precommit { round: 1, .. }));
    assert_eq!(sim.decided[first], Some((later.clone(), 1)));
    assert_eq!(sim.nodes[other].locked().map(|(round, value)| (round, value.clone())), Some((1, later.clone())));
    sim.fire(other, Timeout::Precommit(1));
    sim.fire(old, Timeout::Precommit(1));

    // Round 2. `old` puts the early value forward again, with genuine proof
    // that two thirds prevoted for it in round 0. `other` locked on the
    // later value after that, so it must refuse, however much the liar
    // backs the early value now. If it gave way, it and `old` would decide
    // the early value while `first` has already decided the later one.
    sim.deliver_where(|_, _, m| matches!(m, Message::Proposal { round: 2, .. }));
    assert_eq!(sim.nodes[other].voted(2, false), Some(None), "locked more recently, it must refuse");
    sim.say(liar, &[other, old], prevote(2, Some(&early)));
    sim.say(liar, &[other, old], precommit(2, Some(&early)));
    sim.deliver_where(|_, _, m| matches!(m, Message::Prevote { round: 2, .. } | Message::Precommit { round: 2, .. }));
    assert!(sim.decided[other].is_none() && sim.decided[old].is_none());

    // The network settles and the news gets through. Everyone ends on the
    // value that was decided first.
    sim.run(20_000);
    assert!(sim.all_decided());
    assert_eq!(sim.agreed("the scripted attack").unwrap().0, later);
}
