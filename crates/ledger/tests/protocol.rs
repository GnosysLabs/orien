//! The protocol exercised in memory: several validators, no network.

use orien_crypto::keytree::KeyTree;
use orien_crypto::{Hash32, Kdf, merkle, ots, prf};
use orien_ledger::{
    AccountId, Allocation, Applied, CancelError, Certificate, Checked, Committee, Genesis, Kind,
    Order, Receipt, Reject, SPARE_HEIGHT, Signature, SignedOrder, Signer, State, ValidatorInfo,
    VerifyError, Vote, address, confirmation_item,
};

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

    fn unsigned(&self, seq: u64, to: AccountId, amount: u64) -> Order {
        Order {
            kind: Kind::Payment,
            from: self.id(),
            seq,
            to,
            amount,
            max_fee: 0,
            next_key: self.key(seq + 1).commitment(),
            spare: self.spares.root(),
        }
    }

    fn order(&self, net: &Net, seq: u64, to: AccountId, amount: u64) -> SignedOrder {
        self.order_with_key(net, seq, seq, to, amount)
    }

    fn order_with_key(&self, net: &Net, seq: u64, key: u64, to: AccountId, amount: u64) -> SignedOrder {
        let order = self.unsigned(seq, to, amount);
        let digest = order.digest(&net.network);
        SignedOrder { signature: Signature::Key(self.key(key).sign(digest.as_bytes())), order }
    }

    /// An order signed with spare key number `leaf`.
    fn order_with_spare(&self, net: &Net, seq: u64, leaf: u32, to: AccountId, amount: u64) -> SignedOrder {
        let order = self.unsigned(seq, to, amount);
        let digest = order.digest(&net.network);
        SignedOrder { signature: Signature::Spare(self.spares.sign_with_leaf(leaf, digest.as_bytes())), order }
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
            epoch_seconds: 3600,
            epoch_payments: 50_000,
            start_time: 0,
            min_stake: 10_000,
            max_validators: 33,
            unstake_epochs: 168,
            fee: orien_ledger::Fees::NONE,
            cap: 0,
            shares_per_epoch: 1_000,
            first_target: u64::MAX,
            bitcoin: None,
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
        let signed_by = signed.signed_by(&self.network).ok_or(Reject::BadSignature)?;
        self.validators[v].state.check_order(&signed.order, digest, &signed_by)?;
        Ok(self.sign_batch(v, &[digest]).remove(0))
    }

    fn certify(&mut self, signed: &SignedOrder, voters: &[usize]) -> Certificate {
        let votes = voters.iter().map(|v| self.vote(*v, signed).unwrap()).collect();
        Certificate::new(0, signed, votes)
    }

    /// Validator `v` applies a certificate and signs that it has.
    fn confirm(&mut self, v: usize, certificate: &Certificate) -> Vote {
        self.deliver(v, certificate);
        let item = confirmation_item(&certificate.order.digest(&self.network));
        self.sign_batch(v, &[item]).remove(0)
    }

    fn receipt(&mut self, signed: &SignedOrder, voters: &[usize], confirmers: &[usize]) -> Receipt {
        let certificate = self.certify(signed, voters);
        let confirmations = confirmers.iter().map(|v| self.confirm(*v, &certificate)).collect();
        Receipt { certificate, confirmations }
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
        let certificate = Certificate::new(0, order, votes);
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

#[test]
fn a_receipt_needs_confirmations_as_well_as_votes() {
    let (alice, bob) = (User::new(1), User::new(2));
    let mut net = Net::four(&[(&alice, 100)]);
    let order = alice.order(&net, 0, bob.id(), 40);
    let receipt = net.receipt(&order, &[0, 1, 2], &[1, 2, 3]);
    assert_eq!(net.committee.verify_receipt(&receipt), Ok(()));

    // The certificate alone is not a receipt, and two confirmations of four
    // are not enough.
    let bare = Receipt { certificate: receipt.certificate.clone(), confirmations: Vec::new() };
    assert_eq!(net.committee.verify_receipt(&bare), Err(VerifyError::NotConfirmed { stake: 0, total: 4 }));
    let mut thin = receipt.clone();
    thin.confirmations.pop();
    assert_eq!(net.committee.verify_receipt(&thin), Err(VerifyError::NotConfirmed { stake: 2, total: 4 }));

    // Confirmations follow the same rules as votes: no repeats, no strangers.
    let mut repeated = receipt.clone();
    repeated.confirmations[1] = repeated.confirmations[0].clone();
    assert_eq!(net.committee.verify_receipt(&repeated), Err(VerifyError::VotesOutOfOrder));
    let mut impersonated = receipt.clone();
    impersonated.confirmations[0].validator = 0;
    assert_eq!(net.committee.verify_receipt(&impersonated), Err(VerifyError::BadSignature(0)));

    // A receipt with too few votes fails on its certificate first.
    let mut unvoted = receipt.clone();
    unvoted.certificate.votes.pop();
    assert_eq!(net.committee.verify_receipt(&unvoted), Err(VerifyError::NoQuorum { stake: 2, total: 4 }));
}

#[test]
fn a_vote_cannot_pass_for_a_confirmation() {
    let (alice, bob) = (User::new(1), User::new(2));
    let mut net = Net::four(&[(&alice, 100)]);
    let order = alice.order(&net, 0, bob.id(), 40);
    let digest = order.order.digest(&net.network);
    let receipt = net.receipt(&order, &[0, 1, 2], &[0, 1, 2]);

    // Each kind of signature checks only as what it is.
    let (vote, confirmation) = (&receipt.certificate.votes[0], &receipt.confirmations[0]);
    assert!(net.committee.verify_vote(&digest, vote).is_ok());
    assert!(net.committee.verify_confirmation(&digest, confirmation).is_ok());
    assert!(net.committee.verify_confirmation(&digest, vote).is_err());
    assert!(net.committee.verify_vote(&digest, confirmation).is_err());

    // So three votes cannot be presented as three confirmations...
    let recycled = Receipt {
        certificate: receipt.certificate.clone(),
        confirmations: receipt.certificate.votes.clone(),
    };
    assert_eq!(net.committee.verify_receipt(&recycled), Err(VerifyError::BadSignature(0)));
    // ...nor three confirmations as a certificate.
    let mut inverted = receipt.certificate.clone();
    inverted.votes = receipt.confirmations.clone();
    assert_eq!(net.committee.verify_certificate(&inverted), Err(VerifyError::BadSignature(0)));
}

#[test]
fn signatures_from_another_epoch_count_for_nothing() {
    let (alice, bob) = (User::new(1), User::new(2));
    let mut net = Net::four(&[(&alice, 100)]);
    let order = alice.order(&net, 0, bob.id(), 40);
    let receipt = net.receipt(&order, &[0, 1, 2], &[0, 1, 2]);

    // Relabelling the certificate with another epoch is caught twice over:
    // by the label, and because every vote signed the epoch it was cast in.
    let mut relabelled = receipt.clone();
    relabelled.certificate.epoch = 1;
    assert_eq!(
        net.committee.verify_receipt(&relabelled),
        Err(VerifyError::WrongEpoch { found: 1, current: 0 })
    );
    assert_eq!(net.committee.epoch(), 0);
    // The certificate itself, relabelled, fails the same way.
    assert_eq!(
        net.committee.verify_certificate(&relabelled.certificate),
        Err(VerifyError::WrongEpoch { found: 1, current: 0 })
    );
}

#[test]
fn a_certificate_keeps_the_owners_signed_order() {
    let (alice, bob) = (User::new(1), User::new(2));
    let mut net = Net::four(&[(&alice, 100)]);
    let order = alice.order(&net, 0, bob.id(), 40);
    let certificate = net.certify(&order, &[0, 1, 2]);
    assert_eq!(certificate.signed(), order);
    // With it, a validator that never saw the order can check who signed it
    // against the key its own state holds for the account.
    let digest = certificate.order.digest(&net.network);
    let signed_by = certificate.signature.signed_by(&digest).unwrap();
    assert!(net.validators[3].state.authorizes(&certificate.order, &signed_by));
    // The same signature on a different order points at some other key,
    // which the account does not require.
    let mut altered = certificate.order.clone();
    altered.amount += 1;
    let other_key = certificate.signature.signed_by(&altered.digest(&net.network)).unwrap();
    assert_ne!(other_key, signed_by);
    assert!(!net.validators[3].state.authorizes(&altered, &other_key));
}

impl Net {
    fn checked<'a>(&self, signed: &'a SignedOrder) -> Checked<'a> {
        Checked {
            order: &signed.order,
            digest: signed.order.digest(&self.network),
            by: signed.signed_by(&self.network).unwrap(),
        }
    }

    /// Every validator is shown the two orders and cancels the number.
    fn cancel_everywhere(&mut self, one: &SignedOrder, other: &SignedOrder) -> Result<(), CancelError> {
        let (one, other) = (self.checked(one), self.checked(other));
        let mut outcome = Ok(());
        for validator in &mut self.validators {
            outcome = validator.state.cancel(&one, &other);
        }
        outcome
    }

    fn account(&self, v: usize, id: &AccountId) -> orien_ledger::Account {
        self.validators[v].state.account(id).unwrap().clone()
    }
}

