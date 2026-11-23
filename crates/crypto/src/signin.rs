//! Signing in to websites with an Orien wallet.
//!
//! A website asks the wallet to sign a short message: the site's own
//! origin, the wallet's address, a one-time value (nonce) the site chose,
//! and when it was signed and stops being good. The site checks the proof
//! itself, with nothing but this code: no server, no network.
//!
//! The wallet's spending keys can't do this: each signs once and the next
//! payment needs the next one. So every wallet also has a sign-in key, a
//! tree of trees of one-time keys made from its seed:
//!
//! - three upper levels of 256 one-time keys each, one tree at the top,
//!   then 256 below it, then 65,536 below those, and under each of those
//!   keys a bottom tree of 1,024 keys that sign the sign-in messages;
//! - each upper key signs only the root of the tree below it, always the
//!   same message, so any number of devices may repeat it safely;
//! - each device picks one of the 16,777,216 bottom trees at random and
//!   counts its keys, so devices sharing a wallet never share a key.
//!
//! The top root is tied to the address by the wallet's last spare key
//! (number 15 of 16), which signs it once, always the same message. The
//! address commits to the spare keys and the first key, so a site checks:
//! the message signature leads up to the top root, the spare signature
//! over that root leads to the spare root, and the first key's
//! commitment with that spare root hashes to the address.

use crate::hash::{Hash32, Kdf, Tag, hash, prf};
use crate::keytree::{self, KeyTree, TreeSignature};
use borsh::{BorshDeserialize, BorshSerialize};

/// Height of each upper tree (256 keys).
pub const UPPER_HEIGHT: u8 = 8;
/// Upper levels above the bottom trees.
pub const LEVELS: usize = 3;
/// Height of each bottom tree (1,024 sign-ins).
pub const BOTTOM_HEIGHT: u8 = 10;
/// Height of an account's spare key tree (as in the ledger).
pub const SPARE_HEIGHT: u8 = 4;
/// The spare key that ties the sign-in key to the address.
pub const BINDING_SPARE: u32 = 15;
/// Bottom trees to choose from: 256^3.
pub const SLOTS: u32 = 1 << 24;
/// The longest a sign-in may stay good, in seconds.
pub const MOST_SECONDS: u64 = 600;
/// How far a signer's clock may run ahead of the site's, in seconds.
pub const CLOCK_SLACK: u64 = 120;

/// What the wallet signs.
#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct Message {
    /// The site, as the browser reports it: `https://example.com`.
    pub origin: String,
    /// The wallet signing in.
    pub address: Hash32,
    /// The site's one-time value.
    pub nonce: String,
    /// When it was signed and when it stops being good, in Unix seconds.
    pub issued: u64,
    pub expires: u64,
}

impl Message {
    /// The bytes the bottom key signs.
    pub fn digest(&self) -> Hash32 {
        hash(Tag::SignIn, &[&borsh::to_vec(self).expect("a message always encodes")])
    }
}

/// A signed sign-in, as the wallet hands it to the site.
#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct Proof {
    pub message: Message,
    /// The bottom key's signature on the message.
    pub bottom: TreeSignature,
    /// The upper keys' signatures, from the level just above the bottom
    /// tree to the top.
    pub upper: [TreeSignature; LEVELS],
    /// The binding spare key's signature on the top root.
    pub spare: TreeSignature,
    /// The commitment to the account's first key, which the address holds.
    pub first_key: Hash32,
}

/// Why a proof isn't accepted.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Refused {
    /// The signatures don't lead to the address: forged or damaged.
    Signature,
    /// Signed for another website.
    Origin,
    /// Not the one-time value the site gave.
    Nonce,
    /// Expired, signed too far ahead, or good for too long.
    Time,
}

impl std::fmt::Display for Refused {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Refused::Signature => "the signature doesn't match the wallet",
            Refused::Origin => "it was signed for another website",
            Refused::Nonce => "it isn't the sign-in this website asked for",
            Refused::Time => "it has expired",
        })
    }
}

/// What an upper key signs: the root of the tree below it, with its level
/// and number, so no root can be passed off for another.
pub fn tree_message(level: u8, index: u32, root: &Hash32) -> Hash32 {
    hash(Tag::SignInTree, &[&[level], &index.to_le_bytes(), root.as_bytes()])
}

/// What the binding spare key signs: the top root.
pub fn key_message(top: &Hash32) -> Hash32 {
    hash(Tag::SignInKey, &[top.as_bytes()])
}

/// The address of an account, as the ledger computes it.
pub fn address(first_key: &Hash32, spare_root: &Hash32) -> Hash32 {
    hash(Tag::Address, &[first_key.as_bytes(), spare_root.as_bytes()])
}

