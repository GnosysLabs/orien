//! One validator's view of the ledger, and the two rules it enforces:
//! when to vote for an order, and how a certified order changes balances.

use crate::genesis::{Fees, Genesis};
use crate::types::{AccountId, Kind, LOCK_EPOCHS, Order, SPARE_HEIGHT, SignedBy, Validator, address, lock_hash};
use borsh::{BorshDeserialize, BorshSerialize};
use orien_crypto::merkle;
use orien_crypto::{Hash32, Tag, hash};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fmt;
use std::io;

/// Where the coins of a validator caught cheating go. No key can sign for
/// this address, so they can never be spent: they are burned, and stay
/// counted in the supply.
pub const FORFEITED: AccountId = Hash32([0; 32]);

/// Where an epoch's fees gather until its checkpoint shares them out. No
/// key can sign for this address either.
pub const FEES: AccountId = Hash32([1; 32]);

/// Counts the coins released so far, as a debt: its balance is minus that
/// sum, so all balances together still add up to what the genesis file
/// set up. No key can sign for it.
pub const MINT: AccountId = Hash32([2; 32]);

/// Where released coins wait until work shares earn them. No key can sign
/// for it.
pub const REWARDS: AccountId = Hash32([3; 32]);

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
    /// Coins locked as stake behind `validator`. Not spendable.
    pub staked: u64,
    /// Coins an unstake order has released from the stake during the
    /// current epoch. The checkpoint that closes it dates their release.
    pub unstaking: u64,
    /// Coins on their way back to the balance: each amount becomes
    /// spendable at the checkpoint that closes the epoch named with it.
    /// Until then they are as locked as the stake.
    pub releases: Vec<(u64, u64)>,
    /// The validator this account's stake backs, as its latest stake order
    /// named it.
    pub validator: Option<Validator>,
}

impl Account {
    fn holding(balance: i128) -> Account {
        Account {
            balance,
            seq: 0,
            signer: Signer::First,
            spare: Hash32::default(),
            spares_used: 0,
            staked: 0,
            unstaking: 0,
            releases: Vec::new(),
            validator: None,
        }
    }

