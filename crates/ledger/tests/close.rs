//! Whole epochs closed in memory: real signatures, validators that agree in
//! rounds, messages in any order, a validator that is absent, and a
//! proposer that cheats.

use orien_crypto::keytree::KeyTree;
use orien_crypto::{Hash32, Kdf, merkle, ots, prf};
use orien_ledger::agree::{Agreement, Message, Output, Timeout, Verdict};
use orien_ledger::close::{self, Checkpoint, Gathered, Invalid, Pin, Statement};
use orien_ledger::{
    AccountId, Allocation, Certificate, Committee, Genesis, Kind, Order, Reject, SPARE_HEIGHT, Signature,
    SignedOrder, Signer, State, ValidatorInfo, Vote, address, confirmation_item,
};
use std::collections::{BTreeMap, BTreeSet};

struct User {
    seed: [u8; 32],
    spares: KeyTree,
}

impl User {
    fn new(n: u8) -> User {
        let seed = [n; 32];
        User { seed, spares: KeyTree::generate(prf(&seed, Kdf::WalletSpare, 0), SPARE_HEIGHT) }
    }

    fn key(&self, index: u64) -> ots::SecretKey {
        ots::SecretKey::from_seed(prf(&self.seed, Kdf::WalletKey, index))
    }

    fn id(&self) -> AccountId {
        address(&self.key(0).commitment(), &self.spares.root())
    }

    fn unsigned(&self, seq: u64, to: &User, amount: u64) -> Order {
        Order {
            kind: Kind::Payment,
            from: self.id(),
            seq,
            to: to.id(),
            amount,
            max_fee: 0,
            next_key: self.key(seq + 1).commitment(),
            spare: self.spares.root(),
        }
    }

    fn pay(&self, world: &World, seq: u64, to: &User, amount: u64) -> SignedOrder {
        let order = self.unsigned(seq, to, amount);
        let digest = order.digest(&world.network);
        SignedOrder { signature: Signature::Key(self.key(seq).sign(digest.as_bytes())), order }
    }

    /// A payment signed with spare key number `leaf`.
    fn pay_with_spare(&self, world: &World, seq: u64, leaf: u32, to: &User, amount: u64) -> SignedOrder {
        let order = self.unsigned(seq, to, amount);
        let digest = order.digest(&world.network);
        SignedOrder { signature: Signature::Spare(self.spares.sign_with_leaf(leaf, digest.as_bytes())), order }
    }
}

/// How a dishonest proposer cheats.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Trick {
    /// Leaves out a payment that a validator it names had applied.
    DropPayment,
    /// Claims a state the payments do not lead to.
    WrongState,
    /// Names too few statements.
    FewStatements,
    /// Names a statement another validator never made.
    ForgedStatement,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Role {
    Honest,
    /// Takes no part in the close, as if it had crashed or been cut off.
    Absent,
    /// Proposes a bad checkpoint whenever it is its turn, and never votes.
    Cheat(Trick),
}

struct Validator {
    role: Role,
    tree: KeyTree,
    leaf: u32,
    /// The state the epoch began in.
    start: State,
    state: State,
    /// Payments applied this epoch, in the order they were applied.
    log: Vec<Certificate>,
    /// Orders voted for whose payments it has not applied.
    promises: BTreeMap<AccountId, SignedOrder>,
    evidence: Vec<(SignedOrder, SignedOrder)>,
    /// Payments it signed a confirmation for.
    confirmed: BTreeSet<Hash32>,
    statement: Option<Statement>,
}

/// What the validators taking part held at the moment they stopped: the
/// signed statements, the logs those pin, and every payment in them. It is
/// what a proposer builds from and what everyone judges against.
#[derive(Default)]
struct Stopped {
    statements: BTreeMap<u16, Statement>,
    logs: BTreeMap<u16, Vec<Hash32>>,
    payments: Vec<Certificate>,
}

struct World {
    committee: Committee,
    network: Hash32,
    /// The latest checkpoint's digest, or the network id before the first.
    previous: Hash32,
    validators: Vec<Validator>,
    supply: i128,
    /// The last close: its checkpoint and the payments it holds.
    closed: Option<(Checkpoint, Vec<Certificate>)>,
    /// How many closes had a cheat as first proposer.
    cheat_went_first: usize,
    /// Why honest validators refused what a cheat proposed.
    refusals: Vec<Invalid>,
    stopped: Stopped,
}

