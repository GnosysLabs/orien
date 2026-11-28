//! Orien's buy orders that fill in parts (`programs/orien-book`) on Solana:
//! the secrets' Merkle tree, account decoding and instruction builders,
//! shared by Orien Wallet and the Market server.

use crate::solana::{Account, Instruction, Pubkey, allocate, assign, transfer};
use bitcoin::hashes::{Hash, sha256};

/// The orien-book account's size in bytes.
pub const BOOK_LEN: u64 = 585;
/// Secrets an order commits to: parts it can ever have.
pub const SECRETS: usize = 64;
/// Levels of the secrets' Merkle tree.
pub const DEPTH: usize = 6;
/// Parts in progress at once.
pub const SLOTS: usize = 4;
/// The smallest part, in lamports: 0.01 SOL, as for any SOL swap.
pub const MIN_PART: u64 = crate::sol::MIN_LAMPORTS;

/// orien-book on Solana devnet, for testing.
pub const DEVNET_BOOK_PROGRAM: &str = "5CjV4BhYX4kEydMEmKgX75fiAo8etX4mUxQfyheM6tDP";
/// orien-book on Solana mainnet: none until it's deployed and made final.
pub const MAINNET_BOOK_PROGRAM: Option<&str> = Some("91kXkHE87VAvHr52oKrXcMyycSc1jkyYrSgWLMAuMCdD");

fn sha(parts: &[&[u8]]) -> [u8; 32] {
    let mut all = Vec::with_capacity(parts.iter().map(|p| p.len()).sum());
    for part in parts {
        all.extend_from_slice(part);
    }
    sha256::Hash::hash(&all).to_byte_array()
}

/// The hash a part's secret proves.
pub fn secret_hash(secret: &[u8; 32]) -> [u8; 32] {
    sha(&[secret])
}

/// An order's 64 secret hashes and the Merkle tree over them, as the
/// program checks it: a node is SHA-256 of 0x01, left, right.
pub struct Secrets {
    levels: Vec<Vec<[u8; 32]>>,
}

impl Secrets {
    /// From the 64 hashes (the leaves), in order.
    pub fn from_hashes(hashes: &[[u8; 32]]) -> Option<Secrets> {
        if hashes.len() != SECRETS {
            return None;
        }
        let mut levels = vec![hashes.to_vec()];
        while levels.last().map_or(0, Vec::len) > 1 {
            let below = levels.last().expect("a level");
            levels.push(below.chunks(2).map(|pair| sha(&[&[1u8], &pair[0], &pair[1]])).collect());
        }
        Some(Secrets { levels })
    }

    /// From the 64 secrets.
    pub fn from_secrets(secrets: &[[u8; 32]]) -> Option<Secrets> {
        let hashes: Vec<[u8; 32]> = secrets.iter().map(secret_hash).collect();
        Secrets::from_hashes(&hashes)
    }

    pub fn root(&self) -> [u8; 32] {
        self.levels.last().expect("a root")[0]
    }

    pub fn hash(&self, index: usize) -> [u8; 32] {
        self.levels[0][index]
    }

    /// The sibling at each level, from the leaves up.
    pub fn proof(&self, index: usize) -> [[u8; 32]; DEPTH] {
        let mut at = index;
        std::array::from_fn(|level| {
            let sibling = self.levels[level][at ^ 1];
            at >>= 1;
            sibling
        })
    }
}

/// The root `hash` at `index` proves to with `proof`, as the program checks.
pub fn root_of(hash: &[u8; 32], index: usize, proof: &[[u8; 32]; DEPTH]) -> [u8; 32] {
    let mut node = *hash;
    let mut at = index;
    for sibling in proof {
        node = if at & 1 == 0 { sha(&[&[1u8], &node, sibling]) } else { sha(&[&[1u8], sibling, &node]) };
        at >>= 1;
    }
    node
}

/// Written by `Open`, never changed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BookTerms {
    pub root: [u8; 32],
    /// Where SOL goes back to: refunds and rent.
    pub buyer: Pubkey,
    /// Who may cancel.
    pub authority: Pubkey,
    /// Who may fill: the Market's relayer.
    pub filler: Pubkey,
    /// The buyer's Orien account, which the ORN are locked for.
    pub buyer_orien: [u8; 32],
    pub orn_total: u64,
    pub lamports_total: u64,
    pub expiry: i64,
    pub max_fill_window: i64,
    pub min_part: u64,
}

