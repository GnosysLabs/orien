//! Staking: locking coins behind a validator, and getting them back after
//! the wait. Run with `--nocapture` to see one account's coins step by step.

use orien_crypto::Hash32;
use orien_crypto::keytree::KeyTree;
use orien_ledger::close;
use orien_ledger::{
    Accepted, Allocation, Committee, Genesis, Kind, Member, Order, Reject, SignedBy, State, Validator,
    ValidatorInfo, address, stake_proof_item,
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

/// The key tree of the validator run by the owner of account `n`.
fn tree(n: u8) -> KeyTree {
    KeyTree::generate([n; 32], 2)
}

fn validator(n: u8) -> Validator {
    Validator { root: tree(n).root(), height: 2, address: format!("100.64.1.{n}:7001") }
}

/// A stake order's kind: account `from` stakes behind `validator`, which
/// signed for it with the key tree of validator `signer`.
fn stake(network: &Hash32, from: u8, validator: Validator, signer: u8) -> Kind {
    let item = stake_proof_item(network, &id(from), &validator.address);
    Kind::Stake { proof: tree(signer).sign_with_leaf(0, item.as_bytes()), validator }
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

    vote_and_apply(&mut state, &order(stake(&genesis.id(), 1, validator(1), 1), 1, 0, 1, 12_000)).unwrap();
    assert_eq!(show(&state, "after staking 12,000"), (13_000, 12_000, 0));
    assert_eq!(state.account(&id(1)).unwrap().validator, Some(validator(1)));

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
    vote_and_apply(&mut state, &order(stake(&genesis.id(), 1, validator(1), 1), 1, 0, 1, 20_000)).unwrap();

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
    let network = genesis.id();
    let elsewhere = order(stake(&network, 1, validator(1), 1), 1, 0, 2, 10_000);
    assert_eq!(vote_and_apply(&mut state, &elsewhere), Err(Reject::NotOwnAccount));
    let unstake_elsewhere = order(Kind::Unstake, 1, 0, 2, 1);
    assert_eq!(vote_and_apply(&mut state, &unstake_elsewhere), Err(Reject::NotOwnAccount));
    let nowhere = Validator { address: String::new(), ..validator(1) };
    let nowhere = order(stake(&network, 1, nowhere, 1), 1, 0, 1, 10_000);
    assert_eq!(vote_and_apply(&mut state, &nowhere), Err(Reject::BadValidator));
    let too_tall = Validator { height: 200, ..validator(1) };
    let too_tall = order(stake(&network, 1, too_tall, 1), 1, 0, 1, 10_000);
    assert_eq!(vote_and_apply(&mut state, &too_tall), Err(Reject::BadValidator));
    assert_eq!(state.account(&id(1)).unwrap().staked, 0);
}

/// Closes an epoch in which `orders` went final, as every validator would,
/// and prints the validators it names for the next one.
fn checkpoint(committee: &Committee, state: &State, orders: &[Order], names: &[(u8, &str)]) -> (Committee, State) {
    let orders: Vec<&Order> = orders.iter().collect();
    let end = close::outcome_checked(committee, state, &orders, &[]).unwrap();
    let members = close::next_validators(committee, &[], &end);
    let named: Vec<String> = members
        .iter()
        .map(|m| {
            let name = names.iter().find(|(n, _)| label(m) == *n).map_or("?", |(_, name)| name);
            format!("{name} ({}, {})", m.stake, m.address)
        })
        .collect();
    println!("checkpoint {}: validators of epoch {}: {}", committee.epoch(), committee.epoch() + 1, named.join(", "));
    (committee.following(members), end)
}

/// Who a member is: the number of the account that staked for it, or for a
/// launch seat the first byte of its key.
fn label(member: &Member) -> u8 {
    match member.account {
        Some(account) => (0..=255).find(|n| id(*n) == account).unwrap(),
        None => member.root.0[0],
    }
}

fn roots(members: &[Member]) -> Vec<u8> {
    members.iter().map(label).collect()
}

#[test]
fn the_validators_follow_the_stakes() {
    let names = [(11, "seat-1"), (12, "seat-2"), (1, "ana"), (2, "ben"), (3, "cy"), (4, "dee")];
    let seat = |n: u8| ValidatorInfo {
        name: format!("seat-{n}"),
        root: Hash32([10 + n; 32]),
        height: 10,
        stake: 100_000,
        address: format!("100.64.0.{n}:7001"),
    };
    let mut genesis = genesis(&[(1, 50_000), (2, 30_000), (3, 20_000), (4, 5_000)]);
    genesis.validators = vec![seat(1), seat(2)];
    // A small limit, so that the test shows someone being squeezed out.
    genesis.max_validators = 4;
    genesis.validate().unwrap();
    let network = genesis.id();
    let stake = |n: u8, seq: u64, amount: u64| order(stake(&network, n, validator(n), n), n, seq, n, amount).0;

    let committee = Committee::new(&genesis);
    let state = State::from_genesis(&genesis);
    // Ana and Ben stake enough; Dee's 5,000 is under the minimum of 10,000.
    let (committee, state) =
        checkpoint(&committee, &state, &[stake(1, 0, 40_000), stake(2, 0, 15_000), stake(4, 0, 5_000)], &names);
    assert_eq!(roots(committee.members()), [11, 12, 1, 2]);
    assert_eq!(committee.members()[2].address, "100.64.1.1:7001");

    // Cy stakes more than Ben, and only four fit.
    let (committee, state) = checkpoint(&committee, &state, &[stake(3, 0, 20_000)], &names);
    assert_eq!(roots(committee.members()), [11, 12, 1, 3]);

    // Ana unstakes everything and leaves. Ben is back in.
    let unstake = order(Kind::Unstake, 1, 1, 1, 40_000).0;
    let (committee, state) = checkpoint(&committee, &state, &[unstake], &names);
    assert_eq!(roots(committee.members()), [11, 12, 3, 2]);

    // Her coins come back once the wait is over, counted from the epoch she
    // unstaked in.
    let ana = |state: &State| state.account(&id(1)).unwrap().balance;
    assert_eq!(ana(&state), 10_000);
    let mut state = state;
    state.settle_stakes(2 + genesis.unstake_epochs, genesis.unstake_epochs);
    assert_eq!(ana(&state), 50_000);
    assert_eq!(committee.epoch(), 3);
}

#[test]
fn a_stake_needs_the_validators_own_signature() {
    let genesis = genesis(&[(1, 25_000), (2, 25_000)]);
    let network = genesis.id();
    let holds = |kind: Kind, from: u8| order(kind, from, 0, from, 10_000).0.stake_proof_holds(&network);

    assert!(holds(stake(&network, 1, validator(1), 1), 1));
    // Naming someone else's validator, without its signature.
    assert!(!holds(stake(&network, 2, validator(1), 2), 2));
    // A real signature, but given for another account, another address or
    // another network.
    let Kind::Stake { proof: for_ana, .. } = stake(&network, 1, validator(1), 1) else { unreachable!() };
    assert!(!holds(Kind::Stake { validator: validator(1), proof: for_ana.clone() }, 2));
    let moved = Validator { address: "100.64.9.9:7001".into(), ..validator(1) };
    assert!(!holds(Kind::Stake { validator: moved, proof: for_ana }, 1));
    assert!(!holds(stake(&Hash32([0; 32]), 1, validator(1), 1), 1));
    // Payments and unstake orders carry no proof.
    assert!(holds(Kind::Payment, 1));
    assert!(holds(Kind::Unstake, 1));
}

/// Closes an epoch in which `orders` went final, and prints each
/// validator's share of the vote in the next one.
fn shares(committee: &Committee, state: &State, orders: &[Order], names: &[(u8, &str)], what: &str) -> (Committee, State) {
    let orders: Vec<&Order> = orders.iter().collect();
    let end = close::outcome_checked(committee, state, &orders, &[]).unwrap();
    let next = committee.following(close::next_validators(committee, &[], &end));
    let total = next.total_stake() as f64;
    let named: Vec<String> = next
        .members()
        .iter()
        .map(|m| {
            let name = names.iter().find(|(n, _)| label(m) == *n).map_or("?", |(_, name)| name);
            format!("{name} {:.1}%", 100.0 * m.stake as f64 / total)
        })
        .collect();
    println!("{what:<34} {}", named.join(", "));
    (next, end)
}

#[test]
fn the_launch_seats_fade_as_real_stake_arrives() {
    let names = [(11, "seat-1"), (12, "seat-2"), (1, "ana"), (2, "ben"), (3, "cy")];
    let seat = |n: u8| ValidatorInfo {
        name: format!("seat-{n}"),
        root: Hash32([10 + n; 32]),
        height: 10,
        stake: 1_500_000,
        address: format!("100.64.0.{n}:7001"),
    };
    let mut genesis = genesis(&[(1, 10_000), (2, 1_000_000), (3, 5_000_000)]);
    genesis.validators = vec![seat(1), seat(2)];
    genesis.validate().unwrap();
    let network = genesis.id();
    let stake = |n: u8, seq: u64, amount: u64| order(stake(&network, n, validator(n), n), n, seq, n, amount).0;
    let share = |committee: &Committee, n: u8| {
        let member = committee.members().iter().find(|m| label(m) == n);
        member.map_or(0.0, |m| m.stake as f64 / committee.total_stake() as f64)
    };

    let committee = Committee::new(&genesis);
    let state = State::from_genesis(&genesis);
    let (committee, state) = shares(&committee, &state, &[], &names, "no stake yet");
    assert_eq!(committee.total_stake(), 3_000_000);

    // A newcomer's 10,000 is a third of a percent, not nearly all of it.
    let (committee, state) = shares(&committee, &state, &[stake(1, 0, 10_000)], &names, "ana stakes 10,000");
    assert!(share(&committee, 1) < 0.004);
    assert_eq!(committee.total_stake(), 3_000_000);

    let (committee, state) = shares(&committee, &state, &[stake(2, 0, 1_000_000)], &names, "ben stakes 1,000,000");
    // A million coins buy a third of the vote: enough to stall the network,
    // not to approve anything.
    assert!((share(&committee, 2) - 1.0 / 3.0).abs() < 1e-9);

    // Real stake passes 3 million: the seats are gone.
    let (committee, state) = shares(&committee, &state, &[stake(3, 0, 2_000_000)], &names, "cy stakes 2,000,000");
    assert_eq!(roots(committee.members()), [3, 2, 1]);

    // And they stay gone when stake leaves.
    let unstake = order(Kind::Unstake, 3, 1, 3, 1_500_000).0;
    let (committee, _) = shares(&committee, &state, &[unstake], &names, "cy unstakes 1,500,000");
    assert_eq!(roots(committee.members()), [2, 3, 1]);
}
