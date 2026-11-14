//! The state fingerprint, account proofs and snapshots.

use orien_crypto::Hash32;
use orien_ledger::{
    Accepted, Allocation, Genesis, Kind, Order, Reject, SignedBy, Signer, State, account_root, address,
};

// These tests never check a signature, so an account's first key and spare
// root are any two values, and its address is the commitment to them.
fn first_key(n: u8) -> Hash32 {
    Hash32([n; 32])
}

fn spare(n: u8) -> Hash32 {
    Hash32([n ^ 0xff; 32])
}

fn id(n: u8) -> Hash32 {
    address(&first_key(n), &spare(n))
}

fn genesis(balances: &[(u8, u64)]) -> Genesis {
    Genesis {
        network: "state-test".into(),
        validators: Vec::new(),
        allocations: balances
            .iter()
            .map(|(n, amount)| Allocation { account: id(*n), amount: *amount })
            .collect(),
    }
}

fn pay(from: u8, seq: u64, to: u8, amount: u64) -> Order {
    Order {
        kind: Kind::Payment,
        from: id(from),
        seq,
        to: id(to),
        amount,
        max_fee: 0,
        next_key: first_key(from.wrapping_add(100 + seq as u8)),
        spare: spare(from),
    }
}

fn applied(genesis: &Genesis, orders: &[&Order]) -> State {
    let mut state = State::from_genesis(genesis);
    for order in orders {
        state.apply(order).unwrap();
    }
    state
}

#[test]
fn the_root_does_not_depend_on_the_order_payments_arrive() {
    let genesis = genesis(&[(1, 100), (2, 100)]);
    let (a0, a1, b0, c0) = (pay(1, 0, 2, 10), pay(1, 1, 3, 1), pay(2, 0, 3, 5), pay(3, 0, 4, 2));

    // Two validators learn of the same four payments in different orders.
    // The second even hears of account 3's spend before the payments that
    // funded it.
    let one = applied(&genesis, &[&a0, &a1, &b0, &c0]);
    let two = applied(&genesis, &[&c0, &b0, &a0, &a1]);
    assert_eq!(one.root(), two.root());
    assert_eq!(one.total(), 200);

    // Any difference in what was applied shows in the root.
    assert_ne!(one.root(), State::from_genesis(&genesis).root());
    assert_ne!(one.root(), applied(&genesis, &[&a0, &a1, &b0]).root());
    let other_amount = pay(3, 0, 4, 3);
    assert_ne!(one.root(), applied(&genesis, &[&a0, &a1, &b0, &other_amount]).root());
}

#[test]
fn an_account_proof_holds_only_for_the_true_state() {
    let genesis = genesis(&[(1, 100), (2, 100), (5, 7)]);
    let state = applied(&genesis, &[&pay(1, 0, 2, 10), &pay(2, 0, 3, 5)]);
    let root = state.root();

    for n in [1, 2, 3, 5] {
        let (account, proof) = state.prove(&id(n)).unwrap();
        assert_eq!(account_root(&id(n), &account, &proof), Some(root), "account {n}");

        // The same proof does not stretch to a different balance, payment
        // number, key or account.
        let mut richer = account.clone();
        richer.balance += 1;
        assert_ne!(account_root(&id(n), &richer, &proof), Some(root));
        let mut earlier = account.clone();
        earlier.seq += 1;
        assert_ne!(account_root(&id(n), &earlier, &proof), Some(root));
        let mut rekeyed = account.clone();
        rekeyed.signer = Signer::Key(id(99));
        assert_ne!(account_root(&id(n), &rekeyed, &proof), Some(root));
        let mut other_spares = account.clone();
        other_spares.spare = id(98);
        assert_ne!(account_root(&id(n), &other_spares, &proof), Some(root));
        let mut cancelled = account.clone();
        cancelled.spares_used += 1;
        assert_ne!(account_root(&id(n), &cancelled, &proof), Some(root));
        assert_ne!(account_root(&id(n + 10), &account, &proof), Some(root));
    }
    assert!(state.prove(&id(42)).is_none(), "an account that never held funds has no proof");
    assert_eq!(state.prove(&id(1)).unwrap().0.balance, 90);
}

#[test]
fn a_ledger_with_no_accounts_still_has_a_root() {
    let empty = State::from_genesis(&genesis(&[]));
    assert_eq!(empty.root(), State::from_genesis(&genesis(&[])).root());
    assert_ne!(empty.root(), State::from_genesis(&genesis(&[(1, 1)])).root());
}

#[test]
fn a_snapshot_restores_the_same_state() {
    let genesis = genesis(&[(1, 100), (2, 100)]);
    let state = applied(&genesis, &[&pay(1, 0, 2, 10), &pay(2, 0, 3, 150)]);
    assert_eq!(state.account(&id(2)).unwrap().balance, -40, "a validator catching up can dip below zero");

    let mut restored = State::from_snapshot(&state.snapshot()).unwrap();
    assert_eq!(restored.root(), state.root());
    assert_eq!(restored.accounts(), state.accounts());
    assert_eq!(restored.account(&id(2)), state.account(&id(2)));
    // It carries on exactly as the original would.
    let next = pay(1, 1, 3, 4);
    let mut original = applied(&genesis, &[&pay(1, 0, 2, 10), &pay(2, 0, 3, 150)]);
    original.apply(&next).unwrap();
    restored.apply(&next).unwrap();
    assert_eq!(restored.root(), original.root());

    assert!(State::from_snapshot(&[1, 2, 3]).is_err());
}

#[test]
fn a_repeated_vote_still_needs_the_funds() {
    let order = pay(1, 0, 2, 10);
    let digest = order.digest(&id(0));

    // The validator promised its vote while the account held enough.
    let mut funded = State::from_genesis(&genesis(&[(1, 10)]));
    assert_eq!(funded.check_order(&order, digest, &SignedBy::Key(first_key(1))), Ok(Accepted::Fresh));
    assert_eq!(funded.check_order(&order, digest, &SignedBy::Key(first_key(1))), Ok(Accepted::Repeat));

    // The same promise, restored on a state where the money is not there:
    // what a checkpoint leaves behind when it undoes the funding payment.
    let mut unfunded = State::from_genesis(&genesis(&[(1, 9)]));
    unfunded.restore_lock(id(1), 0, digest);
    assert_eq!(
        unfunded.check_order(&order, digest, &SignedBy::Key(first_key(1))),
        Err(Reject::InsufficientFunds { balance: 9, needed: 10 })
    );

    // A second order under the same number is reported as a conflict even
    // when it also overspends: it is proof the owner signed twice.
    let second = pay(1, 0, 3, 1000);
    assert_eq!(
        funded.check_order(&second, second.digest(&id(0)), &SignedBy::Key(first_key(1))),
        Err(Reject::Conflict { voted: digest })
    );
}
