//! Coins released for work. Run with `--nocapture` to see who earns what.

use orien_crypto::Hash32;
use orien_ledger::close::{self, Moment, Pin, Share, Statement};
use orien_ledger::{COIN, Committee, Genesis, MINT, REWARDS, State, ValidatorInfo, coins};

/// Account `n`'s address, clear of the special addresses 0 to 3.
fn account(n: u8) -> Hash32 {
    Hash32([0x50 + n; 32])
}

/// The first `count` shares for `account` against this seed and target,
/// found by trying numbers in turn, as a miner would.
fn mine(account: Hash32, seed: &Hash32, target: u64, count: usize) -> Vec<Share> {
    (0..).map(|nonce| Share { account, nonce }).filter(|s| s.counts(seed, target)).take(count).collect()
}

fn statement(shares: Vec<Share>) -> Statement {
    Statement {
        epoch: 0,
        validator: 0,
        log: Pin::start(0),
        evidence: Vec::new(),
        next_key: None,
        cheats: Vec::new(),
        shares,
    }
}

#[test]
fn an_hours_coins_go_to_that_hours_work() {
    let seat = ValidatorInfo { name: "v1".into(), root: Hash32([9; 32]), height: 4, stake: 1, address: "a".into() };
    let mut genesis = Genesis::new("work", vec![seat], Vec::new());
    // Easy work for a test: one try in sixteen makes a share.
    genesis.first_target = u64::MAX >> 4;
    genesis.validate().unwrap();
    let committee = Committee::new(&genesis);
    let state = State::from_genesis(&genesis);
    let seed = genesis.id();
    let target = committee.work().target;
    let balance = |state: &State, n: u8| state.account(&account(n)).map_or(0, |a| a.balance);

    // Ana finds three shares and Ben one. One of Ana's is offered twice
    // and counts once; a number that is not a share counts for nothing.
    let mut shares = mine(account(1), &seed, target, 3);
    shares.extend(mine(account(2), &seed, target, 1));
    shares.push(shares[0]);
    let not_a_share = (0..).map(|nonce| Share { account: account(2), nonce }).find(|s| !s.counts(&seed, target));
    shares.push(not_a_share.unwrap());
    let reported = statement(shares);

    // The first hour closes: about 1,977 coins are released and paid three
    // to one.
    let hour = Moment { previous: seed, time: genesis.start_time + 3600 };
    let end = close::outcome_checked(&committee, &state, hour, &[], &[&reported]).unwrap();
    println!("released in the first hour: {} coins", coins(end.released() as i128));
    println!("ana (3 shares): {} coins", coins(balance(&end, 1)));
    println!("ben (1 share):  {} coins", coins(balance(&end, 2)));
    let released = end.released() as i128;
    assert_eq!(released / COIN as i128, 1_976);
    assert_eq!(balance(&end, 1), released * 3 / 4);
    assert_eq!(balance(&end, 2), released / 4);
    // All balances still add up to what the genesis file set up: nothing,
    // with the mint counting what it released as a debt.
    assert_eq!(end.total(), 0);
    assert_eq!(end.account(&MINT).unwrap().balance, -released);
    assert!(end.account(&REWARDS).unwrap().balance < 4, "only what does not divide evenly is left");

    // An hour with no work: its coins wait for the next hour's workers.
    let quiet = close::outcome_checked(&committee, &state, hour, &[], &[&statement(Vec::new())]).unwrap();
    assert_eq!(quiet.account(&REWARDS).unwrap().balance, released);
    assert_eq!(balance(&quiet, 1), 0);

    // Four shares where a thousand were wanted: the next target is four
    // times easier. Far too many shares make it four times harder.
    assert_eq!(close::next_target(&committee, 4), target * 4);
    assert_eq!(close::next_target(&committee, 1_000), target);
    assert_eq!(close::next_target(&committee, 100_000), target / 4);
}
