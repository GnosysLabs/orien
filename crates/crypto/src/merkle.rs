//! Merkle trees over batches. A validator signs one root and every item in
//! the batch gets a short path proving it was included under that signature.

use crate::hash::{Hash32, Tag, hash};
use borsh::{BorshDeserialize, BorshSerialize};

pub const MAX_DEPTH: usize = 32;

/// Proof that an item sits at `index` under some root.
#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct Path {
    pub index: u32,
    pub siblings: Vec<Hash32>,
}

fn leaf(item: &Hash32) -> Hash32 {
    hash(Tag::BatchLeaf, &[item.as_bytes()])
}

fn node(left: &Hash32, right: &Hash32) -> Hash32 {
    hash(Tag::BatchNode, &[left.as_bytes(), right.as_bytes()])
}

fn padding() -> Hash32 {
    hash(Tag::BatchPadding, &[])
}

/// A complete binary tree. Batches are padded to a power of two with a value
/// no real item can hash to.
pub struct Tree {
    levels: Vec<Vec<Hash32>>,
}

impl Tree {
    pub fn build(items: &[Hash32]) -> Tree {
        assert!(!items.is_empty(), "a batch holds at least one item");
        let mut level: Vec<Hash32> = items.iter().map(leaf).collect();
        level.resize(items.len().next_power_of_two(), padding());
        let mut levels = vec![level];
        while levels[levels.len() - 1].len() > 1 {
            let below = &levels[levels.len() - 1];
            let above = below.chunks_exact(2).map(|pair| node(&pair[0], &pair[1])).collect();
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

/// The root implied by `item` and its path, or `None` if the path is malformed.
pub fn root_from_path(item: &Hash32, path: &Path) -> Option<Hash32> {
    let depth = path.siblings.len();
    if depth > MAX_DEPTH || (path.index as u64) >> depth != 0 {
        return None;
    }
    let mut current = leaf(item);
    let mut position = path.index;
    for sibling in &path.siblings {
        current = if position & 1 == 0 { node(&current, sibling) } else { node(sibling, &current) };
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
        assert_ne!(root_from_path(&padding(), &path), Some(tree.root()));
    }
}
