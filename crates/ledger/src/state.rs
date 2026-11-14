//! One validator's view of the ledger, and the two rules it enforces:
//! when to vote for an order, and how a certified order changes balances.

use crate::genesis::Genesis;
use crate::types::{AccountId, Order, SPARE_HEIGHT, SignedBy, address};
use borsh::{BorshDeserialize, BorshSerialize};
use orien_crypto::merkle;
use orien_crypto::{Hash32, Tag, hash};
use std::collections::{BTreeMap, HashMap};
use std::fmt;
use std::io;

/// Which key must sign an account's next order.
#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub enum Signer {
    /// The account has never signed anything. Its address commits to its
    /// first key and to its spare keys, and its first order reveals both.
    First,
    /// The one-time key with this commitment, named by the order before.
    Key(Hash32),
    /// Spare key number `n`, after a cancellation.
    Spare(u32),
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct Account {
    /// Signed, because certificates can arrive out of order: a validator may
    /// learn of a spend before the payment that funded it. Voting still
    /// requires a non-negative result, so a dip below zero is only ever a
    /// validator catching up, never money that does not exist.
    pub balance: i128,
    /// Number of orders this account has made; the number its next one must carry.
    pub seq: u64,
    pub signer: Signer,
    /// The root of the account's spare keys. Not known until its first order
    /// reveals it; all zeros until then.
    pub spare: Hash32,
    /// How many spare keys cancellations have used up.
    pub spares_used: u32,
}

impl Account {
    fn holding(balance: i128) -> Account {
        Account { balance, seq: 0, signer: Signer::First, spare: Hash32::default(), spares_used: 0 }
    }

    fn requires(&self, id: &AccountId, order: &Order, by: &SignedBy) -> bool {
        authorized(id, &self.signer, &self.spare, order, by)
    }
}

/// Whether `by` is the key that an account in this condition requires to
/// have signed `order`.
pub fn authorized(id: &AccountId, signer: &Signer, spare: &Hash32, order: &Order, by: &SignedBy) -> bool {
    match (signer, by) {
        // A first order reveals the first key and the spare root, which
        // together must make the address.
        (Signer::First, SignedBy::Key(key)) => address(key, &order.spare) == *id,
        (Signer::Key(expected), SignedBy::Key(key)) => key == expected && order.spare == *spare,
        (Signer::Spare(expected), SignedBy::Spare { root, leaf }) => {
            leaf == expected && root == spare && order.spare == *spare
        }
        _ => false,
    }
}

/// An order together with what checking its signature found.
pub struct Checked<'a> {
    pub order: &'a Order,
    pub digest: Hash32,
    pub by: SignedBy,
}

/// Why two orders do not cancel an account's current number.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CancelError {
    /// They are not two different orders, both signed with the key the
    /// account requires for its current number.
    NotEvidence,
    /// Every spare key the account had is used up. It stays locked.
    NoSpareLeft,
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
            Reject::BadSignature => f.write_str("not signed with the key the account must use next"),
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

/// What one account contributes to the state root.
fn leaf(id: &AccountId, account: &Account) -> Hash32 {
    hash(
        Tag::Account,
        &[
            id.as_bytes(),
            &account.balance.to_le_bytes(),
            &account.seq.to_le_bytes(),
            account.spare.as_bytes(),
            &account.spares_used.to_le_bytes(),
            &borsh::to_vec(&account.signer).expect("writing to memory cannot fail"),
        ],
    )
}

/// The state root under which `account` is the state of `id`, if `proof` is
/// to be believed. Compare the result with a root you already trust.
pub fn account_root(id: &AccountId, account: &Account, proof: &merkle::Path) -> Option<Hash32> {
    merkle::root_from_path_in(merkle::Kind::State, &leaf(id, account), proof)
}

/// Proofs of any account against one state root. Building this once costs
/// as much as computing the root; each proof after that is cheap.
pub struct Proofs {
    places: HashMap<AccountId, usize>,
    tree: merkle::Tree,
}

impl Proofs {
    pub fn path(&self, id: &AccountId) -> Option<merkle::Path> {
        self.places.get(id).map(|place| self.tree.path(*place))
    }
}

#[derive(Clone)]
pub struct State {
    /// In address order, so that every validator lists them alike.
    accounts: BTreeMap<AccountId, Account>,
    /// The order this validator voted for, per account and payment number.
    /// Entries exist only for payments that are not final yet.
    locks: HashMap<(AccountId, u64), Hash32>,
}

impl State {
    pub fn from_genesis(genesis: &Genesis) -> State {
        let accounts = genesis
            .allocations
            .iter()
            .map(|a| (a.account, Account::holding(a.amount as i128)))
            .collect();
        State { accounts, locks: HashMap::new() }
    }

    pub fn account(&self, id: &AccountId) -> Option<&Account> {
        self.accounts.get(id)
    }

    pub fn accounts(&self) -> usize {
        self.accounts.len()
    }

    /// Whether `by` is the key `order`'s account requires for its next
    /// order. An account never heard of here requires its first key.
    pub fn authorizes(&self, order: &Order, by: &SignedBy) -> bool {
        match self.accounts.get(&order.from) {
            Some(account) => account.requires(&order.from, order, by),
            None => Account::holding(0).requires(&order.from, order, by),
        }
    }