    /// Every coin the account holds: spendable, staked, or waiting to be
    /// released.
    pub fn holdings(&self) -> i128 {
        self.balance
            + self.staked as i128
            + self.unstaking as i128
            + self.releases.iter().map(|(_, amount)| *amount as i128).sum::<i128>()
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
    /// The order does not accept the network's fee.
    FeeTooLow { fee: u64, accepted: u64 },
    /// This validator has not yet seen the account's earlier payments.
    FutureSeq { current: u64 },
    BadSignature,
    InsufficientFunds { balance: i128, needed: u64 },
    /// The validator already voted for a different order with this number.
    Conflict { voted: Hash32 },
    /// A stake or unstake order names an account other than its own.
    NotOwnAccount,
    /// A stake order names a key tree too tall to build, or an address that
    /// is empty or too long.
    BadValidator,
    InsufficientStake { staked: u64, needed: u64 },
    /// A payment to the mint or the rewards pot, which only the checkpoint
    /// moves coins in and out of.
    ReservedAccount,
    /// Locked payments are not open yet on this network.
    LocksNotYet { from: u64 },
    /// A locked payment's deadline is too soon or too far off.
    BadDeadline { earliest: u64, latest: u64 },
    /// A locked payment too small to pay for its own claim.
    LockTooSmall { least: u64 },
    /// A claim of a locked payment that is not waiting for this account,
    /// or not for this amount.
    NoSuchLock,
    /// A claim whose secret does not open the lock.
    WrongSecret,
    /// A claim made after the locked payment's deadline.
    LockExpired { until: u64 },
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
            Reject::FeeTooLow { fee, accepted } => {
                write!(f, "the fee is {fee}, and the order accepts at most {accepted}")
            }
            Reject::InsufficientFunds { balance, needed } => {
                write!(f, "insufficient funds: balance {balance}, needs {needed}")
            }
            Reject::Conflict { voted } => write!(
                f,
                "already voted for a different payment with this number ({})",
                voted.short()
            ),
            Reject::NotOwnAccount => {
                f.write_str("stake and unstake orders must name the sender's own account")
            }
            Reject::BadValidator => f.write_str("the validator named has an unusable key tree or address"),
            Reject::InsufficientStake { staked, needed } => {
                write!(f, "not enough stake: staked {staked}, needs {needed}")
            }
            Reject::ReservedAccount => {
                f.write_str("that address belongs to the network itself (the mint, the rewards or fees pot, or forfeited stake); nobody can pay it")
            }
            Reject::LocksNotYet { from } => write!(f, "locked payments open at epoch {from}"),
            Reject::BadDeadline { earliest, latest } => {
                write!(f, "a locked payment's deadline must be an epoch from {earliest} to {latest}")
            }
            Reject::LockTooSmall { least } => {
                write!(f, "a locked payment must be more than {least} units, the fee its claim pays")
            }
            Reject::NoSuchLock => f.write_str("no locked payment of that amount is waiting for this account"),
            Reject::WrongSecret => f.write_str("that secret does not open the lock"),
            Reject::LockExpired { until } => write!(f, "the locked payment's time ran out at epoch {until}"),
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
            &account.staked.to_le_bytes(),
            &account.unstaking.to_le_bytes(),
            &borsh::to_vec(&account.releases).expect("writing to memory cannot fail"),
            &borsh::to_vec(&account.validator).expect("writing to memory cannot fail"),
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

/// A locked payment waiting to be claimed: `amount` taken from `from` for
/// `to`, who receives it by revealing the secret behind `hash` before epoch
/// `until` begins. Named by the sender and the number of the order that
/// made it.
#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct Escrow {
    pub to: AccountId,
    pub amount: u64,
    pub hash: [u8; 32],
    pub until: u64,
}

#[derive(Clone)]
pub struct State {
    /// In address order, so that every validator lists them alike.
    accounts: BTreeMap<AccountId, Account>,
    /// The order this validator voted for, per account and payment number.
    /// Entries exist only for payments that are not final yet.
    locks: HashMap<(AccountId, u64), Hash32>,
    /// What orders pay, from the genesis file.
    fees: Fees,
    /// The key trees of validators caught cheating, in order. None of them
    /// is ever chosen again. Part of `root()` once there is any.
    banned: BTreeSet<Hash32>,
    /// Locked payments not yet claimed or refunded, by sender and number.
    escrows: BTreeMap<(AccountId, u64), Escrow>,
    /// The secret each claim revealed, with the epoch it was revealed in,
    /// by the locked payment's sender and number: the other side of a swap
    /// reads it here. Kept for a week of epochs.
    revealed: BTreeMap<(AccountId, u64), ([u8; 32], u64)>,
    /// The epoch this state is in. Not part of the root: every validator
    /// sets it alike, from the epoch it is working in.
    epoch: u64,
    /// The first epoch in which locked payments may be made.
    locks_from: u64,
}

/// How many epochs a revealed secret stays readable in the state.
pub const REVEALED_EPOCHS: u64 = 168;

impl State {
    pub fn from_genesis(genesis: &Genesis) -> State {
        let accounts = genesis
            .allocations
            .iter()
            .map(|a| (a.account, Account::holding(a.amount as i128)))
            .collect();
        State {
            accounts,
            locks: HashMap::new(),
            fees: genesis.fee,
            banned: BTreeSet::new(),
            escrows: BTreeMap::new(),
            revealed: BTreeMap::new(),
            epoch: 0,
            locks_from: 0,
        }
    }

    /// What orders pay.
    pub fn fees(&self) -> Fees {
        self.fees
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
        let mut leaves: Vec<Hash32> = self.accounts.iter().map(|(id, account)| leaf(id, account)).collect();
        // The banned keys follow the accounts, so no account's place moves.
        if !self.banned.is_empty() {
            let roots: Vec<&[u8]> = self.banned.iter().map(|root| root.as_bytes().as_slice()).collect();
            leaves.push(hash(Tag::Banned, &roots));
        }
        // So do the locked payments and revealed secrets, when there are any:
        // a state without them has the root it had before they existed.
        if !self.escrows.is_empty() {
            let bytes = borsh::to_vec(&self.escrows).expect("writing to memory cannot fail");
            leaves.push(hash(Tag::Escrows, &[&bytes]));
        }
        if !self.revealed.is_empty() {
            let bytes = borsh::to_vec(&self.revealed).expect("writing to memory cannot fail");
            leaves.push(hash(Tag::Revealed, &[&bytes]));
        }
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
        borsh::to_vec(&(&self.accounts, &self.banned, &self.escrows, &self.revealed))
            .expect("writing to memory cannot fail")
    }

