//! The protocol exercised in memory: several validators, no network.

use token_crypto::keytree::KeyTree;
use token_crypto::{Hash32, Kdf, merkle, ots, prf};
use token_ledger::{
    AccountId, Allocation, Applied, Certificate, Committee, Genesis, Order, Reject, SignedOrder,
    State, ValidatorInfo, VerifyError, Vote,
};

struct User {
    seed: [u8; 32],
}

impl User {
    fn new(n: u8) -> User {
        User { seed: [n; 32] }
    }

    fn key(&self, index: u64) -> ots::SecretKey {
        ots::SecretKey::from_seed(prf(&self.seed, Kdf::WalletKey, index))
    }

    fn id(&self) -> AccountId {
        self.key(0).commitment()
    }

    fn order(&self, net: &Net, seq: u64, to: AccountId, amount: u64) -> SignedOrder {
        self.order_with_key(net, seq, seq, to, amount)
    }

    fn order_with_key(&self, net: &Net, seq: u64, key: u64, to: AccountId, amount: u64) -> SignedOrder {
        let order = Order { from: self.id(), seq, to, amount, next_key: self.key(seq + 1).commitment() };
        let digest = order.digest(&net.network);
        SignedOrder { signature: self.key(key).sign(digest.as_bytes()), order }
    }
}

struct Validator {
    tree: KeyTree,
    next_leaf: u32,
    state: State,
}

struct Net {
    network: Hash32,
    committee: Committee,
    validators: Vec<Validator>,
    supply: i128,
}

impl Net {
    fn new(name: &str, stakes: &[u64], funded: &[(&User, u64)]) -> Net {
        let trees: Vec<KeyTree> =
            (0..stakes.len()).map(|i| KeyTree::generate([100 + i as u8; 32], 5)).collect();
        let genesis = Genesis {
            network: name.into(),
            validators: trees
                .iter()
                .zip(stakes)
                .enumerate()
                .map(|(i, (tree, stake))| ValidatorInfo {
                    name: format!("v{i}"),
                    root: tree.root(),
                    height: 5,
                    stake: *stake,
                    address: String::new(),
                })
                .collect(),
            allocations: funded
                .iter()
                .map(|(user, amount)| Allocation { account: user.id(), amount: *amount })
                .collect(),
        };
        genesis.validate().unwrap();
        Net {
            network: genesis.id(),
            committee: Committee::new(&genesis),
            supply: genesis.supply() as i128,
            validators: trees
                .into_iter()
                .map(|tree| Validator { tree, next_leaf: 0, state: State::from_genesis(&genesis) })
                .collect(),
        }
    }

    fn four(funded: &[(&User, u64)]) -> Net {
        Net::new("test", &[1, 1, 1, 1], funded)
    }

    /// Validator `v` signs one batch covering all `digests`.
    fn sign_batch(&mut self, v: usize, digests: &[Hash32]) -> Vec<Vote> {
        let batch = merkle::Tree::build(digests);
        let message = self.committee.batch_message(&batch.root());
        let validator = &mut self.validators[v];
        let signature = validator.tree.sign_with_leaf(validator.next_leaf, message.as_bytes());
        validator.next_leaf += 1;
        (0..digests.len())
            .map(|i| Vote { validator: v as u16, batch_path: batch.path(i), signature: signature.clone() })
            .collect()
    }

    fn vote(&mut self, v: usize, signed: &SignedOrder) -> Result<Vote, Reject> {
        let digest = signed.order.digest(&self.network);
        let signed_by = ots::recover(&signed.signature, digest.as_bytes());
        self.validators[v].state.check_order(&signed.order, digest, &signed_by)?;
        Ok(self.sign_batch(v, &[digest]).remove(0))
    }

    fn certify(&mut self, signed: &SignedOrder, voters: &[usize]) -> Certificate {
        let votes = voters.iter().map(|v| self.vote(*v, signed).unwrap()).collect();
        Certificate { order: signed.order.clone(), votes }
    }

