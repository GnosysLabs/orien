use borsh::{BorshDeserialize, BorshSerialize};
use orien_crypto::keytree::{self, TreeSignature};
use orien_crypto::{Hash32, Tag, hash, merkle, ots};

/// An account is named by its address: a commitment to its first one-time
/// key and to its spare keys.
pub type AccountId = Hash32;

/// An account has 2^SPARE_HEIGHT spare keys, under one root.
pub const SPARE_HEIGHT: u8 = 4;

/// The address of the account whose first key has this commitment and whose
/// spare keys have this root. Both are fixed before the account ever signs
/// anything, so whoever later learns to forge one of its used keys still has
/// no hold on its spares.
pub fn address(first_key: &Hash32, spare: &Hash32) -> AccountId {
    hash(Tag::Address, &[first_key.as_bytes(), spare.as_bytes()])
}

/// What an order does. Only payments exist so far. More kinds will be added
/// as new variants, which leaves the encoding of the existing ones alone.
#[derive(Clone, Copy, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub enum Kind {
    /// Moves `amount` from `from` to `to`.
    Payment,
}

/// "Account `from`, order number `seq`: send `amount` to `to`."
///
/// An order that is signed but not yet final cannot be signed again, because
/// its key is one-time. So this format carries everything later features
/// need, and is not meant to change.
#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct Order {
    pub kind: Kind,
    pub from: AccountId,
    /// The sender's own counter, starting at 0.
    pub seq: u64,
    pub to: AccountId,
    pub amount: u64,
    /// The largest fee the owner accepts for this order. Stated in the
    /// order, so that a change of fee while it is unfinished cannot leave it
    /// neither payable nor replaceable.
    pub max_fee: u64,
    /// Commitment to the one-time key that will sign order `seq + 1`.
    pub next_key: Hash32,
    /// The root of the account's spare keys. An account's first order
    /// reveals it, checked against the address. Later orders repeat it.
    pub spare: Hash32,
}

impl Order {
    /// What gets signed. Bound to one network so an order cannot be replayed
    /// on another.
    pub fn digest(&self, network: &Hash32) -> Hash32 {
        let bytes = borsh::to_vec(self).expect("serializing to memory cannot fail");
        hash(Tag::Order, &[network.as_bytes(), &bytes])
    }
}

/// The owner's signature on an order.
#[allow(clippy::large_enum_variant)]
#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub enum Signature {
    /// Made with the account's current one-time key. The usual case.
    Key(ots::Signature),
    /// Made with one of the account's spare keys, after its current key was
    /// used twice and a checkpoint cancelled what it signed.
    Spare(TreeSignature),
}

/// What a signature proves about the key that made it. Finding this is the
/// expensive part of checking an order, and needs no ledger state, so it is
/// done before the ledger is consulted.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SignedBy {
    /// The one-time key with this commitment.
    Key(Hash32),
    /// Key number `leaf` of the spare tree with this root.
    Spare { root: Hash32, leaf: u32 },
}

impl Signature {
    /// Who signed the order with this digest, or `None` if the signature is
    /// malformed.
    pub fn signed_by(&self, digest: &Hash32) -> Option<SignedBy> {
        match self {
            Signature::Key(signature) => Some(SignedBy::Key(ots::recover(signature, digest.as_bytes()))),
            Signature::Spare(signature) => {
                let root = keytree::recover_root(SPARE_HEIGHT, signature, digest.as_bytes())?;
                Some(SignedBy::Spare { root, leaf: signature.leaf })
            }
        }
    }
}

/// An order with its owner's signature.
#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct SignedOrder {
    pub order: Order,
    pub signature: Signature,
}

impl SignedOrder {
    /// Who signed this order, on this network.
    pub fn signed_by(&self, network: &Hash32) -> Option<SignedBy> {
        self.signature.signed_by(&self.order.digest(network))
    }
}

/// One validator's signature over a batch that contains some item. The
/// validator signed the whole batch at once; `batch_path` ties the item to
/// that signature. For a vote the item is an order's digest.
#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct Vote {
    pub validator: u16,
    pub batch_path: merkle::Path,
    pub signature: TreeSignature,
}

/// A validator's statement that it has recorded an order's certificate. It
/// has the shape of a vote; the item signed is `confirmation_item`.
pub type Confirmation = Vote;

/// What a validator signs to confirm the order with this digest. Tagged
/// apart from the digest itself, so a vote can never pass for a
/// confirmation or the other way round.
pub fn confirmation_item(order_digest: &Hash32) -> Hash32 {
    hash(Tag::Confirmation, &[order_digest.as_bytes()])
}

/// Votes for one order, all cast in one epoch, from validators holding more
/// than two thirds of the stake. No other order under the same account and
/// number can gather that many in the same epoch.
///
/// A certificate is not yet proof that the payment is final. That takes a
/// `Receipt`.
#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct Certificate {
    pub epoch: u64,
    pub order: Order,
    /// The owner's signature on the order. Kept so that the order can be
    /// voted on again if a checkpoint leaves this certificate out. Votes do
    /// not cover it: a validator checks it against the account's key before
    /// it applies the certificate.
    pub signature: Signature,
    pub votes: Vec<Vote>,
}

impl Certificate {
    pub fn new(epoch: u64, signed: &SignedOrder, votes: Vec<Vote>) -> Certificate {
        Certificate { epoch, order: signed.order.clone(), signature: signed.signature.clone(), votes }
    }

    pub fn signed(&self) -> SignedOrder {
        SignedOrder { order: self.order.clone(), signature: self.signature.clone() }
    }
}

/// Proof that a payment is final: its certificate, and confirmations from
/// validators holding more than two thirds of the stake that they recorded
/// it, in the same epoch. Anyone can check it knowing only who the
/// validators of that epoch were.
#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct Receipt {
    pub certificate: Certificate,
    pub confirmations: Vec<Confirmation>,
}
