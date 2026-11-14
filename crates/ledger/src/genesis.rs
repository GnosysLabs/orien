use crate::types::{AccountId, COIN, Certificate, Confirmation, Kind, Receipt, Vote, confirmation_item};
use borsh::{BorshDeserialize, BorshSerialize};
use orien_crypto::{Hash32, Tag, hash, keytree, merkle};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fmt;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ValidatorInfo {
    pub name: String,
    /// Root of the validator's key tree: its public key.
    pub root: Hash32,
    pub height: u8,
    pub stake: u64,
    /// Where to reach it. Not part of the network's identity, so validators
    /// can move without changing the network.
    pub address: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Allocation {
    pub account: AccountId,
    pub amount: u64,
}

/// Everything that defines a network: who validates, who starts with what,
/// and the settings every validator must share.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Genesis {
    pub network: String,
    pub validators: Vec<ValidatorInfo>,
    pub allocations: Vec<Allocation>,
    /// Seconds from one checkpoint to the next.
    #[serde(default = "an_hour")]
    pub epoch_seconds: u64,
    /// An epoch ends early once it holds this many payments, so that no
    /// checkpoint has too much to reconcile.
    #[serde(default = "fifty_thousand")]
    pub epoch_payments: u64,
    /// When the network starts, in seconds since 1970. The first checkpoint
    /// is due one epoch after it.
    #[serde(default)]
    pub start_time: u64,
    /// The least a stake order may lock for a validator to be chosen. The
    /// validators of the genesis file are exempt: they are the launch seats.
    #[serde(default = "ten_thousand")]
    pub min_stake: u64,
    /// The most validators any epoch may have. The largest stakes win.
    #[serde(default = "thirty_three")]
    pub max_validators: u16,
    /// How many epochs unstaked coins stay locked before they can be spent,
    /// so that a validator that cheats cannot take its stake and run before
    /// the evidence against it is acted on.
    #[serde(default = "a_week_of_epochs")]
    pub unstake_epochs: u64,
    /// What orders pay, taken from the sender's spendable coins when an
    /// order is applied, and shared out to the validators at the
    /// checkpoint.
    #[serde(default)]
    pub fee: Fees,
    /// The most coins that will ever be released, in units.
    #[serde(default = "the_cap")]
    pub cap: u64,
    /// How many work shares an epoch should hold. The work target moves at
    /// each checkpoint to keep near it.
    #[serde(default = "a_thousand")]
    pub shares_per_epoch: u64,
    /// The work target of the first epoch: a share counts if its hash,
    /// read as a number, is below it.
    #[serde(default = "a_first_target")]
    pub first_target: u64,
    /// A Bitcoin block announced in advance. Its hash is part of the
    /// network's identity, and the identity is the seed the first epoch's
    /// work is done against, so nobody can mine before that block exists.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bitcoin: Option<BitcoinBlock>,
}

/// A Bitcoin block, its hash written as block explorers show it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BitcoinBlock {
    pub height: u64,
    pub hash: Hash32,
}

fn the_cap() -> u64 {
    crate::release::CAP
}

fn a_thousand() -> u64 {
    1_000
}

/// About one share in 16 million hashes: a few seconds of one computer's
/// work.
fn a_first_target() -> u64 {
    u64::MAX >> 24
}

/// The rules for releasing coins and paying them for work.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Work {
    /// When the network starts, in seconds since 1970.
    pub start_time: u64,
    pub cap: u64,
    pub shares_per_epoch: u64,
    /// This epoch's target.
    pub target: u64,
}

/// The fee rule: a share of each payment, never less than `min` and never
/// more than `max`. Stake and unstake orders pay `min`. A share of the
/// amount keeps the fee about the same in real money however the coin's
/// value moves; the minimum stops floods of tiny payments, and the maximum
/// keeps a large payment from paying far more than the work it costs.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Fees {
    pub min: u64,
    pub max: u64,
    /// Millionths of the amount: 1,000 is 0.1%.
    pub rate: u32,
}

impl Fees {
    /// No fee at all, for tests.
    pub const NONE: Fees = Fees { min: 0, max: 0, rate: 0 };

    /// The fee for an order of this kind and amount.
    pub fn on(&self, kind: &Kind, amount: u64) -> u64 {
        match kind {
            Kind::Payment => {
                let share = (amount as u128 * self.rate as u128 / 1_000_000) as u64;
                share.clamp(self.min, self.max.max(self.min))
            }
            _ => self.min,
        }
    }
}

