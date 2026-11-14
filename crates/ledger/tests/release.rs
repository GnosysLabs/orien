//! The release curve. Run with `--nocapture` to see its milestones.

use orien_ledger::COIN;
use orien_ledger::release::{CAP, HALF_LIFE, released, released_between};

const HOUR: u64 = 3600;
const DAY: u64 = 24 * HOUR;
const YEAR: u64 = HALF_LIFE / 4;

/// `units` as whole coins and a remainder, for comparing with numbers
/// worked out to 80 digits elsewhere.
fn coins(units: u64) -> f64 {
    units as f64 / COIN as f64
}

#[test]
fn the_curve_matches_its_milestones() {
    // Worked out to 100 digits with Python's decimal arithmetic.
    let expected = [
        ("first hour", HOUR, 1_976.786_246_629_895),
        ("first day", DAY, 47_432.086_274_958_52),
        ("first year", YEAR, 15_910_358.474_628_55),
        ("four years", HALF_LIFE, 50_000_000.0),
        ("eight years", 2 * HALF_LIFE, 75_000_000.0),
    ];
    for (what, seconds, coins_expected) in expected {
        let got = coins(released(seconds));
        println!("{what:<12} {got:>18.9} coins  ({:>6.3}% of the cap)", 100.0 * got / 1e8);
        // Within a ten-thousandth of a coin.
        assert!((got - coins_expected).abs() < 1e-4, "{what}: {got} against {coins_expected}");
    }
    assert_eq!(released(0), 0, "nothing exists at the start");
    assert!(released(1000 * HALF_LIFE) <= CAP, "never more than the cap");
}

#[test]
fn coins_arrive_steadily_and_never_go_back() {
    // An hour at a time across the first eight years: each hour releases
    // something, never more than the hour before, and the hours add up to
    // the whole.
    let mut total = 0u64;
    let mut previous = u64::MAX;
    for hour in 0..(2 * HALF_LIFE / HOUR) {
        let this = released_between(hour * HOUR, (hour + 1) * HOUR);
        assert!(this > 0 && this <= previous.saturating_add(1), "hour {hour}: {this} after {previous}");
        total += this;
        previous = this;
    }
    assert_eq!(total, released(2 * HALF_LIFE / HOUR * HOUR));
    // Closing an epoch early or late changes nothing: two half-hours make
    // the same hour.
    let split = released_between(0, 1800) + released_between(1800, 3600);
    assert_eq!(split, released(3600));
}
