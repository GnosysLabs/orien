use crate::types::{AccountId, Certificate, Confirmation, Receipt, Vote, confirmation_item};
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

/// Everything that defines a network: who validates and who starts with what.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Genesis {
    pub network: String,
    pub validators: Vec<ValidatorInfo>,
    pub allocations: Vec<Allocation>,
}

impl Genesis {
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
        hash(Tag::Genesis, &[&bytes])
    }

    pub fn supply(&self) -> u128 {
        self.allocations.iter().map(|a| a.amount as u128).sum()
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.validators.is_empty() {
            return Err("no validators".into());
        }
        if self.validators.len() > u16::MAX as usize {
            return Err("too many validators".into());
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

#[derive(Clone)]
struct Member {
    root: Hash32,
    height: u8,
    stake: u64,
}

/// The validator set, with what is needed to check votes.
#[derive(Clone)]
pub struct Committee {
    network: Hash32,
    /// The epoch this validator set serves. Signatures made in one epoch
    /// count for nothing in another.
    epoch: u64,
    members: Vec<Member>,
    total_stake: u64,
}

impl Committee {
    pub fn new(genesis: &Genesis) -> Committee {
        let members: Vec<Member> = genesis
            .validators
            .iter()
            .map(|v| Member { root: v.root, height: v.height, stake: v.stake })
            .collect();
        let total_stake = members.iter().map(|m| m.stake).sum();
        Committee { network: genesis.id(), epoch: 0, members, total_stake }
    }

    pub fn epoch(&self) -> u64 {
        self.epoch
    }

    /// The same validators serving another epoch. Their signatures from
    /// this epoch count for nothing there.
    pub fn at_epoch(&self, epoch: u64) -> Committee {
        Committee { epoch, ..self.clone() }
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

    /// Checks that a validator of this epoch signed a batch containing `item`.
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