/// Counted in the smallest unit, a billionth of a coin once coins have
/// decimals: 0.1% of each payment, at least 0.000001 coin, at most 1 coin.
impl Default for Fees {
    fn default() -> Fees {
        Fees { min: 1_000, max: 1_000_000_000, rate: 1_000 }
    }
}

fn an_hour() -> u64 {
    3600
}

fn fifty_thousand() -> u64 {
    50_000
}

fn ten_thousand() -> u64 {
    10_000 * COIN
}

fn thirty_three() -> u16 {
    33
}

fn a_week_of_epochs() -> u64 {
    168
}

impl Genesis {
    /// A network with the usual settings.
    pub fn new(network: &str, validators: Vec<ValidatorInfo>, allocations: Vec<Allocation>) -> Genesis {
        Genesis {
            network: network.into(),
            validators,
            allocations,
            epoch_seconds: an_hour(),
            epoch_payments: fifty_thousand(),
            start_time: 0,
            min_stake: ten_thousand(),
            max_validators: thirty_three(),
            unstake_epochs: a_week_of_epochs(),
            fee: Fees::default(),
            cap: the_cap(),
            shares_per_epoch: a_thousand(),
            first_target: a_first_target(),
            bitcoin: None,
        }
    }

    /// The network's identity. Every signature is bound to it.
    pub fn id(&self) -> Hash32 {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&(self.network.len() as u32).to_le_bytes());
        bytes.extend_from_slice(self.network.as_bytes());
        bytes.extend_from_slice(&(self.validators.len() as u32).to_le_bytes());
        for validator in &self.validators {
            bytes.extend_from_slice(validator.root.as_bytes());
            bytes.push(validator.height);
            bytes.extend_from_slice(&validator.stake.to_le_bytes());
        }
        bytes.extend_from_slice(&(self.allocations.len() as u32).to_le_bytes());
        for allocation in &self.allocations {
            bytes.extend_from_slice(allocation.account.as_bytes());
            bytes.extend_from_slice(&allocation.amount.to_le_bytes());
        }
        let settings = [
            self.epoch_seconds,
            self.epoch_payments,
            self.start_time,
            self.min_stake,
            self.max_validators as u64,
            self.unstake_epochs,
            self.fee.min,
            self.fee.max,
            self.fee.rate as u64,
            self.cap,
            self.shares_per_epoch,
            self.first_target,
        ];
        for setting in settings {
            bytes.extend_from_slice(&setting.to_le_bytes());
        }
        if let Some(block) = &self.bitcoin {
            bytes.extend_from_slice(&block.height.to_le_bytes());
            bytes.extend_from_slice(block.hash.as_bytes());
        }
        hash(Tag::Genesis, &[&bytes])
    }

    pub fn staking(&self) -> Staking {
        Staking {
            min_stake: self.min_stake,
            max_validators: self.max_validators,
            unstake_epochs: self.unstake_epochs,
        }
    }

    pub fn supply(&self) -> u128 {
        self.allocations.iter().map(|a| a.amount as u128).sum()
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.validators.is_empty() {
            return Err("no validators".into());
        }
        if self.validators.len() > self.max_validators as usize {
            return Err(format!("more validators than the limit of {}", self.max_validators));
        }
        let mut roots = HashSet::new();
        let mut total: u64 = 0;
        for validator in &self.validators {
            if validator.stake == 0 {
                return Err(format!("validator {} has no stake", validator.name));
            }
            if validator.height > keytree::MAX_HEIGHT {
                return Err(format!("validator {} has an oversized key tree", validator.name));
            }
            if !roots.insert(validator.root) {
                return Err(format!("validator {} repeats another validator's key", validator.name));
            }
            total = total.checked_add(validator.stake).ok_or("total stake overflows")?;
        }
        let mut accounts = HashSet::new();
        for allocation in &self.allocations {
            if !accounts.insert(allocation.account) {
                return Err(format!("account {} is allocated twice", allocation.account));
            }
        }
        if self.supply() > u64::MAX as u128 {
            return Err("total supply overflows".into());
        }
        if self.epoch_seconds == 0 || self.epoch_payments == 0 {
            return Err("an epoch must have a length and a payment limit".into());
        }
        if self.fee.min > self.fee.max || self.fee.rate > 1_000_000 {
            return Err("the fee's minimum must not exceed its maximum, nor its rate the whole amount".into());
        }
        if self.min_stake == 0 || self.unstake_epochs == 0 {
            return Err("staking needs a minimum stake and an unstake waiting period".into());
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum VerifyError {
    UnknownValidator(u16),
    BadBatchPath(u16),
    BadSignature(u16),
    VotesOutOfOrder,
    NoQuorum { stake: u64, total: u64 },
    /// The certificate's votes were cast in a different epoch.
    WrongEpoch { found: u64, current: u64 },
    /// The certificate is good, but too few validators confirmed it.
    NotConfirmed { stake: u64, total: u64 },
}

impl fmt::Display for VerifyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            VerifyError::UnknownValidator(v) => write!(f, "signature from unknown validator {v}"),
            VerifyError::BadBatchPath(v) => write!(f, "validator {v}'s signature has a malformed batch path"),
            VerifyError::BadSignature(v) => write!(f, "validator {v}'s signature does not verify"),
            VerifyError::VotesOutOfOrder => f.write_str("signatures are repeated or out of order"),
            VerifyError::NoQuorum { stake, total } => {
                write!(f, "votes cover {stake} of {total} stake, which is not more than two thirds")
            }
            VerifyError::WrongEpoch { found, current } => {
                write!(f, "the votes are from epoch {found}, and this is epoch {current}")
            }
            VerifyError::NotConfirmed { stake, total } => write!(
                f,
                "confirmations cover {stake} of {total} stake, which is not more than two thirds"
            ),
        }
    }
}