#[test]
fn a_double_signed_account_carries_on_with_a_spare_key() {
    let (alice, bob, carol) = (User::new(1), User::new(2), User::new(3));
    let mut net = Net::four(&[(&alice, 100)]);
    // Alice's very first order is signed twice, and the vote splits.
    let to_bob = alice.order(&net, 0, bob.id(), 10);
    let to_carol = alice.order(&net, 0, carol.id(), 10);
    for v in [0, 1] {
        net.vote(v, &to_bob).unwrap();
    }
    for v in [2, 3] {
        net.vote(v, &to_carol).unwrap();
    }
    let before = net.validators[0].state.root();

    net.cancel_everywhere(&to_bob, &to_carol).unwrap();
    let account = net.account(0, &alice.id());
    assert_eq!((account.balance, account.seq), (100, 0), "nothing moved and the number stays");
    assert_eq!((account.signer, account.spares_used), (Signer::Spare(0), 1));
    assert_ne!(net.validators[0].state.root(), before, "the cancellation shows in the state root");

    // The burned key signs nothing any more: not the two orders, and not a
    // third that someone who learned to forge that key might make.
    let forged = alice.order(&net, 0, carol.id(), 99);
    for order in [&to_bob, &to_carol, &forged] {
        assert_eq!(net.vote(0, order), Err(Reject::BadSignature));
    }
    // Nor does the wrong spare key.
    assert_eq!(net.vote(0, &alice.order_with_spare(&net, 0, 1, bob.id(), 30)), Err(Reject::BadSignature));

    // Spare key 0 does, under the same number, and the payment goes through.
    let retry = alice.order_with_spare(&net, 0, 0, bob.id(), 30);
    let certificate = net.certify(&retry, &[0, 1, 2, 3]);
    assert_eq!(net.committee.verify_certificate(&certificate), Ok(()));
    net.deliver_all(&certificate);
    assert_eq!((net.balance(0, &alice.id()), net.balance(0, &bob.id())), (70, 30));

    // After that the account is back on ordinary keys.
    assert_eq!(net.account(0, &alice.id()).signer, Signer::Key(alice.key(1).commitment()));
    let next = alice.order(&net, 1, carol.id(), 5);
    let certificate = net.certify(&next, &[0, 1, 2]);
    net.deliver_all(&certificate);
    assert_eq!(net.balance(3, &carol.id()), 5);
}