impl World {
    fn new(validators: usize, funded: &[(&User, u64)]) -> World {
        let trees: Vec<KeyTree> = (0..validators).map(|i| KeyTree::generate([100 + i as u8; 32], 7)).collect();
        let genesis = Genesis {
            network: "close-test".into(),
            validators: trees
                .iter()
                .enumerate()
                .map(|(i, tree)| ValidatorInfo {
                    name: format!("v{i}"),
                    root: tree.root(),
                    height: 7,
                    stake: 1,
                    address: String::new(),
                })
                .collect(),
            allocations: funded.iter().map(|(user, amount)| Allocation { account: user.id(), amount: *amount }).collect(),
        };
        genesis.validate().unwrap();
        World {
            committee: Committee::new(&genesis),
            network: genesis.id(),
            previous: genesis.id(),
            supply: genesis.supply() as i128,
            validators: trees
                .into_iter()
                .map(|tree| Validator {
                    role: Role::Honest,
                    tree,
                    leaf: 0,
                    start: State::from_genesis(&genesis),
                    state: State::from_genesis(&genesis),
                    log: Vec::new(),
                    promises: BTreeMap::new(),
                    evidence: Vec::new(),
                    confirmed: BTreeSet::new(),
                    statement: None,
                })
                .collect(),
            closed: None,
            cheat_went_first: 0,
            refusals: Vec::new(),
            stopped: Stopped::default(),
        }
    }

    fn digest(&self, order: &Order) -> Hash32 {
        order.digest(&self.network)
    }

    /// Validator `v` signs a batch holding this one item.
    fn sign(&mut self, v: usize, item: Hash32) -> Vote {
        let batch = merkle::Tree::build(&[item]);
        let message = self.committee.batch_message(&batch.root());
        let validator = &mut self.validators[v];
        let signature = validator.tree.sign_with_leaf(validator.leaf, message.as_bytes());
        validator.leaf += 1;
        Vote { validator: v as u16, batch_path: batch.path(0), signature }
    }

    fn vote(&mut self, v: usize, signed: &SignedOrder) -> Result<Vote, Reject> {
        let digest = self.digest(&signed.order);
        let signed_by = signed.signed_by(&self.network).ok_or(Reject::BadSignature)?;
        let validator = &mut self.validators[v];
        match validator.state.check_order(&signed.order, digest, &signed_by) {
            Ok(_) => {
                validator.promises.insert(signed.order.from, signed.clone());
            }
            // A second order with a good signature under a number already
            // promised: keep both, as proof the owner signed twice.
            Err(Reject::Conflict { voted }) => {
                if let Some(promise) = validator.promises.get(&signed.order.from) {
                    validator.evidence.push((promise.clone(), signed.clone()));
                }
                return Err(Reject::Conflict { voted });
            }
            Err(reject) => return Err(reject),
        }
        Ok(self.sign(v, digest))
    }

    fn certify(&mut self, signed: &SignedOrder, voters: &[usize]) -> Certificate {
        let votes = voters.iter().map(|v| self.vote(*v, signed).unwrap()).collect();
        Certificate::new(self.committee.epoch(), signed, votes)
    }

    /// Validator `v` applies a certificate and confirms that it has.
    fn apply(&mut self, v: usize, certificate: &Certificate) {
        self.committee.verify_certificate(certificate).unwrap();
        let digest = self.digest(&certificate.order);
        let validator = &mut self.validators[v];
        assert!(validator.statement.is_none(), "a validator that has stopped applies nothing");
        validator.state.apply(&certificate.order).unwrap();
        validator.log.push(certificate.clone());
        validator.promises.remove(&certificate.order.from);
        validator.confirmed.insert(digest);
        self.sign(v, confirmation_item(&digest));
    }

    /// A payment is final once validators holding more than two thirds of
    /// the stake have confirmed it.
    fn is_final(&self, certificate: &Certificate) -> bool {
        let digest = self.digest(&certificate.order);
        let stake = (0..self.validators.len())
            .filter(|v| self.validators[*v].confirmed.contains(&digest))
            .map(|v| self.committee.stake_of(v as u16))
            .sum();
        self.committee.has_quorum(stake)
    }

    fn log_digests(&self, v: usize) -> Vec<Hash32> {
        self.validators[v].log.iter().map(|payment| self.digest(&payment.order)).collect()
    }

    /// Validator `v` stops work on the epoch and makes its statement.
    fn stop(&mut self, v: usize) {
        if self.validators[v].statement.is_none() {
            let statement = Statement {
                epoch: self.committee.epoch(),
                validator: v as u16,
                log: Pin::of(self.committee.epoch(), &self.log_digests(v)),
                evidence: self.validators[v].evidence.clone(),
            };
            self.validators[v].statement = Some(statement);
        }
    }

    fn taking_part(&self) -> Vec<usize> {
        (0..self.validators.len()).filter(|v| self.validators[*v].role != Role::Absent).collect()
    }

    /// Everyone taking part stops, and what they hold is gathered up.
    fn stop_all(&mut self) {
        let taking_part = self.taking_part();
        for v in &taking_part {
            self.stop(*v);
        }
        let mut stopped = Stopped::default();
        let mut seen = BTreeSet::new();
        for v in taking_part {
            stopped.statements.insert(v as u16, self.validators[v].statement.clone().unwrap());
            stopped.logs.insert(v as u16, self.log_digests(v));
            for payment in &self.validators[v].log {
                if seen.insert(self.digest(&payment.order)) {
                    stopped.payments.push(payment.clone());
                }
            }
        }
        self.stopped = stopped;
    }

