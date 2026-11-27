//! Orien's buy orders on Solana that fill in parts.
//!
//! A buyer locks SOL in a fresh order account, for an amount of ORN at a
//! price. Sellers fill it in parts, any size from the order's smallest part
//! up, at that price; the rest stays open for others. Each part settles
//! with a secret of its own: the buyer's wallet commits, when opening, to a
//! Merkle root of 64 secret hashes, and every part proves one unused hash
//! against it. (One shared secret would let a second seller claim SOL with
//! the secret the first part revealed, without delivering any ORN.)
//!
//! Lifecycle:
//!   - `Open`    (0): initializes the order. The order account signs.
//!   - `Fill`    (1): signed by the order's `filler` (the Market's relayer,
//!                    named by the buyer). Reserves `orn` ORN of what's left,
//!                    at the order's price, for `seller`, under hash number
//!                    `index` (proven against the root, never used before),
//!                    until a deadline: a part in progress, in one of 4 slots.
//!   - `Claim`   (2): anyone, with the part's secret `S` (SHA-256(S) = its
//!                    hash) before its deadline: pays the part's SOL to its
//!                    seller. With nothing left and no part in progress, the
//!                    account closes, its rent going back to the buyer.
//!   - `Release` (3): anyone, after a part's deadline: its share goes back
//!                    to what's left.
//!   - `Cancel`  (4): signed by `authority`, while no part is in progress:
//!                    everything goes back to the buyer, and it closes.
//!   - `Expire`  (5): anyone, once the order has expired (every part's
//!                    deadline has then passed too): everything goes back to
//!                    the buyer, and it closes.
//!
//! Prices: a part's lamports are the order's total lamports times its share
//! of the ORN, rounded down; the part that takes the last ORN takes all the
//! lamports left, so the parts add up to the whole exactly.
//!
//! Account data layout (585 bytes, alignment 1, no padding): see `Stored`.

#![cfg_attr(target_os = "solana", no_std)]

use pinocchio::{
    AccountView, ProgramResult,
    entrypoint::{InstructionContext, lazy::MaybeAccount},
    error::ProgramError,
    lazy_program_entrypoint,
    sysvars::{Sysvar, clock::Clock},
};

/// The order account's length in bytes.
pub const LEN: usize = 585;
/// Secrets an order commits to: parts it can ever have.
pub const SECRETS: usize = 64;
/// Levels of the secrets' Merkle tree.
pub const DEPTH: usize = 6;
/// Parts in progress at once.
pub const SLOTS: usize = 4;

lazy_program_entrypoint!(process);

#[cfg(target_os = "solana")]
#[panic_handler]
fn panic(_: &core::panic::PanicInfo<'_>) -> ! {
    unsafe { pinocchio::syscalls::abort() }
}

const BAD: ProgramError = ProgramError::InvalidArgument;
const ZERO_KEY: [u8; 32] = [0u8; 32];

#[derive(Clone, Copy)]
#[repr(C)]
struct OpenArgs {
    /// Merkle root of the 64 secret hashes.
    root: [u8; 32],
    /// Where SOL goes back to (refunds, rent).
    buyer: [u8; 32],
    /// Who may cancel.
    authority: [u8; 32],
    /// Who may fill: the Market's relayer.
    filler: [u8; 32],
    /// The Orien account the ORN are locked for (read by sellers and the Market).
    buyer_orien: [u8; 32],
    orn_total: [u8; 8],
    lamports_total: [u8; 8],
    expiry: [u8; 8],
    max_fill_window: [u8; 8],
    /// The smallest part, in lamports (the last part may be smaller).
    min_part: [u8; 8],
}

#[derive(Clone, Copy)]
#[repr(C)]
struct FillArgs {
    index: u8,
    hash: [u8; 32],
    proof: [[u8; 32]; DEPTH],
    seller: [u8; 32],
    orn: [u8; 8],
    fill_window: [u8; 8],
}

#[derive(Clone, Copy)]
#[repr(C)]
struct Slot {
    active: u8,
    index: u8,
    hash: [u8; 32],
    seller: [u8; 32],
    orn: [u8; 8],
    lamports: [u8; 8],
    deadline: [u8; 8],
}

#[repr(C)]
struct Stored {
    state: u8,
    open: OpenArgs,
    orn_left: [u8; 8],
    lamports_left: [u8; 8],
    used: [u8; 8],
    slots: [Slot; SLOTS],
}

const _: () = assert!(
    size_of::<OpenArgs>() == 200
        && size_of::<FillArgs>() == 273
        && size_of::<Slot>() == 90
        && size_of::<Stored>() == LEN
);

fn account(context: &mut InstructionContext) -> Result<AccountView, ProgramError> {
    match context.next_account()? {
        MaybeAccount::Account(account) => Ok(account),
        _ => Err(BAD),
    }
}

fn now() -> Result<i64, ProgramError> {
    Ok(Clock::get()?.unix_timestamp)
}