#[test]
fn cancelling_needs_two_orders_the_owner_really_signed() {
    let (alice, bob, carol) = (User::new(1), User::new(2), User::new(3));
    let mut net = Net::four(&[(&alice, 100), (&bob, 100)]);
    // Bring alice to number 1 so the account is on an ordinary key.
    let first = alice.order(&net, 0, bob.id(), 1);
    let certificate = net.certify(&first, &[0, 1, 2]);
    net.deliver_all(&certificate);

    let genuine = alice.order(&net, 1, bob.id(), 10);
    let second = alice.order(&net, 1, carol.id(), 10);
    let not_evidence = [
        ("the same order twice", genuine.clone()),
        ("an order under another number", alice.order(&net, 2, carol.id(), 10)),
        ("an order under a number already final", alice.order_with_key(&net, 0, 0, carol.id(), 10)),
        ("an order signed with some other key", alice.order_with_key(&net, 1, 7, carol.id(), 10)),
        ("an order signed with a spare key", alice.order_with_spare(&net, 1, 0, carol.id(), 10)),
        ("another account's order", bob.order(&net, 0, carol.id(), 10)),
    ];
    for (what, other) in &not_evidence {
        assert_eq!(net.cancel_everywhere(&genuine, other), Err(CancelError::NotEvidence), "{what}");
        assert_eq!(net.cancel_everywhere(other, &genuine), Err(CancelError::NotEvidence), "{what}");
    }
    assert_eq!(net.account(0, &alice.id()).signer, Signer::Key(alice.key(1).commitment()));

    // Once one of two signed orders is final, the pair cancels nothing.
    let certificate = net.certify(&genuine, &[0, 1, 2]);
    net.deliver_all(&certificate);
    assert_eq!(net.cancel_everywhere(&genuine, &second), Err(CancelError::NotEvidence));
    assert_eq!(net.balance(0, &bob.id()), 111);

    // Two orders that really were signed under the current number do.
    let (one, other) = (alice.order(&net, 2, bob.id(), 3), alice.order(&net, 2, carol.id(), 3));
    assert_eq!(net.cancel_everywhere(&one, &other), Ok(()));
    assert_eq!(net.account(0, &alice.id()).signer, Signer::Spare(0));
}

