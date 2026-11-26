//! Orien's funded buy-order program (`programs/orien-order`) on Solana:
//! account decoding and pure-Rust instruction builders shared by Orien Wallet
//! and the Market server.

use crate::solana::{Account, Instruction, Pubkey, allocate, assign, transfer};

/// The orien-order account's size in bytes.
pub const ORDER_LEN: u64 = 217;

/// orien-order deployed on Solana devnet for testing (5,664 bytes).
pub const DEVNET_ORDER_PROGRAM: &str = "7eToRgZD6HhgdT1BVawNtndeVs4eovXJiKrh2zB51pwp";

/// orien-order deployed on Solana mainnet (5,664 bytes).
pub const MAINNET_ORDER_PROGRAM: Option<&str> = Some("2HnMh1nAs5cShirEhk726KjBiQr4sxxMNv2qapjdYKN9");

/// On-chain account states.
pub const STATE_OPEN: u8 = 1;
pub const STATE_FILLED: u8 = 2;

/// Default buy-order lifetime: 7 days.
pub const DEFAULT_EXPIRY_SECONDS: i64 = 7 * 24 * 3600;
/// Default fill window: 4 hours (matches Orien's 4-epoch ORN lock, while
/// normal server auto-settlement completes in ~1 minute).
pub const DEFAULT_FILL_WINDOW: i64 = 4 * 3600;

/// Immutable parameters written by `OpenOrder`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OrderTerms {
    pub hash: [u8; 32],
    /// Buyer's Solana refund & rent-return address.
    pub buyer: Pubkey,
    /// Solana pubkey authorized to sign `Cancel` (may equal `buyer`).
    pub authority: Pubkey,
    /// Buyer's 32-byte Orien AccountId.
    pub buyer_orien: [u8; 32],
    /// ORN amount expected in nano-ORN.
    pub orn_amount: u64,
    /// SOL amount locked for the seller in lamports.
    pub amount: u64,
    /// Unix timestamp when the order expires.
    pub expiry: i64,
    /// Optional minimum SOL bond required on `Fill` (lamports).
    pub min_bond: u64,
    /// Maximum fill window allowed on `Fill` (seconds).
    pub max_fill_window: i64,
}

/// Decoded on-chain `orien-order` account (217 bytes).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OrderAccount {
    pub state: u8,
    pub terms: OrderTerms,
    pub base_lamports: u64,
    pub seller: Pubkey,
    pub fill_deadline: i64,
}

impl OrderAccount {
    /// Decodes a finalized Solana account owned by `program`, if initialized.
    pub fn read(account: &Account, program: &Pubkey) -> Option<OrderAccount> {
        if &account.owner != program || account.data.len() != ORDER_LEN as usize {
            return None;
        }
        let d = &account.data;
        let state = d[0];
        if state != STATE_OPEN && state != STATE_FILLED {
            return None;
        }
        let key = |at: usize| -> Pubkey { d[at..at + 32].try_into().expect("32 bytes") };
        let u64le = |at: usize| -> u64 { u64::from_le_bytes(d[at..at + 8].try_into().expect("8 bytes")) };
        let i64le = |at: usize| -> i64 { i64::from_le_bytes(d[at..at + 8].try_into().expect("8 bytes")) };
        Some(OrderAccount {
            state,
            terms: OrderTerms {
                hash: key(1),
                buyer: key(33),
                authority: key(65),
                buyer_orien: key(97),
                orn_amount: u64le(129),
                amount: u64le(137),
                expiry: i64le(145),
                min_bond: u64le(153),
                max_fill_window: i64le(161),
            },
            base_lamports: u64le(169),
            seller: key(177),
            fill_deadline: i64le(209),
        })
    }
}

/// Tag 0 (`OpenOrder`): funds `order` with `terms.amount + deposit`, allocates
/// 217 bytes, assigns it to `program`, and initializes the order terms.
/// Uses `transfer + allocate + assign` (not `create_account`) so stray dust at
/// the order address can never block opening.
pub fn open_order(
    program: &Pubkey,
    payer: &Pubkey,
    order: &Pubkey,
    terms: &OrderTerms,
    deposit: u64,
) -> Vec<Instruction> {
    let mut data = Vec::with_capacity(169);
    data.push(0);
    data.extend_from_slice(&terms.hash);
    data.extend_from_slice(&terms.buyer);
    data.extend_from_slice(&terms.authority);
    data.extend_from_slice(&terms.buyer_orien);
    data.extend_from_slice(&terms.orn_amount.to_le_bytes());
    data.extend_from_slice(&terms.amount.to_le_bytes());
    data.extend_from_slice(&terms.expiry.to_le_bytes());
    data.extend_from_slice(&terms.min_bond.to_le_bytes());
    data.extend_from_slice(&terms.max_fill_window.to_le_bytes());
    vec![
        transfer(payer, order, terms.amount + deposit),
        allocate(order, ORDER_LEN),
        assign(order, program),
        Instruction { program: *program, accounts: vec![(*order, true, true)], data },
    ]
}

