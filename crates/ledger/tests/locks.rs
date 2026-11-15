//! Locked payments: claimable with a secret until a deadline, refunded at
//! the checkpoint after it. Run with `--nocapture` to follow the coins.

use orien_crypto::Hash32;
use orien_ledger::{
    Accepted, Allocation, FEES, Fees, Genesis, Kind, Order, Reject, SignedBy, State, address, lock_hash,
};

fn key(account: u8, seq: u64) -> Hash32 {
    Hash32([account.wrapping_add(seq as u8 * 16); 32])
}

fn spare(account: u8) -> Hash32 {
    Hash32([account ^ 0xff; 32])
}

fn id(account: u8) -> Hash32 {
    address(&key(account, 0), &spare(account))
}

fn order(kind: Kind, from: u8, seq: u64, to: u8, amount: u64) -> (Order, SignedBy) {
    let order = Order {
        kind,
        from: id(from),
        seq,
        to: id(to),
        amount,
        max_fee: 1_000,
        next_key: key(from, seq + 1),
        spare: spare(from),
    };
    (order, SignedBy::Key(key(from, seq)))
}

const SECRET: [u8; 32] = [42; 32];
const ALICE: u8 = 1;
const BOB: u8 = 2;
const CAROL: u8 = 3;

fn lock(seq: u64, amount: u64, until: u64) -> (Order, SignedBy) {
    order(Kind::Lock { hash: lock_hash(&SECRET), until }, ALICE, seq, BOB, amount)
}

fn claim(by: u8, seq: u64, amount: u64, secret: [u8; 32]) -> (Order, SignedBy) {
    order(Kind::Claim { from: id(ALICE), seq: 0, secret }, by, seq, by, amount)
}

fn vote(state: &mut State, (order, by): &(Order, SignedBy)) -> Result<Accepted, Reject> {
    state.check_order(order, Hash32([order.seq as u8 ^ order.from.0[0]; 32]), by)
}

fn vote_and_apply(state: &mut State, signed: &(Order, SignedBy)) -> Result<(), Reject> {
    assert_eq!(vote(state, signed)?, Accepted::Fresh);
    state.apply(&signed.0).unwrap();
    Ok(())
}

fn network() -> State {
    let mut genesis = Genesis::new("locks", Vec::new(), Vec::new());
    genesis.fee = Fees { min: 2, max: 50, rate: 1_000 };
    genesis.allocations = vec![Allocation { account: id(ALICE), amount: 100_000 }];
    let mut state = State::from_genesis(&genesis);
    state.set_epoch(10);
    state
}

fn balance(state: &State, n: u8) -> i128 {
    state.account(&id(n)).map_or(0, |a| a.balance)
}

#[test]
fn a_locked_payment_is_claimed_with_its_secret() {
    let mut state = network();
    let total = state.total();
    // Alice locks 10,000 for Bob until epoch 12; it pays 0.1%, 10.
    vote_and_apply(&mut state, &lock(0, 10_000, 12)).unwrap();
    assert_eq!(balance(&state, ALICE), 100_000 - 10_000 - 10);
    assert_eq!(state.escrow(&id(ALICE), 0).unwrap().amount, 10_000);
    assert_eq!(state.total(), total, "locked coins still count");

    // Bob, who holds nothing at all, claims with the secret. The claim's
    // fee, the minimum of 2, comes out of the locked amount.
    vote_and_apply(&mut state, &claim(BOB, 0, 10_000, SECRET)).unwrap();
    println!("alice {}, bob {}, fees {}", balance(&state, ALICE), balance(&state, BOB), state.account(&FEES).unwrap().balance);
    assert_eq!(balance(&state, BOB), 10_000 - 2);
    assert_eq!(state.account(&FEES).unwrap().balance, 12);
    assert!(state.escrow(&id(ALICE), 0).is_none());
    assert_eq!(state.revealed(&id(ALICE), 0), Some(SECRET), "the other side of a swap reads it here");
    assert_eq!(state.total(), total);

    // It cannot be claimed twice.
    assert_eq!(vote(&mut state, &claim(BOB, 1, 10_000, SECRET)), Err(Reject::NoSuchLock));
}

#[test]
fn a_claim_needs_the_right_payee_amount_secret_and_time() {
    let mut state = network();
    vote_and_apply(&mut state, &lock(0, 10_000, 12)).unwrap();
    assert_eq!(vote(&mut state, &claim(BOB, 0, 10_000, [7; 32])), Err(Reject::WrongSecret));
    assert_eq!(vote(&mut state, &claim(BOB, 0, 9_999, SECRET)), Err(Reject::NoSuchLock));
    assert_eq!(vote(&mut state, &claim(CAROL, 0, 10_000, SECRET)), Err(Reject::NoSuchLock), "only the payee");
    // From epoch 12 on, the time is up.
    state.set_epoch(12);
    assert_eq!(vote(&mut state, &claim(BOB, 0, 10_000, SECRET)), Err(Reject::LockExpired { until: 12 }));
}

