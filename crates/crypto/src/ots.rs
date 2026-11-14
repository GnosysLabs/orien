//! One-time signatures (Winternitz, base 16), built from BLAKE3 only.
//!
//! A key signs exactly one message. Signing a second, different message with
//! the same key hands an attacker the material to forge more, so `sign`
//! consumes the key and the ledger rotates every account's key with every
//! payment.
//!
//! How it works: the secret is 51 random values. Hashing each one 15 times
//! gives 51 chain ends, and the public commitment is a hash of those ends. To
//! sign, the message digest is written as 48 base-16 digits plus a 3-digit
//! checksum, and the signer reveals, for each digit `d`, the value `d` steps
//! along that chain. A verifier walks each revealed value the remaining
//! `15 - d` steps and checks that the ends hash to the commitment. Hashes only
//! go forward, so a forger can only raise digits, and raising any message
//! digit lowers the checksum, which would need a step backward.
//!
//! The shape (24-byte values, 51 chains of 15 steps) is the same as the
//! one-time layer inside SLH-DSA-192 (FIPS 205).

use crate::hash::{Hash32, Kdf, Tag, hash, prf};
use borsh::{BorshDeserialize, BorshSerialize};
use std::fmt;

/// Size in bytes of every chain value.
pub const N: usize = 24;
/// Steps from a chain's secret start to its public end.
const TOP: u8 = 15;
const MESSAGE_DIGITS: usize = 2 * N;
const CHECKSUM_DIGITS: usize = 3;
pub const CHAINS: usize = MESSAGE_DIGITS + CHECKSUM_DIGITS;
pub const SIGNATURE_BYTES: usize = N * (CHAINS + 1);

pub type Value = [u8; N];

#[derive(Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct Signature {
    /// Per-key salt mixed into every hash. It stays secret until the key
    /// signs, so nobody can prepare colliding messages in advance.
    pub pub_seed: Value,
    pub chains: [Value; CHAINS],
}

impl fmt::Debug for Signature {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ots::Signature({}..)", hex::encode(&self.pub_seed[..4]))
    }
}

/// A one-time signing key. Everything is derived from a 32-byte seed.
pub struct SecretKey {
    seed: [u8; 32],
}

impl SecretKey {
    pub fn from_seed(seed: [u8; 32]) -> Self {
        SecretKey { seed }
    }

    fn pub_seed(&self) -> Value {
        truncate(&prf(&self.seed, Kdf::OtsPubSeed, 0))
    }

    fn chain_start(&self, chain: usize) -> Value {
        truncate(&prf(&self.seed, Kdf::OtsChain, chain as u64))
    }

    /// The 32-byte public commitment to this key. Safe to publish.
    pub fn commitment(&self) -> Hash32 {
        let pub_seed = self.pub_seed();
        let mut ends = [[0u8; N]; CHAINS];
        for (chain, end) in ends.iter_mut().enumerate() {
            *end = walk(&pub_seed, chain as u8, self.chain_start(chain), 0, TOP);
        }
        commit(&pub_seed, &ends)
    }

    /// Signs `message`, consuming the key so it cannot sign anything else.
    pub fn sign(self, message: &[u8]) -> Signature {
        let pub_seed = self.pub_seed();
        let digits = digits(&pub_seed, message);
        let mut chains = [[0u8; N]; CHAINS];
        for (chain, value) in chains.iter_mut().enumerate() {
            *value = walk(&pub_seed, chain as u8, self.chain_start(chain), 0, digits[chain]);
        }
        Signature { pub_seed, chains }
    }
}

/// Returns the commitment under which `signature` is a valid signature of
/// `message`. The caller compares it with the commitment it expects.
pub fn recover(signature: &Signature, message: &[u8]) -> Hash32 {
    let digits = digits(&signature.pub_seed, message);
    let mut ends = [[0u8; N]; CHAINS];
    for (chain, end) in ends.iter_mut().enumerate() {
        *end = walk(
            &signature.pub_seed,
            chain as u8,
            signature.chains[chain],
            digits[chain],
            TOP,
        );
    }
    commit(&signature.pub_seed, &ends)
}

fn truncate(wide: &[u8; 32]) -> Value {
    let mut value = [0u8; N];
    value.copy_from_slice(&wide[..N]);
    value
}