    fn deliver(&mut self, v: usize, certificate: &Certificate) -> Applied {
        self.committee.verify_certificate(certificate).unwrap();
        let applied = self.validators[v].state.apply(&certificate.order).unwrap();
        assert_eq!(self.validators[v].state.total(), self.supply, "money was created or destroyed");
        applied
    }

    fn deliver_all(&mut self, certificate: &Certificate) {
        for v in 0..self.validators.len() {
            self.deliver(v, certificate);
        }
    }

    fn balance(&self, v: usize, id: &AccountId) -> i128 {
        self.validators[v].state.account(id).map_or(0, |a| a.balance)
    }
}

#[test]
fn a_payment_becomes_final() {
    let (alice, bob) = (User::new(1), User::new(2));
    let mut net = Net::four(&[(&alice, 100)]);
    let order = alice.order(&net, 0, bob.id(), 30);
    let certificate = net.certify(&order, &[0, 1, 2]);
    net.deliver_all(&certificate);
    for v in 0..4 {
        assert_eq!(net.balance(v, &alice.id()), 70);
        assert_eq!(net.balance(v, &bob.id()), 30);
        assert_eq!(net.validators[v].state.seq(&alice.id()), 1);
    }
}

#[test]
fn two_orders_with_one_number_cannot_both_be_certified() {
    let (alice, bob, carol) = (User::new(1), User::new(2), User::new(3));
    let mut net = Net::four(&[(&alice, 100)]);
    let to_bob = alice.order(&net, 0, bob.id(), 100);
    let to_carol = alice.order(&net, 0, carol.id(), 100);

    let certificate = net.certify(&to_bob, &[0, 1, 2]);
    assert!(net.committee.verify_certificate(&certificate).is_ok());

    // The three validators that voted refuse the conflicting order.
    for v in 0..3 {
        assert!(matches!(net.vote(v, &to_carol), Err(Reject::Conflict { .. })));
    }
    // The fourth may vote for it, but one vote proves nothing.
    let lone = net.certify(&to_carol, &[3]);
    assert!(matches!(
        net.committee.verify_certificate(&lone),
        Err(VerifyError::NoQuorum { stake: 1, total: 4 })
    ));
}

#[test]
fn a_final_payment_number_is_spent_for_good() {
    let (alice, bob, carol) = (User::new(1), User::new(2), User::new(3));
    let mut net = Net::four(&[(&alice, 100)]);
    let first = alice.order(&net, 0, bob.id(), 60);
    let certificate = net.certify(&first, &[0, 1, 2]);
    net.deliver_all(&certificate);

    let replay = alice.order(&net, 0, carol.id(), 60);
    for v in 0..4 {
        assert_eq!(net.vote(v, &replay), Err(Reject::StaleSeq { current: 1 }));
    }
}

#[test]
fn split_votes_lock_the_cheater_and_nobody_else() {
    let (alice, bob, carol) = (User::new(1), User::new(2), User::new(3));
    let mut net = Net::four(&[(&alice, 100), (&bob, 100)]);
    let to_bob = alice.order(&net, 0, bob.id(), 100);
    let to_carol = alice.order(&net, 0, carol.id(), 100);

    let half_a = net.certify(&to_bob, &[0, 1]);
    let half_b = net.certify(&to_carol, &[2, 3]);
    assert!(net.committee.verify_certificate(&half_a).is_err());
    assert!(net.committee.verify_certificate(&half_b).is_err());
    // No validator will change its vote, so alice's account is stuck...
    assert!(matches!(net.vote(2, &to_bob), Err(Reject::Conflict { .. })));
    assert!(matches!(net.vote(0, &to_carol), Err(Reject::Conflict { .. })));

    // ...while bob pays as usual.
    let order = bob.order(&net, 0, carol.id(), 40);
    let certificate = net.certify(&order, &[0, 1, 2, 3]);
    net.deliver_all(&certificate);
    assert_eq!(net.balance(0, &carol.id()), 40);
}