    /// The checkpoint an honest proposer builds from everything it can fetch.
    fn honest_proposal(&self, proposer: usize) -> (Checkpoint, Vec<Certificate>) {
        let merged: Vec<&Statement> = self.stopped.statements.values().collect();
        let start = &self.validators[proposer].start;
        let payments = self.stopped.payments.clone();
        let (checkpoint, _) =
            close::propose(&self.committee, self.previous, 1_000, start, &payments, &merged).unwrap();
        (checkpoint, payments)
    }

    /// The checkpoint a cheating proposer puts forward.
    fn cheating_proposal(&self, proposer: usize, trick: Trick) -> (Checkpoint, Vec<Certificate>) {
        let (mut checkpoint, mut payments) = self.honest_proposal(proposer);
        let start = &self.validators[proposer].start;
        let statements = &self.stopped.statements;
        match trick {
            Trick::DropPayment => {
                // Drop the last payment, which no other payment builds on.
                let mut fewer = payments.clone();
                fewer.sort_by_key(|payment| (payment.order.from, payment.order.seq));
                fewer.pop();
                let merged: Vec<&Statement> = statements.values().collect();
                if let Ok((without, _)) = close::propose(&self.committee, self.previous, 1_000, start, &fewer, &merged) {
                    (checkpoint, payments) = (without, fewer);
                }
            }
            Trick::WrongState => checkpoint.state.0[0] ^= 1,
            Trick::FewStatements => checkpoint.merged.truncate(2),
            Trick::ForgedStatement => {
                let victim = self.taking_part().into_iter().find(|v| *v != proposer).unwrap();
                let forged = Statement {
                    epoch: self.committee.epoch(),
                    validator: victim as u16,
                    log: Pin::start(self.committee.epoch()),
                    evidence: Vec::new(),
                };
                let entry = checkpoint.merged.iter_mut().find(|(validator, _)| *validator == victim as u16).unwrap();
                entry.1 = forged.digest(&self.network);
            }
        }
        (checkpoint, payments)
    }

    /// What validator `v` makes of a proposed checkpoint, given the payments
    /// its proposer offered for it.
    fn judge(&self, v: usize, checkpoint: &Checkpoint, payments: Option<&Vec<Certificate>>) -> Result<State, Invalid> {
        let payments = payments.ok_or(Invalid::WrongCount)?;
        let gathered = Gathered { statements: &self.stopped.statements, logs: &self.stopped.logs, payments };
        close::validate(&self.committee, &self.previous, &self.validators[v].start, checkpoint, &gathered)
    }

    /// Validator `v` takes on a decided checkpoint. Whatever it had applied
    /// that the checkpoint leaves out is undone, and the orders behind those
    /// payments go back to being promises it keeps.
    fn adopt(&mut self, v: usize, end: &State, payments: &[Certificate]) {
        let network = self.network;
        let held: BTreeSet<Hash32> = payments.iter().map(|payment| payment.order.digest(&network)).collect();
        let validator = &mut self.validators[v];
        for undone in validator.log.drain(..) {
            if !held.contains(&undone.order.digest(&network)) {
                validator.promises.insert(undone.order.from, undone.signed());
            }
        }
        validator.start = end.clone();
        validator.state = end.clone();
        // A promise stands while its order could still be voted: the number
        // has not moved on, and the key that signed it is still the one the
        // account requires.
        validator.promises.retain(|_, promise| {
            let current = promise.order.seq == end.seq(&promise.order.from);
            current && promise.signed_by(&network).is_some_and(|by| end.authorizes(&promise.order, &by))
        });
        for promise in validator.promises.values() {
            validator.state.restore_lock(promise.order.from, promise.order.seq, promise.order.digest(&network));
        }
        // Evidence is kept for as long as it still bears on the account. A
        // close this validator missed cannot have acted on it.
        validator.evidence.retain(|(one, _)| {
            let current = one.order.seq == end.seq(&one.order.from);
            current && one.signed_by(&network).is_some_and(|by| end.authorizes(&one.order, &by))
        });
        validator.confirmed.clear();
        validator.statement = None;
    }

