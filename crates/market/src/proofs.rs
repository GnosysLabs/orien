//! Proving that wallets are an account's own: each wallet sends the
//! Market's wallet a tiny amount, different for each waiting proof, and once
//! that payment is final the wallet is verified. An account may verify as
//! many wallets as it likes, each by choice. Verified wallets carry their
//! owner's name and picture, and the name the wallet has in Orien Wallet
//! ("Savings"), in other people's Orien Wallet, so nobody can put their name
//! on someone else's address.

use crate::store::{Store, now, orien_address};
use anyhow::Result;
use rusqlite::{OptionalExtension, params};
use serde::Serialize;

/// The smallest and largest proof amounts, in ORN's smallest units:
/// 0.0001 to 0.000999 ORN, so the whole proof, with its fee, costs less than
/// 0.001 ORN.
const LOWEST: u64 = 100_000;
const HIGHEST: u64 = 999_000;
/// Proof amounts go up in these steps, so they read cleanly: 0.000417.
const STEP: u64 = 1_000;
/// Most addresses one lookup may name.
pub const MAX_LOOKUP: usize = 100;
/// Most wallets one account may have verified or waiting.
pub const MAX_WALLETS: usize = 20;

/// What a wallet has to send to prove it is the account's, or that it has.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Proof {
    /// The wallet being proven.
    pub address: String,
    /// Its name in the account's Orien Wallet, if an app has said.
    pub name: Option<String>,
    /// The amount to send, in ORN's smallest units, and written out.
    pub amount: u64,
    pub orn: String,
    /// Where to send it: the Market's own wallet.
    pub to: String,
    pub verified: bool,
}

/// One of an account's verified wallets, as everyone sees it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct VerifiedWallet {
    pub address: String,
    /// Its name in the owner's Orien Wallet, such as "Savings".
    pub name: Option<String>,
}

/// A profile found by name, with the wallets it verified, for choosing
/// whom to pay.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct PayableProfile {
    pub id: i64,
    pub name: String,
    pub picture: Option<String>,
    /// When the account was made, to tell people with one name apart.
    pub created: u64,
    pub wallets: Vec<VerifiedWallet>,
}

/// Most profiles one search gives.
pub const MAX_FOUND: usize = 10;

/// Whose a verified wallet is: the account's public profile (name, bio,
/// picture, banner and the usernames it shows), and the wallet's own name.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct WalletProfile {
    pub id: i64,
    pub name: String,
    pub picture: Option<String>,
    pub wallet_name: Option<String>,
    pub bio: String,
    pub banner: Option<String>,
    #[serde(skip)]
    pub contact: crate::store::Contact,
}

/// The wallet's newest name in any of the account's apps.
const NAME: &str = "(SELECT w.name FROM account_wallets w WHERE w.account = v.account AND w.address = v.address
                    ORDER BY w.updated DESC LIMIT 1)";

