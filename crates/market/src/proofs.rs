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

/// Whose a verified wallet is: the account's name and picture, and the
/// wallet's own name.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct WalletProfile {
    pub id: i64,
    pub name: String,
    pub picture: Option<String>,
    pub wallet_name: Option<String>,
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

    /// Stops `account` naming `address`, verified or waiting.
    pub fn remove_proof(&self, account: i64, address: &str) -> Result<()> {
        let address = orien_address(address)?;
        self.db.execute("DELETE FROM verified_wallets WHERE account = ?1 AND address = ?2", params![account, address])?;
        Ok(())
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

    pub fn mark_proven(&self, account: i64, address: &str) -> Result<()> {
        self.db.execute(
            "UPDATE verified_wallets SET proven = ?3 WHERE account = ?1 AND address = ?2",
            params![account, address, now() as i64],
        )?;
        Ok(())
    }

    /// Whose verified wallets are among `addresses`, by address. When
    /// several accounts verified one wallet, the latest wins.
    pub fn wallet_profiles(&self, addresses: &[String]) -> Result<Vec<(String, WalletProfile)>> {
        let mut found: Vec<(String, WalletProfile)> = Vec::new();
        let mut query = self.db.prepare(&format!(
            "SELECT a.id, a.name, a.picture, {NAME} FROM verified_wallets v JOIN accounts a ON a.id = v.account
             WHERE v.address = ?1 AND v.proven IS NOT NULL
               AND NOT EXISTS (SELECT 1 FROM suspended s WHERE s.account = a.id)
             ORDER BY v.proven DESC LIMIT 1"
        ))?;
        for address in addresses {
            let Ok(address) = orien_address(address) else { continue };
            if found.iter().any(|(a, _)| *a == address) {
                continue;
            }
            let profile = query
                .query_row([&address], |row| {
                    Ok(WalletProfile { id: row.get(0)?, name: row.get(1)?, picture: row.get(2)?, wallet_name: row.get(3)? })
                })
                .optional()?;
            if let Some(profile) = profile {
                found.push((address, profile));
            }
        }
        Ok(found)
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
}