/// A part in progress.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Part {
    pub slot: u8,
    pub index: u8,
    pub hash: [u8; 32],
    pub seller: Pubkey,
    pub orn: u64,
    pub lamports: u64,
    pub deadline: i64,
}

/// A decoded orien-book account.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BookAccount {
    pub terms: BookTerms,
    pub orn_left: u64,
    pub lamports_left: u64,
    pub used: u64,
    pub parts: Vec<Part>,
}

impl BookAccount {
    /// Decodes an open order owned by `program`.
    pub fn read(account: &Account, program: &Pubkey) -> Option<BookAccount> {
        if &account.owner != program || account.data.len() != BOOK_LEN as usize || account.data[0] != 1 {
            return None;
        }
        let d = &account.data;
        let key = |at: usize| -> [u8; 32] { d[at..at + 32].try_into().expect("32 bytes") };
        let u64le = |at: usize| u64::from_le_bytes(d[at..at + 8].try_into().expect("8 bytes"));
        let i64le = |at: usize| i64::from_le_bytes(d[at..at + 8].try_into().expect("8 bytes"));
        let parts = (0..SLOTS)
            .filter_map(|slot| {
                let at = 225 + slot * 90;
                (d[at] == 1).then(|| Part {
                    slot: slot as u8,
                    index: d[at + 1],
                    hash: key(at + 2),
                    seller: key(at + 34),
                    orn: u64le(at + 66),
                    lamports: u64le(at + 74),
                    deadline: i64le(at + 82),
                })
            })
            .collect();
        Some(BookAccount {
            terms: BookTerms {
                root: key(1),
                buyer: key(33),
                authority: key(65),
                filler: key(97),
                buyer_orien: key(129),
                orn_total: u64le(161),
                lamports_total: u64le(169),
                expiry: i64le(177),
                max_fill_window: i64le(185),
                min_part: u64le(193),
            },
            orn_left: u64le(201),
            lamports_left: u64le(209),
            used: u64le(217),
            parts,
        })
    }

    /// The lamports a part of `orn` would take, as the program works it out,
    /// or None when the program would refuse that size.
    pub fn share(&self, orn: u64) -> Option<u64> {
        if orn == 0 || orn > self.orn_left {
            return None;
        }
        if orn == self.orn_left {
            return Some(self.lamports_left);
        }
        let share = (self.terms.lamports_total as u128 * orn as u128 / self.terms.orn_total as u128) as u64;
        (share >= self.terms.min_part && self.lamports_left.checked_sub(share)? >= self.terms.min_part).then_some(share)
    }

    /// A secret number never used yet, if any are left.
    pub fn unused(&self) -> Option<usize> {
        (0..SECRETS).find(|i| self.used & (1u64 << i) == 0)
    }
}

/// Tag 0 (`Open`): funds `order` with `terms.lamports_total + deposit`,
/// allocates it, gives it to `program`, and writes the terms. Transfer,
/// allocate and assign (not create_account), so stray dust at the order's
/// address can never block opening.
pub fn open(program: &Pubkey, payer: &Pubkey, order: &Pubkey, terms: &BookTerms, deposit: u64) -> Vec<Instruction> {
    let mut data = Vec::with_capacity(201);
    data.push(0);
    data.extend_from_slice(&terms.root);
    data.extend_from_slice(&terms.buyer);
    data.extend_from_slice(&terms.authority);
    data.extend_from_slice(&terms.filler);
    data.extend_from_slice(&terms.buyer_orien);
    data.extend_from_slice(&terms.orn_total.to_le_bytes());
    data.extend_from_slice(&terms.lamports_total.to_le_bytes());
    data.extend_from_slice(&terms.expiry.to_le_bytes());
    data.extend_from_slice(&terms.max_fill_window.to_le_bytes());
    data.extend_from_slice(&terms.min_part.to_le_bytes());
    vec![
        transfer(payer, order, terms.lamports_total + deposit),
        allocate(order, BOOK_LEN),
        assign(order, program),
        Instruction { program: *program, accounts: vec![(*order, true, true)], data },
    ]
}

