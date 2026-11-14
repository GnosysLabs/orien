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

pub use genesis::{Allocation, Committee, Fees, Genesis, Member, ValidatorInfo, VerifyError};
pub use state::{
    Accepted, Account, Applied, CancelError, Checked, FEES, FORFEITED, Proofs, Reject, Signer, State, account_root,
    authorized,
};
pub use types::{
    AccountId, Certificate, Confirmation, Kind, Order, Receipt, COIN, SPARE_HEIGHT, Signature, SignedBy,
    SignedOrder, STAKE_PROOF_LEAF, Validator, Vote, address, coins, confirmation_item, parse_coins, sign_stake_proof, stake_proof_item,
};
