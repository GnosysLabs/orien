//! One validator's view of the ledger, and the two rules it enforces:
//! when to vote for an order, and how a certified order changes balances.

use crate::genesis::Genesis;
use crate::types::{AccountId, Order};
use borsh::{BorshDeserialize, BorshSerialize};
use std::collections::HashMap;
use std::fmt;
use token_crypto::Hash32;

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct Account {
    /// Signed, because certificates can arrive out of order: a validator may
    /// learn of a spend before the payment that funded it. Voting still
    /// requires a non-negative result, so a dip below zero is only ever a
    /// validator catching up, never money that does not exist.
    pub balance: i128,
    /// Number of payments this account has made; the number its next one must carry.
    pub seq: u64,
    /// Commitment to the one-time key that must sign the next payment.
    pub key: Hash32,
}

impl Account {
    fn empty(id: AccountId) -> Account {
        Account { balance: 0, seq: 0, key: id }
    }
}

/// Why a validator refuses to vote for an order.
#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub enum Reject {
    UnknownAccount,
    ZeroAmount,
    /// The account has already moved past this payment number.
    StaleSeq { current: u64 },
    /// This validator has not yet seen the account's earlier payments.
    FutureSeq { current: u64 },
    BadSignature,
    InsufficientFunds { balance: i128, needed: u64 },
    /// The validator already voted for a different order with this number.
    Conflict { voted: Hash32 },
}

impl fmt::Display for Reject {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Reject::UnknownAccount => f.write_str("account has never held funds here"),
            Reject::ZeroAmount => f.write_str("amount is zero"),
            Reject::StaleSeq { current } => {
                write!(f, "that payment number is already final (account is at #{current})")
            }
            Reject::FutureSeq { current } => {
                write!(f, "earlier payments are missing here (account is at #{current})")
            }
            Reject::BadSignature => f.write_str("signature does not match the account's current key"),
            Reject::InsufficientFunds { balance, needed } => {
                write!(f, "insufficient funds: balance {balance}, needs {needed}")
            }
            Reject::Conflict { voted } => write!(
                f,
                "already voted for a different payment with this number ({})",
                voted.short()
            ),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Accepted {
    /// First time this order is seen; the vote is now locked in.
    Fresh,
    /// The same order was accepted before.
    Repeat,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Applied {
    New,
    Already,
}

pub struct State {
    accounts: HashMap<AccountId, Account>,
    /// The order this validator voted for, per account and payment number.
    /// Entries exist only for payments that are not final yet.
    locks: HashMap<(AccountId, u64), Hash32>,
}

impl State {
    pub fn from_genesis(genesis: &Genesis) -> State {
        let accounts = genesis
            .allocations
            .iter()
            .map(|a| (a.account, Account { balance: a.amount as i128, seq: 0, key: a.account }))
            .collect();
        State { accounts, locks: HashMap::new() }
    }

    pub fn account(&self, id: &AccountId) -> Option<&Account> {
        self.accounts.get(id)
    }

    pub fn accounts(&self) -> usize {
        self.accounts.len()
    }

    /// The payment number `id` must use next, as far as this validator knows.
    pub fn seq(&self, id: &AccountId) -> u64 {
        self.accounts.get(id).map_or(0, |a| a.seq)
    }

    pub fn lock(&self, id: &AccountId, seq: u64) -> Option<&Hash32> {
        self.locks.get(&(*id, seq))
    }

    /// Sum of all balances. Applying a certificate moves money and never
    /// creates it, so this always equals the genesis supply.
    pub fn total(&self) -> i128 {
        self.accounts.values().map(|a| a.balance).sum()
    }

    /// Re-establishes a vote recorded before a restart.
    pub fn restore_lock(&mut self, from: AccountId, seq: u64, digest: Hash32) {
        if seq >= self.seq(&from) {
            self.locks.insert((from, seq), digest);
        }
    }

    /// Decides whether to vote for `order`.
    ///
    /// `digest` is the order's digest and `signed_by` is the key commitment
    /// recovered from its signature. On `Fresh`, the vote is locked: no other
    /// order with this account and number will be accepted. The caller must
    /// make that lock durable before releasing its vote.
    pub fn check_order(
        &mut self,
        order: &Order,
        digest: Hash32,
        signed_by: &Hash32,
    ) -> Result<Accepted, Reject> {
        if order.amount == 0 {
            return Err(Reject::ZeroAmount);
        }
        let account = self.accounts.get(&order.from).ok_or(Reject::UnknownAccount)?;
        if order.seq < account.seq {
            return Err(Reject::StaleSeq { current: account.seq });
        }
        if order.seq > account.seq {
            return Err(Reject::FutureSeq { current: account.seq });
        }
        if *signed_by != account.key {
            return Err(Reject::BadSignature);
        }
        if let Some(voted) = self.locks.get(&(order.from, order.seq)) {
            return if *voted == digest {
                Ok(Accepted::Repeat)
            } else {
                Err(Reject::Conflict { voted: *voted })
            };
        }
        if account.balance < order.amount as i128 {
            return Err(Reject::InsufficientFunds { balance: account.balance, needed: order.amount });
        }
        self.locks.insert((order.from, order.seq), digest);
        Ok(Accepted::Fresh)
    }

    /// Applies an order that a valid certificate proves final. The caller
    /// has already verified the certificate.
    ///
    /// Balances are not checked here: the validators who voted checked them,
    /// and this validator may simply not have seen the funding payment yet.
    pub fn apply(&mut self, order: &Order) -> Result<Applied, Reject> {
        let current = self.seq(&order.from);
        if order.seq < current {
            return Ok(Applied::Already);
        }
        if order.seq > current {
            return Err(Reject::FutureSeq { current });
        }
        let sender = self.accounts.entry(order.from).or_insert_with(|| Account::empty(order.from));
        sender.balance -= order.amount as i128;
        sender.seq += 1;
        sender.key = order.next_key;
        let recipient = self.accounts.entry(order.to).or_insert_with(|| Account::empty(order.to));
        recipient.balance += order.amount as i128;
        self.locks.remove(&(order.from, order.seq));
        Ok(Applied::New)
    }
}
