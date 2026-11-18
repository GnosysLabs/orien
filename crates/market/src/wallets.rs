//! What the marketplace asks the Orien network: whether an address typed
//! on the website has ever held ORN (a mistyped one never has), and what
//! came into an address.

use anyhow::{Context, Result};
use orien_ledger::{AccountId, Income};
use crate::tokenomics::Tokenomics;
use serde::Serialize;
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

/// Something that came into an address, named by `key` so it is told about
/// once: a payment, a locked payment claimed for it, or a mining reward.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Credit {
    pub key: String,
    pub amount: u64,
    pub epoch: u64,
    /// A mining reward, rather than something someone sent.
    pub mined: bool,
}

/// Where the marketplace learns what was paid to it.
pub trait Payments: Send + Sync {
    /// Payments, claimed locked payments and mining rewards to `to` from
    /// epoch `from` on, final ones only, for notifications.
    fn credits(&self, to: &AccountId, from: u64) -> Result<Vec<Credit>> {
        Ok(self
            .received(to, from)?
            .into_iter()
            .enumerate()
            .map(|(i, p)| Credit {
                key: format!("{}:{}:{i}", hex::encode(p.from.as_bytes()), p.epoch),
                amount: p.amount,
                epoch: p.epoch,
                mined: false,
            })
            .collect())
    }
    /// The epoch the network is in now.
    fn epoch(&self) -> Result<u64>;
    /// Payments to `to` from epoch `from` on, final ones only.
    fn received(&self, to: &AccountId, from: u64) -> Result<Vec<Incoming>>;
    /// Whether `account` has ever held ORN or paid anything.
    fn has_held(&self, account: &AccountId) -> Result<bool>;
    /// How the network is doing, for the founder's dashboard.
    fn status(&self) -> Result<NetworkStatus>;
    /// Where the ORN are, from the latest checkpoint.
    fn tokenomics(&self) -> Result<Tokenomics>;
    /// Up to `max` checkpoints from epoch `from` on, as each of this
    /// epoch's validators hands them out (in seat order; None where one did
    /// not answer), for the explorer.
    fn checkpoints_each(&self, _from: u64, _max: u32) -> Vec<Option<Vec<orien_ledger::close::Decided>>> {
        Vec::new()
    }
    /// The validators the genesis file names: the first epoch's.
    fn launch_seats(&self) -> Vec<orien_ledger::Member> {
        Vec::new()
    }
    /// An address as the network knows it, for the explorer.
    fn lookup(&self, _id: &AccountId) -> Result<crate::explorer::Looked> {
        anyhow::bail!("addresses cannot be looked up here")
    }
}

/// One validator, as it answered (or did not).
#[derive(Clone, Debug, Default, Serialize)]
pub struct ValidatorStatus {
    pub endpoint: String,
    pub up: bool,
    pub name: String,
    /// Its place in this epoch's list of validators.
    pub seat: Option<u16>,
    pub epoch: u64,
    pub closing: bool,
    pub accounts: u64,
    pub certificates: u64,
    /// Signing keys it has used of its key tree, and how many it has.
    pub keys_used: u32,
    pub keys_total: u32,
}

/// The network as a whole.
#[derive(Clone, Debug, Default, Serialize)]
pub struct NetworkStatus {
    pub validators: Vec<ValidatorStatus>,
    pub epoch: u64,
    /// When the epoch under way began, in seconds since 1970.
    pub epoch_started: u64,
    pub epoch_seconds: u64,
    /// Mining's target now, and the one its math started at (launch for
    /// BLAKE3, the switch for RandomX): a lower target is harder.
    pub target: Option<u64>,
    pub first_target: u64,
    /// "Blake3" or "RandomX".
    pub math: String,
}

/// The live network, asked through its validators.
pub struct Network {
    client: Client,
    epoch_seconds: u64,
    first_target: u64,
    start_time: u64,
    cap: u64,
    fees: orien_ledger::Fees,
    /// The validators the genesis file names.
    launch_seats: Vec<orien_ledger::Member>,
    /// Worked out once per epoch: the snapshot only changes at a checkpoint.
    tokenomics: Mutex<Option<Tokenomics>>,
}

