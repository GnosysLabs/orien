//! Hashing. Every other construction in the system is built from the two
//! functions here, so the whole design rests on BLAKE3 and nothing else.

use borsh::{BorshDeserialize, BorshSerialize};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;
use std::str::FromStr;

/// A 32-byte hash: account ids, key commitments, Merkle nodes, digests.
#[derive(
    Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default, BorshSerialize, BorshDeserialize,
)]
pub struct Hash32(pub [u8; 32]);

impl Hash32 {
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    /// First eight hex characters, for logs and tables.
    pub fn short(&self) -> String {
        hex::encode(&self.0[..4])
    }
}

impl fmt::Display for Hash32 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&hex::encode(self.0))
    }
}

impl fmt::Debug for Hash32 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseHashError;

impl fmt::Display for ParseHashError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("expected 64 hexadecimal characters")
    }
}

impl std::error::Error for ParseHashError {}

impl FromStr for Hash32 {
    type Err = ParseHashError;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let mut bytes = [0u8; 32];
        hex::decode_to_slice(text.trim(), &mut bytes).map_err(|_| ParseHashError)?;
        Ok(Hash32(bytes))
    }
}

impl Serialize for Hash32 {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for Hash32 {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = <String as Deserialize>::deserialize(deserializer)?;
        text.parse().map_err(serde::de::Error::custom)
    }
}

/// Domain tags for public hashing. Each use of the hash function gets its own
/// tag, so a value computed for one purpose can never be passed off as another.
#[derive(Clone, Copy)]
#[repr(u8)]
pub enum Tag {
    OtsChain = 0x01,
    OtsCommitment = 0x02,
    OtsMessage = 0x03,
    KeyTreeNode = 0x10,
    BatchLeaf = 0x20,
    BatchNode = 0x21,
    BatchPadding = 0x22,
    Order = 0x30,
    BatchMessage = 0x31,
    Genesis = 0x32,
    Account = 0x33,
    Confirmation = 0x34,
    Address = 0x35,
    StateLeaf = 0x40,
    StateNode = 0x41,
    StatePadding = 0x42,
}

/// Hashes `parts` under `tag`. Parts are concatenated without separators, so
/// callers pass fixed-size parts, with at most one variable-size part last.
pub fn hash(tag: Tag, parts: &[&[u8]]) -> Hash32 {
    let mut hasher = blake3::Hasher::new();
    hasher.update(&[tag as u8]);
    for part in parts {
        hasher.update(part);
    }
    Hash32(*hasher.finalize().as_bytes())
}

/// Purposes for secret derivation.
#[derive(Clone, Copy)]
#[repr(u8)]
pub enum Kdf {
    OtsPubSeed = 0x01,
    OtsChain = 0x02,
    WalletKey = 0x03,
    TreeLeaf = 0x04,
    WalletSpare = 0x05,
}

/// Derives a secret from `key`. Keyed BLAKE3 is a pseudorandom function:
/// outputs reveal nothing about the key or about each other.
pub fn prf(key: &[u8; 32], purpose: Kdf, index: u64) -> [u8; 32] {
    let mut input = [0u8; 9];
    input[0] = purpose as u8;
    input[1..].copy_from_slice(&index.to_le_bytes());
    *blake3::keyed_hash(key, &input).as_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_round_trip() {
        let value = hash(Tag::Order, &[b"hello"]);
        assert_eq!(value.to_string().parse::<Hash32>().unwrap(), value);
        assert!("abc".parse::<Hash32>().is_err());
    }

    #[test]
    fn tags_separate_domains() {
        assert_ne!(hash(Tag::BatchLeaf, &[b"x"]), hash(Tag::BatchNode, &[b"x"]));
    }

    #[test]
    fn prf_outputs_are_independent() {
        let key = [7u8; 32];
        assert_ne!(prf(&key, Kdf::WalletKey, 0), prf(&key, Kdf::WalletKey, 1));
        assert_ne!(prf(&key, Kdf::WalletKey, 0), prf(&key, Kdf::TreeLeaf, 0));
    }
}