    /// The state a snapshot was taken of, on a network whose orders pay
    /// `fees`. No votes are promised in it.
    pub fn from_snapshot(bytes: &[u8], fees: Fees) -> io::Result<State> {
        // A snapshot from before locked payments holds only the accounts and
        // the barred keys; it reads as a state with no locked payments.
        type Before = (BTreeMap<AccountId, Account>, BTreeSet<Hash32>);
        type Now = (BTreeMap<AccountId, Account>, BTreeSet<Hash32>, BTreeMap<(AccountId, u64), Escrow>, BTreeMap<(AccountId, u64), ([u8; 32], u64)>);
        let (accounts, banned, escrows, revealed) = match borsh::from_slice::<Now>(bytes) {
            Ok(now) => now,
            Err(_) => {
                let (accounts, banned): Before = borsh::from_slice(bytes)?;
                (accounts, banned, BTreeMap::new(), BTreeMap::new())
            }
        };
        Ok(State { accounts, locks: HashMap::new(), fees, banned, escrows, revealed, epoch: 0, locks_from: 0 })
    }

    /// Sets the epoch this state is in. Every validator sets it alike.
    pub fn set_epoch(&mut self, epoch: u64) {
        self.epoch = epoch;
    }

    pub fn epoch(&self) -> u64 {
        self.epoch
    }

    /// Sets the first epoch in which locked payments may be made.
    pub fn set_locks_from(&mut self, epoch: u64) {
        self.locks_from = epoch;
    }

    /// The locked payment that `from` made as order `seq`, if it is still
    /// waiting.
    pub fn escrow(&self, from: &AccountId, seq: u64) -> Option<&Escrow> {
        self.escrows.get(&(*from, seq))
    }

    /// The secret that claimed the locked payment `from` made as order
    /// `seq`, once a claim has revealed it.
    pub fn revealed(&self, from: &AccountId, seq: u64) -> Option<[u8; 32]> {
        self.revealed.get(&(*from, seq)).map(|(secret, _)| *secret)
    }

    /// At the checkpoint closing `epoch`: every locked payment whose time is
    /// up goes back to its sender, and secrets revealed a week ago are let go.
    pub fn refund_expired(&mut self, epoch: u64) {
        let expired: Vec<_> =
            self.escrows.iter().filter(|(_, e)| e.until <= epoch + 1).map(|(key, _)| *key).collect();
        for key in expired {
            let escrow = self.escrows.remove(&key).expect("listed just now");
            self.accounts.entry(key.0).or_insert_with(|| Account::holding(0)).balance += escrow.amount as i128;
        }
        self.revealed.retain(|_, (_, revealed)| revealed.saturating_add(REVEALED_EPOCHS) > epoch);
    }

    /// The key trees of validators caught cheating, which are never chosen
    /// again.
    pub fn banned(&self) -> &BTreeSet<Hash32> {
        &self.banned
    }

    /// Bars a validator's key tree for good.
    pub fn ban(&mut self, root: Hash32) {
        self.banned.insert(root);
    }

    /// An account that has paid out more than it holds, if there is one.
    /// A validator still catching up can see this in passing. A closed
    /// epoch must not contain it.
    pub fn overdrawn(&self) -> Option<AccountId> {
        self.accounts.iter().find(|(id, account)| account.balance < 0 && **id != MINT).map(|(id, _)| *id)
    }

    /// Every coin in every account, staked and waiting ones included.
    /// Applying a certificate moves money and never creates it, so this
    /// always equals the genesis supply.
    pub fn total(&self) -> i128 {
        self.accounts.values().map(Account::holdings).sum::<i128>()
            + self.escrows.values().map(|e| e.amount as i128).sum::<i128>()
    }

    /// Every account that has named a validator and has at least
    /// `min_stake` staked, with that validator and stake, in address order.
    /// Coins being unstaked no longer count.
    pub fn stakers(&self, min_stake: u64) -> Vec<(AccountId, &Validator, u64)> {
        self.accounts
            .iter()
            .filter(|(_, account)| account.staked >= min_stake)
            .filter_map(|(id, account)| account.validator.as_ref().map(|v| (*id, v, account.staked)))
            .collect()
    }