/// The wallet that signed `proof`, if it is good for this website, this
/// one-time value and this moment (Unix seconds). The site must also make
/// sure it accepts each one-time value only once.
pub fn verify(proof: &Proof, origin: &str, nonce: &str, now: u64) -> Result<Hash32, Refused> {
    let m = &proof.message;
    if m.origin != origin {
        return Err(Refused::Origin);
    }
    if m.nonce != nonce {
        return Err(Refused::Nonce);
    }
    if m.expires <= m.issued || m.expires - m.issued > MOST_SECONDS || now >= m.expires || m.issued > now + CLOCK_SLACK {
        return Err(Refused::Time);
    }
    if top_root(proof).and_then(|top| bound_address(proof, &top)) != Some(m.address) {
        return Err(Refused::Signature);
    }
    Ok(m.address)
}

/// The top root the message signature leads to.
fn top_root(proof: &Proof) -> Option<Hash32> {
    let mut root = keytree::recover_root(BOTTOM_HEIGHT, &proof.bottom, proof.message.digest().as_bytes())?;
    // The bottom tree's number, from the leaves used above it: top first.
    let [low, middle, top] = &proof.upper;
    let indices = [(top.leaf << 16) | (middle.leaf << 8) | low.leaf, (top.leaf << 8) | middle.leaf, top.leaf];
    for (level, signature) in proof.upper.iter().enumerate() {
        root = keytree::recover_root(UPPER_HEIGHT, signature, tree_message(level as u8, indices[level], &root).as_bytes())?;
    }
    Some(root)
}

/// The address the binding ties this top root to.
fn bound_address(proof: &Proof, top: &Hash32) -> Option<Hash32> {
    if proof.spare.leaf != BINDING_SPARE {
        return None;
    }
    let spare_root = keytree::recover_root(SPARE_HEIGHT, &proof.spare, key_message(top).as_bytes())?;
    Some(address(&proof.first_key, &spare_root))
}

/// The signing side: a wallet's sign-in key, made from its seed.
pub struct SignInKey {
    seed: [u8; 32],
}

impl SignInKey {
    /// From the wallet's seed.
    pub fn from_wallet_seed(wallet_seed: &[u8; 32]) -> Self {
        SignInKey { seed: prf(wallet_seed, Kdf::WalletSignIn, 0) }
    }

    /// The tree at `level` (0 just above the bottom trees, 2 the top) with
    /// this number; level `u8::MAX` is a bottom tree.
    fn tree(&self, level: u8, index: u32) -> KeyTree {
        let height = if level == u8::MAX { BOTTOM_HEIGHT } else { UPPER_HEIGHT };
        KeyTree::generate(prf(&self.seed, Kdf::SignInTree, ((level as u64) << 32) | index as u64), height)
    }

    /// The top root, which the binding spare key signs.
    pub fn top(&self) -> Hash32 {
        self.tree(LEVELS as u8 - 1, 0).root()
    }

