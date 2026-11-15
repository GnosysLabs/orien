//! Wallets' own encrypted stores, kept here for wallets to find again from
//! any device with the same recovery phrase. The marketplace holds only
//! sealed bytes it cannot read, under an id that says nothing about the
//! wallet, and replaces them only for the holder of the vault's token,
//! whose hash it keeps from the first write. See `orien_net::vault`.

use crate::store::{Store, now};
use anyhow::{Result, bail};
use orien_net::vault::{MAX_BLOB, token_hash};
use rusqlite::{OptionalExtension, params};

fn hex64(text: &str) -> bool {
    text.len() == 64 && text.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
}

impl Store {
    pub(crate) fn create_vault_table(&self) -> Result<()> {
        self.db.execute_batch(
            "CREATE TABLE IF NOT EXISTS vaults (
                 id TEXT PRIMARY KEY,
                 token_hash TEXT NOT NULL,
                 blob BLOB NOT NULL,
                 updated INTEGER NOT NULL
             );",
        )?;
        Ok(())
    }

    /// What the vault `id` holds, and when it was last written.
    pub fn vault(&self, id: &str) -> Result<Option<(Vec<u8>, u64)>> {
        if !hex64(id) {
            bail!("that is not a vault");
        }
        Ok(self
            .db
            .query_row("SELECT blob, updated FROM vaults WHERE id = ?1", [id], |row| Ok((row.get(0)?, row.get(1)?)))
            .optional()?)
    }

    /// Replaces what the vault `id` holds, or starts it, for the holder of
    /// its token.
    pub fn put_vault(&self, id: &str, token: &str, blob: &[u8]) -> Result<()> {
        if !hex64(id) || !hex64(token) {
            bail!("that is not a vault");
        }
        if blob.len() > MAX_BLOB {
            bail!("a vault holds at most {} KB", MAX_BLOB / 1024);
        }
        let hash = token_hash(token);
        let kept: Option<String> =
            self.db.query_row("SELECT token_hash FROM vaults WHERE id = ?1", [id], |row| row.get(0)).optional()?;
        if kept.is_some_and(|kept| kept != hash) {
            bail!("that is not this vault's token");
        }
        self.db.execute(
            "INSERT INTO vaults (id, token_hash, blob, updated) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT (id) DO UPDATE SET blob = excluded.blob, updated = excluded.updated",
            params![id, hash, blob, now()],
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use orien_net::vault::VaultKeys;

    #[test]
    fn only_the_tokens_holder_replaces_a_vault() {
        let store = Store::in_memory().unwrap();
        let mine = VaultKeys::from_seed(&[1; 32]);
        let other = VaultKeys::from_seed(&[2; 32]);
        assert_eq!(store.vault(&mine.id).unwrap(), None);
        store.put_vault(&mine.id, &mine.token, b"first").unwrap();
        store.put_vault(&mine.id, &mine.token, b"second").unwrap();
        assert_eq!(store.vault(&mine.id).unwrap().unwrap().0, b"second");
        assert!(store.put_vault(&mine.id, &other.token, b"taken over").is_err(), "nobody else replaces it");
        assert_eq!(store.vault(&mine.id).unwrap().unwrap().0, b"second");
        assert!(store.put_vault(&mine.id, &mine.token, &vec![0; MAX_BLOB + 1]).is_err());
        assert!(store.vault("../../etc").is_err());
    }
}