    /// Shares out the fees gathered so far among `earners`, each an account
    /// with its stake, in proportion to stake. What does not divide evenly,
    /// and everything when there is nobody to pay, stays in the pot for the
    /// next checkpoint.
    pub fn pay_fees(&mut self, earners: &[(AccountId, u64)]) {
        self.pay_out(FEES, earners);
    }

    /// How many units have been released so far.
    pub fn released(&self) -> u64 {
        self.accounts.get(&MINT).map_or(0, |mint| (-mint.balance) as u64)
    }

    /// Releases coins up to `total` released in all, into the rewards pot.
    /// Returns how many were released now.
    pub fn release_up_to(&mut self, total: u64) -> u64 {
        let new = total.saturating_sub(self.released());
        if new > 0 {
            self.accounts.entry(MINT).or_insert_with(|| Account::holding(0)).balance -= new as i128;
            self.accounts.entry(REWARDS).or_insert_with(|| Account::holding(0)).balance += new as i128;
        }
        new
    }

    /// Shares out the rewards pot among `earners`, each an account with its
    /// count of work shares, the way `pay_fees` does.
    pub fn pay_rewards(&mut self, earners: &[(AccountId, u64)]) {
        self.pay_out(REWARDS, earners);
    }

    fn pay_out(&mut self, from: AccountId, earners: &[(AccountId, u64)]) {
        let pot = self.accounts.get(&from).map_or(0, |pot| pot.balance.max(0) as u128);
        let total: u128 = earners.iter().map(|(_, stake)| *stake as u128).sum();
        if pot == 0 || total == 0 {
            return;
        }
        let mut paid = 0i128;
        for (account, stake) in earners {
            let share = (pot * *stake as u128 / total) as i128;
            if share > 0 {
                self.accounts.entry(*account).or_insert_with(|| Account::holding(0)).balance += share;
                paid += share;
            }
        }
        if let Some(pot) = self.accounts.get_mut(&from) {
            pot.balance -= paid;
        }
    }

    /// Takes everything `id` has staked, including coins still waiting to
    /// be released, and burns it.
    pub fn forfeit(&mut self, id: &AccountId) {
        let Some(account) = self.accounts.get_mut(id) else { return };
        let taken = account.staked as i128
            + account.unstaking as i128
            + account.releases.iter().map(|(_, amount)| *amount as i128).sum::<i128>();
        account.staked = 0;
        account.unstaking = 0;
        account.releases.clear();
        if taken > 0 {
            self.accounts.entry(FORFEITED).or_insert_with(|| Account::holding(0)).balance += taken;
        }
    }

    /// Replaces the key tree of the validator `id` names, when its
    /// validator has announced a new one at a checkpoint.
    pub fn rekey_validator(&mut self, id: &AccountId, root: Hash32, height: u8) {
        if let Some(validator) = self.accounts.get_mut(id).and_then(|a| a.validator.as_mut()) {
            validator.root = root;
            validator.height = height;
        }
    }