impl Store {
    pub(crate) fn create_proof_table(&self) -> Result<()> {
        self.db.execute_batch(
            "CREATE TABLE IF NOT EXISTS verified_wallets (
                 account INTEGER NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
                 address TEXT NOT NULL,
                 amount INTEGER NOT NULL,
                 epoch INTEGER NOT NULL,
                 created INTEGER NOT NULL,
                 proven INTEGER,
                 PRIMARY KEY (account, address)
             );
             CREATE INDEX IF NOT EXISTS verified_wallets_address ON verified_wallets (address);",
        )?;
        // The first version kept one proof per account.
        let old: bool =
            self.db.query_row("SELECT COUNT(*) > 0 FROM sqlite_master WHERE type = 'table' AND name = 'wallet_proofs'", [], |row| row.get(0))?;
        if old {
            self.db.execute_batch(
                "INSERT OR IGNORE INTO verified_wallets (account, address, amount, epoch, created, proven)
                     SELECT account, address, amount, epoch, created, proven FROM wallet_proofs;
                 DROP TABLE wallet_proofs;",
            )?;
        }
        // Accounts that verified a wallet before that also made it the one
        // they're paid at.
        self.db.execute_batch(
            "UPDATE accounts SET payout = (SELECT v.address FROM verified_wallets v WHERE v.account = accounts.id
                                              AND v.proven IS NOT NULL ORDER BY v.proven LIMIT 1)
             WHERE payout IS NULL
               AND EXISTS (SELECT 1 FROM verified_wallets v WHERE v.account = accounts.id AND v.proven IS NOT NULL);",
        )?;
        Ok(())
    }

    /// The proof for `address` (the payout wallet when None): the one already
    /// asked for, or a new one from `epoch` on. None when there is no
    /// payout wallet to prove.
    pub fn start_proof(&self, account: i64, address: Option<&str>, epoch: u64, to: &str) -> Result<Option<Proof>> {
        let address = match address {
            Some(address) => orien_address(address)?,
            None => match self.payout_of(account)? {
                Some(payout) => payout,
                None => return Ok(None),
            },
        };
        if let Some(proof) = self.proofs(account, to)?.into_iter().find(|p| p.address == address) {
            return Ok(Some(proof));
        }
        let count: i64 = self.db.query_row("SELECT COUNT(*) FROM verified_wallets WHERE account = ?1", [account], |row| row.get(0))?;
        anyhow::ensure!((count as usize) < MAX_WALLETS, "at most {MAX_WALLETS} wallets; remove one first");
        // An amount no other waiting proof uses, so each payment names one.
        let taken: Vec<u64> = {
            let mut query = self.db.prepare("SELECT amount FROM verified_wallets WHERE proven IS NULL")?;
            query.query_map([], |row| row.get::<_, i64>(0))?.collect::<rusqlite::Result<Vec<_>>>()?.into_iter().map(|a| a as u64).collect()
        };
        let steps = (HIGHEST - LOWEST) / STEP + 1;
        let mut amount = 0;
        for _ in 0..50 {
            let mut bytes = [0u8; 8];
            getrandom::fill(&mut bytes).map_err(|e| anyhow::anyhow!("no randomness: {e}"))?;
            let candidate = LOWEST + u64::from_le_bytes(bytes) % steps * STEP;
            if !taken.contains(&candidate) {
                amount = candidate;
                break;
            }
        }
        anyhow::ensure!(amount > 0, "too many wallets are being verified right now; try again in a few minutes");
        self.db.execute(
            "INSERT INTO verified_wallets (account, address, amount, epoch, created, proven) VALUES (?1, ?2, ?3, ?4, ?5, NULL)",
            params![account, address, amount as i64, epoch as i64, now() as i64],
        )?;
        Ok(self.proofs(account, to)?.into_iter().find(|p| p.address == address))
    }

    /// Every wallet `account` verified or asked to, verified ones first.
    pub fn proofs(&self, account: i64, to: &str) -> Result<Vec<Proof>> {
        let mut query = self.db.prepare(&format!(
            "SELECT v.address, {NAME}, v.amount, v.proven FROM verified_wallets v WHERE v.account = ?1
             ORDER BY v.proven IS NULL, v.created"
        ))?;
        let rows = query.query_map([account], |row| {
            let amount = row.get::<_, i64>(2)? as u64;
            Ok(Proof {
                address: row.get(0)?,
                name: row.get(1)?,
                amount,
                orn: crate::store::orn(amount),
                to: to.to_string(),
                verified: row.get::<_, Option<i64>>(3)?.is_some(),
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// Stops `account` naming `address`, verified or waiting. The wallet
    /// buyers pay moves to another verified one; with none left and
    /// listings open, it can't be removed.
    pub fn remove_proof(&self, account: i64, address: &str) -> Result<()> {
        let address = orien_address(address)?;
        let paid_here = self.payout_of(account)?.as_deref() == Some(address.as_str());
        if paid_here {
            let other: Option<String> = self
                .db
                .query_row(
                    "SELECT address FROM verified_wallets WHERE account = ?1 AND address != ?2 AND proven IS NOT NULL ORDER BY proven LIMIT 1",
                    params![account, address],
                    |row| row.get(0),
                )
                .optional()?;
            match other {
                Some(other) => self.set_wallet(account, &other)?,
                None => {
                    let open: i64 =
                        self.db.query_row("SELECT COUNT(*) FROM listings WHERE seller = ?1 AND open = 1", [account], |row| row.get(0))?;
                    anyhow::ensure!(open == 0, "buyers pay this wallet: verify another first, or close your listings");
                    self.db.execute("UPDATE accounts SET payout = NULL WHERE id = ?1", [account])?;
                }
            }
        }
        self.db.execute("DELETE FROM verified_wallets WHERE account = ?1 AND address = ?2", params![account, address])?;
        Ok(())
    }

    /// For tests: makes `address` the account's verified wallet it is paid at.
    #[doc(hidden)]
    pub fn verified_wallet_for_tests(&self, account: i64, address: &str) -> Result<()> {
        self.set_wallet(account, address)?;
        self.start_proof(account, Some(address), 0, "to")?;
        self.mark_proven(account, &orien_address(address)?)
    }

    /// Whether the wallet `account` is paid at is one of its verified ones.
    pub fn wallet_verified(&self, account: i64) -> Result<bool> {
        Ok(match self.payout_of(account)? {
            Some(payout) => self.is_verified_wallet(account, &payout)?,
            None => false,
        })
    }

    /// Whether `address` is one of `account`'s verified wallets.
    pub fn is_verified_wallet(&self, account: i64, address: &str) -> Result<bool> {
        Ok(self
            .db
            .query_row(
                "SELECT 1 FROM verified_wallets WHERE account = ?1 AND address = ?2 AND proven IS NOT NULL",
                params![account, address],
                |_| Ok(()),
            )
            .optional()?
            .is_some())
    }

    /// `account`'s verified wallets, as its profile shows them.
    pub fn verified_wallets(&self, account: i64) -> Result<Vec<VerifiedWallet>> {
        let mut query = self.db.prepare(&format!(
            "SELECT v.address, {NAME} FROM verified_wallets v WHERE v.account = ?1 AND v.proven IS NOT NULL ORDER BY v.proven"
        ))?;
        let rows = query.query_map([account], |row| Ok(VerifiedWallet { address: row.get(0)?, name: row.get(1)? }))?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// Proofs still waiting for their payment: account, wallet, amount, and
    /// the epoch they were asked for in.
    pub fn waiting_proofs(&self) -> Result<Vec<(i64, String, u64, u64)>> {
        let mut query = self.db.prepare("SELECT account, address, amount, epoch FROM verified_wallets WHERE proven IS NULL")?;
        let rows = query.query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get::<_, i64>(2)? as u64, row.get::<_, i64>(3)? as u64)))?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// Marks a wallet verified. An account with no wallet to be paid at
    /// yet is paid at this one from now on, so it can sell straight away.
    pub fn mark_proven(&self, account: i64, address: &str) -> Result<()> {
        let tx = self.db.unchecked_transaction()?;
        let at = now();
        let changed = tx.execute(
            "UPDATE verified_wallets SET proven = ?3 WHERE account = ?1 AND address = ?2",
            params![account, address, at],
        )?;
        if changed != 0 {
            self.note_referral_wallet(account, address, at)?;
        }
        let unpaid: bool =
            self.db.query_row("SELECT payout IS NULL FROM accounts WHERE id = ?1", [account], |row| row.get(0)).optional()?.unwrap_or(false);
        if unpaid {
            self.set_wallet(account, address)?;
        }
        tx.commit()?;
        Ok(())
    }

    /// Whose verified wallets are among `addresses`, by address. When
    /// several accounts verified one wallet, the latest wins.
    /// Leaves out accounts that blocked `viewer`, or that `viewer` blocked.
    pub fn wallet_profiles(&self, addresses: &[String], viewer: Option<i64>) -> Result<Vec<(String, WalletProfile)>> {
        let mut found: Vec<(String, WalletProfile)> = Vec::new();
        let mut query = self.db.prepare(&format!(
            "SELECT a.id, a.name, a.picture, {NAME}, a.bio, a.banner, a.telegram, a.x, a.discord
             FROM verified_wallets v JOIN accounts a ON a.id = v.account
             WHERE v.address = ?1 AND v.proven IS NOT NULL
               AND NOT EXISTS (SELECT 1 FROM suspended s WHERE s.account = a.id)
               AND NOT EXISTS (SELECT 1 FROM blocks b WHERE (b.blocker = a.id AND b.blocked = ?2) OR (b.blocker = ?2 AND b.blocked = a.id))
             ORDER BY v.proven DESC LIMIT 1"
        ))?;
        for address in addresses {
            let Ok(address) = orien_address(address) else { continue };
            if found.iter().any(|(a, _)| *a == address) {
                continue;
            }
            let profile = query
                .query_row(params![&address, viewer.unwrap_or(-1)], |row| {
                    Ok(WalletProfile {
                        id: row.get(0)?,
                        name: row.get(1)?,
                        picture: row.get(2)?,
                        wallet_name: row.get(3)?,
                        bio: row.get(4)?,
                        banner: row.get(5)?,
                        contact: crate::store::Contact { telegram: row.get(6)?, x: row.get(7)?, discord: row.get(8)? },
                    })
                })
                .optional()?;
            if let Some(profile) = profile {
                found.push((address, profile));
            }
        }
        Ok(found)
    }

    /// Profiles whose name holds `query`, with at least one verified
    /// wallet: exact names first, then names that start with it.
    pub fn search_payable(&self, query: &str, viewer: Option<i64>) -> Result<Vec<PayableProfile>> {
        let query = query.trim().to_lowercase();
        if query.chars().count() < 2 {
            return Ok(Vec::new());
        }
        let escaped: String = query.chars().flat_map(|c| if matches!(c, '%' | '_' | '\\') { vec!['\\', c] } else { vec![c] }).collect();
        let mut statement = self.db.prepare(
            "SELECT a.id, a.name, a.picture, a.created FROM accounts a
             WHERE lower(a.name) LIKE ?1 ESCAPE '\\'
               AND EXISTS (SELECT 1 FROM verified_wallets v WHERE v.account = a.id AND v.proven IS NOT NULL)
               AND NOT EXISTS (SELECT 1 FROM suspended s WHERE s.account = a.id)
               AND NOT EXISTS (SELECT 1 FROM blocks b WHERE (b.blocker = a.id AND b.blocked = ?5) OR (b.blocker = ?5 AND b.blocked = a.id))
             ORDER BY lower(a.name) = ?2 DESC, lower(a.name) LIKE ?3 ESCAPE '\\' DESC, lower(a.name), a.id
             LIMIT ?4",
        )?;
        let found: Vec<(i64, String, Option<String>, u64)> = statement
            .query_map(params![format!("%{escaped}%"), query, format!("{escaped}%"), MAX_FOUND as i64, viewer.unwrap_or(-1)], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get::<_, i64>(3)? as u64))
            })?
            .collect::<rusqlite::Result<_>>()?;
        found
            .into_iter()
            .map(|(id, name, picture, created)| Ok(PayableProfile { id, name, picture, created, wallets: self.verified_wallets(id)? }))
            .collect()
    }

    fn payout_of(&self, account: i64) -> Result<Option<String>> {
        Ok(self.db.query_row("SELECT payout FROM accounts WHERE id = ?1", [account], |row| row.get(0)).optional()?.flatten())
    }
}