    /// The payment number `id` must use next, as far as this validator knows.
    pub fn seq(&self, id: &AccountId) -> u64 {
        self.accounts.get(id).map_or(0, |a| a.seq)
    }

    pub fn lock(&self, id: &AccountId, seq: u64) -> Option<&Hash32> {
        self.locks.get(&(*id, seq))
    }

    fn tree(&self) -> merkle::Tree {
        let leaves: Vec<Hash32> = self.accounts.iter().map(|(id, account)| leaf(id, account)).collect();
        merkle::Tree::of(merkle::Kind::State, &leaves)
    }

    /// A fingerprint of every account: the root of a Merkle tree over the
    /// accounts in address order. Two validators hold the same balances,
    /// payment numbers and keys exactly when their roots are equal, however
    /// differently the payments reached them. Votes promised for payments
    /// that are not final are a validator's own business and not part of it.
    pub fn root(&self) -> Hash32 {
        self.tree().root()
    }

    /// What is needed to prove any account of this state against `root()`.
    pub fn proofs(&self) -> Proofs {
        let places = self.accounts.keys().enumerate().map(|(place, id)| (*id, place)).collect();
        Proofs { places, tree: self.tree() }
    }

    /// An account's state, with a proof of it against `root()`.
    pub fn prove(&self, id: &AccountId) -> Option<(Account, merkle::Path)> {
        let index = self.accounts.keys().position(|known| known == id)?;
        Some((self.accounts[id].clone(), self.tree().path(index)))
    }

    /// Every account, in a form `from_snapshot` reads back.
    pub fn snapshot(&self) -> Vec<u8> {
        borsh::to_vec(&self.accounts).expect("writing to memory cannot fail")
    }

    /// The state a snapshot was taken of. No votes are promised in it.
    pub fn from_snapshot(bytes: &[u8]) -> io::Result<State> {
        Ok(State { accounts: borsh::from_slice(bytes)?, locks: HashMap::new() })
    }

    /// An account that has paid out more than it holds, if there is one.
    /// A validator still catching up can see this in passing. A closed
    /// epoch must not contain it.
    pub fn overdrawn(&self) -> Option<AccountId> {
        self.accounts.iter().find(|(_, account)| account.balance < 0).map(|(id, _)| *id)
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
    /// `digest` is the order's digest and `signed_by` is what its signature
    /// proves about the key that made it. On `Fresh`, the vote is locked: no
    /// other order with this account and number will be accepted. The caller
    /// must make that lock durable before releasing its vote.
    pub fn check_order(
        &mut self,
        order: &Order,
        digest: Hash32,
        signed_by: &SignedBy,
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
        if !account.requires(&order.from, order, signed_by) {
            return Err(Reject::BadSignature);
        }
        let promised = self.locks.get(&(order.from, order.seq));
        if let Some(voted) = promised
            && *voted != digest
        {
            return Err(Reject::Conflict { voted: *voted });
        }
        // Checked on every vote, a repeated one included. A promise to vote
        // for an order is not a promise that its funds are still there: a
        // checkpoint can undo the payment that provided them.
        if account.balance < order.amount as i128 {
            return Err(Reject::InsufficientFunds { balance: account.balance, needed: order.amount });
        }
        if promised.is_some() {
            return Ok(Accepted::Repeat);
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
        let sender = self.accounts.entry(order.from).or_insert_with(|| Account::holding(0));
        sender.balance -= order.amount as i128;
        sender.seq += 1;
        if sender.signer == Signer::First {
            sender.spare = order.spare;
        }
        sender.signer = Signer::Key(order.next_key);
        let recipient = self.accounts.entry(order.to).or_insert_with(|| Account::holding(0));
        recipient.balance += order.amount as i128;
        self.locks.remove(&(order.from, order.seq));
        Ok(Applied::New)
    }

    /// Cancels an account's current order number, given evidence that its
    /// owner signed twice: two different orders under that number, each
    /// signed with the key the account requires.
    ///
    /// Nothing moves and the number stays. The burned key is retired: the
    /// account's next order must be signed with its next unused spare key.
    /// Any vote promised here under that number is dropped.
    ///
    /// The caller applies every final payment first. If one of the two
    /// orders became final, the number has moved on and this is no longer
    /// evidence of anything that matters.
    pub fn cancel(&mut self, one: &Checked, other: &Checked) -> Result<(), CancelError> {
        let id = one.order.from;
        let account = self.accounts.get_mut(&id).ok_or(CancelError::NotEvidence)?;
        let evidence = other.order.from == id
            && one.digest != other.digest
            && one.order.seq == account.seq
            && other.order.seq == account.seq
            && account.requires(&id, one.order, &one.by)
            && account.requires(&id, other.order, &other.by);
        if !evidence {
            return Err(CancelError::NotEvidence);
        }
        if account.spares_used >= 1 << SPARE_HEIGHT {
            return Err(CancelError::NoSpareLeft);
        }
        if account.signer == Signer::First {
            // Both orders revealed the spare root the address commits to.
            account.spare = one.order.spare;
        }
        account.signer = Signer::Spare(account.spares_used);
        account.spares_used += 1;
        let seq = account.seq;
        self.locks.remove(&(id, seq));
        Ok(())
    }
}
