//! Where the ORN are, for the founder's dashboard: how many exist and how
//! many are still to come, how many wallets hold some, how much is staked,
//! and how the coins are spread across wallets.
//!
//! Worked out from a snapshot of every account, which validators hand out
//! for each epoch (the state at its start, as the last checkpoint left it).
//! For showing only: nothing checks the snapshot against the checkpoint.

use orien_ledger::release::released;
use orien_ledger::{AccountId, FEES, FORFEITED, MINT, REWARDS, State};
use serde::Serialize;

const COIN: u128 = 1_000_000_000;

/// A range of holdings, and the wallets in it.
#[derive(Clone, Debug, Serialize)]
pub struct Bucket {
    /// "100 to 1,000 ORN".
    pub label: String,
    pub wallets: u64,
    /// In ORN's smallest units.
    pub total: u128,
}

#[derive(Clone, Debug, Serialize)]
pub struct Holder {
    pub address: String,
    /// Everything it holds: spendable, staked and on its way back.
    pub holds: u128,
    pub staked: u64,
    /// Its share of the ORN in wallets, from 0 to 1.
    pub share: f64,
}

#[derive(Clone, Debug, Serialize)]
pub struct Tokenomics {
    /// The epoch whose start the snapshot shows.
    pub epoch: u64,
    /// The most there will ever be, and how many exist so far, in units.
    pub cap: u64,
    pub released: u64,
    /// Released in the hour from now, by the release curve.
    pub per_hour: u64,
    /// ORN in wallets: everything released, less what waits in the pots.
    pub in_wallets: u128,
    /// Released but not yet paid to miners.
    pub rewards_pot: u128,
    /// Fees waiting for the next checkpoint to share them out.
    pub fees_pot: u128,
    /// Forfeited by validators caught cheating: gone for good.
    pub burned: u128,
    /// In locked payments (swaps) not yet claimed or refunded.
    pub locked: u128,
    pub staked: u128,
    pub unstaking: u128,
    pub stakers: u64,
    /// Accounts the network knows, ones holding anything, and ones holding
    /// at least one ORN.
    pub accounts: u64,
    pub holding: u64,
    pub holding_one: u64,
    /// Shares of the ORN in wallets held by the largest 1, 10 and 100.
    pub top1: f64,
    pub top10: f64,
    pub top100: f64,
    pub buckets: Vec<Bucket>,
    pub top: Vec<Holder>,
}

