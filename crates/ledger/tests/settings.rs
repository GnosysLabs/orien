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
    assert_eq!((genesis.min_stake, genesis.max_validators, genesis.unstake_epochs), (10_000 * orien_ledger::COIN, 33, 168));
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

#[test]
fn amounts_read_and_show_in_coins_with_nine_decimals() {
    use orien_ledger::{COIN, coins, parse_coins};
    assert_eq!(parse_coins("12"), Ok(12 * COIN));
    assert_eq!(parse_coins("0.25"), Ok(COIN / 4));
    assert_eq!(parse_coins(".5"), Ok(COIN / 2));
    assert_eq!(parse_coins("0.000000001"), Ok(1));
    for bad in ["", ".", "1.2.3", "-1", "1e9", "0.0000000001", "99999999999999999999"] {
        assert!(parse_coins(bad).is_err(), "{bad}");
    }
    assert_eq!(coins(12 * COIN as i128), "12");
    assert_eq!(coins(COIN as i128 / 4), "0.25");
    assert_eq!(coins(1), "0.000000001");
    assert_eq!(coins(-(COIN as i128) - 5), "-1.000000005");
}

#[test]
fn a_bitcoin_block_is_part_of_the_network() {
    use orien_ledger::BitcoinBlock;
    let plain = genesis(4);
    let mut anchored = genesis(4);
    anchored.bitcoin = Some(BitcoinBlock { height: 950_000, hash: Hash32([7; 32]) });
    let mut other_block = anchored.clone();
    other_block.bitcoin = Some(BitcoinBlock { height: 950_000, hash: Hash32([8; 32]) });
    // Until the block exists, nobody knows its hash, so nobody knows the
    // network's identity, which is the first epoch's seed.
    assert_ne!(anchored.id(), plain.id());
    assert_ne!(anchored.id(), other_block.id());
}