#[test]
fn cancelling_drops_the_votes_promised_under_that_number() {
    let (alice, bob, carol) = (User::new(1), User::new(2), User::new(3));
    let mut net = Net::four(&[(&alice, 100)]);
    let (to_bob, to_carol) = (alice.order(&net, 0, bob.id(), 10), alice.order(&net, 0, carol.id(), 10));
    net.vote(0, &to_bob).unwrap();
    let digest = to_bob.order.digest(&net.network);
    assert_eq!(net.validators[0].state.lock(&alice.id(), 0), Some(&digest));

    net.cancel_everywhere(&to_bob, &to_carol).unwrap();
    assert_eq!(net.validators[0].state.lock(&alice.id(), 0), None);
    // The validator is free to vote for the order signed with the spare.
    assert!(net.vote(0, &alice.order_with_spare(&net, 0, 0, carol.id(), 10)).is_ok());
}

#[test]
fn every_double_signing_uses_up_one_spare_key() {
    let (alice, bob, carol) = (User::new(1), User::new(2), User::new(3));
    let mut net = Net::four(&[(&alice, 100)]);
    let (one, other) = (alice.order(&net, 0, bob.id(), 1), alice.order(&net, 0, carol.id(), 1));
    net.cancel_everywhere(&one, &other).unwrap();

    // The owner goes on signing twice with each spare key in turn.
    let spares = 1u32 << SPARE_HEIGHT;
    for leaf in 0..spares {
        assert_eq!(net.account(0, &alice.id()).signer, Signer::Spare(leaf));
        let one = alice.order_with_spare(&net, 0, leaf, bob.id(), 1);
        let other = alice.order_with_spare(&net, 0, leaf, carol.id(), 1);
        let outcome = net.cancel_everywhere(&one, &other);
        if leaf + 1 < spares {
            assert_eq!(outcome, Ok(()));
        } else {
            // The last spare key was the last chance. The account stays
            // where it is, and nobody else is affected.
            assert_eq!(outcome, Err(CancelError::NoSpareLeft));
        }
    }
    let account = net.account(0, &alice.id());
    assert_eq!((account.signer, account.spares_used, account.balance), (Signer::Spare(spares - 1), spares, 100));
}

#[test]
fn an_address_commits_to_the_spare_keys() {
    let (alice, bob, mallory) = (User::new(1), User::new(2), User::new(9));
    let mut net = Net::four(&[(&alice, 100)]);

    // Alice's first order reveals her spare root. An order naming someone
    // else's spare keys is not hers, even signed with her real first key.
    let mut swapped = alice.unsigned(0, bob.id(), 10);
    swapped.spare = mallory.spares.root();
    let digest = swapped.digest(&net.network);
    let swapped = SignedOrder { signature: Signature::Key(alice.key(0).sign(digest.as_bytes())), order: swapped };
    assert_eq!(net.vote(0, &swapped), Err(Reject::BadSignature));

    // Once the root is on record, every later order must repeat it.
    let first = alice.order(&net, 0, bob.id(), 10);
    let certificate = net.certify(&first, &[0, 1, 2]);
    net.deliver_all(&certificate);
    assert_eq!(net.account(0, &alice.id()).spare, alice.spares.root());
    let mut later = alice.unsigned(1, bob.id(), 10);
    later.spare = mallory.spares.root();
    let digest = later.digest(&net.network);
    let later = SignedOrder { signature: Signature::Key(alice.key(1).sign(digest.as_bytes())), order: later };
    assert_eq!(net.vote(0, &later), Err(Reject::BadSignature));
    assert!(net.vote(0, &alice.order(&net, 1, bob.id(), 10)).is_ok());

    // Different spare keys make a different address.
    assert_ne!(alice.id(), address(&alice.key(0).commitment(), &mallory.spares.root()));
}
