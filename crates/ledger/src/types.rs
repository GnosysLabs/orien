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

/// One validator's approval of an order. The validator signed a whole batch
/// at once; `batch_path` ties this order to that signature.
#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct Vote {
    pub validator: u16,
    pub batch_path: merkle::Path,
    pub signature: TreeSignature,
}

/// Proof that an order is final: votes from validators holding more than two
/// thirds of the stake. Anyone can check it with the genesis file alone.
#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct Certificate {
    pub order: Order,
    pub votes: Vec<Vote>,
}