#[test]
fn only_the_current_one_time_key_can_sign() {
    let (alice, bob, mallory) = (User::new(1), User::new(2), User::new(9));
    let mut net = Net::four(&[(&alice, 100)]);

    // Signed with the wrong key of the right wallet.
    let wrong_key = alice.order_with_key(&net, 0, 1, bob.id(), 10);
    assert_eq!(net.vote(0, &wrong_key), Err(Reject::BadSignature));

    // Signed by someone else, claiming alice's account.
    let mut forged = mallory.order(&net, 0, mallory.id(), 100);
    forged.order.from = alice.id();
    assert_eq!(net.vote(0, &forged), Err(Reject::BadSignature));

    // After payment #0, key #0 is dead.
    let first = alice.order(&net, 0, bob.id(), 10);
    let certificate = net.certify(&first, &[0, 1, 2]);
    net.deliver_all(&certificate);
    let old_key = alice.order_with_key(&net, 1, 0, bob.id(), 10);
    assert_eq!(net.vote(0, &old_key), Err(Reject::BadSignature));
    let good = alice.order(&net, 1, bob.id(), 10);
    assert!(net.vote(0, &good).is_ok());
}

#[test]
fn an_order_cannot_be_replayed_on_another_network() {
    let (alice, bob) = (User::new(1), User::new(2));
    let net_a = Net::new("net-a", &[1, 1, 1, 1], &[(&alice, 100)]);
    let mut net_b = Net::new("net-b", &[1, 1, 1, 1], &[(&alice, 100)]);
    let order = alice.order(&net_a, 0, bob.id(), 100);
    assert_eq!(net_b.vote(0, &order), Err(Reject::BadSignature));
}

#[test]
fn overspending_and_empty_payments_are_refused() {
    let (alice, bob, carol) = (User::new(1), User::new(2), User::new(3));
    let mut net = Net::four(&[(&alice, 100)]);
    let too_much = alice.order(&net, 0, bob.id(), 101);
    assert_eq!(net.vote(0, &too_much), Err(Reject::InsufficientFunds { balance: 100, needed: 101 }));
    let nothing = alice.order(&net, 0, bob.id(), 0);
    assert_eq!(net.vote(0, &nothing), Err(Reject::ZeroAmount));
    let unfunded = carol.order(&net, 0, bob.id(), 1);
    assert_eq!(net.vote(0, &unfunded), Err(Reject::UnknownAccount));
    let skipped = alice.order(&net, 1, bob.id(), 1);
    assert_eq!(net.vote(0, &skipped), Err(Reject::FutureSeq { current: 0 }));
}

#[test]
fn certificates_must_be_genuine() {
    let (alice, bob) = (User::new(1), User::new(2));
    let mut net = Net::four(&[(&alice, 100)]);
    let order = alice.order(&net, 0, bob.id(), 30);
    let good = net.certify(&order, &[0, 1, 2]);
    assert!(net.committee.verify_certificate(&good).is_ok());

    let mut inflated = good.clone();
    inflated.order.amount = 99;
    assert_eq!(net.committee.verify_certificate(&inflated), Err(VerifyError::BadSignature(0)));

    let mut repeated = good.clone();
    repeated.votes = vec![good.votes[0].clone(), good.votes[0].clone(), good.votes[1].clone()];
    assert_eq!(net.committee.verify_certificate(&repeated), Err(VerifyError::VotesOutOfOrder));

    let mut reordered = good.clone();
    reordered.votes.swap(0, 1);
    assert_eq!(net.committee.verify_certificate(&reordered), Err(VerifyError::VotesOutOfOrder));

    let mut short = good.clone();
    short.votes.pop();
    assert_eq!(
        net.committee.verify_certificate(&short),
        Err(VerifyError::NoQuorum { stake: 2, total: 4 })
    );

    let mut impersonated = good.clone();
    impersonated.votes[2].validator = 3;
    assert_eq!(net.committee.verify_certificate(&impersonated), Err(VerifyError::BadSignature(3)));

    let mut outsider = good.clone();
    outsider.votes[2].validator = 40;
    assert_eq!(net.committee.verify_certificate(&outsider), Err(VerifyError::UnknownValidator(40)));
}

