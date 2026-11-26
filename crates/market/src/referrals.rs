//! An invite qualifies after the new account's verified wallet earns mining
//! rewards in ten distinct checkpoints. The server never holds the payout key.

use crate::api::{App, Failure, refused};
use crate::explorer::Mined;
use crate::store::{Store, now, orien_address, orn, random};
use anyhow::{Result, ensure};
use axum::Json;
use axum::extract::State;
use axum::http::{HeaderMap, header};
use axum::response::{IntoResponse, Response};
use orien_ledger::{AccountId, Income};
use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

const UNIT: u64 = 1_000_000_000;
pub const REWARD: u64 = 7 * UNIT;
pub const BUDGET: u64 = 1_400 * UNIT;
const PAIR_REWARD: u64 = REWARD * 2;
const HOURS: u64 = 10;
const INVITER_LIMIT: u64 = 5;
const CLAIM_WINDOW: u64 = 7 * 24 * 3600;

#[derive(Clone, Serialize)]
pub(crate) struct Campaign {
    pub wallet: String,
    pub budget: u64,
    pub remaining: u64,
}

impl Campaign {
    fn open(&self) -> bool {
        self.remaining >= PAIR_REWARD
    }
}

#[derive(Serialize)]
struct ReferralStatus {
    active: bool,
    open: bool,
    remaining: String,
    reward: u64,
    hours_required: u64,
    invite_limit: u64,
    code: Option<String>,
    link: Option<String>,
    can_invite: bool,
    can_claim: bool,
    invited: u64,
    completed: u64,
    paid: u64,
    claim: Option<ClaimStatus>,
}

#[derive(Serialize)]
struct ClaimStatus {
    hours: u64,
    status: &'static str,
    verified: bool,
    mining_wallet: Option<String>,
}

#[derive(Serialize)]
pub(crate) struct Payout {
    pub id: i64,
    pub recipient: String,
    pub name: Option<String>,
    pub address: String,
    pub epoch: u64,
    pub sequence: Option<u64>,
    pub suspended: bool,
}