    /// Closes the epoch: everyone taking part stops, they agree on a
    /// checkpoint with messages delivered in an order `seed` picks, and
    /// each takes it on. Returns the checkpoint.
    fn close(&mut self, seed: u64) -> Checkpoint {
        let taking_part = self.taking_part();
        self.stop_all();
        let honest: Vec<usize> =
            taking_part.iter().copied().filter(|v| self.validators[*v].role == Role::Honest).collect();
        let weights: Vec<u64> = (0..self.validators.len()).map(|v| self.committee.stake_of(v as u16)).collect();
        let mut machines: BTreeMap<usize, Agreement<Checkpoint>> =
            honest.iter().map(|v| (*v, Agreement::new(*v as u16, weights.clone(), self.previous))).collect();
        // The payments each proposed checkpoint stands on, as its proposer offers them.
        let mut offered: BTreeMap<Hash32, Vec<Certificate>> = BTreeMap::new();
        let mut queue: Vec<(usize, usize, Message<Checkpoint>)> = Vec::new();
        let mut timeouts: Vec<(usize, Timeout)> = Vec::new();
        let mut decided: BTreeMap<usize, Checkpoint> = BTreeMap::new();
        let mut rng = seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1;
        let mut random = move |below: usize| {
            rng ^= rng << 13;
            rng ^= rng >> 7;
            rng ^= rng << 17;
            (rng % below.max(1) as u64) as usize
        };

        // A cheat has its bad checkpoint ready for each of its turns.
        for round in 0..12 {
            let proposer = machines[&honest[0]].proposer(round) as usize;
            if let Role::Cheat(trick) = self.validators[proposer].role {
                self.cheat_went_first += (round == 0) as usize;
                let (value, payments) = self.cheating_proposal(proposer, trick);
                let refusal = self.judge(honest[0], &value, Some(&payments));
                self.refusals.push(refusal.err().expect("the cheat's checkpoint should not pass"));
                offered.insert(value.digest(), payments);
                for to in &honest {
                    let message = Message::Proposal { round, value: value.clone(), valid_round: None, backers: Vec::new() };
                    queue.push((*to, proposer, message));
                }
            }
        }

        let mut work: Vec<(usize, Vec<Output<Checkpoint>>)> =
            honest.iter().map(|v| (*v, machines.get_mut(v).unwrap().start())).collect();
        for _ in 0..200_000 {
            while let Some((v, outputs)) = work.pop() {
                for output in outputs {
                    match output {
                        Output::Broadcast(message) => {
                            queue.extend(honest.iter().filter(|to| **to != v).map(|to| (*to, v, message.clone())));
                        }
                        Output::Schedule(timeout) => timeouts.push((v, timeout)),
                        Output::NeedValue { round } => {
                            let (value, payments) = self.honest_proposal(v);
                            offered.insert(value.digest(), payments);
                            work.push((v, machines.get_mut(&v).unwrap().propose(round, value)));
                        }
                        Output::Decided { value, .. } => {
                            let payments = offered[&value.digest()].clone();
                            let end = self.judge(v, &value, Some(&payments)).expect("an invalid checkpoint was decided");
                            self.adopt(v, &end, &payments);
                            decided.insert(v, value);
                        }
                    }
                }
            }
            if decided.len() == honest.len() {
                break;
            }
            if !queue.is_empty() {
                let (to, from, message) = queue.swap_remove(random(queue.len()));
                // A validator that has decided has already taken on the new
                // state; what it would judge against is gone.
                if decided.contains_key(&to) {
                    continue;
                }
                let verdict = |world: &World, value: &Checkpoint| Verdict {
                    valid: world.judge(to, value, offered.get(&value.digest())).is_ok(),
                    timely: true,
                };
                let machine = machines.get_mut(&to).unwrap();
                let outputs = match message {
                    Message::Proposal { round, value, valid_round, backers } => {
                        let verdict = verdict(self, &value);
                        machine.on_proposal(round, value, valid_round, &backers, verdict)
                    }
                    Message::Prevote { round, id } => machine.on_prevote(from as u16, round, id),
                    Message::Precommit { round, id } => machine.on_precommit(from as u16, round, id),
                    Message::Commit { round, value, backers } => {
                        let verdict = verdict(self, &value);
                        machine.on_commit(round, value, &backers, verdict)
                    }
                };
                work.push((to, outputs));
            } else if !timeouts.is_empty() {
                let (v, timeout) = timeouts.swap_remove(random(timeouts.len()));
                work.push((v, machines.get_mut(&v).unwrap().on_timeout(timeout)));
            } else {
                break;
            }
        }
        assert_eq!(decided.len(), honest.len(), "the close did not finish");
        let checkpoint = decided.values().next().unwrap().clone();
        assert!(decided.values().all(|other| *other == checkpoint), "validators decided different checkpoints");

        self.closed = Some((checkpoint.clone(), offered[&checkpoint.digest()].clone()));
        self.previous = checkpoint.digest();
        self.committee = self.committee.at_epoch(checkpoint.epoch + 1);
        // A cheat takes no part in the agreement but is not cut off from it.
        for v in taking_part {
            if matches!(self.validators[v].role, Role::Cheat(_)) {
                self.catch_up(v);
            }
        }
        checkpoint
    }

