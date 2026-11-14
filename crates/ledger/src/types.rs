use borsh::{BorshDeserialize, BorshSerialize};
use orien_crypto::keytree::TreeSignature;
use orien_crypto::{Hash32, Tag, hash, merkle, ots};

/// An account is named by the commitment to its first one-time key.
pub type AccountId = Hash32;

/// "Account `from`, payment number `seq`: send `amount` to `to`."
#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct Order {
    pub from: AccountId,
    /// The sender's own payment counter, starting at 0. It is also the index
    /// of the one-time key that signs this order.
    pub seq: u64,
    pub to: AccountId,
    pub amount: u64,
    /// Commitment to the one-time key that will sign payment `seq + 1`.
    pub next_key: Hash32,
}

impl Order {
    /// What gets signed. Bound to one network so an order cannot be replayed
    /// on another.
    pub fn digest(&self, network: &Hash32) -> Hash32 {
        let bytes = borsh::to_vec(self).expect("serializing to memory cannot fail");
        hash(Tag::Order, &[network.as_bytes(), &bytes])
    }
}

/// An order with its owner's one-time signature.
#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct SignedOrder {
    pub order: Order,
    pub signature: ots::Signature,
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
    pub signature: ots::Signature,
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
