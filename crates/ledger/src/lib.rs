//! The rules of the ledger, with no networking or storage.
//!
//! There are no blocks and no global order. Each account numbers its own
//! payments; a payment is final once validators holding more than two thirds
//! of the stake have voted for it. Two different payments with the same
//! number can never both reach that threshold, because any two such groups
//! of validators share an honest member, and an honest validator votes for
//! only one payment per account and number.

pub mod genesis;
pub mod state;
pub mod types;

pub use genesis::{Allocation, Committee, Genesis, ValidatorInfo, VerifyError};
pub use state::{Accepted, Account, Applied, Reject, State, account_root};
pub use types::{AccountId, Certificate, Order, SignedOrder, Vote};