/// Tag 1 (`Cancel`): signed by `authority`, refunds all lamports to `buyer`
/// while open or after a fill has lapsed, then closes `order`.
pub fn cancel(program: &Pubkey, order: &Pubkey, authority: &Pubkey, buyer: &Pubkey) -> Instruction {
    let auth_writable = authority == buyer;
    Instruction {
        program: *program,
        accounts: vec![(*order, false, true), (*authority, true, auth_writable), (*buyer, false, true)],
        data: vec![1],
    }
}

/// Tag 2 (`Fill`): optionally deposits `bond` from `payer` to `order`, then
/// records `seller` and `fill_window`. First come, first served.
pub fn fill(
    program: &Pubkey,
    payer: &Pubkey,
    order: &Pubkey,
    seller: &Pubkey,
    fill_window: i64,
    bond: u64,
) -> Vec<Instruction> {
    let mut data = Vec::with_capacity(41);
    data.push(2);
    data.extend_from_slice(seller);
    data.extend_from_slice(&fill_window.to_le_bytes());
    let mut out = Vec::with_capacity(2);
    if bond > 0 {
        out.push(transfer(payer, order, bond));
    }
    out.push(Instruction { program: *program, accounts: vec![(*order, false, true)], data });
    out
}

/// Tag 3 (`Claim`): pays `amount + bond` to the recorded `seller` and returns
/// the rent deposit to `buyer` using `secret`, then closes `order`.
pub fn claim(
    program: &Pubkey,
    order: &Pubkey,
    seller: &Pubkey,
    buyer: &Pubkey,
    secret: &[u8; 32],
) -> Instruction {
    let mut data = Vec::with_capacity(33);
    data.push(3);
    data.extend_from_slice(secret);
    Instruction {
        program: *program,
        accounts: vec![(*order, false, true), (*seller, false, true), (*buyer, false, true)],
        data,
    }
}

/// Tag 4 (`Reopen`): after a fill lapses before `expiry`, pays any forfeited
/// bond to `buyer` and returns `order` to open.
pub fn reopen(program: &Pubkey, order: &Pubkey, buyer: &Pubkey) -> Instruction {
    Instruction {
        program: *program,
        accounts: vec![(*order, false, true), (*buyer, false, true)],
        data: vec![4],
    }
}

/// Tag 5 (`Expire`): at or after `expiry`, refunds all lamports to `buyer` and
/// closes `order`.
pub fn expire(program: &Pubkey, order: &Pubkey, buyer: &Pubkey) -> Instruction {
    Instruction {
        program: *program,
        accounts: vec![(*order, false, true), (*buyer, false, true)],
        data: vec![5],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn instructions_and_account_layout_match_program() {
        let program = [1u8; 32];
        let payer = [2u8; 32];
        let order = [3u8; 32];
        let terms = OrderTerms {
            hash: [4u8; 32],
            buyer: [5u8; 32],
            authority: [6u8; 32],
            buyer_orien: [7u8; 32],
            orn_amount: 500_000_000_000,
            amount: 1_000_000_000,
            expiry: 1_800_604_800,
            min_bond: 5_000_000,
            max_fill_window: 14_400,
        };
        let open_ixs = open_order(&program, &payer, &order, &terms, 1_752_600);
        assert_eq!(open_ixs.len(), 4);
        assert_eq!(open_ixs[3].data.len(), 169);
        assert_eq!(open_ixs[3].data[0], 0);

        // Simulate on-chain Stored bytes and verify round-trip decode.
        let mut raw = vec![STATE_FILLED];
        raw.extend_from_slice(&open_ixs[3].data[1..]);
        raw.extend_from_slice(&(terms.amount + 1_752_600).to_le_bytes());
        raw.extend_from_slice(&[8u8; 32]); // seller
        raw.extend_from_slice(&1_800_014_400i64.to_le_bytes());
        assert_eq!(raw.len(), ORDER_LEN as usize);

        let acct = Account { lamports: terms.amount + 1_752_600 + 5_000_000, owner: program, data: raw, executable: false };
        let decoded = OrderAccount::read(&acct, &program).expect("decode");
        assert_eq!(decoded.state, STATE_FILLED);
        assert_eq!(decoded.terms, terms);
        assert_eq!(decoded.base_lamports, terms.amount + 1_752_600);
        assert_eq!(decoded.seller, [8u8; 32]);
        assert_eq!(decoded.fill_deadline, 1_800_014_400);

        let fill_ixs = fill(&program, &payer, &order, &[8u8; 32], 3600, 5_000_000);
        assert_eq!(fill_ixs.len(), 2);
        assert_eq!(fill_ixs[1].data.len(), 41);
        assert_eq!(fill_ixs[1].data[0], 2);

        let claim_ix = claim(&program, &order, &[8u8; 32], &terms.buyer, &[9u8; 32]);
        assert_eq!(claim_ix.data.len(), 33);
        assert_eq!(claim_ix.data[0], 3);

        assert_eq!(cancel(&program, &order, &terms.authority, &terms.buyer).data, vec![1]);
        assert_eq!(reopen(&program, &order, &terms.buyer).data, vec![4]);
        assert_eq!(expire(&program, &order, &terms.buyer).data, vec![5]);
    }
}