/// Tag 1 (`Fill`): signed by the order's `filler`. Reserves `orn` of what's
/// left for `seller`, under secret number `index` with its `hash` and
/// `proof`, for `fill_window` seconds.
#[allow(clippy::too_many_arguments)]
pub fn fill(
    program: &Pubkey,
    order: &Pubkey,
    filler: &Pubkey,
    index: usize,
    hash: &[u8; 32],
    proof: &[[u8; 32]; DEPTH],
    seller: &Pubkey,
    orn: u64,
    fill_window: i64,
) -> Instruction {
    let mut data = Vec::with_capacity(274);
    data.push(1);
    data.push(index as u8);
    data.extend_from_slice(hash);
    for sibling in proof {
        data.extend_from_slice(sibling);
    }
    data.extend_from_slice(seller);
    data.extend_from_slice(&orn.to_le_bytes());
    data.extend_from_slice(&fill_window.to_le_bytes());
    Instruction { program: *program, accounts: vec![(*order, false, true), (*filler, true, false)], data }
}

/// Tag 2 (`Claim`): with the part's `secret`, pays slot `slot`'s SOL to its
/// `seller`; closes the order to `buyer` when nothing is left.
pub fn claim(program: &Pubkey, order: &Pubkey, slot: u8, seller: &Pubkey, buyer: &Pubkey, secret: &[u8; 32]) -> Instruction {
    let mut data = Vec::with_capacity(34);
    data.push(2);
    data.push(slot);
    data.extend_from_slice(secret);
    Instruction { program: *program, accounts: vec![(*order, false, true), (*seller, false, true), (*buyer, false, true)], data }
}

/// Tag 3 (`Release`): after slot `slot`'s deadline, its share goes back.
pub fn release(program: &Pubkey, order: &Pubkey, slot: u8) -> Instruction {
    Instruction { program: *program, accounts: vec![(*order, false, true)], data: vec![3, slot] }
}

/// Tag 4 (`Cancel`): signed by `authority`, with no part in progress:
/// everything back to `buyer`.
pub fn cancel(program: &Pubkey, order: &Pubkey, authority: &Pubkey, buyer: &Pubkey) -> Instruction {
    let auth_writable = authority == buyer;
    Instruction {
        program: *program,
        accounts: vec![(*order, false, true), (*authority, true, auth_writable), (*buyer, false, true)],
        data: vec![4],
    }
}

/// Tag 5 (`Expire`): after expiry, everything back to `buyer`.
pub fn expire(program: &Pubkey, order: &Pubkey, buyer: &Pubkey) -> Instruction {
    Instruction { program: *program, accounts: vec![(*order, false, true), (*buyer, false, true)], data: vec![5] }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_proof_leads_to_the_root_and_only_its_own() {
        let secrets: Vec<[u8; 32]> = (0..64u8).map(|i| [i; 32]).collect();
        let tree = Secrets::from_secrets(&secrets).unwrap();
        for i in 0..SECRETS {
            assert_eq!(root_of(&tree.hash(i), i, &tree.proof(i)), tree.root());
            assert_ne!(root_of(&tree.hash(i), (i + 1) % SECRETS, &tree.proof(i)), tree.root());
        }
        assert!(Secrets::from_secrets(&secrets[..63]).is_none());
    }

    #[test]
    fn an_account_reads_back_as_written() {
        let program = [7u8; 32];
        let mut data = vec![0u8; BOOK_LEN as usize];
        data[0] = 1;
        data[1..33].copy_from_slice(&[1; 32]);
        data[161..169].copy_from_slice(&1_000u64.to_le_bytes());
        data[169..177].copy_from_slice(&100_000_000u64.to_le_bytes());
        data[193..201].copy_from_slice(&MIN_PART.to_le_bytes());
        data[201..209].copy_from_slice(&700u64.to_le_bytes());
        data[209..217].copy_from_slice(&70_000_000u64.to_le_bytes());
        data[217..225].copy_from_slice(&0b11u64.to_le_bytes());
        let at = 225 + 90;
        data[at] = 1;
        data[at + 1] = 1;
        data[at + 66..at + 74].copy_from_slice(&300u64.to_le_bytes());
        let account = Account { lamports: 1, data, owner: program, executable: false };
        let book = BookAccount::read(&account, &program).unwrap();
        assert_eq!((book.orn_left, book.lamports_left, book.unused()), (700, 70_000_000, Some(2)));
        assert_eq!(book.parts.len(), 1);
        assert_eq!((book.parts[0].slot, book.parts[0].index, book.parts[0].orn), (1, 1, 300));
        assert_eq!(book.share(700), Some(70_000_000));
        assert_eq!(book.share(100), Some(10_000_000));
        assert_eq!(book.share(50), None);
        assert_eq!(book.share(650), None);
    }
}