    /// The stake side of closing `epoch`. Releases whose wait ends with
    /// this epoch become spendable, and coins unstaked during it start
    /// their wait of `unstake_epochs` epochs.
    pub fn settle_stakes(&mut self, epoch: u64, unstake_epochs: u64) {
        for account in self.accounts.values_mut() {
            let mut released = 0;
            account.releases.retain(|(until, amount)| {
                let due = *until <= epoch;
                if due {
                    released += *amount as i128;
                }
                !due
            });
            account.balance += released;
            if account.unstaking > 0 {
                account.releases.push((epoch.saturating_add(unstake_epochs), account.unstaking));
                account.unstaking = 0;
            }
        }
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
        // A payee claiming a locked payment may hold nothing yet.
        let newcomer = Account::holding(0);
        let account = match self.accounts.get(&order.from) {
            Some(account) => account,
            None if matches!(order.kind, Kind::Claim { .. }) => &newcomer,
            None => return Err(Reject::UnknownAccount),
        };
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
        // The network's own accounts take coins only by its rules: a payment to
        // the fees pot would hand coins to validators, and one to the forfeited
        // account would destroy them, both most likely by mistake.
        if [MINT, REWARDS, FEES, FORFEITED].contains(&order.to) {
            return Err(Reject::ReservedAccount);
        }
        if !matches!(order.kind, Kind::Payment | Kind::Lock { .. }) && order.to != order.from {
            return Err(Reject::NotOwnAccount);
        }
        match &order.kind {
            Kind::Lock { until, .. } => {
                if self.epoch < self.locks_from {
                    return Err(Reject::LocksNotYet { from: self.locks_from });
                }
                let (earliest, latest) = (self.epoch + LOCK_EPOCHS.start(), self.epoch + LOCK_EPOCHS.end());
                if !(earliest..=latest).contains(until) {
                    return Err(Reject::BadDeadline { earliest, latest });
                }
                if order.amount <= self.fees.min {
                    return Err(Reject::LockTooSmall { least: self.fees.min });
                }
            }
            Kind::Claim { from, seq, secret } => {
                if self.epoch < self.locks_from {
                    return Err(Reject::LocksNotYet { from: self.locks_from });
                }
                let escrow = self.escrows.get(&(*from, *seq)).ok_or(Reject::NoSuchLock)?;
                if escrow.to != order.from || escrow.amount != order.amount {
                    return Err(Reject::NoSuchLock);
                }
                if self.epoch >= escrow.until {
                    return Err(Reject::LockExpired { until: escrow.until });
                }
                if lock_hash(secret) != escrow.hash {
                    return Err(Reject::WrongSecret);
                }
            }
            _ => {}
        }
        if let Kind::Stake { validator, .. } = &order.kind
            && !validator.is_usable()
        {
            return Err(Reject::BadValidator);
        }
        // Checked on every vote, a repeated one included. A promise to vote
        // for an order is not a promise that its funds are still there: a
        // checkpoint can undo the payment that provided them, or take the
        // stake of a validator that cheated.
        let fee = self.fees.on(&order.kind, order.amount);
        if order.max_fee < fee {
            return Err(Reject::FeeTooLow { fee, accepted: order.max_fee });
        }
        let spends = match order.kind {
            Kind::Unstake => {
                if account.staked < order.amount {
                    return Err(Reject::InsufficientStake { staked: account.staked, needed: order.amount });
                }
                fee
            }
            // A claim's fee comes out of the locked amount.
            Kind::Claim { .. } => 0,
            _ => order.amount.saturating_add(fee),
        };
        if account.balance < spends as i128 {
            return Err(Reject::InsufficientFunds { balance: account.balance, needed: spends });
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
        let fee = self.fees.on(&order.kind, order.amount);
        sender.seq += 1;
        // A claim pays its fee out of the locked amount.
        if !matches!(order.kind, Kind::Claim { .. }) {
            sender.balance -= fee as i128;
        }
        if sender.signer == Signer::First {
            sender.spare = order.spare;
        }
        sender.signer = Signer::Key(order.next_key);
        match &order.kind {
            Kind::Payment => {
                sender.balance -= order.amount as i128;
                let recipient = self.accounts.entry(order.to).or_insert_with(|| Account::holding(0));
                recipient.balance += order.amount as i128;
            }
            Kind::Stake { validator, .. } => {
                sender.balance -= order.amount as i128;
                sender.staked += order.amount;
                sender.validator = Some(validator.clone());
            }
            Kind::Unstake => {
                // The voters saw enough stake. A checkpoint may since have
                // taken it, and coins that are gone cannot be released.
                let amount = order.amount.min(sender.staked);
                sender.staked -= amount;
                sender.unstaking += amount;
            }
            Kind::Lock { hash, until } => {
                sender.balance -= order.amount as i128;
                // A validator can learn of the claim before the lock: then
                // the payee is already paid, and nothing is left to hold.
                let key = (order.from, order.seq);
                if !self.revealed.contains_key(&key) {
                    let escrow = Escrow { to: order.to, amount: order.amount, hash: *hash, until: *until };
                    self.escrows.insert(key, escrow);
                }
            }
            Kind::Claim { from, seq, secret } => {
                sender.balance += order.amount as i128 - fee as i128;
                self.escrows.remove(&(*from, *seq));
                self.revealed.insert((*from, *seq), (*secret, self.epoch));
            }
        }
        if fee > 0 {
            self.accounts.entry(FEES).or_insert_with(|| Account::holding(0)).balance += fee as i128;
        }
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
