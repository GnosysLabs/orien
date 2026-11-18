//! The explorer: what anyone can look up about the network, for the
//! website's explorer pages. The network as it is now (validators, supply),
//! each hourly checkpoint, and any address's balance and activity.
//!
//! For showing only. Checkpoints are taken as validators hand them out and
//! kept in order: each must name the one before it, so a validator cannot
//! slip in one that does not fit the chain. Wallets check signatures
//! themselves and never rely on this.

use crate::api::{App, Failure};
use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::header;
use axum::response::{IntoResponse, Response};
use orien_ledger::Income;
use orien_net::history::Received;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// How long the network's figures are kept before asking again, so a busy
/// page never sends validators more than a few questions a minute.
const FRESH: Duration = Duration::from_secs(15);
/// How long an address's figures are kept.
const FRESH_ADDRESS: Duration = Duration::from_secs(20);
/// Most addresses kept at once.
const MAX_ADDRESSES: usize = 2000;
/// Checkpoints on the overview, newest first.
const RECENT: usize = 24;
/// Checkpoints per page of the list.
const PAGE: u64 = 50;
/// Most activity entries shown for one address, newest first.
const MAX_ENTRIES: usize = 500;

/// An address as the network knows it.
#[derive(Clone, Debug, Default)]
pub struct Looked {
    /// Spendable now.
    pub balance: i128,
    pub staked: u64,
    /// Unstaked and on its way back.
    pub returning: u64,
    /// How many payments it has sent.
    pub payments_made: u64,
    /// Oldest first.
    pub entries: Vec<Received>,
    /// The first epoch the answering validator kept history for.
    pub since: u64,
}

/// One hourly checkpoint.
#[derive(Clone, Debug, Serialize)]
pub struct Checkpoint {
    pub epoch: u64,
    /// When it was made, in seconds since 1970.
    pub time: u64,
    pub payments: u64,
    /// New ORN released with it, in units.
    pub released: String,
    /// Which validators hold this very checkpoint, by seat, and how many
    /// there were. The checkpoint keeps only the signatures it needed (more
    /// than two thirds of the stake), so each validator is asked.
    pub agreed: Vec<u16>,
    pub of: usize,
    /// The voting round it was settled in.
    pub round: u32,
    /// The root of every account's state after it, and its own digest.
    pub state: String,
    pub digest: String,
    /// Validators named for the next epoch.
    #[serde(skip)]
    next: Vec<orien_ledger::Member>,
    #[serde(skip)]
    digest_bytes: orien_crypto::Hash32,
}

