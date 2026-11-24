//! Wallets' own encrypted stores, kept here for wallets to find again from
//! any device with the same recovery phrase. The marketplace holds only
//! sealed bytes it cannot read, under an id that says nothing about the
//! wallet, and replaces them only for the holder of the vault's token,
//! whose hash it keeps from the first write. See `orien_net::vault`.

use crate::store::{Store, now};
use anyhow::{Result, bail};
use orien_net::vault::{MAX_BLOB, MAX_PICTURE, token_hash};
use rusqlite::{OptionalExtension, params};

/// How many sealed pictures a vault may keep beside it: pictures for
/// themes the person built, synced between their devices. Each weighs at
/// most `orien_net::vault::MAX_PICTURE`.
pub const MAX_PICTURES: i64 = 24;

/// A picture's name: lower-case letters, digits and dashes, up to 40.
fn picture_id(text: &str) -> bool {
    (1..=40).contains(&text.len()) && text.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

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
             );
             CREATE TABLE IF NOT EXISTS vault_pictures (
                 vault TEXT NOT NULL,
                 id TEXT NOT NULL,
                 blob BLOB NOT NULL,
                 updated INTEGER NOT NULL,
                 PRIMARY KEY (vault, id)
             );",
        )?;
        Ok(())
    }

    /// Whether `token` is the vault `id`'s; with `start`, a vault not yet
    /// written is started with it, empty, so its pictures can come first.
    fn vault_token(&self, id: &str, token: &str, start: bool) -> Result<()> {
        if !hex64(id) || !hex64(token) {
            bail!("that is not a vault");
        }
        let hash = token_hash(token);
        let kept: Option<String> =
            self.db.query_row("SELECT token_hash FROM vaults WHERE id = ?1", [id], |row| row.get(0)).optional()?;
        match kept {
            Some(kept) if kept != hash => bail!("that is not this vault's token"),
            Some(_) => Ok(()),
            None if start => {
                self.db.execute(
                    "INSERT INTO vaults (id, token_hash, blob, updated) VALUES (?1, ?2, x'', ?3)",
                    params![id, hash, now()],
                )?;
                Ok(())
            }
            None => bail!("that is not this vault's token"),
        }
    }

    /// Keeps the sealed picture `picture` beside the vault `id`, replacing
    /// one of the same name, for the holder of the vault's token.
    pub fn put_vault_picture(&self, id: &str, token: &str, picture: &str, blob: &[u8]) -> Result<()> {
        if !picture_id(picture) {
            bail!("a picture's name is 1 to 40 lower-case letters, digits and dashes");
        }
        if blob.len() > MAX_PICTURE {
            bail!("a picture holds at most {} MB", MAX_PICTURE / 1024 / 1024);
        }
        self.vault_token(id, token, true)?;
        let others: i64 = self.db.query_row(
            "SELECT COUNT(*) FROM vault_pictures WHERE vault = ?1 AND id <> ?2",
            params![id, picture],
            |row| row.get(0),
        )?;
        if others >= MAX_PICTURES {
            bail!("a vault keeps at most {MAX_PICTURES} pictures; delete one first");
        }
        self.db.execute(
            "INSERT INTO vault_pictures (vault, id, blob, updated) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT (vault, id) DO UPDATE SET blob = excluded.blob, updated = excluded.updated",
            params![id, picture, blob, now()],
        )?;
        Ok(())
    }

    /// The sealed picture `picture` beside the vault `id`, for the holder of
    /// its token.
    pub fn vault_picture(&self, id: &str, token: &str, picture: &str) -> Result<Option<Vec<u8>>> {
        if !picture_id(picture) {
            bail!("that is not a picture's name");
        }
        self.vault_token(id, token, false)?;
        Ok(self
            .db
            .query_row("SELECT blob FROM vault_pictures WHERE vault = ?1 AND id = ?2", params![id, picture], |row| row.get(0))
            .optional()?)
    }

    /// Deletes the picture `picture` beside the vault `id`, for the holder of
    /// its token. Gives whether there was one.
    pub fn delete_vault_picture(&self, id: &str, token: &str, picture: &str) -> Result<bool> {
        if !picture_id(picture) {
            bail!("that is not a picture's name");
        }
        self.vault_token(id, token, false)?;
        Ok(self.db.execute("DELETE FROM vault_pictures WHERE vault = ?1 AND id = ?2", params![id, picture])? == 1)
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

    #[test]
    fn a_vaults_pictures_are_its_tokens_holders_alone() {
        let store = Store::in_memory().unwrap();
        let mine = VaultKeys::from_seed(&[1; 32]);
        let other = VaultKeys::from_seed(&[2; 32]);
        // Pictures may come before the vault itself: it starts empty.
        store.put_vault_picture(&mine.id, &mine.token, "sunset-1", b"sealed one").unwrap();
        assert_eq!(store.vault(&mine.id).unwrap().unwrap().0, b"");
        store.put_vault_picture(&mine.id, &mine.token, "sunset-1", b"sealed two").unwrap();
        assert_eq!(store.vault_picture(&mine.id, &mine.token, "sunset-1").unwrap().unwrap(), b"sealed two");
        assert_eq!(store.vault_picture(&mine.id, &mine.token, "none").unwrap(), None);
        assert!(store.vault_picture(&mine.id, &other.token, "sunset-1").is_err(), "nobody else reads it");
        assert!(store.put_vault_picture(&mine.id, &other.token, "sunset-1", b"x").is_err(), "nor writes it");
        assert!(store.delete_vault_picture(&mine.id, &other.token, "sunset-1").is_err(), "nor deletes it");
        assert!(store.vault_picture(&other.id, &other.token, "sunset-1").is_err(), "a vault never written");
        assert!(store.put_vault_picture(&mine.id, &mine.token, "Bad/Name", b"x").is_err());
        assert!(store.put_vault_picture(&mine.id, &mine.token, "big", &vec![0; MAX_PICTURE + 1]).is_err());
        for i in 1..MAX_PICTURES {
            store.put_vault_picture(&mine.id, &mine.token, &format!("p{i}"), b"x").unwrap();
        }
        assert!(store.put_vault_picture(&mine.id, &mine.token, "one-too-many", b"x").is_err());
        store.put_vault_picture(&mine.id, &mine.token, "p1", b"replaced").unwrap();
        assert!(store.delete_vault_picture(&mine.id, &mine.token, "p1").unwrap());
        assert!(!store.delete_vault_picture(&mine.id, &mine.token, "p1").unwrap());
        store.put_vault_picture(&mine.id, &mine.token, "one-more", b"x").unwrap();
        // The vault itself is still written by its token's holder.
        store.put_vault(&mine.id, &mine.token, b"contacts").unwrap();
        assert!(store.put_vault(&mine.id, &other.token, b"taken over").is_err());
    }
}
