//! Proving that a wallet is an account's own: the wallet sends the Market's
//! wallet a tiny amount, different for each account, and once that payment
//! is final the wallet is verified. Only verified wallets carry their
//! owner's name and picture in other people's Orien Wallet, so nobody can
//! put their name on someone else's address.

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

/// What an account's wallet has to send to prove it, or that it has.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Proof {
    /// The wallet being proven: the account's payout wallet.
    pub address: String,
    /// The amount to send, in ORN's smallest units, and written out.
    pub amount: u64,
    pub orn: String,
    /// Where to send it: the Market's own wallet.
    pub to: String,
    pub verified: bool,
}

/// A name and picture for a verified wallet.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct WalletProfile {
    pub name: String,
    pub picture: Option<String>,
}

impl Store {
    pub(crate) fn create_proof_table(&self) -> Result<()> {
        self.db.execute_batch(
            "CREATE TABLE IF NOT EXISTS wallet_proofs (
                 account INTEGER PRIMARY KEY REFERENCES accounts(id) ON DELETE CASCADE,
                 address TEXT NOT NULL,
                 amount INTEGER NOT NULL,
                 epoch INTEGER NOT NULL,
                 created INTEGER NOT NULL,
                 proven INTEGER
             );
             CREATE INDEX IF NOT EXISTS wallet_proofs_address ON wallet_proofs (address);",
        )?;
        Ok(())
    }

    /// The proof for `account`'s payout wallet: the one already asked for,
    /// or a new one from `epoch` on. None when it has no payout wallet.
    pub fn start_proof(&self, account: i64, epoch: u64, to: &str) -> Result<Option<Proof>> {
        let Some(address) = self.payout_of(account)? else { return Ok(None) };
        if let Some(proof) = self.proof(account, to)?
            && proof.address == address
        {
            return Ok(Some(proof));
        }
        // An amount no other waiting proof uses, so each payment names one.
        let taken: Vec<u64> = {
            let mut query = self.db.prepare("SELECT amount FROM wallet_proofs WHERE proven IS NULL")?;
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
            "INSERT OR REPLACE INTO wallet_proofs (account, address, amount, epoch, created, proven) VALUES (?1, ?2, ?3, ?4, ?5, NULL)",
            params![account, address, amount as i64, epoch as i64, now() as i64],
        )?;
        self.proof(account, to)
    }

    /// `account`'s proof, if it asked for one, for its current payout wallet
    /// or an earlier one.
    pub fn proof(&self, account: i64, to: &str) -> Result<Option<Proof>> {
        let payout = self.payout_of(account)?;
        Ok(self
            .db
            .query_row("SELECT address, amount, proven FROM wallet_proofs WHERE account = ?1", [account], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)? as u64, row.get::<_, Option<i64>>(2)?))
            })
            .optional()?
            .map(|(address, amount, proven)| Proof {
                verified: proven.is_some() && payout.as_deref() == Some(address.as_str()),
                address,
                amount,
                orn: crate::store::orn(amount),
                to: to.to_string(),
            }))
    }

    /// Whether `account`'s payout wallet is verified.
    pub fn wallet_verified(&self, account: i64) -> Result<bool> {
        Ok(self
            .db
            .query_row(
                "SELECT 1 FROM wallet_proofs p JOIN accounts a ON a.id = p.account
                 WHERE p.account = ?1 AND p.proven IS NOT NULL AND a.payout = p.address",
                [account],
                |_| Ok(()),
            )
            .optional()?
            .is_some())
    }

    /// Proofs still waiting for their payment: account, wallet, amount, and
    /// the epoch they were asked for in.
    pub fn waiting_proofs(&self) -> Result<Vec<(i64, String, u64, u64)>> {
        let mut query = self.db.prepare("SELECT account, address, amount, epoch FROM wallet_proofs WHERE proven IS NULL")?;
        let rows = query.query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get::<_, i64>(2)? as u64, row.get::<_, i64>(3)? as u64)))?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    pub fn mark_proven(&self, account: i64) -> Result<()> {
        self.db.execute("UPDATE wallet_proofs SET proven = ?2 WHERE account = ?1", params![account, now() as i64])?;
        Ok(())
    }

    /// The names and pictures of verified wallets among `addresses`, by
    /// address. When several accounts verified one wallet, the latest wins.
    pub fn wallet_profiles(&self, addresses: &[String]) -> Result<Vec<(String, WalletProfile)>> {
        let mut found: Vec<(String, WalletProfile)> = Vec::new();
        let mut query = self.db.prepare(
            "SELECT a.name, a.picture FROM wallet_proofs p JOIN accounts a ON a.id = p.account
             WHERE p.address = ?1 AND p.proven IS NOT NULL AND a.payout = p.address
               AND NOT EXISTS (SELECT 1 FROM suspended s WHERE s.account = a.id)
             ORDER BY p.proven DESC LIMIT 1",
        )?;
        for address in addresses {
            let Ok(address) = orien_address(address) else { continue };
            if found.iter().any(|(a, _)| *a == address) {
                continue;
            }
            if let Some(profile) =
                query.query_row([&address], |row| Ok(WalletProfile { name: row.get(0)?, picture: row.get(1)? })).optional()?
            {
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
        let paid = received
            .iter()
            .any(|p| p.amount == amount && p.epoch >= epoch && hex::encode(p.from.as_bytes()) == address);
        if paid {
            app.store().mark_proven(account)?;
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
