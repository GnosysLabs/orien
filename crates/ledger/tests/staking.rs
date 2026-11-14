//! Staking: locking coins behind a validator, and getting them back after
//! the wait. Run with `--nocapture` to see one account's coins step by step.

use orien_crypto::Hash32;
use orien_ledger::{
    Accepted, Allocation, Genesis, Kind, Order, Reject, SignedBy, State, Validator, address,
};

// These tests never check a signature, so an account's keys are any values,
// and its address is the commitment to its first key and spare root.
fn key(account: u8, seq: u64) -> Hash32 {
    Hash32([account.wrapping_add(seq as u8 * 16); 32])
}

fn spare(account: u8) -> Hash32 {
    Hash32([account ^ 0xff; 32])
}

fn id(account: u8) -> Hash32 {
    address(&key(account, 0), &spare(account))
}

fn genesis(balances: &[(u8, u64)]) -> Genesis {
    let mut genesis = Genesis::new("staking", Vec::new(), Vec::new());
    genesis.allocations =
        balances.iter().map(|(account, amount)| Allocation { account: id(*account), amount: *amount }).collect();
    genesis
}

fn validator(port: u16) -> Validator {
    Validator { root: Hash32([7; 32]), height: 10, address: format!("100.64.0.1:{port}") }
}

/// An order, with the key that signs it.
fn order(kind: Kind, from: u8, seq: u64, to: u8, amount: u64) -> (Order, SignedBy) {
    let order = Order {
        kind,
        from: id(from),
        seq,
        to: id(to),
        amount,
        max_fee: 0,
        next_key: key(from, seq + 1),
        spare: spare(from),
    };
    (order, SignedBy::Key(key(from, seq)))
}

/// Votes for an order, as a validator would, and applies it as if it went
/// final.
fn vote_and_apply(state: &mut State, (order, by): &(Order, SignedBy)) -> Result<(), Reject> {
    let digest = Hash32([order.seq as u8; 32]);
    assert_eq!(state.check_order(order, digest, by)?, Accepted::Fresh);
    state.apply(order).unwrap();
    Ok(())
}

fn show(state: &State, when: &str) -> (i128, u64, u64) {
    let account = state.account(&id(1)).unwrap();
    let waiting: u64 = account.unstaking + account.releases.iter().map(|(_, amount)| amount).sum::<u64>();
    println!("{when:<44} spendable {:>6}  staked {:>6}  waiting {:>6}", account.balance, account.staked, waiting);
    (account.balance, account.staked, waiting)
}

#[test]
fn stake_then_unstake_and_wait() {
    let genesis = genesis(&[(1, 25_000)]);
    let wait = genesis.unstake_epochs;
    let mut state = State::from_genesis(&genesis);

    assert_eq!(show(&state, "at the start"), (25_000, 0, 0));

    vote_and_apply(&mut state, &order(Kind::Stake(validator(7001)), 1, 0, 1, 12_000)).unwrap();
    assert_eq!(show(&state, "after staking 12,000"), (13_000, 12_000, 0));
    assert_eq!(state.account(&id(1)).unwrap().validator, Some(validator(7001)));

    vote_and_apply(&mut state, &order(Kind::Unstake, 1, 1, 1, 5_000)).unwrap();
    assert_eq!(show(&state, "after unstaking 5,000 in epoch 3"), (13_000, 7_000, 5_000));

    state.settle_stakes(3, wait);
    assert_eq!(show(&state, "checkpoint 3: the wait starts"), (13_000, 7_000, 5_000));
    state.settle_stakes(3 + wait - 1, wait);
    assert_eq!(show(&state, &format!("checkpoint {}: one epoch to go", 3 + wait - 1)), (13_000, 7_000, 5_000));
    state.settle_stakes(3 + wait, wait);
    assert_eq!(show(&state, &format!("checkpoint {}: the wait is over", 3 + wait)), (18_000, 7_000, 0));

    assert_eq!(state.total(), 25_000, "staking never creates or destroys coins");
}

#[test]
fn locked_coins_cannot_be_spent_or_unstaked_twice() {
    let genesis = genesis(&[(1, 25_000)]);
    let mut state = State::from_genesis(&genesis);
    vote_and_apply(&mut state, &order(Kind::Stake(validator(7001)), 1, 0, 1, 20_000)).unwrap();

    // 5,000 are spendable; the staked 20,000 are not.
    assert_eq!(
        vote_and_apply(&mut state, &order(Kind::Payment, 1, 1, 2, 5_001)),
        Err(Reject::InsufficientFunds { balance: 5_000, needed: 5_001 })
    );
    assert_eq!(
        vote_and_apply(&mut state, &order(Kind::Unstake, 1, 1, 1, 20_001)),
        Err(Reject::InsufficientStake { staked: 20_000, needed: 20_001 })
    );
    vote_and_apply(&mut state, &order(Kind::Unstake, 1, 1, 1, 20_000)).unwrap();
    // Coins on their way back are still locked.
    assert_eq!(
        vote_and_apply(&mut state, &order(Kind::Unstake, 1, 2, 1, 1)),
        Err(Reject::InsufficientStake { staked: 0, needed: 1 })
    );
    assert_eq!(
        vote_and_apply(&mut state, &order(Kind::Payment, 1, 2, 2, 5_001)),
        Err(Reject::InsufficientFunds { balance: 5_000, needed: 5_001 })
    );
}

#[test]
fn a_stake_order_names_its_own_account_and_a_usable_validator() {
    let genesis = genesis(&[(1, 25_000)]);
    let mut state = State::from_genesis(&genesis);
    let elsewhere = order(Kind::Stake(validator(7001)), 1, 0, 2, 10_000);
    assert_eq!(vote_and_apply(&mut state, &elsewhere), Err(Reject::NotOwnAccount));
    let unstake_elsewhere = order(Kind::Unstake, 1, 0, 2, 1);
    assert_eq!(vote_and_apply(&mut state, &unstake_elsewhere), Err(Reject::NotOwnAccount));
    let nowhere = Validator { address: String::new(), ..validator(7001) };
    assert_eq!(vote_and_apply(&mut state, &order(Kind::Stake(nowhere), 1, 0, 1, 10_000)), Err(Reject::BadValidator));
    let too_tall = Validator { height: 200, ..validator(7001) };
    assert_eq!(vote_and_apply(&mut state, &order(Kind::Stake(too_tall), 1, 0, 1, 10_000)), Err(Reject::BadValidator));
    assert_eq!(state.account(&id(1)).unwrap().staked, 0);
}