    /// A validator that missed the close takes on its checkpoint afterwards.
    fn catch_up(&mut self, v: usize) {
        let (checkpoint, payments) = self.closed.clone().unwrap();
        // It is worked out as it was when decided: for the epoch it closed.
        let committee = self.committee.at_epoch(checkpoint.epoch);
        let network = self.network;
        let merged: Vec<&Statement> = self
            .stopped
            .statements
            .values()
            .filter(|statement| checkpoint.merged.contains(&(statement.validator, statement.digest(&network))))
            .collect();
        let end = close::outcome(&committee, &self.validators[v].start, &payments, &merged).unwrap();
        assert_eq!(end.root(), checkpoint.state);
        self.adopt(v, &end, &payments);
    }

    fn account(&self, v: usize, user: &User) -> orien_ledger::Account {
        self.validators[v].state.account(&user.id()).unwrap().clone()
    }

    fn balance(&self, v: usize, user: &User) -> i128 {
        self.validators[v].state.account(&user.id()).map_or(0, |account| account.balance)
    }

    /// Every validator in `validators` holds the checkpoint's state, with
    /// no coin made or lost.
    fn assert_agreed(&self, checkpoint: &Checkpoint, validators: &[usize]) {
        for v in validators {
            assert_eq!(self.validators[*v].state.root(), checkpoint.state, "validator {v} holds a different state");
            assert_eq!(self.validators[*v].state.total(), self.supply, "validator {v}: coins were made or lost");
            assert_eq!(self.validators[*v].state.overdrawn(), None);
        }
    }

    /// Takes a payment through votes and confirmations at every validator.
    /// Says whether it ended up final.
    fn finish(&mut self, signed: &SignedOrder) -> bool {
        let certificate = self.finalize(signed, &[0, 1, 2, 3]);
        self.is_final(&certificate)
    }

    /// Votes, certifies and applies a payment at every validator named.
    fn finalize(&mut self, signed: &SignedOrder, validators: &[usize]) -> Certificate {
        let certificate = self.certify(signed, validators);
        for v in validators {
            self.apply(*v, &certificate);
        }
        certificate
    }
}

fn people() -> (User, User, User, User) {
    (User::new(1), User::new(2), User::new(3), User::new(4))
}

#[test]
fn a_close_gives_every_validator_one_state_and_loses_nothing_final() {
    let (alice, bob, carol, _) = people();
    let mut world = World::new(4, &[(&alice, 100), (&bob, 100)]);
    // Final, and known everywhere.
    let everywhere = world.finalize(&alice.pay(&world, 0, &bob, 10), &[0, 1, 2, 3]);
    // Final, though validator 3 never heard of it.
    let most = world.finalize(&bob.pay(&world, 0, &carol, 20), &[0, 1, 2]);
    // Certified, but applied by one validator only: not final.
    let lone = world.certify(&alice.pay(&world, 1, &carol, 5), &[0, 1, 2]);
    world.apply(3, &lone);
    // Interrupted: two validators voted, and nobody holds a certificate.
    let interrupted = bob.pay(&world, 1, &alice, 7);
    for v in [0, 1] {
        world.vote(v, &interrupted).unwrap();
    }
    assert!(world.is_final(&everywhere) && world.is_final(&most) && !world.is_final(&lone));

    let checkpoint = world.close(1);
    world.assert_agreed(&checkpoint, &[0, 1, 2, 3]);
    // The payment one validator had applied is in its pinned log, so it is in.
    assert_eq!(checkpoint.payments, 3);
    for v in 0..4 {
        assert_eq!((world.balance(v, &alice), world.balance(v, &bob), world.balance(v, &carol)), (85, 90, 25));
    }

    // The interrupted payment is no part of what was agreed. The two
    // validators that voted for it still hold their promise, and the other
    // two never made one.
    let digest = world.digest(&interrupted.order);
    for v in [0, 1] {
        assert_eq!(world.validators[v].state.lock(&bob.id(), 1), Some(&digest));
    }
    assert_eq!(world.validators[2].state.lock(&bob.id(), 1), None);

    // In the new epoch the old epoch's votes count for nothing, and the
    // interrupted payment is finished with fresh ones.
    assert!(world.committee.verify_certificate(&everywhere).is_err());
    let finished = world.finalize(&interrupted, &[0, 1, 2, 3]);
    assert!(world.is_final(&finished));
    assert_eq!((finished.epoch, world.balance(0, &alice)), (1, 92));
}

#[test]
fn a_payment_only_one_validator_saw_cannot_stop_the_close() {
    // The flaw the review found in the first draft: a validator could turn
    // down a checkpoint because of something only it knew, here an order
    // nobody else has seen, and leave the rest locked on a checkpoint that
    // could never pass. With one validator gone, the close needs every one
    // of the other three, the one holding that order included.
    for seed in 0..25 {
        let (alice, bob, _, _) = people();
        let mut world = World::new(4, &[(&alice, 100)]);
        let interrupted = alice.pay(&world, 0, &bob, 10);
        world.vote(0, &interrupted).unwrap();
        world.validators[3].role = Role::Absent;

        let checkpoint = world.close(seed);
        world.assert_agreed(&checkpoint, &[0, 1, 2]);
        assert_eq!(checkpoint.payments, 0);

        // Its promise outlives the checkpoint, and the payment is finished
        // in the next epoch.
        let finished = world.finalize(&interrupted, &[0, 1, 2]);
        assert!(world.is_final(&finished), "seed {seed}");
    }
}