fn sha256(parts: &[&[u8]]) -> [u8; 32] {
    let mut out = [0u8; 32];
    #[cfg(target_os = "solana")]
    unsafe {
        pinocchio::syscalls::sol_sha256(parts.as_ptr() as *const u8, parts.len() as u64, out.as_mut_ptr());
    }
    #[cfg(not(target_os = "solana"))]
    let _ = parts;
    out
}

/// The root the secret hash at `index` proves to, with its `proof` (the
/// sibling at each level, from the leaves up). A node is SHA-256 of 0x01,
/// left, right: never a 32-byte leaf, so a node can't pass as a leaf.
fn root_of(hash: &[u8; 32], index: u8, proof: &[[u8; 32]; DEPTH]) -> [u8; 32] {
    let mut node = *hash;
    let mut at = index;
    for sibling in proof {
        node = if at & 1 == 0 { sha256(&[&[1u8][..], &node[..], &sibling[..]]) } else { sha256(&[&[1u8][..], &sibling[..], &node[..]]) };
        at >>= 1;
    }
    node
}

fn close_to(order: &mut AccountView, recipient: &mut AccountView) -> ProgramResult {
    let total = order.lamports();
    recipient.set_lamports(recipient.lamports().checked_add(total).ok_or(BAD)?);
    order.set_lamports(0);
    // SAFETY: stored data is no longer accessed after closing.
    unsafe { order.close_unchecked() };
    Ok(())
}

fn u64_of(bytes: [u8; 8]) -> u64 {
    u64::from_le_bytes(bytes)
}

fn i64_of(bytes: [u8; 8]) -> i64 {
    i64::from_le_bytes(bytes)
}

