//! Orien's hash-and-time lock on Solana, kept as small as it can be.
//!
//! Coins wait in a lock account until someone shows the secret behind its
//! hash (SHA-256, as Orien's own locks use), which pays them to the seller,
//! or until its deadline passes, which pays them back to the buyer. Anyone
//! may send either instruction: the coins can only ever go to the two
//! addresses written in the lock.
//!
//! The lock account is created and funded by the buyer's wallet with the
//! System Program, in the same transaction as `Lock`, owned by this program,
//! 113 bytes long, at a fresh address whose key signs the transaction. So
//! this program never calls another one. Solana itself refuses any account
//! left below its rent deposit, so the program does no rent sums either.
//!
//! The lock account's data, 113 bytes:
//!   [0]        1 once locked
//!   [1..33]    SHA-256 of the secret
//!   [33..65]   who is paid with the secret (the seller)
//!   [65..97]   who is paid back after the deadline (the buyer)
//!   [97..105]  the deadline, a Unix time in seconds, little-endian i64
//!   [105..113] the amount locked, in lamports, little-endian u64
//!
//! Instructions, by their first byte:
//!   0 Lock    [lock (signer)]                   data: the 112 bytes above, after the state byte
//!   1 Claim   [lock, seller, buyer]             data: the 32-byte secret; before the deadline only.
//!                                               The seller gets the amount, the buyer the rest (the deposit).
//!   2 Refund  [lock, buyer]                     after the deadline: everything to the buyer.

#![cfg_attr(target_os = "solana", no_std)]

use pinocchio::{
    entrypoint::InstructionContext,
    error::ProgramError,
    lazy_program_entrypoint,
    sysvars::{Sysvar, clock::Clock},
    AccountView, ProgramResult,
};

/// The lock account's length.
pub const LEN: usize = 113;

lazy_program_entrypoint!(process);


/// No messages: a failed instruction is enough.
#[cfg(target_os = "solana")]
#[panic_handler]
fn panic(_: &core::panic::PanicInfo<'_>) -> ! {
    unsafe { pinocchio::syscalls::abort() }
}

const BAD: ProgramError = ProgramError::InvalidArgument;

/// The lock account's data, 113 bytes with no padding.
#[repr(C)]
struct Stored {
    state: u8,
    terms: Terms,
}

/// What `Lock` writes: everything but the state byte.
#[derive(Clone, Copy)]
#[repr(C)]
struct Terms {
    hash: [u8; 32],
    seller: [u8; 32],
    buyer: [u8; 32],
    deadline: [u8; 8],
    amount: [u8; 8],
}

const _: () = assert!(size_of::<Stored>() == LEN && size_of::<Terms>() == LEN - 1);

fn account(context: &mut InstructionContext) -> Result<AccountView, ProgramError> {
    match context.next_account()? {
        pinocchio::entrypoint::lazy::MaybeAccount::Account(account) => Ok(account),
        // The same account twice is never right here.
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

pub fn process(mut context: InstructionContext) -> ProgramResult {
    let program = context.program_id().clone();
    let mut lock = account(&mut context)?;
    if !lock.owned_by(&program) || lock.data_len() != LEN {
        return Err(BAD);
    }
    // SAFETY: the length is checked, Stored has no padding and alignment 1,
    // and nothing else touches this account's data during the call.
    let stored = unsafe { &mut *(lock.borrow_unchecked_mut().as_mut_ptr() as *mut Stored) };
    // The instruction's data sits in the runtime's input for the whole call,
    // apart from the accounts read below.
    let data = context.instruction_data();
    let (len, data) = (data.len(), data.as_ptr());
    if len == 0 {
        return Err(BAD);
    }
    let tag = unsafe { *data };
    let rest = unsafe { data.add(1) };

    if tag == 0 {
        // Lock: a fresh account, signed for, written once. The seller and
        // the buyer differ, and the coins are all there.
        if !lock.is_signer() || len != LEN || stored.state != 0 {
            return Err(BAD);
        }
        // SAFETY: the length is checked; Terms has alignment 1.
        let terms = unsafe { &*(rest as *const Terms) };
        if terms.seller == terms.buyer || lock.lamports() < u64::from_le_bytes(terms.amount) {
            return Err(BAD);
        }
        stored.terms = *terms;
        stored.state = 1;
        return Ok(());
    }

    // Claim and Refund: a lock in place, paid out in full, then closed.
    if stored.state != 1 {
        return Err(BAD);
    }
    let terms = &stored.terms;
    let (deadline, amount, total) = (i64::from_le_bytes(terms.deadline), u64::from_le_bytes(terms.amount), lock.lamports());

    if tag == 1 {
        let mut to_seller = account(&mut context)?;
        let mut to_buyer = account(&mut context)?;
        if len != 33
            || to_seller.address().as_array() != &terms.seller
            || to_buyer.address().as_array() != &terms.buyer
            // SAFETY: the length is checked.
            || sha256(unsafe { &*(rest as *const [u8; 32]) }) != terms.hash
            || now()? >= deadline
        {
            return Err(BAD);
        }
        to_seller.set_lamports(to_seller.lamports().checked_add(amount).ok_or(BAD)?);
        to_buyer.set_lamports(to_buyer.lamports().checked_add(total.checked_sub(amount).ok_or(BAD)?).ok_or(BAD)?);
    } else if tag == 2 {
        let mut to_buyer = account(&mut context)?;
        if len != 1 || to_buyer.address().as_array() != &terms.buyer || now()? < deadline {
            return Err(BAD);
        }
        to_buyer.set_lamports(to_buyer.lamports().checked_add(total).ok_or(BAD)?);
    } else {
        return Err(BAD);
    }
    lock.set_lamports(0);
    // SAFETY: the data is no longer used.
    unsafe { lock.close_unchecked() };
    Ok(())
}
