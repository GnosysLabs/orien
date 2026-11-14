//! The staking settings of the genesis file.

use orien_crypto::Hash32;
use orien_ledger::genesis::{Genesis, ValidatorInfo};

fn genesis(validators: usize) -> Genesis {
    let validators = (0..validators)
        .map(|i| ValidatorInfo {
            name: format!("v{i}"),
            root: Hash32([i as u8 + 1; 32]),
            height: 5,
            stake: 1,
            address: String::new(),
        })
        .collect();
    Genesis::new("settings", validators, Vec::new())
}

#[test]
fn the_usual_settings() {
    let genesis = genesis(4);
    assert_eq!((genesis.min_stake, genesis.max_validators, genesis.unstake_epochs), (10_000, 33, 168));
    genesis.validate().unwrap();
}

#[test]
fn each_setting_is_part_of_the_network() {
    let usual = genesis(4).id();
    let mut other = genesis(4);
    other.min_stake += 1;
    assert_ne!(other.id(), usual);
    let mut other = genesis(4);
    other.max_validators += 1;
    assert_ne!(other.id(), usual);
    let mut other = genesis(4);
    other.unstake_epochs += 1;
    assert_ne!(other.id(), usual);
}

#[test]
fn the_launch_seats_fit_the_limit() {
    let mut genesis = genesis(4);
    genesis.max_validators = 3;
    assert!(genesis.validate().is_err());
    genesis.max_validators = 4;
    genesis.validate().unwrap();
}

#[test]
fn staking_needs_a_minimum_and_a_wait() {
    let mut genesis = genesis(4);
    genesis.min_stake = 0;
    assert!(genesis.validate().is_err());
    let mut genesis = self::genesis(4);
    genesis.unstake_epochs = 0;
    assert!(genesis.validate().is_err());
}
