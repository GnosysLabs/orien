//! The wallets each account's Orien Wallet apps hold, by name, so the
//! website can offer "Sell from" with the names people know them by. Each
//! device reports its own list, which replaces only its own rows, so two
//! devices holding different wallets don't wipe each other.

use crate::store::{Store, now, orien_address};
use anyhow::{Result, bail};
use rusqlite::params;
use serde::Serialize;

/// Wallets one device may report.
pub const MAX_WALLETS: usize = 50;
/// The longest wallet name kept.
const MAX_NAME: usize = 40;

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct NamedWallet {
    pub address: String,
    pub name: String,
}

impl Store {
    pub(crate) fn create_account_wallet_table(&self) -> Result<()> {
        self.db.execute_batch(
            "CREATE TABLE IF NOT EXISTS account_wallets (
                 account INTEGER NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
                 device TEXT NOT NULL,
                 address TEXT NOT NULL,
                 name TEXT NOT NULL,
                 updated INTEGER NOT NULL,
                 PRIMARY KEY (account, device, address)
             );",
        )?;
        Ok(())
    }

    /// The wallets `device` holds for `account`, replacing what it said before.
    pub fn report_wallets(&self, account: i64, device: &str, wallets: &[(String, String)]) -> Result<usize> {
        let device = device.trim();
        if device.is_empty() || device.len() > 100 {
            bail!("name this device with 1 to 100 characters");
        }
        if wallets.len() > MAX_WALLETS {
            bail!("at most {MAX_WALLETS} wallets");
        }
        let mut tidy: Vec<(String, String)> = Vec::new();
        for (address, name) in wallets {
            let address = orien_address(address)?;
            let name: String = name.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(MAX_NAME).collect();
            let name = if name.is_empty() { "Wallet".to_string() } else { name };
            if !tidy.iter().any(|(a, _)| *a == address) {
                tidy.push((address, name));
            }
        }
        let tx = self.db.unchecked_transaction()?;
        tx.execute("DELETE FROM account_wallets WHERE account = ?1 AND device = ?2", params![account, device])?;
        for (address, name) in &tidy {
            tx.execute(
                "INSERT INTO account_wallets (account, device, address, name, updated) VALUES (?1, ?2, ?3, ?4, ?5)",
                params![account, device, address, name, now() as i64],
            )?;
        }
        tx.commit()?;
        Ok(tidy.len())
    }

    /// How many wallets the account's apps have reported.
    pub fn reported_wallets(&self, account: i64) -> Result<i64> {
        Ok(self.db.query_row("SELECT COUNT(DISTINCT address) FROM account_wallets WHERE account = ?1", [account], |row| row.get(0))?)
    }

    /// Every wallet the account's apps hold, each once with its newest name,
    /// the linked wallet first, then by name.
    pub fn account_wallets(&self, account: i64) -> Result<Vec<NamedWallet>> {
        let mut statement = self.db.prepare(
            "SELECT address, name FROM account_wallets w WHERE account = ?1
             AND updated = (SELECT MAX(updated) FROM account_wallets v WHERE v.account = w.account AND v.address = w.address)
             GROUP BY address ORDER BY name COLLATE NOCASE, address",
        )?;
        let mut wallets: Vec<NamedWallet> = statement
            .query_map([account], |row| Ok(NamedWallet { address: row.get(0)?, name: row.get(1)? }))?
            .collect::<rusqlite::Result<_>>()?;
        let linked: Option<String> = self.db.query_row("SELECT payout FROM accounts WHERE id = ?1", [account], |row| row.get(0))?;
        if let Some(linked) = linked {
            match wallets.iter().position(|w| w.address == linked) {
                Some(at) => {
                    let main = wallets.remove(at);
                    wallets.insert(0, main);
                }
                None => wallets.insert(0, NamedWallet { address: linked, name: "Your linked wallet".into() }),
            }
        }
        Ok(wallets)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
    const C: &str = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";

    #[test]
    fn each_device_reports_its_own_wallets_and_the_linked_one_comes_first() {
        let store = Store::in_memory().unwrap();
        let maya = store.create_account("Maya", "maya").unwrap().id;
        let w = |a: &str, n: &str| (a.to_string(), n.to_string());
        store.report_wallets(maya, "mac", &[w(A, "Savings"), w(B, "  Mining   rig ")]).unwrap();
        store.report_wallets(maya, "phone", &[w(C, "Phone"), w(A, "Savings")]).unwrap();
        let names = |s: &Store| s.account_wallets(maya).unwrap().into_iter().map(|w| w.name).collect::<Vec<_>>();
        assert_eq!(names(&store), ["Mining rig", "Phone", "Savings"]);

        store.set_wallet(maya, C).unwrap();
        assert_eq!(names(&store), ["Phone", "Mining rig", "Savings"], "the linked wallet first");

        // The Mac drops a wallet: the phone's are untouched.
        store.report_wallets(maya, "mac", &[w(A, "Savings")]).unwrap();
        assert_eq!(names(&store), ["Phone", "Savings"]);
        store.report_wallets(maya, "phone", &[]).unwrap();
        store.report_wallets(maya, "mac", &[]).unwrap();
        assert_eq!(names(&store), ["Your linked wallet"], "the linked one stays even if no app reports it");
        assert_eq!(store.reported_wallets(maya).unwrap(), 0);

        assert!(store.report_wallets(maya, "mac", &[w("123", "Bad")]).is_err());
        assert!(store.report_wallets(maya, "", &[w(A, "x")]).is_err());
    }
}