/// One step along a chain. The salt, chain number and position are mixed in
/// so that no two hash calls in the whole system share an input, which keeps
/// an attack on one key from helping against any other.
fn step(pub_seed: &Value, chain: u8, position: u8, value: &Value) -> Value {
    let mut input = [0u8; 1 + N + 2 + N];
    input[0] = Tag::OtsChain as u8;
    input[1..1 + N].copy_from_slice(pub_seed);
    input[1 + N] = chain;
    input[2 + N] = position;
    input[3 + N..].copy_from_slice(value);
    truncate(blake3::hash(&input).as_bytes())
}

fn walk(pub_seed: &Value, chain: u8, mut value: Value, from: u8, to: u8) -> Value {
    for position in from..to {
        value = step(pub_seed, chain, position, &value);
    }
    value
}

/// Message digest as base-16 digits, followed by the checksum digits.
fn digits(pub_seed: &Value, message: &[u8]) -> [u8; CHAINS] {
    let digest = hash(Tag::OtsMessage, &[pub_seed, message]);
    let mut out = [0u8; CHAINS];
    for (i, byte) in digest.0[..N].iter().enumerate() {
        out[2 * i] = byte >> 4;
        out[2 * i + 1] = byte & 0x0f;
    }
    // At most 48 * 15 = 720, which fits in three base-16 digits.
    let checksum: u32 = out[..MESSAGE_DIGITS].iter().map(|d| (TOP - d) as u32).sum();
    out[MESSAGE_DIGITS] = (checksum >> 8) as u8 & 0x0f;
    out[MESSAGE_DIGITS + 1] = (checksum >> 4) as u8 & 0x0f;
    out[MESSAGE_DIGITS + 2] = checksum as u8 & 0x0f;
    out
}

fn commit(pub_seed: &Value, ends: &[Value; CHAINS]) -> Hash32 {
    let mut hasher = blake3::Hasher::new();
    hasher.update(&[Tag::OtsCommitment as u8]);
    hasher.update(pub_seed);
    for end in ends {
        hasher.update(end);
    }
    Hash32(*hasher.finalize().as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(n: u8) -> SecretKey {
        SecretKey::from_seed([n; 32])
    }

    #[test]
    fn signature_recovers_the_commitment() {
        let commitment = key(1).commitment();
        let signature = key(1).sign(b"pay bob 5");
        assert_eq!(recover(&signature, b"pay bob 5"), commitment);
    }

    #[test]
    fn a_different_message_does_not_verify() {
        let commitment = key(1).commitment();
        let signature = key(1).sign(b"pay bob 5");
        assert_ne!(recover(&signature, b"pay bob 50"), commitment);
    }

    #[test]
    fn a_different_key_does_not_verify() {
        let signature = key(2).sign(b"pay bob 5");
        assert_ne!(recover(&signature, b"pay bob 5"), key(1).commitment());
    }

    #[test]
    fn any_tampering_breaks_the_signature() {
        let commitment = key(3).commitment();
        let signature = key(3).sign(b"message");
        for chain in 0..CHAINS {
            let mut forged = signature.clone();
            forged.chains[chain][0] ^= 1;
            assert_ne!(recover(&forged, b"message"), commitment);
        }
        let mut forged = signature.clone();
        forged.pub_seed[0] ^= 1;
        assert_ne!(recover(&forged, b"message"), commitment);
    }

    #[test]
    fn checksum_counts_the_remaining_steps() {
        let seed = [9u8; N];
        for i in 0..200u32 {
            let d = digits(&seed, &i.to_le_bytes());
            let expected: u32 = d[..MESSAGE_DIGITS].iter().map(|x| 15 - *x as u32).sum();
            let encoded = ((d[48] as u32) << 8) | ((d[49] as u32) << 4) | d[50] as u32;
            assert_eq!(encoded, expected);
            assert!(d.iter().all(|x| *x <= 15));
        }
    }

    /// The property that makes forgery impossible: a second message always
    /// needs at least one chain walked backward from what the first revealed.
    #[test]
    fn every_other_message_needs_a_backward_step() {
        let seed = [5u8; N];
        let signed = digits(&seed, b"the signed message");
        for i in 0..2000u32 {
            let other = digits(&seed, &i.to_le_bytes());
            assert!((0..CHAINS).any(|c| other[c] < signed[c]));
        }
    }

    #[test]
    fn signature_is_1248_bytes() {
        let signature = key(4).sign(b"size");
        assert_eq!(SIGNATURE_BYTES, 1248);
        assert_eq!(borsh::to_vec(&signature).unwrap().len(), SIGNATURE_BYTES);
    }
}