#[test]
fn a_lock_needs_a_sensible_deadline_and_amount() {
    let mut state = network();
    // From epoch 10: at least 2 epochs, at most 720.
    let bad = Reject::BadDeadline { earliest: 12, latest: 730 };
    assert_eq!(vote(&mut state, &lock(0, 10_000, 11)), Err(bad.clone()));
    assert_eq!(vote(&mut state, &lock(0, 10_000, 731)), Err(bad));
    // Too small to pay for its own claim.
    assert_eq!(vote(&mut state, &lock(0, 2, 12)), Err(Reject::LockTooSmall { least: 2 }));
    // Closed until the network opens them.
    state.set_locks_from(20);
    assert_eq!(vote(&mut state, &lock(0, 10_000, 12)), Err(Reject::LocksNotYet { from: 20 }));
    state.set_locks_from(0);
    assert_eq!(vote(&mut state, &lock(0, 10_000, 12)), Ok(Accepted::Fresh));
}

#[test]
fn an_unclaimed_lock_goes_back_at_the_checkpoint_after_its_deadline() {
    let mut state = network();
    let total = state.total();
    vote_and_apply(&mut state, &lock(0, 10_000, 12)).unwrap();
    // The checkpoint closing epoch 10 leaves it; the one closing epoch 11,
    // the last epoch it can be claimed in, sends it back.
    state.refund_expired(10);
    assert!(state.escrow(&id(ALICE), 0).is_some());
    state.refund_expired(11);
    assert!(state.escrow(&id(ALICE), 0).is_none());
    assert_eq!(balance(&state, ALICE), 100_000 - 10, "only the lock's fee is spent");
    assert_eq!(state.total(), total);
}

#[test]
fn a_claim_that_arrives_before_its_lock_still_adds_up() {
    // A validator catching up can learn of the claim first.
    let mut voters = network();
    vote_and_apply(&mut voters, &lock(0, 10_000, 12)).unwrap();
    assert_eq!(vote(&mut voters, &claim(BOB, 0, 10_000, SECRET)), Ok(Accepted::Fresh));

    let mut late = network();
    let total = late.total();
    late.apply(&claim(BOB, 0, 10_000, SECRET).0).unwrap();
    late.apply(&lock(0, 10_000, 12).0).unwrap();
    assert!(late.escrow(&id(ALICE), 0).is_none(), "nothing is left to hold");
    assert_eq!(balance(&late, BOB), 10_000 - 2);
    assert_eq!(balance(&late, ALICE), 100_000 - 10_000 - 10);
    assert_eq!(late.total(), total);
    // And it ends where the validators that saw the lock first end.
    voters.apply(&claim(BOB, 0, 10_000, SECRET).0).unwrap();
    assert_eq!(late.root(), voters.root());
}

#[test]
fn revealed_secrets_are_kept_for_a_week_of_epochs() {
    let mut state = network();
    vote_and_apply(&mut state, &lock(0, 10_000, 12)).unwrap();
    vote_and_apply(&mut state, &claim(BOB, 0, 10_000, SECRET)).unwrap();
    state.refund_expired(10 + 167);
    assert_eq!(state.revealed(&id(ALICE), 0), Some(SECRET));
    state.refund_expired(10 + 168);
    assert_eq!(state.revealed(&id(ALICE), 0), None);
}

#[test]
fn snapshots_carry_locks_and_older_snapshots_still_read() {
    let mut state = network();
    let before = state.root();
    // A snapshot from before locked payments: the accounts and barred keys.
    let fees = Fees { min: 2, max: 50, rate: 1_000 };
    let old = State::from_snapshot(&state.snapshot(), fees).unwrap();
    assert_eq!(old.root(), before);
    let accounts_and_bans = {
        let full = state.snapshot();
        // The new snapshot ends with two empty maps, four bytes each.
        full[..full.len() - 8].to_vec()
    };
    assert_eq!(State::from_snapshot(&accounts_and_bans, fees).unwrap().root(), before);

    // With a lock waiting, the root covers it, and a snapshot keeps it.
    vote_and_apply(&mut state, &lock(0, 10_000, 12)).unwrap();
    let restored = State::from_snapshot(&state.snapshot(), fees).unwrap();
    assert_eq!(restored.root(), state.root());
    assert_eq!(restored.escrow(&id(ALICE), 0), state.escrow(&id(ALICE), 0));
}

#[test]
fn locked_payments_and_secrets_are_proven_against_the_state_root() {
    let mut state = network();
    // Nothing locked: nothing to show, and nothing hidden.
    let (escrows, revealed) = state.proofs().locks().open(&state.root()).unwrap();
    assert!(escrows.is_empty() && revealed.is_empty());

    vote_and_apply(&mut state, &lock(0, 10_000, 12)).unwrap();
    vote_and_apply(&mut state, &lock(1, 5_000, 20)).unwrap();
    vote_and_apply(&mut state, &claim(BOB, 0, 10_000, SECRET)).unwrap();
    let root = state.root();
    let proof = state.proofs().locks().clone();
    let (escrows, revealed) = proof.open(&root).unwrap();
    assert_eq!(escrows.get(&(id(ALICE), 1)).map(|e| e.amount), Some(5_000));
    assert_eq!(revealed.get(&(id(ALICE), 0)).map(|r| r.0), Some(SECRET));

    // A changed list, or a list moved to another place, proves nothing.
    let mut forged = proof.clone();
    *forged.escrows.as_mut().unwrap().0.last_mut().unwrap() ^= 1;
    assert!(forged.open(&root).is_none());
    let mut swapped = proof.clone();
    swapped.escrows = proof.revealed.clone();
    assert!(swapped.open(&root).is_none());
    // Nor does it hold under another root.
    assert!(proof.open(&network().root()).is_none());
}