#[test]
fn a_payment_undone_at_a_checkpoint_leaves_no_coins_behind() {
    // The second flaw the review found: a payment undone at a checkpoint
    // could leave a later payment that spent its money still votable.
    let (alice, bob, carol, dave) = people();
    let mut world = World::new(4, &[(&alice, 100), (&bob, 1)]);
    // Alice signs her whole balance away twice under one number.
    let to_bob = alice.pay(&world, 0, &bob, 100);
    let to_carol = alice.pay(&world, 0, &carol, 100);
    assert!(world.vote(2, &to_carol).is_ok());
    let certificate = world.certify(&to_bob, &[0, 1, 3]);
    // Validator 2, promised to the other order, is shown this one and keeps both.
    assert!(matches!(world.vote(2, &to_bob), Err(Reject::Conflict { .. })));
    // The certificate reaches validator 0 alone. There Bob now holds 101,
    // and validator 0 votes for Bob to pay it all on to Dave.
    world.apply(0, &certificate);
    assert!(!world.is_final(&certificate));
    let onward = bob.pay(&world, 0, &dave, 101);
    assert!(world.vote(0, &onward).is_ok());

    // Validator 0 misses the close. The other three have applied nothing,
    // and one of them holds proof that Alice signed twice.
    world.validators[0].role = Role::Absent;
    let checkpoint = world.close(3);
    world.assert_agreed(&checkpoint, &[1, 2, 3]);
    assert_eq!(checkpoint.payments, 0);
    assert_eq!((world.account(1, &alice).signer, world.balance(1, &alice)), (Signer::Spare(0), 100));

    // Validator 0 comes back, takes on the checkpoint and undoes the
    // payment it had applied.
    world.validators[0].role = Role::Honest;
    world.catch_up(0);
    world.assert_agreed(&checkpoint, &[0, 1, 2, 3]);
    assert_eq!(world.balance(0, &bob), 1);

    // It still holds its promise for Bob's onward payment. The promise is
    // worth nothing, because every vote checks the balance again, the
    // repeated vote of a validator that promised included.
    assert!(world.validators[0].promises.contains_key(&bob.id()));
    for v in 0..4 {
        assert_eq!(world.vote(v, &onward).err(), Some(Reject::InsufficientFunds { balance: 1, needed: 101 }));
        // Neither of Alice's two orders can be voted for any more.
        assert_eq!(world.vote(v, &to_bob).err().unwrap(), Reject::BadSignature);
        assert_eq!(world.vote(v, &to_carol).err().unwrap(), Reject::BadSignature);
    }

    // Alice pays with her first spare key, and the books balance.
    let retry = world.finalize(&alice.pay_with_spare(&world, 0, 0, &carol, 40), &[0, 1, 2, 3]);
    assert!(world.is_final(&retry));
    let checkpoint = world.close(4);
    world.assert_agreed(&checkpoint, &[0, 1, 2, 3]);
    assert_eq!((world.balance(2, &alice), world.balance(2, &carol), world.balance(2, &dave)), (60, 40, 0));
    assert_eq!(world.balance(2, &bob), 1);
}

#[test]
fn a_cheating_proposer_gets_nothing_accepted() {
    let mut went_first = 0;
    for trick in [Trick::DropPayment, Trick::WrongState, Trick::FewStatements, Trick::ForgedStatement] {
        for seed in 0..12u64 {
            let (alice, bob, carol, _) = people();
            let mut world = World::new(4, &[(&alice, 100), (&bob, 100)]);
            world.validators[(seed % 4) as usize].role = Role::Cheat(trick);
            let first = world.finalize(&alice.pay(&world, 0, &bob, 10), &[0, 1, 2, 3]);
            let second = world.finalize(&bob.pay(&world, 0, &carol, 20), &[0, 1, 2, 3]);
            assert!(world.is_final(&first) && world.is_final(&second));

            let checkpoint = world.close(seed);
            world.assert_agreed(&checkpoint, &[0, 1, 2, 3]);
            assert_eq!(checkpoint.payments, 2, "{trick:?}, seed {seed}: a final payment was left out");
            // Each cheat is refused for the reason it should be.
            for refusal in &world.refusals {
                let expected = match trick {
                    Trick::DropPayment => matches!(refusal, Invalid::MissingPayment(_)),
                    Trick::WrongState => *refusal == Invalid::WrongState,
                    Trick::FewStatements => *refusal == Invalid::TooFewStatements,
                    Trick::ForgedStatement => matches!(refusal, Invalid::UnknownStatement(_)),
                };
                assert!(expected, "{trick:?} was refused as {refusal:?}");
            }
            went_first += world.cheat_went_first;
        }
    }
    assert!(went_first >= 8, "the cheat was first proposer in only {went_first} closes");
}

