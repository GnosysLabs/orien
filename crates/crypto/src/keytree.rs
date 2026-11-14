//! A validator's signing key: a Merkle tree whose leaves are one-time keys.
//!
//! The root is the validator's public key. Each signature uses up one leaf
//! and carries the path from that leaf to the root. The tree is generated
//! from a seed, so only the seed has to be stored.

use crate::hash::{Hash32, Kdf, Tag, hash, prf};
use crate::ots;
use borsh::{BorshDeserialize, BorshSerialize};
use std::thread;

/// Tallest tree supported: 2^24 one-time keys.
pub const MAX_HEIGHT: u8 = 24;

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct TreeSignature {
    pub leaf: u32,
    pub ots: ots::Signature,
    pub path: Vec<Hash32>,
}

pub struct KeyTree {
    seed: [u8; 32],
    height: u8,
    /// Heap layout: `nodes[1]` is the root and the children of `nodes[i]`
    /// are `nodes[2i]` and `nodes[2i + 1]`. Leaves fill the upper half.
    nodes: Vec<Hash32>,
}

fn leaf_key(seed: &[u8; 32], leaf: u32) -> ots::SecretKey {
    ots::SecretKey::from_seed(prf(seed, Kdf::TreeLeaf, leaf as u64))
}

fn node(left: &Hash32, right: &Hash32) -> Hash32 {
    hash(Tag::KeyTreeNode, &[left.as_bytes(), right.as_bytes()])
}

impl KeyTree {
    /// Builds the whole tree from `seed`, using every available core.
    pub fn generate(seed: [u8; 32], height: u8) -> KeyTree {
        assert!(height <= MAX_HEIGHT, "key tree height above {MAX_HEIGHT}");
        let leaves = 1usize << height;
        let mut nodes = vec![Hash32::default(); 2 * leaves];
        let workers = thread::available_parallelism().map_or(1, |n| n.get()).min(leaves);
        let chunk = leaves.div_ceil(workers);
        thread::scope(|scope| {
            for (c, slots) in nodes[leaves..].chunks_mut(chunk).enumerate() {
                scope.spawn(move || {
                    for (j, slot) in slots.iter_mut().enumerate() {
                        *slot = leaf_key(&seed, (c * chunk + j) as u32).commitment();
                    }
                });
            }
        });
        for i in (1..leaves).rev() {
            nodes[i] = node(&nodes[2 * i], &nodes[2 * i + 1]);
        }
        KeyTree { seed, height, nodes }
    }

    pub fn root(&self) -> Hash32 {
        self.nodes[1]
    }

    pub fn height(&self) -> u8 {
        self.height
    }

    /// Number of signatures this key can ever make.
    pub fn capacity(&self) -> u32 {
        1u32 << self.height
    }

    /// Signs `message` with one-time key number `leaf`.
    ///
    /// The caller must guarantee that each leaf signs at most once, ever,
    /// including across restarts. Reusing a leaf for a different message
    /// lets others forge this validator's signature.
    pub fn sign_with_leaf(&self, leaf: u32, message: &[u8]) -> TreeSignature {
        assert!(leaf < self.capacity(), "one-time keys exhausted");
        let leaves = 1usize << self.height;
        let mut path = Vec::with_capacity(self.height as usize);
        let mut position = leaves + leaf as usize;
        while position > 1 {
            path.push(self.nodes[position ^ 1]);
            position >>= 1;
        }
        TreeSignature { leaf, ots: leaf_key(&self.seed, leaf).sign(message), path }
    }
}

/// Checks `signature` on `message` against a validator's root.
pub fn verify(root: &Hash32, height: u8, signature: &TreeSignature, message: &[u8]) -> bool {
    if signature.path.len() != height as usize || (signature.leaf as u64) >> height != 0 {
        return false;
    }
    let mut current = ots::recover(&signature.ots, message);
    let mut position = signature.leaf;
    for sibling in &signature.path {
        current = if position & 1 == 0 { node(&current, sibling) } else { node(sibling, &current) };
        position >>= 1;
    }
    current == *root
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_leaf_signs_and_verifies() {
        let tree = KeyTree::generate([1; 32], 4);
        for leaf in 0..tree.capacity() {
            let signature = tree.sign_with_leaf(leaf, b"batch");
            assert!(verify(&tree.root(), 4, &signature, b"batch"));
            assert!(!verify(&tree.root(), 4, &signature, b"other"));
        }
    }

    #[test]
    fn generation_is_deterministic() {
        assert_eq!(KeyTree::generate([2; 32], 5).root(), KeyTree::generate([2; 32], 5).root());
        assert_ne!(KeyTree::generate([2; 32], 5).root(), KeyTree::generate([3; 32], 5).root());
    }

    #[test]
    fn signatures_do_not_transfer_between_validators() {
        let ours = KeyTree::generate([1; 32], 3);
        let theirs = KeyTree::generate([2; 32], 3);
        let signature = theirs.sign_with_leaf(0, b"batch");
        assert!(!verify(&ours.root(), 3, &signature, b"batch"));
    }

    #[test]
    fn a_moved_or_truncated_signature_fails() {
        let tree = KeyTree::generate([1; 32], 3);
        let signature = tree.sign_with_leaf(5, b"batch");
        let mut moved = signature.clone();
        moved.leaf = 4;
        assert!(!verify(&tree.root(), 3, &moved, b"batch"));
        let mut out_of_range = signature.clone();
        out_of_range.leaf = 5 + 8;
        assert!(!verify(&tree.root(), 3, &out_of_range, b"batch"));
        let mut short = signature.clone();
        short.path.pop();
        assert!(!verify(&tree.root(), 3, &short, b"batch"));
        assert!(!verify(&tree.root(), 2, &short, b"batch"));
    }

    #[test]
    fn a_single_key_tree_works() {
        let tree = KeyTree::generate([8; 32], 0);
        let signature = tree.sign_with_leaf(0, b"only");
        assert!(signature.path.is_empty());
        assert!(verify(&tree.root(), 0, &signature, b"only"));
    }
}