impl Network {
    pub fn open(genesis: &Path) -> Result<Network> {
        let genesis = orien_net::keys::load_genesis(genesis).context("cannot read the genesis file")?;
        Ok(Network {
            client: Client::new(&genesis),
            epoch_seconds: genesis.epoch_seconds,
            first_target: genesis.first_target,
            start_time: genesis.start_time,
            cap: genesis.cap,
            fees: genesis.fee,
            launch_seats: orien_ledger::Committee::new(&genesis).members().to_vec(),
            tokenomics: Mutex::new(None),
        })
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

    fn credits(&self, to: &AccountId, from: u64) -> Result<Vec<Credit>> {
        let page = self.client.history(to, from, 1000).context("no validator keeps history")?;
        Ok(page
            .entries
            .iter()
            .filter(|entry| matches!(entry.income, Income::Payment | Income::Claim | Income::Mining))
            .map(|entry| Credit {
                // The order behind it names it; anything without one, by
                // what and when.
                key: match entry.order {
                    Some((from, seq)) => format!("{}:{seq}", hex::encode(from.as_bytes())),
                    None => format!("{:?}:{}:{}", entry.income, entry.epoch, entry.amount),
                },
                amount: entry.amount,
                epoch: entry.epoch,
                mined: entry.income == Income::Mining,
            })
            .collect())
    }

    fn has_held(&self, account: &AccountId) -> Result<bool> {
        let status = self.client.account(account, 0);
        if status.views.iter().all(Option::is_none) {
            anyhow::bail!("no validator answers");
        }
        let held = status.views.iter().flatten().any(|view| view.balance > 0 || view.seq > 0);
        Ok(held || status.seq > 0 || status.proven.is_some())
    }

    fn tokenomics(&self) -> Result<Tokenomics> {
        let epoch = self.epoch()?;
        if let Some(known) = self.tokenomics.lock().unwrap().as_ref().filter(|t| t.epoch == epoch) {
            return Ok(known.clone());
        }
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
        // Any validator will do; the first that hands over the whole snapshot.
        for endpoint in self.client.endpoints() {
            let Some(bytes) = snapshot(&endpoint, epoch) else { continue };
            let Ok(state) = orien_ledger::State::from_snapshot(&bytes, self.fees) else { continue };
            let found = crate::tokenomics::of(&state, epoch, self.start_time, self.cap, now);
            *self.tokenomics.lock().unwrap() = Some(found.clone());
            return Ok(found);
        }
        anyhow::bail!("no validator handed over the state for epoch {epoch}")
    }

    fn checkpoints_each(&self, from: u64, max: u32) -> Vec<Option<Vec<orien_ledger::close::Decided>>> {
        use orien_net::wire::{Request, Response};
        let request = Request::Checkpoints { from, max };
        std::thread::scope(|scope| {
            let asks: Vec<_> = (0..self.client.validators())
                .map(|index| {
                    let request = &request;
                    scope.spawn(move || match self.client.call(index, request) {
                        Ok(Response::Checkpoints(found)) => Some(found),
                        _ => None,
                    })
                })
                .collect();
            asks.into_iter().map(|ask| ask.join().ok().flatten()).collect()
        })
    }

    fn launch_seats(&self) -> Vec<orien_ledger::Member> {
        self.launch_seats.clone()
    }

    fn lookup(&self, id: &AccountId) -> Result<crate::explorer::Looked> {
        let status = self.client.account(id, 0);
        if status.views.iter().all(Option::is_none) {
            anyhow::bail!("no validator answers");
        }
        let proven = status.proven.as_ref();
        let balance = status.spendable.or(proven.map(|account| account.balance)).unwrap_or(0);
        let staked = proven.map_or(0, |account| account.staked);
        let returning = proven.map_or(0, |account| {
            account.unstaking + account.releases.iter().map(|(_, amount)| amount).sum::<u64>()
        });
        // Everything, oldest first, a page at a time: an hour's activity
        // fits in a page for all but the busiest addresses.
        let mut entries = Vec::new();
        let mut next = Some(0);
        let mut since = 0;
        for _ in 0..10 {
            let Some(at) = next else { break };
            let Some(page) = self.client.activity(id, at, 1000).or_else(|| self.client.history(id, at, 1000)) else {
                break;
            };
            since = page.since;
            entries.extend(page.entries);
            next = page.next;
        }
        Ok(crate::explorer::Looked { balance, staked, returning, payments_made: status.seq, entries, since })
    }

    fn status(&self) -> Result<NetworkStatus> {
        let endpoints = self.client.endpoints();
        let infos = self.client.info();
        let epoch = infos.iter().flatten().map(|info| info.epoch).max().unwrap_or(0);
        let epoch_started =
            infos.iter().flatten().filter(|info| info.epoch == epoch).map(|info| info.epoch_started).max().unwrap_or(0);
        let validators: Vec<ValidatorStatus> = infos
            .into_iter()
            .enumerate()
            .map(|(index, info)| {
                let endpoint = endpoints.get(index).cloned().unwrap_or_default();
                match info {
                    Ok(info) => ValidatorStatus {
                        endpoint,
                        up: true,
                        name: info.name,
                        seat: info.validator,
                        epoch: info.epoch,
                        closing: info.closing,
                        accounts: info.accounts,
                        certificates: info.certificates,
                        keys_used: info.keys_used,
                        keys_total: info.keys_total,
                    },
                    Err(_) => ValidatorStatus { endpoint, ..ValidatorStatus::default() },
                }
            })
            .collect();
        if validators.iter().all(|v| !v.up) {
            anyhow::bail!("no validator answers");
        }
        let work = self.client.work();
        let rules = self.client.committee().work();
        let math = rules.math(epoch);
        let first_target = if math == orien_ledger::work::Math::RandomX { rules.randomx_target } else { self.first_target };
        Ok(NetworkStatus {
            validators,
            epoch,
            epoch_started,
            epoch_seconds: self.epoch_seconds,
            target: work.map(|(_, _, target)| target),
            first_target,
            math: format!("{math:?}"),
        })
    }
}

/// The state at the start of `epoch`, as one validator keeps it.
fn snapshot(endpoint: &str, epoch: u64) -> Option<Vec<u8>> {
    use orien_net::wire::{Request, Response};
    let mut bytes = Vec::new();
    loop {
        let request = Request::Snapshot { epoch, offset: bytes.len() as u64, max: 4 << 20 };
        let Ok(Response::Snapshot(Some((piece, total)))) = orien_net::client::rpc(endpoint, &request, std::time::Duration::from_secs(20))
        else {
            return None;
        };
        if piece.is_empty() && (bytes.len() as u64) < total {
            return None;
        }
        bytes.extend_from_slice(&piece);
        if bytes.len() as u64 >= total {
            return Some(bytes);
        }
    }
}

/// For tests: payments added by hand.
#[derive(Default)]
pub struct Fake {
    pub epoch: Mutex<u64>,
    pub payments: Mutex<Vec<(AccountId, Incoming)>>,
    /// Accounts that hold ORN.
    pub held: Mutex<Vec<AccountId>>,
    /// Mining rewards: to whom, how much, in which epoch.
    pub mined: Mutex<Vec<(AccountId, u64, u64)>>,
}

impl Payments for Fake {
    fn epoch(&self) -> Result<u64> {
        Ok(*self.epoch.lock().unwrap())
    }

