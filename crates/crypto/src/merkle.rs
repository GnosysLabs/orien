//! Merkle trees. A validator signs one root over a batch and every item in
//! the batch gets a short path proving it was included under that signature.
//! The same construction, under different tags, gives a root over every
//! account and a short proof of any one of them.

use crate::hash::{Hash32, Tag, hash};
use borsh::{BorshDeserialize, BorshSerialize};

pub const MAX_DEPTH: usize = 32;

/// Proof that an item sits at `index` under some root.
#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct Path {
    pub index: u32,
    pub siblings: Vec<Hash32>,
}

/// What a tree is built over. Each kind hashes with its own tags, so a path
/// through one kind of tree proves nothing about another.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// The orders a validator signs together.
    Batch,
    /// Every account in the ledger.
    State,
}

impl Kind {
    fn leaf(self, item: &Hash32) -> Hash32 {
        let tag = match self {
            Kind::Batch => Tag::BatchLeaf,
            Kind::State => Tag::StateLeaf,
        };
        hash(tag, &[item.as_bytes()])
    }

    fn node(self, left: &Hash32, right: &Hash32) -> Hash32 {
        let tag = match self {
            Kind::Batch => Tag::BatchNode,
            Kind::State => Tag::StateNode,
        };
        hash(tag, &[left.as_bytes(), right.as_bytes()])
    }

    fn padding(self) -> Hash32 {
        let tag = match self {
            Kind::Batch => Tag::BatchPadding,
            Kind::State => Tag::StatePadding,
        };
        hash(tag, &[])
    }
}

/// A complete binary tree. Items are padded to a power of two with a value
/// no real item can hash to.
pub struct Tree {
    levels: Vec<Vec<Hash32>>,
}

impl Tree {
    /// A tree over one batch of orders.
    pub fn build(items: &[Hash32]) -> Tree {
        assert!(!items.is_empty(), "a batch holds at least one item");
        Tree::of(Kind::Batch, items)
    }

    /// A tree of the given kind. With no items its root is the padding value.
    pub fn of(kind: Kind, items: &[Hash32]) -> Tree {
        let mut level: Vec<Hash32> = items.iter().map(|item| kind.leaf(item)).collect();
        level.resize(items.len().next_power_of_two(), kind.padding());
        let mut levels = vec![level];
        while levels[levels.len() - 1].len() > 1 {
            let below = &levels[levels.len() - 1];
            let above = below.chunks_exact(2).map(|pair| kind.node(&pair[0], &pair[1])).collect();
            levels.push(above);
        }
        Tree { levels }
    }

    pub fn root(&self) -> Hash32 {
        self.levels[self.levels.len() - 1][0]
    }

    pub fn path(&self, index: usize) -> Path {
        let mut siblings = Vec::with_capacity(self.levels.len() - 1);
        let mut position = index;
        for level in &self.levels[..self.levels.len() - 1] {
            siblings.push(level[position ^ 1]);
            position >>= 1;
        }
        Path { index: index as u32, siblings }
    }
}

/// The batch root implied by `item` and its path, or `None` if the path is
/// malformed.
pub fn root_from_path(item: &Hash32, path: &Path) -> Option<Hash32> {
    root_from_path_in(Kind::Batch, item, path)
}

/// The root of a tree of the given kind implied by `item` and its path.
pub fn root_from_path_in(kind: Kind, item: &Hash32, path: &Path) -> Option<Hash32> {
    let depth = path.siblings.len();
    if depth > MAX_DEPTH || (path.index as u64) >> depth != 0 {
        return None;
    }
    let mut current = kind.leaf(item);
    let mut position = path.index;
    for sibling in &path.siblings {
        current = if position & 1 == 0 {
            kind.node(&current, sibling)
        } else {
            kind.node(sibling, &current)
        };
        position >>= 1;
    }
    Some(current)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn items(count: usize) -> Vec<Hash32> {
        (0..count).map(|i| hash(Tag::Order, &[&(i as u64).to_le_bytes()])).collect()
    }

    #[test]
    fn every_item_proves_inclusion() {
        for count in [1, 2, 3, 4, 5, 7, 8, 9, 17, 100] {
            let items = items(count);
            let tree = Tree::build(&items);
            for (i, item) in items.iter().enumerate() {
                assert_eq!(root_from_path(item, &tree.path(i)), Some(tree.root()), "{count}/{i}");
            }
        }
    }

    #[test]
    fn wrong_item_or_position_fails() {
        let items = items(6);
        let tree = Tree::build(&items);
        let path = tree.path(2);
        assert_ne!(root_from_path(&items[3], &path), Some(tree.root()));
        let moved = Path { index: 3, siblings: path.siblings.clone() };
        assert_ne!(root_from_path(&items[2], &moved), Some(tree.root()));
    }

    #[test]
    fn malformed_paths_are_rejected() {
        let item = items(1)[0];
        assert_eq!(root_from_path(&item, &Path { index: 1, siblings: vec![] }), None);
        assert_eq!(root_from_path(&item, &Path { index: 4, siblings: vec![item; 2] }), None);
        assert_eq!(root_from_path(&item, &Path { index: 0, siblings: vec![item; 33] }), None);
    }

    #[test]
    fn padding_cannot_pose_as_an_item() {
        let items = items(3);
        let tree = Tree::build(&items);
        let path = tree.path(3);
        assert_ne!(root_from_path(&Kind::Batch.padding(), &path), Some(tree.root()));
    }

    #[test]
    fn a_path_in_one_kind_of_tree_proves_nothing_in_another() {
        let items = items(5);
        let (batch, state) = (Tree::of(Kind::Batch, &items), Tree::of(Kind::State, &items));
        assert_ne!(batch.root(), state.root());
        let path = state.path(1);
        assert_eq!(root_from_path_in(Kind::State, &items[1], &path), Some(state.root()));
        assert_ne!(root_from_path_in(Kind::Batch, &items[1], &path), Some(state.root()));
        // A tree over nothing still has a root.
        assert_eq!(Tree::of(Kind::State, &[]).root(), Kind::State.padding());
    }
}