/// Looks for the payments that prove waiting wallets, and marks them.
pub fn check_once(app: &crate::api::App) -> Result<usize> {
    let waiting = app.store().waiting_proofs()?;
    let Some(from) = waiting.iter().map(|(_, _, _, epoch)| *epoch).min() else { return Ok(0) };
    let to: orien_ledger::AccountId = app.address().parse().map_err(|_| anyhow::anyhow!("the Market's address is not an Orien address"))?;
    let received = app.payments().received(&to, from)?;
    let mut proven = 0;
    for (account, address, amount, epoch) in waiting {
        let paid = received.iter().any(|p| p.amount == amount && p.epoch >= epoch && hex::encode(p.from.as_bytes()) == address);
        if paid {
            app.store().mark_proven(account, &address)?;
            proven += 1;
        }
    }
    Ok(proven)
}

/// Watches for proof payments for as long as the server runs.
pub fn watch(app: std::sync::Arc<crate::api::App>) {
    std::thread::spawn(move || {
        loop {
            if let Err(error) = check_once(&app) {
                eprintln!("wallet proofs: {error:#}");
            }
            std::thread::sleep(std::time::Duration::from_secs(5));
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_proof_from_the_first_version_carries_over() {
        let store = Store::in_memory().unwrap();
        let maya = store.create_account("Maya", "maya").unwrap().id;
        let wallet = "7a".repeat(32);
        store.db.execute_batch("DELETE FROM verified_wallets; CREATE TABLE wallet_proofs (account INTEGER PRIMARY KEY, address TEXT NOT NULL, amount INTEGER NOT NULL, epoch INTEGER NOT NULL, created INTEGER NOT NULL, proven INTEGER);").unwrap();
        store.db.execute("INSERT INTO wallet_proofs VALUES (?1, ?2, 417000, 30, 1, 2)", params![maya, wallet]).unwrap();
        store.create_proof_table().unwrap();
        assert_eq!(store.verified_wallets(maya).unwrap(), [VerifiedWallet { address: wallet, name: None }]);
        let gone: bool = store.db.query_row("SELECT COUNT(*) = 0 FROM sqlite_master WHERE name = 'wallet_proofs'", [], |r| r.get(0)).unwrap();
        assert!(gone);
    }

    #[test]
    fn a_first_verified_wallet_is_the_one_paid_at() {
        let store = Store::in_memory().unwrap();
        let sam = store.create_account("Sam", "sam").unwrap().id;
        let (first, second) = ("1a".repeat(32), "2b".repeat(32));
        for wallet in [&first, &second] {
            store.start_proof(sam, Some(wallet), 1, "to").unwrap();
            store.mark_proven(sam, wallet).unwrap();
        }
        assert_eq!(store.account(sam).unwrap().unwrap().wallet.as_deref(), Some(first.as_str()), "the first, and it stays");
        assert!(store.account(sam).unwrap().unwrap().wallet_verified);
    }
}