impl std::error::Error for VerifyError {}

/// One validator of an epoch: the key it signs with, the weight of its
/// vote, and where to reach it.
#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct Member {
    /// Root of the validator's key tree.
    pub root: Hash32,
    pub height: u8,
    pub stake: u64,
    pub address: String,
    /// The account whose stake makes it a validator. None for the
    /// validators of the genesis file, which hold launch seats.
    pub account: Option<AccountId>,
    /// For a launch seat, its place among the genesis file's validators.
    pub seat: Option<u16>,
}

/// The rules for choosing validators, from the genesis file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Staking {
    pub min_stake: u64,
    pub max_validators: u16,
    pub unstake_epochs: u64,
}

/// The validator set of one epoch, with what is needed to check its votes.
#[derive(Clone)]
pub struct Committee {
    network: Hash32,
    /// The epoch this validator set serves. Signatures made in one epoch
    /// count for nothing in another.
    epoch: u64,
    members: Vec<Member>,
    total_stake: u64,
    staking: Staking,
    work: Work,
    /// Each launch seat's share of the seats' weight: its stake in the
    /// genesis file, in genesis order.
    seats: Vec<u64>,
}

impl Committee {
    pub fn new(genesis: &Genesis) -> Committee {
        let members: Vec<Member> = genesis
            .validators
            .iter()
            .enumerate()
            .map(|(seat, v)| Member {
                root: v.root,
                height: v.height,
                stake: v.stake,
                address: v.address.clone(),
                account: None,
                seat: Some(seat as u16),
            })
            .collect();
        let total_stake = members.iter().map(|m| m.stake).sum();
        let seats = genesis.validators.iter().map(|v| v.stake).collect();
        let work = Work {
            start_time: genesis.start_time,
            cap: genesis.cap,
            shares_per_epoch: genesis.shares_per_epoch,
            target: genesis.first_target,
        };
        Committee { network: genesis.id(), epoch: 0, members, total_stake, staking: genesis.staking(), work, seats }
    }

    /// Each launch seat's share of the seats' weight, in genesis order.
    /// Together they are the weight the seats carry while no real stake
    /// exists.
    pub fn seats(&self) -> &[u64] {
        &self.seats
    }

    pub fn staking(&self) -> Staking {
        self.staking
    }

    pub fn work(&self) -> Work {
        self.work
    }

    /// The same validators with another work target: the one the checkpoint
    /// that opened their epoch set.
    pub fn with_target(&self, target: u64) -> Committee {
        Committee { work: Work { target, ..self.work }, ..self.clone() }
    }

    pub fn epoch(&self) -> u64 {
        self.epoch
    }

    /// The same validators serving another epoch. Their signatures from
    /// this epoch count for nothing there.
    pub fn at_epoch(&self, epoch: u64) -> Committee {
        Committee { epoch, ..self.clone() }
    }

    pub fn members(&self) -> &[Member] {
        &self.members
    }

    /// The validator set of the epoch after this one, as the checkpoint
    /// that closes this epoch names it.
    pub fn following(&self, members: Vec<Member>) -> Committee {
        self.with(self.epoch + 1, members)
    }

    /// These validators, serving `epoch` of the same network.
    pub fn with(&self, epoch: u64, members: Vec<Member>) -> Committee {
        let total_stake = members.iter().map(|m| m.stake).sum();
        Committee {
            network: self.network,
            epoch,
            members,
            total_stake,
            staking: self.staking,
            work: self.work,
            seats: self.seats.clone(),
        }
    }