pub fn process(mut context: InstructionContext) -> ProgramResult {
    let program = context.program_id().clone();
    let mut order = account(&mut context)?;
    if !order.owned_by(&program) || order.data_len() != LEN {
        return Err(BAD);
    }
    // SAFETY: length is checked, Stored has alignment 1 and no padding,
    // and nothing else touches this account's data during the call.
    let stored = unsafe { &mut *(order.borrow_unchecked_mut().as_mut_ptr() as *mut Stored) };
    let data = context.instruction_data();
    let (len, ptr) = (data.len(), data.as_ptr());
    if len == 0 {
        return Err(BAD);
    }
    let tag = unsafe { *ptr };
    let rest = unsafe { ptr.add(1) };

    // 0: Open [order (writable, signer)]
    if tag == 0 {
        if !order.is_signer() || len != 1 + size_of::<OpenArgs>() || stored.state != 0 {
            return Err(BAD);
        }
        // SAFETY: length is checked; OpenArgs has alignment 1.
        let args = unsafe { &*(rest as *const OpenArgs) };
        let orn_total = u64_of(args.orn_total);
        let lamports_total = u64_of(args.lamports_total);
        if orn_total == 0
            || lamports_total == 0
            || u64_of(args.min_part) == 0
            || i64_of(args.max_fill_window) <= 0
            || args.buyer == ZERO_KEY
            || args.authority == ZERO_KEY
            || args.filler == ZERO_KEY
            || i64_of(args.expiry) <= now()?
            || order.lamports() < lamports_total
        {
            return Err(BAD);
        }
        stored.open = *args;
        stored.orn_left = args.orn_total;
        stored.lamports_left = args.lamports_total;
        stored.used = [0; 8];
        stored.state = 1;
        return Ok(());
    }

    if stored.state != 1 {
        return Err(BAD);
    }
    let expiry = i64_of(stored.open.expiry);
    let active = stored.slots.iter().any(|slot| slot.active == 1);

    // 1: Fill [order (writable), filler (signer)]
    if tag == 1 {
        if len != 1 + size_of::<FillArgs>() {
            return Err(BAD);
        }
        let filler = account(&mut context)?;
        if !filler.is_signer() || filler.address().as_array() != &stored.open.filler {
            return Err(BAD);
        }
        // SAFETY: length is checked; FillArgs has alignment 1.
        let args = unsafe { &*(rest as *const FillArgs) };
        let t = now()?;
        let window = i64_of(args.fill_window);
        let orn = u64_of(args.orn);
        let orn_left = u64_of(stored.orn_left);
        let lamports_left = u64_of(stored.lamports_left);
        let min_part = u64_of(stored.open.min_part);
        let used = u64_of(stored.used);
        let bit = 1u64.checked_shl(args.index as u32).ok_or(BAD)?;
        if t >= expiry
            || (args.index as usize) >= SECRETS
            || used & bit != 0
            || root_of(&args.hash, args.index, &args.proof) != stored.open.root
            || args.seller == ZERO_KEY
            || args.seller == stored.open.buyer
            || window <= 0
            || window > i64_of(stored.open.max_fill_window)
            || orn == 0
            || orn > orn_left
        {
            return Err(BAD);
        }
        // Its share of the lamports, rounded down; the last of the ORN takes
        // all the lamports left.
        let lamports = if orn == orn_left {
            lamports_left
        } else {
            let share = (u64_of(stored.open.lamports_total) as u128 * orn as u128) / u64_of(stored.open.orn_total) as u128;
            let share = u64::try_from(share).map_err(|_| BAD)?;
            // Never a part below the smallest, nor so little left after it.
            if share < min_part || lamports_left.checked_sub(share).ok_or(BAD)? < min_part {
                return Err(BAD);
            }
            share
        };
        if lamports == 0 {
            return Err(BAD);
        }
        let deadline = t.checked_add(window).ok_or(BAD)?.min(expiry);
        if deadline <= t {
            return Err(BAD);
        }
        let slot = stored.slots.iter_mut().find(|slot| slot.active == 0).ok_or(BAD)?;
        *slot = Slot {
            active: 1,
            index: args.index,
            hash: args.hash,
            seller: args.seller,
            orn: orn.to_le_bytes(),
            lamports: lamports.to_le_bytes(),
            deadline: deadline.to_le_bytes(),
        };
        stored.used = (used | bit).to_le_bytes();
        stored.orn_left = (orn_left - orn).to_le_bytes();
        stored.lamports_left = (lamports_left - lamports).to_le_bytes();
        return Ok(());
    }

    // 2: Claim [order (writable), seller (writable), buyer (writable)]
    if tag == 2 {
        if len != 1 + 1 + 32 {
            return Err(BAD);
        }
        let mut to_seller = account(&mut context)?;
        let mut to_buyer = account(&mut context)?;
        // SAFETY: len is checked.
        let index = unsafe { *rest } as usize;
        let secret = unsafe { &*(rest.add(1) as *const [u8; 32]) };
        let slot = stored.slots.get(index).ok_or(BAD)?;
        if slot.active != 1
            || to_seller.address().as_array() != &slot.seller
            || to_buyer.address().as_array() != &stored.open.buyer
            || now()? >= i64_of(slot.deadline)
            || sha256(&[&secret[..]]) != slot.hash
        {
            return Err(BAD);
        }
        let pay = u64_of(slot.lamports);
        order.set_lamports(order.lamports().checked_sub(pay).ok_or(BAD)?);
        to_seller.set_lamports(to_seller.lamports().checked_add(pay).ok_or(BAD)?);
        stored.slots[index] = Slot { active: 0, index: 0, hash: [0; 32], seller: [0; 32], orn: [0; 8], lamports: [0; 8], deadline: [0; 8] };
        let done = u64_of(stored.orn_left) == 0 && !stored.slots.iter().any(|slot| slot.active == 1);
        if done {
            return close_to(&mut order, &mut to_buyer);
        }
        return Ok(());
    }

    // 3: Release [order (writable)]
    if tag == 3 {
        if len != 2 {
            return Err(BAD);
        }
        let index = unsafe { *rest } as usize;
        let slot = stored.slots.get(index).ok_or(BAD)?;
        if slot.active != 1 || now()? < i64_of(slot.deadline) {
            return Err(BAD);
        }
        let orn_left = u64_of(stored.orn_left).checked_add(u64_of(slot.orn)).ok_or(BAD)?;
        let lamports_left = u64_of(stored.lamports_left).checked_add(u64_of(slot.lamports)).ok_or(BAD)?;
        stored.orn_left = orn_left.to_le_bytes();
        stored.lamports_left = lamports_left.to_le_bytes();
        stored.slots[index] = Slot { active: 0, index: 0, hash: [0; 32], seller: [0; 32], orn: [0; 8], lamports: [0; 8], deadline: [0; 8] };
        return Ok(());
    }

    // 4: Cancel [order (writable), authority (signer), buyer (writable; may duplicate authority)]
    if tag == 4 {
        if len != 1 || active {
            return Err(BAD);
        }
        let mut auth = account(&mut context)?;
        if !auth.is_signer() || auth.address().as_array() != &stored.open.authority {
            return Err(BAD);
        }
        return match context.next_account()? {
            MaybeAccount::Account(mut to_buyer) => {
                if to_buyer.address().as_array() != &stored.open.buyer {
                    return Err(BAD);
                }
                close_to(&mut order, &mut to_buyer)
            }
            MaybeAccount::Duplicated(1) => {
                if auth.address().as_array() != &stored.open.buyer {
                    return Err(BAD);
                }
                close_to(&mut order, &mut auth)
            }
            _ => Err(BAD),
        };
    }

    // 5: Expire [order (writable), buyer (writable)]
    if tag == 5 {
        if len != 1 {
            return Err(BAD);
        }
        let mut to_buyer = account(&mut context)?;
        if to_buyer.address().as_array() != &stored.open.buyer || now()? < expiry {
            return Err(BAD);
        }
        return close_to(&mut order, &mut to_buyer);
    }

    Err(BAD)
}