    /// Signs `message` with key `leaf` of bottom tree `slot`. The caller
    /// must use each (slot, leaf) once, ever, and pass the binding the
    /// wallet's spare key made over `key_message(top())`.
    pub fn sign(&self, slot: u32, leaf: u32, message: Message, spare: TreeSignature, first_key: Hash32) -> Proof {
        assert!(slot < SLOTS && leaf < 1 << BOTTOM_HEIGHT);
        let bottom_tree = self.tree(u8::MAX, slot);
        let bottom = bottom_tree.sign_with_leaf(leaf, message.digest().as_bytes());
        let (a, b, c) = (slot >> 16, (slot >> 8) & 0xff, slot & 0xff);
        // Level 0 tree (a, b) signs bottom tree slot with key c; level 1
        // tree a signs level 0 tree (a, b) with key b; the top signs level 1
        // tree a with key a.
        let level0 = self.tree(0, (a << 8) | b);
        let level1 = self.tree(1, a);
        let level2 = self.tree(2, 0);
        let upper = [
            level0.sign_with_leaf(c, tree_message(0, slot, &bottom_tree.root()).as_bytes()),
            level1.sign_with_leaf(b, tree_message(1, (a << 8) | b, &level0.root()).as_bytes()),
            level2.sign_with_leaf(a, tree_message(2, a, &level1.root()).as_bytes()),
        ];
        Proof { message, bottom, upper, spare, first_key }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A stand-in wallet: its first key, spare tree and sign-in key.
    struct Wallet {
        first_key: Hash32,
        spares: KeyTree,
        key: SignInKey,
    }

    impl Wallet {
        fn new(seed: u8) -> Wallet {
            let seed = [seed; 32];
            Wallet {
                first_key: crate::ots::SecretKey::from_seed(prf(&seed, Kdf::WalletKey, 0)).commitment(),
                spares: KeyTree::generate(prf(&seed, Kdf::WalletSpare, 0), SPARE_HEIGHT),
                key: SignInKey::from_wallet_seed(&seed),
            }
        }
        fn address(&self) -> Hash32 {
            address(&self.first_key, &self.spares.root())
        }
        fn proof(&self, slot: u32, leaf: u32, origin: &str, nonce: &str, issued: u64) -> Proof {
            let message = Message { origin: origin.into(), address: self.address(), nonce: nonce.into(), issued, expires: issued + 300 };
            let binding = self.spares.sign_with_leaf(BINDING_SPARE, key_message(&self.key.top()).as_bytes());
            self.key.sign(slot, leaf, message, binding, self.first_key)
        }
    }

    const SITE: &str = "https://example.com";

    #[test]
    fn a_good_sign_in_names_its_wallet() {
        let wallet = Wallet::new(7);
        for (slot, leaf) in [(0, 0), (0x00ab_cdef, 1023), (SLOTS - 1, 5)] {
            let proof = wallet.proof(slot, leaf, SITE, "n-1", 1_000);
            assert_eq!(verify(&proof, SITE, "n-1", 1_010), Ok(wallet.address()));
        }
    }

    #[test]
    fn the_wrong_site_nonce_or_time_is_refused() {
        let wallet = Wallet::new(7);
        let proof = wallet.proof(3, 4, SITE, "n-1", 1_000);
        assert_eq!(verify(&proof, "https://evil.example", "n-1", 1_010), Err(Refused::Origin));
        assert_eq!(verify(&proof, SITE, "n-2", 1_010), Err(Refused::Nonce));
        assert_eq!(verify(&proof, SITE, "n-1", 1_300), Err(Refused::Time));
        assert_eq!(verify(&proof, SITE, "n-1", 1_000 - CLOCK_SLACK - 1), Err(Refused::Time));
    }

    #[test]
    fn changed_or_borrowed_parts_are_refused() {
        let wallet = Wallet::new(7);
        let other = Wallet::new(8);
        let good = wallet.proof(3, 4, SITE, "n-1", 1_000);

        // Claiming another wallet's address.
        let mut claimed = good.clone();
        claimed.message.address = other.address();
        assert_eq!(verify(&claimed, SITE, "n-1", 1_010), Err(Refused::Signature));

        // Another wallet's binding, or another spare key.
        let mut bound = good.clone();
        bound.spare = other.proof(3, 4, SITE, "n-1", 1_000).spare;
        assert_eq!(verify(&bound, SITE, "n-1", 1_010), Err(Refused::Signature));
        let mut spare = good.clone();
        spare.spare = wallet.spares.sign_with_leaf(3, key_message(&wallet.key.top()).as_bytes());
        assert_eq!(verify(&spare, SITE, "n-1", 1_010), Err(Refused::Signature));

        // A message changed after signing.
        let mut longer = good.clone();
        longer.message.expires += 60;
        assert_eq!(verify(&longer, SITE, "n-1", 1_010), Err(Refused::Signature));

        // An upper signature moved to another slot.
        let mut moved = good.clone();
        moved.upper[0].leaf ^= 1;
        assert_eq!(verify(&moved, SITE, "n-1", 1_010), Err(Refused::Signature));
    }

    #[test]
    fn the_address_matches_the_ledgers() {
        // The ledger's own formula, restated: hash(Address, first || spare).
        let wallet = Wallet::new(9);
        assert_eq!(wallet.address(), hash(Tag::Address, &[wallet.first_key.as_bytes(), wallet.spares.root().as_bytes()]));
    }

    /// Writes proofs for the JavaScript checker's test
    /// (apps/wallet/signin-js/test.js): ORIEN_SIGNIN_VECTORS=file cargo test
    /// -p orien-crypto write_vectors -- --ignored
    #[test]
    #[ignore]
    fn write_vectors() {
        let path = std::env::var("ORIEN_SIGNIN_VECTORS").expect("ORIEN_SIGNIN_VECTORS");
        let wallet = Wallet::new(7);
        let encode = |proof: &Proof| hex::encode(borsh::to_vec(proof).unwrap());
        let good = wallet.proof(0x00ab_cdef, 513, SITE, "n-1", 1_000);
        let mut forged = good.clone();
        forged.message.address = Wallet::new(8).address();
        let long_site = "https://a-very-long-website-name-that-makes-the-message-longer.example.com";
        let long = wallet.proof(5, 6, long_site, &"x".repeat(200), 1_000);
        let vectors = serde_json::json!({
            "address": wallet.address().to_string(),
            "good": encode(&good),
            "forged": encode(&forged),
            "long": encode(&long),
            "long_site": long_site,
            "blake3": [
                ["", hex::encode(blake3::hash(b"").as_bytes())],
                [hex::encode([7u8; 1249]), hex::encode(blake3::hash(&[7u8; 1249]).as_bytes())],
                [hex::encode(vec![9u8; 5000]), hex::encode(blake3::hash(&[9u8; 5000]).as_bytes())],
            ],
        });
        std::fs::write(path, vectors.to_string()).unwrap();
    }

    #[test]
    fn a_proof_survives_encoding() {
        let wallet = Wallet::new(7);
        let proof = wallet.proof(1, 2, SITE, "n-1", 1_000);
        let bytes = borsh::to_vec(&proof).unwrap();
        assert_eq!(Proof::try_from_slice(&bytes).unwrap(), proof);
    }
}
