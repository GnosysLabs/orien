//! What the marketplace asks the Orien network: whether an address typed
//! on the website has ever held ORN (a mistyped one never has), and what
//! came into an address.

use anyhow::{Context, Result};
use orien_ledger::{AccountId, Income};
use orien_net::client::Client;
use std::path::Path;
use std::sync::Mutex;

/// A payment that came in.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Incoming {
    pub from: AccountId,
    pub amount: u64,
    pub epoch: u64,
}

/// Where the marketplace learns what was paid to it.
pub trait Payments: Send + Sync {
    /// The epoch the network is in now.
    fn epoch(&self) -> Result<u64>;
    /// Payments to `to` from epoch `from` on, final ones only.
    fn received(&self, to: &AccountId, from: u64) -> Result<Vec<Incoming>>;
    /// Whether `account` has ever held ORN or paid anything.
    fn has_held(&self, account: &AccountId) -> Result<bool>;
}

/// The live network, asked through its validators.
pub struct Network {
    client: Client,
}

impl Network {
    pub fn open(genesis: &Path) -> Result<Network> {
        let genesis = orien_net::keys::load_genesis(genesis).context("cannot read the genesis file")?;
        Ok(Network { client: Client::new(&genesis) })
    }
}

impl Payments for Network {
    fn epoch(&self) -> Result<u64> {
        let epochs: Vec<u64> = self.client.info().into_iter().filter_map(|info| info.ok().map(|i| i.epoch)).collect();
        epochs.into_iter().max().context("no validator answers")
    }

    fn received(&self, to: &AccountId, from: u64) -> Result<Vec<Incoming>> {
        let mut found = Vec::new();
        let mut next = Some(from);
        // A few pages at most: a check looks back minutes, not months.
        for _ in 0..5 {
            let Some(at) = next else { break };
            let page = self.client.history(to, at, 1000).context("no validator keeps history")?;
            found.extend(
                page.entries
                    .iter()
                    .filter(|entry| entry.income == Income::Payment)
                    .filter_map(|entry| Some(Incoming { from: entry.counterparty?, amount: entry.amount, epoch: entry.epoch })),
            );
            next = page.next;
        }
        Ok(found)
    }

    fn has_held(&self, account: &AccountId) -> Result<bool> {
        let status = self.client.account(account, 0);
        if status.views.iter().all(Option::is_none) {
            anyhow::bail!("no validator answers");
        }
        let held = status.views.iter().flatten().any(|view| view.balance > 0 || view.seq > 0);
        Ok(held || status.seq > 0 || status.proven.is_some())
    }
}

/// For tests: payments added by hand.
#[derive(Default)]
pub struct Fake {
    pub epoch: Mutex<u64>,
    pub payments: Mutex<Vec<(AccountId, Incoming)>>,
    /// Accounts that hold ORN.
    pub held: Mutex<Vec<AccountId>>,
}

impl Payments for Fake {
    fn epoch(&self) -> Result<u64> {
        Ok(*self.epoch.lock().unwrap())
    }

    fn received(&self, to: &AccountId, from: u64) -> Result<Vec<Incoming>> {
        let payments = self.payments.lock().unwrap();
        Ok(payments.iter().filter(|(t, p)| t == to && p.epoch >= from).map(|(_, p)| p.clone()).collect())
    }

    fn has_held(&self, account: &AccountId) -> Result<bool> {
        Ok(self.held.lock().unwrap().contains(account))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Asks the live network: `cargo test -p orien-market has_held_live -- --ignored`.
    #[test]
    #[ignore]
    fn has_held_live() {
        let genesis = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../web/public/network/genesis.json");
        let network = Network::open(&genesis).unwrap();
        let market: AccountId = "74fa1ab82c45ebeed539b03146243ddfc6f0ccb5163f45ea87d21692ba774b7a".parse().unwrap();
        assert!(network.has_held(&market).unwrap(), "the marketplace's wallet has been paid");
        let nobody: AccountId = "1234".repeat(16).parse().unwrap();
        assert!(!network.has_held(&nobody).unwrap(), "a made-up address never held anything");
    }
}