/// The tokenomics of `state`, the state at the start of `epoch`, on a
/// network that started at `start_time` with at most `cap` units, `now`
/// seconds since 1970.
pub fn of(state: &State, epoch: u64, start_time: u64, cap: u64, now: u64) -> Tokenomics {
    let system = [FORFEITED, FEES, MINT, REWARDS];
    let holds = |id: &AccountId| state.account(id).map_or(0, |a| a.holdings().max(0) as u128);
    let mut wallets: Vec<(AccountId, u128, u64)> = state
        .each_account()
        .filter(|(id, _)| !system.contains(id))
        .map(|(id, account)| (*id, account.holdings().max(0) as u128, account.staked))
        .collect();
    let in_wallets: u128 = wallets.iter().map(|(_, holds, _)| holds).sum();
    let staked: u128 = wallets.iter().map(|(_, _, staked)| *staked as u128).sum();
    let unstaking: u128 = state
        .each_account()
        .map(|(_, a)| a.unstaking as u128 + a.releases.iter().map(|(_, amount)| *amount as u128).sum::<u128>())
        .sum();
    wallets.sort_by(|a, b| b.1.cmp(&a.1));
    let share = |n: usize| {
        if in_wallets == 0 {
            return 0.0;
        }
        wallets.iter().take(n).map(|(_, holds, _)| *holds).sum::<u128>() as f64 / in_wallets as f64
    };
    let edges: [(u128, &str); 6] = [
        (0, "under 1 ORN"),
        (COIN, "1 to 100 ORN"),
        (100 * COIN, "100 to 1,000 ORN"),
        (1_000 * COIN, "1,000 to 10,000 ORN"),
        (10_000 * COIN, "10,000 to 100,000 ORN"),
        (100_000 * COIN, "100,000 ORN or more"),
    ];
    let buckets = edges
        .iter()
        .enumerate()
        .map(|(i, (low, label))| {
            let high = edges.get(i + 1).map_or(u128::MAX, |(next, _)| *next);
            let inside: Vec<u128> = wallets.iter().map(|(_, h, _)| *h).filter(|h| *h > 0 && *h >= *low && *h < high).collect();
            Bucket { label: label.to_string(), wallets: inside.len() as u64, total: inside.iter().sum() }
        })
        .collect();
    let seconds = now.saturating_sub(start_time);
    let at = |s: u64| released(s).min(cap);
    Tokenomics {
        epoch,
        cap,
        released: state.released(),
        per_hour: at(seconds + 3600).saturating_sub(at(seconds)),
        in_wallets,
        rewards_pot: holds(&REWARDS),
        fees_pot: holds(&FEES),
        burned: holds(&FORFEITED),
        locked: state.escrowed(),
        staked,
        unstaking,
        stakers: wallets.iter().filter(|(_, _, staked)| *staked > 0).count() as u64,
        accounts: wallets.len() as u64,
        holding: wallets.iter().filter(|(_, holds, _)| *holds > 0).count() as u64,
        holding_one: wallets.iter().filter(|(_, holds, _)| *holds >= COIN).count() as u64,
        top1: share(1),
        top10: share(10),
        top100: share(100),
        buckets,
        top: wallets
            .iter()
            .take(25)
            .filter(|(_, holds, _)| *holds > 0)
            .map(|(id, holds, staked)| Holder {
                address: id.to_string(),
                holds: *holds,
                staked: *staked,
                share: if in_wallets == 0 { 0.0 } else { *holds as f64 / in_wallets as f64 },
            })
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use orien_crypto::Hash32;
    use orien_ledger::{Account, Signer};
    use std::collections::{BTreeMap, BTreeSet};

    fn account(balance: i128, staked: u64) -> Account {
        Account {
            balance,
            seq: 0,
            signer: Signer::First,
            spare: Hash32::default(),
            spares_used: 0,
            staked,
            unstaking: 0,
            releases: Vec::new(),
            validator: None,
        }
    }

    #[test]
    fn counts_wallets_shares_and_ranges() {
        let mut accounts = BTreeMap::new();
        // 1,000 released: 50 still in the rewards pot, the rest in wallets.
        accounts.insert(MINT, account(-1_000 * COIN as i128, 0));
        accounts.insert(REWARDS, account(50 * COIN as i128, 0));
        accounts.insert(Hash32([7; 32]), account(550 * COIN as i128, 200 * COIN as u64));
        accounts.insert(Hash32([8; 32]), account(50 * COIN as i128, 0));
        accounts.insert(Hash32([9; 32]), account(COIN as i128 / 2, 0));
        accounts.insert(Hash32([10; 32]), account(0, 0));
        let bytes = borsh::to_vec(&(&accounts, &BTreeSet::<Hash32>::new())).unwrap();
        let state = State::from_snapshot(&bytes, Default::default()).unwrap();
        let t = of(&state, 9, 0, 100_000_000 * COIN as u64, 3600);
        assert_eq!(t.released, 1_000 * COIN as u64);
        assert_eq!(t.rewards_pot, 50 * COIN);
        assert_eq!(t.in_wallets, 750 * COIN + 50 * COIN + COIN / 2);
        assert_eq!(t.staked, 200 * COIN);
        assert_eq!((t.accounts, t.holding, t.holding_one, t.stakers), (4, 3, 2, 1));
        assert!((t.top1 - 750.0 / 800.5).abs() < 1e-9);
        assert_eq!(t.top[0].address, Hash32([7; 32]).to_string());
        let wallets: Vec<u64> = t.buckets.iter().map(|b| b.wallets).collect();
        assert_eq!(wallets, vec![1, 1, 1, 0, 0, 0]);
        assert!(t.per_hour > 0);
    }
}
