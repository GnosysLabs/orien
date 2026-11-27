//! The Bitcoin side of a trustless swap between ORN and Bitcoin.
pub mod book;
pub mod chain;
pub mod htlc;
pub mod order;
pub mod sol;
pub mod solana;
pub mod swap;
#[cfg(feature = "testing")]
pub mod testing;

/// The Bitcoin library swaps are built on, for programs that use them.
pub use bitcoin;
