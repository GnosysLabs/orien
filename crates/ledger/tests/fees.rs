//! The fee every order pays. Run with `--nocapture` to see where it goes.

use orien_crypto::Hash32;
use orien_ledger::{Accepted, Allocation, FEES, Fees, Genesis, Kind, Order, Reject, SignedBy, State, address};

fn key(account: u8, seq: u64) -> Hash32 {
    Hash32([account.wrapping_add(seq as u8 * 16); 32])
}

fn spare(account: u8) -> Hash32 {
    Hash32([account ^ 0xff; 32])
}

fn id(account: u8) -> Hash32 {
    address(&key(account, 0), &spare(account))
}

fn order(from: u8, seq: u64, to: u8, amount: u64, max_fee: u64) -> (Order, SignedBy) {
    let order = Order {
        kind: Kind::Payment,
        from: id(from),
        seq,
        to: id(to),
        amount,
        max_fee,
        next_key: key(from, seq + 1),
        spare: spare(from),
    };
    (order, SignedBy::Key(key(from, seq)))
}

fn vote_and_apply(state: &mut State, (order, by): &(Order, SignedBy)) -> Result<(), Reject> {
    assert_eq!(state.check_order(order, Hash32([order.seq as u8; 32]), by)?, Accepted::Fresh);
    state.apply(order).unwrap();
    Ok(())
}

#[test]
fn a_payment_pays_a_share_of_its_amount_between_a_minimum_and_a_maximum() {
    // 0.1% of each payment, at least 2 and at most 50.
    let fees = Fees { min: 2, max: 50, rate: 1_000 };
    for (amount, fee) in [(1, 2), (1_999, 2), (5_000, 5), (20_000, 20), (50_000, 50), (10_000_000, 50)] {
        println!("a payment of {amount:>10} pays {fee}");
        assert_eq!(fees.on(&Kind::Payment, amount), fee);
    }
    // Stake and unstake orders pay the minimum, whatever they move.
    assert_eq!(fees.on(&Kind::Unstake, 1_000_000), 2);
}

#[test]
fn every_order_pays_its_fee_into_the_epochs_pot() {
    let mut genesis = Genesis::new("fees", Vec::new(), Vec::new());
    genesis.fee = Fees { min: 2, max: 50, rate: 1_000 };
    genesis.allocations = vec![Allocation { account: id(1), amount: 100_000 }];
    let mut state = State::from_genesis(&genesis);
    let balance = |state: &State, n: u8| state.account(&id(n)).map_or(0, |a| a.balance);

    // An order that does not accept its fee is refused: 10,000 pays 10.
    assert_eq!(
        vote_and_apply(&mut state, &order(1, 0, 2, 10_000, 9)),
        Err(Reject::FeeTooLow { fee: 10, accepted: 9 })
    );
    vote_and_apply(&mut state, &order(1, 0, 2, 10_000, 10)).unwrap();
    let pot = |state: &State| state.account(&FEES).unwrap().balance;
    println!("ana pays bob 10,000: ana {}, bob {}, fees {}", balance(&state, 1), balance(&state, 2), pot(&state));
    assert_eq!((balance(&state, 1), balance(&state, 2), pot(&state)), (89_990, 10_000, 10));
    assert_eq!(state.total(), 100_000, "the fee moves coins and creates none");

    // The fee has to be covered as well as the amount.
    assert_eq!(
        vote_and_apply(&mut state, &order(1, 1, 2, 89_950, 50)),
        Err(Reject::InsufficientFunds { balance: 89_990, needed: 90_000 })
    );
    vote_and_apply(&mut state, &order(1, 1, 2, 89_940, 50)).unwrap();
    assert_eq!((balance(&state, 1), pot(&state)), (0, 60));
}