#[test]
fn every_way_a_checkpoint_can_be_wrong_is_named() {
    let (alice, bob, carol, _) = people();
    let mut world = World::new(4, &[(&alice, 100)]);
    let first = world.finalize(&alice.pay(&world, 0, &bob, 60), &[0, 1, 2, 3]);
    let second = world.finalize(&bob.pay(&world, 0, &carol, 50), &[0, 1, 2, 3]);
    world.stop_all();
    let (good, payments) = world.honest_proposal(0);
    assert!(world.judge(1, &good, Some(&payments)).is_ok());
    let refused = |change: &dyn Fn(&mut Checkpoint)| {
        let mut checkpoint = good.clone();
        change(&mut checkpoint);
        world.judge(1, &checkpoint, Some(&payments)).err().unwrap()
    };

    assert_eq!(refused(&|c| c.epoch = 1), Invalid::WrongEpoch);
    assert_eq!(refused(&|c| c.previous = Hash32([9; 32])), Invalid::WrongPrevious);
    assert_eq!(refused(&|c| c.merged.truncate(2)), Invalid::TooFewStatements);
    assert_eq!(refused(&|c| c.merged.reverse()), Invalid::TooFewStatements);
    assert_eq!(refused(&|c| c.merged[2].0 = 1), Invalid::TooFewStatements);
    assert_eq!(refused(&|c| c.merged[0].1 = Hash32([9; 32])), Invalid::UnknownStatement(0));
    assert_eq!(refused(&|c| c.payments += 1), Invalid::WrongCount);
    assert_eq!(refused(&|c| c.state.0[5] ^= 1), Invalid::WrongState);
    // The time is not part of this judgement, though it is part of what is agreed.
    assert!(world.judge(1, &Checkpoint { time: 7, ..good.clone() }, Some(&payments)).is_ok());
    assert_ne!(Checkpoint { time: 7, ..good.clone() }.digest(), good.digest());

    // A payment that a named validator had applied is left out.
    assert_eq!(world.judge(1, &good, Some(&vec![first.clone()])).err().unwrap(), Invalid::MissingPayment(0));
    // A named validator's log is not the one its statement pins.
    let mut logs = world.stopped.logs.clone();
    logs.get_mut(&2).unwrap().pop();
    let gathered = Gathered { statements: &world.stopped.statements, logs: &logs, payments: &payments };
    let start = &world.validators[1].start;
    assert_eq!(
        close::validate(&world.committee, &world.previous, start, &good, &gathered).err().unwrap(),
        Invalid::MissingLog(2)
    );

    // The payments themselves are checked too.
    let outcome = |payments: &[Certificate]| close::outcome(&world.committee, start, payments, &[]).err().unwrap();
    // Bob's spend without the payment that funded it.
    assert_eq!(outcome(std::slice::from_ref(&second)), Invalid::Overdrawn(bob.id()));
    assert_eq!(outcome(&[first.clone(), first.clone()]), Invalid::OutOfSequence(alice.id(), 0));
    let mut too_few_votes = first.clone();
    too_few_votes.votes.truncate(2);
    assert_eq!(outcome(&[too_few_votes]), Invalid::BadPayment(alice.id(), 0));
    let mut wrong_owner = first.clone();
    wrong_owner.signature = second.signature.clone();
    assert_eq!(outcome(&[wrong_owner]), Invalid::BadPayment(alice.id(), 0));
    let mut altered = first.clone();
    altered.order.amount = 61;
    assert_eq!(outcome(&[altered]), Invalid::BadPayment(alice.id(), 0));
}

