//! Orien's funded buy-order escrow on Solana, kept as small as it can be.
//!
//! A buyer deposits SOL into a fresh order account (created and funded with
//! the System Program in the same transaction as `OpenOrder`), committing to
//! a secret hash `H` (SHA-256), the ORN amount and Orien destination address,
//! a refund address, a cancel authority, an expiry, an optional minimum fill
//! bond, and a maximum fill window.
//!
//! Lifecycle:
//!   - `OpenOrder` (0): initializes the order (`state = 1`, open).
//!   - `Cancel`    (1): signed by `authority`, returns all lamports to `buyer`
//!                      while open (`state == 1`) or after a fill has lapsed
//!                      (`state == 2 && now >= fill_deadline`), then closes.
//!   - `Fill`      (2): first come, first served while open and unexpired.
//!                      Records `seller` payout and `fill_deadline`, checks
//!                      any `min_bond` added above `base_lamports` (`state = 2`).
//!   - `Claim`     (3): anyone may call while `state == 2 && now < fill_deadline`
//!                      with secret `S` where `SHA256(S) == hash`. Pays
//!                      `amount + bond` to `seller` and `base_lamports - amount`
//!                      (the rent deposit) to `buyer`, then closes.
//!   - `Reopen`    (4): anyone may call when `state == 2 && now >= fill_deadline
//!                      && now < expiry`. Pays any forfeited bond to `buyer`
//!                      and resets the order to open (`state = 1`).
//!   - `Expire`    (5): anyone may call once `now >= expiry` (`state == 1 || 2`).
//!                      Returns all lamports to `buyer` and closes.
//!
//! Account data layout (217 bytes, alignment 1, no padding):
//!   [0]        state: u8 (0 = empty, 1 = open, 2 = filled)
//!   [1..33]    hash: [u8; 32]
//!   [33..65]   buyer: [u8; 32] (refund & rent-return SOL address)
//!   [65..97]   authority: [u8; 32] (cancel signer; may equal buyer)
//!   [97..129]  buyer_orien: [u8; 32] (Orien destination AccountId)
//!   [129..137] orn_amount: u64 LE (nano-ORN)
//!   [137..145] amount: u64 LE (lamports for seller)
//!   [145..153] expiry: i64 LE (Unix seconds)
//!   [153..161] min_bond: u64 LE (lamports filler must add above base_lamports)
//!   [161..169] max_fill_window: i64 LE (max seconds a fill may hold the order)
//!   [169..177] base_lamports: u64 LE (lamports at OpenOrder = amount + rent)
//!   [177..209] seller: [u8; 32] (recorded seller payout; zero while open)
//!   [209..217] fill_deadline: i64 LE (Unix seconds; 0 while open)

#![cfg_attr(target_os = "solana", no_std)]

use pinocchio::{
    AccountView, ProgramResult,
    entrypoint::{InstructionContext, lazy::MaybeAccount},
    error::ProgramError,
    lazy_program_entrypoint,
    sysvars::{Sysvar, clock::Clock},
};

/// The order account's length in bytes.
pub const LEN: usize = 217;

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
    hash: [u8; 32],
    buyer: [u8; 32],
    authority: [u8; 32],
    buyer_orien: [u8; 32],
    orn_amount: [u8; 8],
    amount: [u8; 8],
    expiry: [u8; 8],
    min_bond: [u8; 8],
    max_fill_window: [u8; 8],
}

#[derive(Clone, Copy)]
#[repr(C)]
struct FillArgs {
    seller: [u8; 32],
    fill_window: [u8; 8],
}

#[repr(C)]
struct Stored {
    state: u8,
    open: OpenArgs,
    base_lamports: [u8; 8],
    seller: [u8; 32],
    fill_deadline: [u8; 8],
}

