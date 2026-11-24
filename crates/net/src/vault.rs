//! A wallet's own small encrypted store on the Orien marketplace server,
//! for what should follow it from device to device without an account: its
//! contacts.
//!
//! Everything comes from the wallet's seed, which its recovery phrase
//! restores on any device:
//! - an `id`, where the store is kept, which says nothing about the wallet
//!   and cannot be linked to its address;
//! - a key that seals what is stored (XChaCha20-Poly1305), so the server
//!   holds only bytes it cannot read;
//! - a `token` that lets its holder replace what is stored. The server keeps
//!   only the token's hash, from the first write on.
//!
//! The server is `https://orien.network/api/market/vault/{id}`: GET returns
//! `{ "blob": base64 }`, PUT takes `{ "token": hex, "blob": base64 }`.

use anyhow::{Result, anyhow};
use chacha20poly1305::aead::{Aead, KeyInit};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};

/// The most a vault may hold, sealed.
pub const MAX_BLOB: usize = 64 * 1024;

/// The most one picture kept beside a vault may be, sealed: a theme's
/// background picture, at most 2560 across.
pub const MAX_PICTURE: usize = 2 * 1024 * 1024;

/// Saved by the wallet app, privately, so a wallet with a password can keep
/// its contacts in step without asking for the password each time.
#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct VaultKeys {
    /// Where the store is kept: 64 hex digits.
    pub id: String,
    /// What lets its holder replace it: 64 hex digits.
    pub token: String,
    key: [u8; 32],
}

impl VaultKeys {
    /// The vault keys of the wallet with this seed.
    pub fn from_seed(seed: &[u8; 32]) -> VaultKeys {
        VaultKeys {
            id: hex::encode(blake3::derive_key("orien wallet vault 2026-11-15 id", seed)),
            token: hex::encode(blake3::derive_key("orien wallet vault 2026-11-15 token", seed)),
            key: blake3::derive_key("orien wallet vault 2026-11-15 key", seed),
        }
    }

    /// `plain`, sealed with a fresh nonce in front.
    pub fn seal(&self, plain: &[u8]) -> Result<Vec<u8>> {
        self.seal_at_most(plain, MAX_BLOB)
    }

    /// A picture kept beside the vault, sealed the same way.
    pub fn seal_picture(&self, plain: &[u8]) -> Result<Vec<u8>> {
        self.seal_at_most(plain, MAX_PICTURE)
    }

    fn seal_at_most(&self, plain: &[u8], most: usize) -> Result<Vec<u8>> {
        let nonce: [u8; 24] = crate::keys::random_seed()[..24].try_into().expect("24 of 32 bytes");
        let sealed = XChaCha20Poly1305::new(&self.key.into())
            .encrypt(XNonce::from_slice(&nonce), plain)
            .map_err(|_| anyhow!("could not seal the vault"))?;
        let mut out = nonce.to_vec();
        out.extend(sealed);
        if out.len() > most {
            return Err(anyhow!("too much to keep in the vault"));
        }
        Ok(out)
    }

    /// What `sealed` holds, if this wallet sealed it.
    pub fn open(&self, sealed: &[u8]) -> Result<Vec<u8>> {
        if sealed.len() < 24 {
            return Err(anyhow!("the vault holds something damaged"));
        }
        let (nonce, body) = sealed.split_at(24);
        XChaCha20Poly1305::new(&self.key.into())
            .decrypt(XNonce::from_slice(nonce), body)
            .map_err(|_| anyhow!("the vault holds something this wallet did not seal"))
    }
}

/// The hash the server keeps of a vault's token.
pub fn token_hash(token: &str) -> String {
    hex::encode(blake3::derive_key("orien wallet vault 2026-11-15 token hash", token.as_bytes()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_same_seed_opens_what_it_sealed() {
        let mine = VaultKeys::from_seed(&[1; 32]);
        let again = VaultKeys::from_seed(&[1; 32]);
        let other = VaultKeys::from_seed(&[2; 32]);
        assert_eq!((mine.id.len(), mine.token.len()), (64, 64));
        assert_eq!(mine.id, again.id, "a restored wallet finds its vault");
        assert_ne!(mine.id, other.id);
        assert_ne!(mine.id, mine.token);
        let sealed = mine.seal(b"[{\"name\":\"Bob\"}]").unwrap();
        assert_eq!(again.open(&sealed).unwrap(), b"[{\"name\":\"Bob\"}]");
        assert!(other.open(&sealed).is_err(), "another wallet cannot read it");
        assert_ne!(token_hash(&mine.token), mine.token);
    }
}
