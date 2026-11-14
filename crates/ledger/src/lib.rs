//! The rules of the ledger, with no networking or storage.
//!
//! There are no blocks and no global order. Each account numbers its own
//! payments; a payment is final once validators holding more than two thirds
//! of the stake have voted for it. Two different payments with the same
//! number can never both reach that threshold, because any two such groups
//! of validators share an honest member, and an honest validator votes for
//! only one payment per account and number.

pub mod agree;
pub mod close;
pub mod genesis;
pub mod state;
pub mod types;

pub use genesis::{Allocation, Committee, Genesis, Member, ValidatorInfo, VerifyError};
pub use state::{
    Accepted, Account, Applied, CancelError, Checked, Proofs, Reject, Signer, State, account_root,
    authorized,
};
pub use types::{
    AccountId, Certificate, Confirmation, Kind, Order, Receipt, SPARE_HEIGHT, Signature, SignedBy,
    SignedOrder, Validator, Vote, address, confirmation_item, stake_proof_item,
};