#[derive(Clone, Debug, Serialize)]
pub struct Validator {
    pub seat: usize,
    pub name: String,
    pub up: bool,
    /// The epoch it says it is in: behind the others when catching up.
    pub epoch: u64,
    /// Of the latest checkpoints (up to 24), how many it holds too.
    pub agreed: usize,
    pub of: usize,
    pub stake: String,
    /// The account behind its stake; none for the launch seats.
    pub account: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct Supply {
    pub cap: String,
    pub released: String,
    pub per_hour: String,
    pub in_wallets: String,
    pub staked: String,
    pub stakers: u64,
    pub holding: u64,
    pub holding_one: u64,
    pub burned: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct Overview {
    pub epoch: u64,
    pub epoch_started: u64,
    pub epoch_seconds: u64,
    pub mining: String,
    pub validators: Vec<Validator>,
    pub supply: Option<Supply>,
    pub checkpoints: Vec<Checkpoint>,
}

#[derive(Clone, Debug, Serialize)]
pub struct Entry {
    pub epoch: u64,
    /// When its hour's checkpoint was made; none for the hour under way.
    pub time: Option<u64>,
    /// "received", "sent", "mined", "fees", "claimed", "refunded" or
    /// "unstaked".
    pub kind: &'static str,
    pub amount: String,
    pub counterparty: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct Address {
    pub address: String,
    pub balance: String,
    pub staked: String,
    pub returning: String,
    pub payments_made: u64,
    /// Newest first.
    pub activity: Vec<Entry>,
    /// Whether older activity was left out.
    pub more: bool,
    /// Validators' records start at this checkpoint, made at `since_time`.
    pub since: u64,
    pub since_time: Option<u64>,
}

/// What the explorer has already asked the network.
#[derive(Default)]
pub struct Explorer {
    overview: Mutex<Option<(Instant, Overview)>>,
    checkpoints: Mutex<BTreeMap<u64, Checkpoint>>,
    /// When validators missing from recent checkpoints were last asked again.
    rechecked: Mutex<Option<Instant>>,
    addresses: Mutex<HashMap<String, (Instant, Address)>>,
}

impl Explorer {
    /// Fetches the checkpoints after the last one kept, up to `epoch`, and
    /// asks again about validators that did not answer for recent ones.
    fn catch_up(&self, payments: &dyn crate::wallets::Payments, epoch: u64) {
        loop {
            let (from, previous) = {
                let kept = self.checkpoints.lock().unwrap();
                match kept.last_key_value() {
                    Some((last, checkpoint)) => (last + 1, Some(checkpoint.clone())),
                    None => (0, None),
                }
            };
            if from >= epoch {
                break;
            }
            let answers = payments.checkpoints_each(from, 64);
            // The chain comes from the first answer that has any; the others
            // only say whether they hold the same checkpoints.
            let Some(found) = answers.iter().flatten().find(|found| !found.is_empty()) else { break };
            let holds = holdings(&answers);
            let mut previous = previous;
            let mut kept = self.checkpoints.lock().unwrap();
            let mut added = false;
            for decided in found {
                let checkpoint = &decided.checkpoint;
                let expected = previous.as_ref().map_or(from, |p| p.epoch + 1);
                if checkpoint.epoch != expected {
                    break;
                }
                if let Some(previous) = &previous
                    && checkpoint.previous != previous.digest_bytes
                {
                    break;
                }
                let of = previous.as_ref().map_or_else(|| payments.launch_seats().len(), |p| p.next.len());
                let digest = checkpoint.digest();
                // Those whose signatures it carries, and those holding it.
                let mut agreed: Vec<u16> = decided.precommits.iter().map(|vote| vote.validator).collect();
                agreed.extend(holders(&holds, checkpoint.epoch, &digest));
                agreed.sort_unstable();
                agreed.dedup();
                let row = Checkpoint {
                    epoch: checkpoint.epoch,
                    time: checkpoint.time,
                    payments: checkpoint.payments,
                    released: checkpoint.released.to_string(),
                    agreed,
                    of,
                    round: decided.round,
                    state: hex::encode(checkpoint.state.as_bytes()),
                    digest: hex::encode(digest.as_bytes()),
                    next: checkpoint.validators.clone(),
                    digest_bytes: digest,
                };
                kept.insert(row.epoch, row.clone());
                previous = Some(row);
                added = true;
            }
            if !added {
                break;
            }
        }
        self.recheck(payments);
    }

    /// Asks again, at most once a minute, about recent checkpoints some
    /// validator did not answer for: one restarting a moment ago holds them
    /// once it is back.
    fn recheck(&self, payments: &dyn crate::wallets::Payments) {
        {
            let mut rechecked = self.rechecked.lock().unwrap();
            if rechecked.is_some_and(|at| at.elapsed() < Duration::from_secs(60)) {
                return;
            }
            *rechecked = Some(Instant::now());
        }
        let from = {
            let kept = self.checkpoints.lock().unwrap();
            kept.values().rev().take(64).filter(|c| c.agreed.len() < c.of).map(|c| c.epoch).min()
        };
        let Some(from) = from else { return };
        let holds = holdings(&payments.checkpoints_each(from, 64));
        let mut kept = self.checkpoints.lock().unwrap();
        for checkpoint in kept.range_mut(from..).map(|(_, c)| c) {
            let digest = checkpoint.digest_bytes;
            checkpoint.agreed.extend(holders(&holds, checkpoint.epoch, &digest));
            checkpoint.agreed.sort_unstable();
            checkpoint.agreed.dedup();
        }
    }

    fn overview(&self, payments: &dyn crate::wallets::Payments) -> anyhow::Result<Overview> {
        if let Some((at, known)) = self.overview.lock().unwrap().as_ref()
            && at.elapsed() < FRESH
        {
            return Ok(known.clone());
        }
        let status = payments.status()?;
        self.catch_up(payments, status.epoch);
        let kept = self.checkpoints.lock().unwrap();
        let members = kept.values().next_back().map(|last| last.next.clone()).unwrap_or_default();
        let recent: Vec<&Checkpoint> = kept.values().rev().take(RECENT).collect();
        let validators = members
            .iter()
            .enumerate()
            .map(|(seat, member)| {
                let answered = status.validators.iter().find(|v| v.up && v.seat == Some(seat as u16));
                Validator {
                    seat,
                    name: answered.map_or_else(|| format!("Validator {}", seat + 1), |v| v.name.clone()),
                    up: answered.is_some(),
                    epoch: answered.map_or(0, |v| v.epoch),
                    agreed: recent.iter().filter(|c| c.agreed.contains(&(seat as u16))).count(),
                    of: recent.len(),
                    stake: member.stake.to_string(),
                    account: member.account.map(|id| hex::encode(id.as_bytes())),
                }
            })
            .collect();
        let checkpoints = kept.values().rev().take(RECENT).cloned().collect();
        drop(kept);
        let supply = payments.tokenomics().ok().map(|t| Supply {
            cap: t.cap.to_string(),
            released: t.released.to_string(),
            per_hour: t.per_hour.to_string(),
            in_wallets: t.in_wallets.to_string(),
            staked: t.staked.to_string(),
            stakers: t.stakers,
            holding: t.holding,
            holding_one: t.holding_one,
            burned: t.burned.to_string(),
        });
        let overview = Overview {
            epoch: status.epoch,
            epoch_started: status.epoch_started,
            epoch_seconds: status.epoch_seconds,
            mining: status.math,
            validators,
            supply,
            checkpoints,
        };
        *self.overview.lock().unwrap() = Some((Instant::now(), overview.clone()));
        Ok(overview)
    }

    fn address(&self, payments: &dyn crate::wallets::Payments, address: &str) -> anyhow::Result<Address> {
        if let Some((at, known)) = self.addresses.lock().unwrap().get(address)
            && at.elapsed() < FRESH_ADDRESS
        {
            return Ok(known.clone());
        }
        // The checkpoints, for when the records start.
        let _ = self.overview(payments);
        let bytes: [u8; 32] = hex::decode(address)?.try_into().map_err(|_| anyhow::anyhow!("not an address"))?;
        let looked = payments.lookup(&orien_crypto::Hash32(bytes))?;
        let more = looked.entries.len() > MAX_ENTRIES;
        let activity = looked
            .entries
            .iter()
            .rev()
            .take(MAX_ENTRIES)
            .map(|entry| Entry {
                epoch: entry.epoch,
                time: entry.time,
                kind: match entry.income {
                    Income::Payment => "received",
                    Income::Sent => "sent",
                    Income::Mining => "mined",
                    Income::Fees => "fees",
                    Income::Claim => "claimed",
                    Income::Refund => "refunded",
                    Income::Unstaked => "unstaked",
                },
                amount: entry.amount.to_string(),
                counterparty: entry.counterparty.map(|id| hex::encode(id.as_bytes())),
            })
            .collect();
        let found = Address {
            address: address.to_string(),
            balance: looked.balance.max(0).to_string(),
            staked: looked.staked.to_string(),
            returning: looked.returning.to_string(),
            payments_made: looked.payments_made,
            activity,
            more,
            since: looked.since,
            since_time: self.checkpoints.lock().unwrap().get(&looked.since).map(|c| c.time),
        };
        let mut kept = self.addresses.lock().unwrap();
        if kept.len() >= MAX_ADDRESSES {
            kept.clear();
        }
        kept.insert(address.to_string(), (Instant::now(), found.clone()));
        Ok(found)
    }
}

/// Each answering validator's checkpoints, by seat: epoch to digest.
fn holdings(answers: &[Option<Vec<orien_ledger::close::Decided>>]) -> Vec<(u16, HashMap<u64, orien_crypto::Hash32>)> {
    answers
        .iter()
        .enumerate()
        .filter_map(|(seat, found)| {
            let found = found.as_ref()?;
            Some((seat as u16, found.iter().map(|d| (d.checkpoint.epoch, d.checkpoint.digest())).collect()))
        })
        .collect()
}

/// The seats holding the checkpoint of `epoch` with this digest.
fn holders<'a>(
    holds: &'a [(u16, HashMap<u64, orien_crypto::Hash32>)],
    epoch: u64,
    digest: &'a orien_crypto::Hash32,
) -> impl Iterator<Item = u16> + 'a {
    holds.iter().filter(move |(_, known)| known.get(&epoch) == Some(digest)).map(|(seat, _)| *seat)
}

type Answer = Result<Response, Failure>;

fn unavailable(error: anyhow::Error) -> Failure {
    Failure::Unavailable(format!("the network did not answer: {error:#}"))
}

/// The network now: validators, supply and the latest checkpoints.
pub async fn overview(State(app): State<Arc<App>>) -> Answer {
    let found = tokio::task::spawn_blocking(move || app.explorer().overview(app.payments().as_ref()))
        .await
        .map_err(|e| Failure::Broken(e.into()))?
        .map_err(unavailable)?;
    Ok(([(header::CACHE_CONTROL, "public, max-age=15")], Json(found)).into_response())
}

#[derive(Deserialize)]
pub struct Before {
    before: Option<u64>,
}

/// Checkpoints, newest first, a page at a time: those before `before`.
pub async fn checkpoints(State(app): State<Arc<App>>, Query(query): Query<Before>) -> Answer {
    let found = tokio::task::spawn_blocking(move || -> anyhow::Result<serde_json::Value> {
        let explorer = app.explorer();
        explorer.overview(app.payments().as_ref())?;
        let kept = explorer.checkpoints.lock().unwrap();
        let before = query.before.unwrap_or(u64::MAX);
        let page: Vec<Checkpoint> = kept.range(..before).rev().take(PAGE as usize).map(|(_, c)| c.clone()).collect();
        let more = page.last().is_some_and(|last| last.epoch > 0);
        Ok(serde_json::json!({ "checkpoints": page, "more": more }))
    })
    .await
    .map_err(|e| Failure::Broken(e.into()))?
    .map_err(unavailable)?;
    Ok(([(header::CACHE_CONTROL, "public, max-age=15")], Json(found)).into_response())
}

/// One checkpoint, with the validators of its hour.
pub async fn checkpoint(State(app): State<Arc<App>>, Path(epoch): Path<u64>) -> Answer {
    let found = tokio::task::spawn_blocking(move || -> anyhow::Result<Option<serde_json::Value>> {
        let explorer = app.explorer();
        explorer.overview(app.payments().as_ref())?;
        let kept = explorer.checkpoints.lock().unwrap();
        let Some(checkpoint) = kept.get(&epoch) else { return Ok(None) };
        // Its hour's validators: those the checkpoint before named.
        let members = match epoch.checked_sub(1) {
            Some(before) => kept.get(&before).map(|c| c.next.clone()).unwrap_or_default(),
            None => app.payments().launch_seats(),
        };
        let validators: Vec<serde_json::Value> = members
            .iter()
            .enumerate()
            .map(|(seat, member)| {
                serde_json::json!({
                    "seat": seat,
                    "stake": member.stake.to_string(),
                    "account": member.account.map(|id| hex::encode(id.as_bytes())),
                    "agreed": checkpoint.agreed.contains(&(seat as u16)),
                })
            })
            .collect();
        Ok(Some(serde_json::json!({
            "checkpoint": checkpoint,
            "previous": hex::encode(
                epoch.checked_sub(1).and_then(|b| kept.get(&b)).map(|c| c.digest_bytes).unwrap_or_default().as_bytes()
            ),
            "validators": validators,
            "latest": kept.keys().next_back(),
        })))
    })
    .await
    .map_err(|e| Failure::Broken(e.into()))?
    .map_err(unavailable)?
    .ok_or(Failure::NotFound)?;
    Ok(([(header::CACHE_CONTROL, "public, max-age=60")], Json(found)).into_response())
}

/// The largest holders, from the latest checkpoint.
pub async fn top(State(app): State<Arc<App>>) -> Answer {
    let found = tokio::task::spawn_blocking(move || app.payments().tokenomics())
        .await
        .map_err(|e| Failure::Broken(e.into()))?
        .map_err(unavailable)?;
    let holders: Vec<serde_json::Value> = found
        .top
        .iter()
        .map(|holder| {
            serde_json::json!({
                "address": holder.address,
                "holds": holder.holds.to_string(),
                "staked": holder.staked.to_string(),
                "sent": holder.sent,
                "share": holder.share,
            })
        })
        .collect();
    let body = serde_json::json!({
        "epoch": found.epoch,
        "in_wallets": found.in_wallets.to_string(),
        "holding": found.holding,
        "holders": holders,
    });
    Ok(([(header::CACHE_CONTROL, "public, max-age=60")], Json(body)).into_response())
}

/// An address's balance and activity.
pub async fn address(State(app): State<Arc<App>>, Path(address): Path<String>) -> Answer {
    let address = crate::store::orien_address(&address).map_err(|e| Failure::Refused(e.to_string()))?;
    let found = tokio::task::spawn_blocking(move || app.explorer().address(app.payments().as_ref(), &address))
        .await
        .map_err(|e| Failure::Broken(e.into()))?
        .map_err(unavailable)?;
    Ok(([(header::CACHE_CONTROL, "public, max-age=20")], Json(found)).into_response())
}
