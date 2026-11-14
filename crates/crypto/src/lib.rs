//! Cryptographic core of the ledger.
//!
//! Signatures, key trees and batch proofs are all built from one primitive,
//! the BLAKE3 hash function. Nothing here relies on elliptic curves, lattices
//! or any other structured assumption a quantum computer might break.

pub mod hash;
pub mod keytree;
pub mod merkle;
pub mod ots;

pub use hash::{Hash32, Kdf, Tag, hash, prf};