#[test]
fn a_certificate_applies_exactly_once() {
    let (alice, bob) = (User::new(1), User::new(2));
    let mut net = Net::four(&[(&alice, 100)]);
    let order = alice.order(&net, 0, bob.id(), 30);
    let certificate = net.certify(&order, &[0, 1, 2]);
    assert_eq!(net.deliver(0, &certificate), Applied::New);
    assert_eq!(net.deliver(0, &certificate), Applied::Already);
    assert_eq!(net.balance(0, &bob.id()), 30);
}

/// Certificates can reach a validator in any order. Its view may dip below
/// zero while it catches up, and ends in the same place as everyone else's.
#[test]
fn late_funding_settles_to_the_same_balances() {
    let (alice, bob, carol) = (User::new(1), User::new(2), User::new(3));
    let mut net = Net::four(&[(&alice, 100)]);

    let funding = alice.order(&net, 0, bob.id(), 60);
    let funding_cert = net.certify(&funding, &[0, 1, 2]);
    for v in 0..3 {
        net.deliver(v, &funding_cert);
    }
    // Validator 3 has not heard that bob was paid, so it will not vote for his spend.
    let spend = bob.order(&net, 0, carol.id(), 50);
    assert_eq!(net.vote(3, &spend), Err(Reject::UnknownAccount));
    let spend_cert = net.certify(&spend, &[0, 1, 2]);

    // Validator 3 receives the spend before the funding.
    net.deliver(3, &spend_cert);
    assert_eq!(net.balance(3, &bob.id()), -50);
    net.deliver(3, &funding_cert);
    for v in 0..3 {
        net.deliver(v, &spend_cert);
    }
    for v in 0..4 {
        assert_eq!(net.balance(v, &alice.id()), 40);
        assert_eq!(net.balance(v, &bob.id()), 10);
        assert_eq!(net.balance(v, &carol.id()), 50);
    }
}

#[test]
fn one_signature_covers_a_whole_batch() {
    let users: Vec<User> = (1..=5).map(User::new).collect();
    let funded: Vec<(&User, u64)> = users.iter().map(|u| (u, 10)).collect();
    let mut net = Net::four(&funded);
    let target = User::new(50).id();
    let orders: Vec<SignedOrder> = users.iter().map(|u| u.order(&net, 0, target, 10)).collect();
    let digests: Vec<Hash32> = orders.iter().map(|o| o.order.digest(&net.network)).collect();

    // Each of three validators signs once for all five orders.
    let mut votes_by_validator = Vec::new();
    for v in 0..3 {
        votes_by_validator.push(net.sign_batch(v, &digests));
        assert_eq!(net.validators[v].next_leaf, 1);
    }
    for (i, order) in orders.iter().enumerate() {
        let votes = votes_by_validator.iter().map(|votes| votes[i].clone()).collect();
        let certificate = Certificate { order: order.order.clone(), votes };
        assert!(net.committee.verify_certificate(&certificate).is_ok());
        // A vote for one order in the batch is not a vote for another.
        let mut crossed = certificate.clone();
        crossed.order = orders[(i + 1) % 5].order.clone();
        assert!(net.committee.verify_certificate(&crossed).is_err());
    }
}

#[test]
fn quorum_is_more_than_two_thirds_of_stake() {
    let solo = Net::new("solo", &[1], &[]);
    assert!(solo.committee.has_quorum(1));

    let three = Net::new("three", &[1, 1, 1], &[]);
    assert!(!three.committee.has_quorum(2));
    assert!(three.committee.has_quorum(3));

    let four = Net::new("four", &[1, 1, 1, 1], &[]);
    assert!(!four.committee.has_quorum(2));
    assert!(four.committee.has_quorum(3));

    let weighted = Net::new("weighted", &[50, 30, 20], &[]);
    assert!(!weighted.committee.has_quorum(50));
    assert!(!weighted.committee.has_quorum(66));
    assert!(weighted.committee.has_quorum(70));
}