    pub fn network(&self) -> &Hash32 {
        &self.network
    }

    pub fn len(&self) -> usize {
        self.members.len()
    }

    pub fn is_empty(&self) -> bool {
        self.members.is_empty()
    }

    pub fn total_stake(&self) -> u64 {
        self.total_stake
    }

    pub fn stake_of(&self, validator: u16) -> u64 {
        self.members.get(validator as usize).map_or(0, |m| m.stake)
    }

    /// True when `stake` is more than two thirds of the total.
    pub fn has_quorum(&self, stake: u64) -> bool {
        3 * stake as u128 > 2 * self.total_stake as u128
    }

    /// The message a validator signs for a batch with this Merkle root. It
    /// names the network and the epoch.
    pub fn batch_message(&self, batch_root: &Hash32) -> Hash32 {
        hash(
            Tag::BatchMessage,
            &[self.network.as_bytes(), &self.epoch.to_le_bytes(), batch_root.as_bytes()],
        )
    }

    /// Checks one vote for the order with this digest. Returns the voter's stake.
    pub fn verify_vote(&self, order_digest: &Hash32, vote: &Vote) -> Result<u64, VerifyError> {
        self.verify_signed(order_digest, vote)
    }

    /// Checks one confirmation of the order with this digest. Returns the
    /// confirming validator's stake.
    pub fn verify_confirmation(
        &self,
        order_digest: &Hash32,
        confirmation: &Confirmation,
    ) -> Result<u64, VerifyError> {
        self.verify_signed(&confirmation_item(order_digest), confirmation)
    }

    /// Checks that a validator of this epoch signed a batch containing
    /// `item`. Returns that validator's stake.
    pub fn verify_item(&self, item: &Hash32, signature: &Vote) -> Result<u64, VerifyError> {
        self.verify_signed(item, signature)
    }

    fn verify_signed(&self, item: &Hash32, vote: &Vote) -> Result<u64, VerifyError> {
        let member = self
            .members
            .get(vote.validator as usize)
            .ok_or(VerifyError::UnknownValidator(vote.validator))?;
        let batch_root = merkle::root_from_path(item, &vote.batch_path)
            .ok_or(VerifyError::BadBatchPath(vote.validator))?;
        let message = self.batch_message(&batch_root);
        if !keytree::verify(&member.root, member.height, &vote.signature, message.as_bytes()) {
            return Err(VerifyError::BadSignature(vote.validator));
        }
        Ok(member.stake)
    }

    /// Adds up the stake behind signatures over `item`. They must be valid
    /// and listed in increasing validator order, which also rules out repeats.
    fn stake_behind(&self, item: &Hash32, signatures: &[Vote]) -> Result<u64, VerifyError> {
        let mut stake: u64 = 0;
        let mut previous: Option<u16> = None;
        for signature in signatures {
            if previous.is_some_and(|p| signature.validator <= p) {
                return Err(VerifyError::VotesOutOfOrder);
            }
            previous = Some(signature.validator);
            stake += self.verify_signed(item, signature)?;
        }
        Ok(stake)
    }

    /// Checks that a certificate holds votes of this epoch from more than
    /// two thirds of the stake.
    ///
    /// The owner's signature inside it is not checked here: that needs the
    /// account's key, which only a validator's state knows.
    pub fn verify_certificate(&self, certificate: &Certificate) -> Result<(), VerifyError> {
        if certificate.epoch != self.epoch {
            return Err(VerifyError::WrongEpoch { found: certificate.epoch, current: self.epoch });
        }
        let digest = certificate.order.digest(&self.network);
        let stake = self.stake_behind(&digest, &certificate.votes)?;
        if !self.has_quorum(stake) {
            return Err(VerifyError::NoQuorum { stake, total: self.total_stake });
        }
        Ok(())
    }

    /// Checks that a receipt proves its payment final: a good certificate,
    /// and confirmations of it from more than two thirds of the stake.
    pub fn verify_receipt(&self, receipt: &Receipt) -> Result<(), VerifyError> {
        self.verify_certificate(&receipt.certificate)?;
        let digest = receipt.certificate.order.digest(&self.network);
        let stake = self.stake_behind(&confirmation_item(&digest), &receipt.confirmations)?;
        if !self.has_quorum(stake) {
            return Err(VerifyError::NotConfirmed { stake, total: self.total_stake });
        }
        Ok(())
    }
}