impl Store {
    pub(crate) fn create_referral_tables(&self) -> Result<()> {
        self.db.execute_batch(
            "CREATE TABLE IF NOT EXISTS referral_campaign (
                id INTEGER PRIMARY KEY CHECK (id = 1),
                wallet TEXT NOT NULL,
                budget INTEGER NOT NULL
             );
             CREATE TABLE IF NOT EXISTS referral_wallets (
                address TEXT PRIMARY KEY,
                account TEXT NOT NULL,
                proven INTEGER NOT NULL
             );
             CREATE INDEX IF NOT EXISTS referral_wallets_account ON referral_wallets (account, proven);
             INSERT OR IGNORE INTO referral_wallets (address, account, proven)
                SELECT v.address, a.handle, v.proven FROM verified_wallets v JOIN accounts a ON a.id = v.account
                WHERE v.proven IS NOT NULL ORDER BY v.proven, v.account;
             CREATE TABLE IF NOT EXISTS referral_codes (
                account TEXT PRIMARY KEY,
                code TEXT NOT NULL UNIQUE
             );
             CREATE TABLE IF NOT EXISTS referral_claims (
                id INTEGER PRIMARY KEY,
                invitee TEXT NOT NULL UNIQUE,
                inviter TEXT NOT NULL,
                claimed INTEGER NOT NULL,
                earned_epoch INTEGER,
                invitee_wallet TEXT UNIQUE,
                inviter_wallet TEXT,
                closed INTEGER NOT NULL DEFAULT 0
             );
             CREATE INDEX IF NOT EXISTS referral_claims_inviter ON referral_claims (inviter, earned_epoch);
             CREATE TABLE IF NOT EXISTS referral_hours (
                claim INTEGER NOT NULL REFERENCES referral_claims(id),
                epoch INTEGER NOT NULL,
                PRIMARY KEY (claim, epoch)
             );
             CREATE TABLE IF NOT EXISTS referral_payments (
                claim INTEGER NOT NULL REFERENCES referral_claims(id),
                recipient TEXT NOT NULL CHECK (recipient IN ('inviter', 'invitee')),
                sequence INTEGER NOT NULL UNIQUE,
                PRIMARY KEY (claim, recipient)
             );",
        )?;
        Ok(())
    }

    pub(crate) fn note_referral_wallet(
        &self,
        account: i64,
        address: &str,
        proven: u64,
    ) -> Result<()> {
        // Keep the first owner even after unlinking or deleting their account.
        self.db.execute(
            "INSERT OR IGNORE INTO referral_wallets (address, account, proven)
             SELECT ?2, handle, ?3 FROM accounts WHERE id = ?1",
            params![account, address, proven],
        )?;
        Ok(())
    }

    pub(crate) fn referral_campaign(&self) -> Result<Option<Campaign>> {
        Ok(self.db.query_row(
            "SELECT wallet, budget, budget - ?1 * (SELECT COUNT(*) FROM referral_claims WHERE earned_epoch IS NOT NULL)
             FROM referral_campaign WHERE id = 1",
            [PAIR_REWARD],
            |row| Ok(Campaign { wallet: row.get(0)?, budget: row.get(1)?, remaining: row.get(2)? }),
        ).optional()?)
    }

    pub(crate) fn start_referrals(&self, wallet: &str) -> Result<()> {
        let changed = self.db.execute(
            "INSERT OR IGNORE INTO referral_campaign (id, wallet, budget) VALUES (1, ?1, ?2)",
            params![orien_address(wallet)?, BUDGET],
        )?;
        ensure!(changed == 1, "the referral campaign has already started");
        Ok(())
    }

    fn referral_account(&self, id: i64) -> Result<(String, u64)> {
        Ok(self.db.query_row(
            "SELECT handle, created FROM accounts WHERE id = ?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?)
    }

    fn referral_wallet(&self, handle: &str, funding_wallet: &str) -> Result<Option<String>> {
        Ok(self
            .db
            .query_row(
                "SELECT v.address FROM verified_wallets v JOIN accounts a ON a.id = v.account
             JOIN referral_wallets w ON w.address = v.address AND w.account = a.handle
             WHERE a.handle = ?1 AND v.proven IS NOT NULL AND v.address != ?2
               AND NOT EXISTS (SELECT 1 FROM suspended s WHERE s.account = a.id)
             ORDER BY v.address IS NOT a.payout, w.proven, v.address LIMIT 1",
                params![handle, funding_wallet],
                |row| row.get(0),
            )
            .optional()?)
    }

    fn referral_totals(&self, inviter: &str) -> Result<(u64, u64, u64)> {
        Ok(self.db.query_row(
            "SELECT COUNT(*), COUNT(c.earned_epoch), COUNT(p.sequence) FROM referral_claims c
             LEFT JOIN referral_payments p ON p.claim = c.id AND p.recipient = 'inviter'
             WHERE c.inviter = ?1 AND c.closed = 0",
            [inviter],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )?)
    }

    fn referral_code(&self, account: &str) -> Result<String> {
        loop {
            if let Some(code) = self
                .db
                .query_row(
                    "SELECT code FROM referral_codes WHERE account = ?1",
                    [account],
                    |r| r.get(0),
                )
                .optional()?
            {
                return Ok(code);
            }
            let code = hex::encode_upper(random::<6>()?);
            self.db.execute(
                "INSERT OR IGNORE INTO referral_codes (account, code) VALUES (?1, ?2)",
                params![account, code],
            )?;
        }
    }

    fn referral_status(&self, id: i64, origin: &str) -> Result<ReferralStatus> {
        let (account, created) = self.referral_account(id)?;
        let campaign = self.referral_campaign()?;
        let open = campaign.as_ref().is_some_and(Campaign::open);
        let (invited, completed, paid) = self.referral_totals(&account)?;
        let mining_wallet = campaign
            .as_ref()
            .map(|c| self.referral_wallet(&account, &c.wallet))
            .transpose()?
            .flatten();
        let verified = mining_wallet.is_some();
        let claim = self.db.query_row(
            "SELECT (SELECT COUNT(*) FROM referral_hours h WHERE h.claim = c.id), c.earned_epoch, c.closed, p.sequence
             FROM referral_claims c LEFT JOIN referral_payments p ON p.claim = c.id AND p.recipient = 'invitee'
             WHERE c.invitee = ?1",
            [&account],
            |row| {
                let status = if row.get::<_, Option<u64>>(3)?.is_some() { "paid" }
                    else if row.get::<_, Option<u64>>(1)?.is_some() { "qualified" }
                    else if row.get::<_, bool>(2)? || !open { "closed" }
                    else { "pending" };
                Ok(ClaimStatus { hours: row.get(0)?, status, verified, mining_wallet: mining_wallet.clone() })
            },
        ).optional()?;
        let can_invite = open && verified && completed < INVITER_LIMIT;
        let code = can_invite
            .then(|| self.referral_code(&account))
            .transpose()?;
        let link = code
            .as_ref()
            .map(|code| format!("{origin}/try/?ref={code}"));
        Ok(ReferralStatus {
            active: campaign.is_some(),
            open,
            remaining: campaign
                .as_ref()
                .map_or_else(|| "0".into(), |c| orn(c.remaining)),
            reward: REWARD / UNIT,
            hours_required: HOURS,
            invite_limit: INVITER_LIMIT,
            code,
            link,
            can_invite,
            can_claim: open && claim.is_none() && now().saturating_sub(created) <= CLAIM_WINDOW,
            invited,
            completed,
            paid: paid * (REWARD / UNIT),
            claim,
        })
    }

    fn claim_referral(&self, id: i64, code: &str) -> Result<()> {
        let code = code.trim().to_ascii_uppercase();
        ensure!(
            code.len() == 12 && code.bytes().all(|b| b.is_ascii_hexdigit()),
            "enter the 12-character invite code"
        );
        let tx = self.db.unchecked_transaction()?;
        let (account, created) = self.referral_account(id)?;
        let existing: Option<String> = tx.query_row(
            "SELECT r.code FROM referral_claims c JOIN referral_codes r ON r.account = c.inviter WHERE c.invitee = ?1",
            [&account],
            |row| row.get(0),
        ).optional()?;
        if let Some(existing) = existing {
            ensure!(existing == code, "your account already has a referral code");
            return Ok(());
        }
        ensure!(
            now().saturating_sub(created) <= CLAIM_WINDOW,
            "you can enter a code within seven days of creating your account"
        );
        let campaign = self
            .referral_campaign()?
            .filter(Campaign::open)
            .ok_or_else(|| anyhow::anyhow!("referrals are closed"))?;
        let inviter: String = tx
            .query_row(
                "SELECT account FROM referral_codes WHERE code = ?1",
                [&code],
                |r| r.get(0),
            )
            .optional()?
            .ok_or_else(|| anyhow::anyhow!("that referral code was not found"))?;
        ensure!(inviter != account, "you can't use your own referral code");
        ensure!(
            self.referral_wallet(&inviter, &campaign.wallet)?.is_some(),
            "that referral code is no longer available"
        );
        ensure!(
            self.referral_totals(&inviter)?.1 < INVITER_LIMIT,
            "that inviter has reached their limit"
        );
        tx.execute(
            "INSERT INTO referral_claims (invitee, inviter, claimed) VALUES (?1, ?2, ?3)",
            params![account, inviter, now()],
        )?;
        tx.commit()?;
        Ok(())
    }

    pub(crate) fn track_referral_mining(&self, mined: &Mined) -> Result<()> {
        let tx = self.db.unchecked_transaction()?;
        let Some(mut campaign) = self.referral_campaign()?.filter(Campaign::open) else {
            return Ok(());
        };
        let pending = {
            let mut query = tx.prepare(
                "SELECT id, invitee, inviter, claimed FROM referral_claims
                 WHERE earned_epoch IS NULL AND closed = 0 ORDER BY claimed, id",
            )?;
            query
                .query_map([], |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, u64>(3)?,
                    ))
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?
        };
        for (id, invitee, inviter, claimed) in pending {
            if !campaign.open() {
                break;
            }
            if self.referral_totals(&inviter)?.1 >= INVITER_LIMIT {
                tx.execute("UPDATE referral_claims SET closed = 1 WHERE id = ?1", [id])?;
                continue;
            }
            let Some(wallet) = self.referral_wallet(&invitee, &campaign.wallet)? else {
                continue;
            };
            let proven: u64 = tx.query_row(
                "SELECT proven FROM referral_wallets WHERE address = ?1",
                [&wallet],
                |r| r.get(0),
            )?;
            let start = claimed.max(proven);
            let mut hours: u64 = tx.query_row(
                "SELECT COUNT(*) FROM referral_hours WHERE claim = ?1",
                [id],
                |r| r.get(0),
            )?;
            if hours < HOURS {
                // The previous checkpoint must close after the wallet was verified
                // and the code claimed, so a partial starting epoch does not count.
                if let Some((&previous, _)) = mined.times.iter().find(|(_, time)| **time >= start) {
                    for (&epoch, paid) in mined.by_epoch.range(previous.saturating_add(1)..) {
                        if !mined.times.contains_key(&epoch)
                            || !paid
                                .iter()
                                .any(|(address, amount)| address == &wallet && *amount > 0)
                        {
                            continue;
                        }
                        hours += tx.execute(
                            "INSERT OR IGNORE INTO referral_hours (claim, epoch) VALUES (?1, ?2)",
                            params![id, epoch],
                        )? as u64;
                        if hours == HOURS {
                            break;
                        }
                    }
                }
            }
            if hours == HOURS
                && let Some(inviter_wallet) = self.referral_wallet(&inviter, &campaign.wallet)?
            {
                tx.execute(
                    "UPDATE referral_claims SET earned_epoch = ?2, invitee_wallet = ?3, inviter_wallet = ?4 WHERE id = ?1",
                    params![id, mined.upto.saturating_add(1), wallet, inviter_wallet],
                )?;
                campaign.remaining -= PAIR_REWARD;
            }
        }
        tx.commit()?;
        Ok(())
    }

    pub(crate) fn referral_payouts(&self) -> Result<Vec<Payout>> {
        let mut query = self.db.prepare(
            "WITH rewards AS (
                SELECT id, 'inviter' AS recipient, inviter AS account, inviter_wallet AS address, earned_epoch FROM referral_claims
                UNION ALL
                SELECT id, 'invitee', invitee, invitee_wallet, earned_epoch FROM referral_claims
             )
             SELECT r.id, r.recipient, a.name, r.address, r.earned_epoch, p.sequence,
                    EXISTS (SELECT 1 FROM suspended s WHERE s.account = a.id)
             FROM rewards r
             LEFT JOIN accounts a ON a.handle = r.account
             LEFT JOIN referral_payments p ON p.claim = r.id AND p.recipient = r.recipient
             WHERE r.earned_epoch IS NOT NULL ORDER BY p.sequence IS NOT NULL, r.id, r.recipient",
        )?;
        let rows = query.query_map([], |r| {
            Ok(Payout {
                id: r.get(0)?,
                recipient: r.get(1)?,
                name: r.get(2)?,
                address: r.get(3)?,
                epoch: r.get(4)?,
                sequence: r.get(5)?,
                suspended: r.get(6)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    pub(crate) fn record_referral_payment(
        &self,
        id: i64,
        recipient: &str,
        sequences: &[u64],
    ) -> Result<()> {
        let tx = self.db.unchecked_transaction()?;
        for sequence in sequences {
            tx.execute(
                "INSERT OR IGNORE INTO referral_payments (claim, recipient, sequence) VALUES (?1, ?2, ?3)",
                params![id, recipient, sequence],
            )?;
            let recorded: bool = tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM referral_payments WHERE claim = ?1 AND recipient = ?2)",
                params![id, recipient],
                |r| r.get(0),
            )?;
            if recorded {
                tx.commit()?;
                return Ok(());
            }
        }
        anyhow::bail!("no new finalized reward payment from the campaign wallet was found")
    }
}

pub(crate) fn address_id(address: &str) -> Result<AccountId> {
    Ok(orien_crypto::Hash32(
        hex::decode(orien_address(address)?)?
            .try_into()
            .expect("an Orien address is 32 bytes"),
    ))
}

pub(crate) fn funding_required(network: &dyn crate::wallets::Payments) -> u64 {
    BUDGET + (BUDGET / REWARD) * network.payment_fee(REWARD)
}

pub(crate) fn paid_sequences(
    network: &dyn crate::wallets::Payments,
    from: AccountId,
    to: AccountId,
    epoch: u64,
) -> Result<Vec<u64>> {
    let mut start = epoch;
    let mut found = Vec::new();
    while let Some(history) = network.earnings(&to, start) {
        found.extend(history.entries.into_iter().filter_map(|entry| {
            let (sender, sequence) = entry.order?;
            (sender == from
                && entry.income == Income::Payment
                && entry.amount == REWARD
                && entry.time.is_some()
                && entry.epoch >= epoch)
                .then_some(sequence)
        }));
        let Some(next) = history.next.filter(|next| *next > start) else {
            break;
        };
        start = next;
    }
    Ok(found)
}

type Answer = Result<Response, Failure>;

pub(crate) async fn status(State(app): State<Arc<App>>, headers: HeaderMap) -> Answer {
    let id = app.signed_in(&headers)?;
    let status = app.store().referral_status(id, app.origin())?;
    Ok(([(header::CACHE_CONTROL, "no-store")], Json(status)).into_response())
}

#[derive(Deserialize)]
pub(crate) struct Code {
    code: String,
}

pub(crate) async fn claim(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Json(body): Json<Code>,
) -> Answer {
    let id = app.signed_in(&headers)?;
    let store = app.store();
    store.claim_referral(id, &body.code).map_err(refused)?;
    Ok(Json(store.referral_status(id, app.origin())?).into_response())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    const FUNDING: &str = "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff";
    const INVITER_WALLET: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const INVITEE_WALLET: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

    fn campaign() -> (Store, i64, i64, String) {
        let store = Store::in_memory().unwrap();
        let inviter = store.create_account("Inviter", "inviter").unwrap().id;
        let invitee = store.create_account("Invitee", "invitee").unwrap().id;
        store
            .verified_wallet_for_tests(inviter, INVITER_WALLET)
            .unwrap();
        store.start_referrals(FUNDING).unwrap();
        let code = store
            .referral_status(inviter, "https://orien.network")
            .unwrap()
            .code
            .unwrap();
        (store, inviter, invitee, code)
    }

    fn checkpoints(hours: u64) -> Mined {
        let base = now() + 3600;
        let times = (1..=hours + 1)
            .map(|epoch| (epoch, base + epoch * 3600))
            .collect();
        let by_epoch: BTreeMap<_, _> = (1..=hours + 1)
            .map(|epoch| {
                (
                    epoch,
                    vec![
                        (INVITEE_WALLET.to_string(), 1),
                        (INVITEE_WALLET.to_string(), 2),
                    ],
                )
            })
            .collect();
        Mined {
            since: 1,
            upto: hours + 1,
            by_epoch,
            times,
        }
    }

    #[test]
    fn referral_rewards_need_ten_distinct_checkpoints_and_pay_each_side_once() {
        let (store, inviter, invitee, code) = campaign();
        assert!(store.claim_referral(inviter, &code).is_err());
        store.claim_referral(invitee, &code.to_lowercase()).unwrap();
        store.claim_referral(invitee, &code).unwrap();
        store.track_referral_mining(&checkpoints(10)).unwrap();
        assert_eq!(
            store
                .referral_status(invitee, "")
                .unwrap()
                .claim
                .unwrap()
                .hours,
            0
        );

        store
            .verified_wallet_for_tests(invitee, INVITEE_WALLET)
            .unwrap();
        store.track_referral_mining(&checkpoints(9)).unwrap();
        store.track_referral_mining(&checkpoints(9)).unwrap();
        let claim = store.referral_status(invitee, "").unwrap().claim.unwrap();
        assert_eq!((claim.hours, claim.status), (9, "pending"));
        assert!(store.referral_payouts().unwrap().is_empty());

        store.track_referral_mining(&checkpoints(10)).unwrap();
        store.track_referral_mining(&checkpoints(10)).unwrap();
        assert_eq!(
            store.referral_campaign().unwrap().unwrap().remaining,
            BUDGET - 2 * REWARD
        );
        let payouts = store.referral_payouts().unwrap();
        assert_eq!(payouts.len(), 2);
        assert_eq!(
            (payouts[0].address.as_str(), payouts[1].address.as_str()),
            (INVITEE_WALLET, INVITER_WALLET)
        );
        let id = payouts[0].id;
        store.record_referral_payment(id, "invitee", &[15]).unwrap();
        assert!(store.record_referral_payment(id, "inviter", &[15]).is_err());
        store
            .record_referral_payment(id, "inviter", &[15, 16])
            .unwrap();
        assert_eq!(store.referral_status(inviter, "").unwrap().paid, 7);
        assert_eq!(
            store
                .referral_status(invitee, "")
                .unwrap()
                .claim
                .unwrap()
                .status,
            "paid"
        );

        store.remove_proof(invitee, INVITEE_WALLET).unwrap();
        let repeat = store.create_account("Another", "other").unwrap().id;
        store
            .verified_wallet_for_tests(repeat, INVITEE_WALLET)
            .unwrap();
        assert!(!store.referral_status(repeat, "").unwrap().can_invite);
    }

    #[test]
    fn the_hundredth_referral_is_the_last() {
        let (store, _, invitee, code) = campaign();
        for i in 0..99 {
            store.db.execute(
                "INSERT INTO referral_claims (invitee, inviter, claimed, earned_epoch) VALUES (?1, 'past-inviter', 0, 0)",
                [format!("past-invitee-{i}")],
            ).unwrap();
        }
        store.claim_referral(invitee, &code).unwrap();
        store
            .verified_wallet_for_tests(invitee, INVITEE_WALLET)
            .unwrap();
        store.track_referral_mining(&checkpoints(10)).unwrap();
        let campaign = store.referral_campaign().unwrap().unwrap();
        assert_eq!(campaign.remaining, 0);
        assert!(!campaign.open());
        let late = store.create_account("Late", "late").unwrap().id;
        assert!(store.claim_referral(late, &code).is_err());
    }
}