#[test]
fn whatever_happens_before_a_close_nothing_final_is_lost() {
    /// What became of one account's first payment before the close.
    #[derive(Debug)]
    enum Fate {
        /// Applied and confirmed by enough validators to be final.
        Final(Certificate),
        /// Certified, but applied by this one validator alone.
        Lone(Certificate, usize),
        /// Voted for by a few validators and never certified.
        Interrupted(SignedOrder),
        /// Signed twice, with the votes split. This validator saw both.
        Doubled(usize),
        Nothing,
    }
    let (mut cancelled, mut freed_later, mut undone) = (0, 0, 0);
    for seed in 0..60u64 {
        let mut rng = seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1;
        let mut random = move |below: usize| {
            rng ^= rng << 13;
            rng ^= rng >> 7;
            rng ^= rng << 17;
            (rng % below as u64) as usize
        };
        let users: Vec<User> = (1..=5).map(User::new).collect();
        let funded: Vec<(&User, u64)> = users.iter().map(|user| (user, 1000)).collect();
        let mut world = World::new(4, &funded);
        let absent = [None, Some(0), Some(1), Some(2), Some(3), None][random(6)];

        let mut fates = Vec::new();
        for (payer, user) in users.iter().enumerate() {
            let payee = &users[(payer + 1 + random(4)) % 5];
            let order = user.pay(&world, 0, payee, 1 + random(500) as u64);
            let skip = random(4);
            let three: Vec<usize> = (0..4).filter(|v| *v != skip).collect();
            fates.push(match random(6) {
                0 => Fate::Final(world.finalize(&order, &[0, 1, 2, 3])),
                1 => Fate::Final(world.finalize(&order, &three)),
                2 => {
                    let certificate = world.certify(&order, &three);
                    let only = three[random(3)];
                    world.apply(only, &certificate);
                    Fate::Lone(certificate, only)
                }
                3 => {
                    for v in three.iter().take(1 + random(2)) {
                        world.vote(*v, &order).unwrap();
                    }
                    Fate::Interrupted(order)
                }
                4 => {
                    let other = user.pay(&world, 0, payee, 777);
                    for v in [0, 1] {
                        world.vote(v, &order).unwrap();
                    }
                    for v in [2, 3] {
                        world.vote(v, &other).unwrap();
                    }
                    let witness = random(2);
                    assert!(world.vote(witness, &other).is_err());
                    Fate::Doubled(witness)
                }
                _ => Fate::Nothing,
            });
        }
        for fate in &fates {
            if let Fate::Final(certificate) = fate {
                assert!(world.is_final(certificate));
            }
        }

        if let Some(absent) = absent {
            world.validators[absent].role = Role::Absent;
        }
        let taking_part = world.taking_part();
        let checkpoint = world.close(seed);
        world.assert_agreed(&checkpoint, &taking_part);
        let context = format!("seed {seed}, absent {absent:?}");
        for (user, fate) in users.iter().zip(&fates) {
            let account = world.account(taking_part[0], user);
            match fate {
                // Final before the close: in it, whoever was absent.
                Fate::Final(_) => assert_eq!(account.seq, 1, "{context}: a final payment was lost"),
                // In, exactly when the one validator that applied it took part.
                Fate::Lone(_, only) => assert_eq!(account.seq, taking_part.contains(only) as u64, "{context}"),
                Fate::Interrupted(_) | Fate::Nothing => assert_eq!((account.seq, account.signer), (0, Signer::First)),
                // Cancelled, exactly when the validator holding the proof took part.
                Fate::Doubled(witness) => {
                    let expected = if taking_part.contains(witness) { Signer::Spare(0) } else { Signer::First };
                    assert_eq!((account.seq, account.signer), (0, expected), "{context}");
                }
            }
        }

        // The absent validator returns and takes on the same state.
        if let Some(absent) = absent {
            world.validators[absent].role = Role::Honest;
            world.catch_up(absent);
        }
        world.assert_agreed(&checkpoint, &[0, 1, 2, 3]);

        // In the next epoch, everything left unfinished is finished.
        for (user, fate) in users.iter().zip(&fates) {
            let account = world.account(0, user);
            match fate {
                Fate::Lone(certificate, _) if account.seq == 0 => {
                    // Undone at the validator that had applied it, and done again by everyone.
                    undone += 1;
                    assert!(world.finish(&certificate.signed()), "{context}");
                }
                Fate::Interrupted(order) => assert!(world.finish(order), "{context}"),
                Fate::Doubled(_) if account.signer == Signer::Spare(0) => {
                    cancelled += 1;
                    let retry = user.pay_with_spare(&world, 0, 0, &users[0], 3);
                    assert!(world.finish(&retry), "{context}");
                }
                _ => {}
            }
        }

        // A second close. The validator that missed the first now brings
        // its proof, and the account it concerns is cancelled this time.
        let second = world.close(seed + 1000);
        world.assert_agreed(&second, &[0, 1, 2, 3]);
        assert_eq!(second.previous, checkpoint.digest());
        for (user, fate) in users.iter().zip(&fates) {
            let account = world.account(0, user);
            match fate {
                Fate::Doubled(witness) if !taking_part.contains(witness) => {
                    freed_later += 1;
                    assert_eq!((account.seq, account.signer), (0, Signer::Spare(0)), "{context}");
                }
                Fate::Doubled(_) | Fate::Interrupted(_) | Fate::Final(_) | Fate::Lone(..) => {
                    assert_eq!(account.seq, 1, "{context}")
                }
                Fate::Nothing => assert_eq!(account.seq, 0),
            }
        }
    }
    // The runs must have reached each of the cases that matter.
    assert!(cancelled > 10 && freed_later > 2 && undone > 2, "{cancelled} {freed_later} {undone}");
}