const _: () = assert!(
    size_of::<OpenArgs>() == 168
        && size_of::<FillArgs>() == 40
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

fn sha256(secret: &[u8; 32]) -> [u8; 32] {
    let mut out = [0u8; 32];
    #[cfg(target_os = "solana")]
    unsafe {
        let parts: [&[u8]; 1] = [secret];
        pinocchio::syscalls::sol_sha256(parts.as_ptr() as *const u8, 1, out.as_mut_ptr());
    }
    #[cfg(not(target_os = "solana"))]
    let _ = secret;
    out
}

fn close_to(order: &mut AccountView, recipient: &mut AccountView, total: u64) -> ProgramResult {
    recipient.set_lamports(recipient.lamports().checked_add(total).ok_or(BAD)?);
    order.set_lamports(0);
    // SAFETY: stored data is no longer accessed after closing.
    unsafe { order.close_unchecked() };
    Ok(())
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

    // 0: OpenOrder [order (writable, signer)]
    if tag == 0 {
        if !order.is_signer() || len != 1 + size_of::<OpenArgs>() || stored.state != 0 {
            return Err(BAD);
        }
        // SAFETY: length is checked; OpenArgs has alignment 1.
        let args = unsafe { &*(rest as *const OpenArgs) };
        let orn_amount = u64::from_le_bytes(args.orn_amount);
        let amount = u64::from_le_bytes(args.amount);
        let expiry = i64::from_le_bytes(args.expiry);
        let max_fill_window = i64::from_le_bytes(args.max_fill_window);
        if orn_amount == 0
            || amount == 0
            || max_fill_window <= 0
            || args.buyer == ZERO_KEY
            || args.authority == ZERO_KEY
            || expiry <= now()?
            || order.lamports() < amount
        {
            return Err(BAD);
        }
        stored.open = *args;
        stored.base_lamports = order.lamports().to_le_bytes();
        stored.seller = ZERO_KEY;
        stored.fill_deadline = 0i64.to_le_bytes();
        stored.state = 1;
        return Ok(());
    }

    // All remaining instructions require an initialized order (open or filled).
    if stored.state != 1 && stored.state != 2 {
        return Err(BAD);
    }

    let amount = u64::from_le_bytes(stored.open.amount);
    let expiry = i64::from_le_bytes(stored.open.expiry);
    let min_bond = u64::from_le_bytes(stored.open.min_bond);
    let max_fill_window = i64::from_le_bytes(stored.open.max_fill_window);
    let base_lamports = u64::from_le_bytes(stored.base_lamports);
    let fill_deadline = i64::from_le_bytes(stored.fill_deadline);
    let total = order.lamports();

    // 1: Cancel [order (writable), authority (signer), buyer (writable; may duplicate authority)]
    if tag == 1 {
        if len != 1 {
            return Err(BAD);
        }
        let mut auth = account(&mut context)?;
        if !auth.is_signer() || auth.address().as_array() != &stored.open.authority {
            return Err(BAD);
        }
        if stored.state == 2 && now()? < fill_deadline {
            return Err(BAD);
        }
        return match context.next_account()? {
            MaybeAccount::Account(mut to_buyer) => {
                if to_buyer.address().as_array() != &stored.open.buyer {
                    return Err(BAD);
                }
                close_to(&mut order, &mut to_buyer, total)
            }
            MaybeAccount::Duplicated(1) => {
                if auth.address().as_array() != &stored.open.buyer {
                    return Err(BAD);
                }
                close_to(&mut order, &mut auth, total)
            }
            _ => Err(BAD),
        };
    }

    // 2: Fill [order (writable)]
    if tag == 2 {
        if len != 1 + size_of::<FillArgs>() || stored.state != 1 {
            return Err(BAD);
        }
        // SAFETY: length is checked; FillArgs has alignment 1.
        let args = unsafe { &*(rest as *const FillArgs) };
        let fill_window = i64::from_le_bytes(args.fill_window);
        let t = now()?;
        let bond_paid = total.checked_sub(base_lamports).ok_or(BAD)?;
        if t >= expiry
            || args.seller == ZERO_KEY
            || args.seller == stored.open.buyer
            || fill_window <= 0
            || fill_window > max_fill_window
            || bond_paid < min_bond
        {
            return Err(BAD);
        }
        let deadline = t.checked_add(fill_window).ok_or(BAD)?.min(expiry);
        if deadline <= t {
            return Err(BAD);
        }
        stored.seller = args.seller;
        stored.fill_deadline = deadline.to_le_bytes();
        stored.state = 2;
        return Ok(());
    }

    // 3: Claim [order (writable), seller (writable), buyer (writable)]
    if tag == 3 {
        if len != 33 || stored.state != 2 {
            return Err(BAD);
        }
        let mut to_seller = account(&mut context)?;
        let mut to_buyer = account(&mut context)?;
        if to_seller.address().as_array() != &stored.seller
            || to_buyer.address().as_array() != &stored.open.buyer
            || now()? >= fill_deadline
            // SAFETY: len == 33 checked above.
            || sha256(unsafe { &*(rest as *const [u8; 32]) }) != stored.open.hash
        {
            return Err(BAD);
        }
        let bond_paid = total.checked_sub(base_lamports).ok_or(BAD)?;
        let seller_pay = amount.checked_add(bond_paid).ok_or(BAD)?;
        let buyer_rent = base_lamports.checked_sub(amount).ok_or(BAD)?;
        to_seller.set_lamports(to_seller.lamports().checked_add(seller_pay).ok_or(BAD)?);
        to_buyer.set_lamports(to_buyer.lamports().checked_add(buyer_rent).ok_or(BAD)?);
        order.set_lamports(0);
        // SAFETY: data is no longer used.
        unsafe { order.close_unchecked() };
        return Ok(());
    }

    // 4: Reopen [order (writable), buyer (writable)]
    if tag == 4 {
        if len != 1 || stored.state != 2 {
            return Err(BAD);
        }
        let mut to_buyer = account(&mut context)?;
        let t = now()?;
        if to_buyer.address().as_array() != &stored.open.buyer || t < fill_deadline || t >= expiry {
            return Err(BAD);
        }
        let bond_paid = total.checked_sub(base_lamports).ok_or(BAD)?;
        if bond_paid > 0 {
            to_buyer.set_lamports(to_buyer.lamports().checked_add(bond_paid).ok_or(BAD)?);
            order.set_lamports(base_lamports);
        }
        stored.seller = ZERO_KEY;
        stored.fill_deadline = 0i64.to_le_bytes();
        stored.state = 1;
        return Ok(());
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
        return close_to(&mut order, &mut to_buyer, total);
    }

    Err(BAD)
}