    fn received(&self, to: &AccountId, from: u64) -> Result<Vec<Incoming>> {
        let payments = self.payments.lock().unwrap();
        Ok(payments.iter().filter(|(t, p)| t == to && p.epoch >= from).map(|(_, p)| p.clone()).collect())
    }

    fn credits(&self, to: &AccountId, from: u64) -> Result<Vec<Credit>> {
        let mut credits: Vec<Credit> = self
            .received(to, from)?
            .into_iter()
            .enumerate()
            .map(|(i, p)| Credit { key: format!("paid:{}:{i}", p.epoch), amount: p.amount, epoch: p.epoch, mined: false })
            .collect();
        let mined = self.mined.lock().unwrap();
        credits.extend(mined.iter().enumerate().filter(|(_, (t, _, epoch))| t == to && *epoch >= from).map(|(i, (_, amount, epoch))| Credit {
            key: format!("mined:{epoch}:{i}"),
            amount: *amount,
            epoch: *epoch,
            mined: true,
        }));
        Ok(credits)
    }

    fn has_held(&self, account: &AccountId) -> Result<bool> {
        Ok(self.held.lock().unwrap().contains(account))
    }

    fn tokenomics(&self) -> Result<Tokenomics> {
        anyhow::bail!("no snapshot in tests")
    }

    fn status(&self) -> Result<NetworkStatus> {
        Ok(NetworkStatus { epoch: *self.epoch.lock().unwrap(), epoch_seconds: 3600, first_target: 1, ..NetworkStatus::default() })
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
        let t = network.tokenomics().unwrap();
        println!("{}", serde_json::to_string_pretty(&t).unwrap());
        assert!(t.released > 0 && t.holding > 0);
    }
}
