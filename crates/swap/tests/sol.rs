//! Whole ORN–SOL swaps against a pretend Orien and a pretend Solana. The
//! pretend Solana reads the real transactions the swap builds, checks their
//! signatures, and applies Solana's rules (fees, "nothing or the rent
//! minimum") and orien-lock's, so the bytes themselves are tested.

use anyhow::{Result, anyhow, bail, ensure};
use ed25519_dalek::{Signature, VerifyingKey};
use orien_crypto::Hash32;
use orien_swap::sol::{
    CUSHION, FEE, MIN_LAMPORTS, ORN_EPOCHS, SAFETY_SECONDS, SOL_LOCK_SECONDS, Sol, SolOffer, SolReply, SolStage, SolSwap,
};
use orien_swap::solana::{self, Keypair, LOCK_LEN, LAMPORTS_PER_SOL, Landed, Lock, Pubkey};
use orien_swap::swap::{LockRef, LockState, Orien, Role, Waiting};
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

// ---------------------------------------------------------------- Orien

#[derive(Default)]
struct Ledger {
    epoch: u64,
    escrows: HashMap<(Hash32, u64), Waiting>,
    revealed: HashMap<(Hash32, u64), [u8; 32]>,
    balances: HashMap<Hash32, u64>,
    next_seq: HashMap<Hash32, u64>,
}

impl Ledger {
    fn close(&mut self) {
        let epoch = self.epoch;
        let expired: Vec<_> = self.escrows.iter().filter(|(_, w)| w.until <= epoch + 1).map(|(k, _)| *k).collect();
        for key in expired {
            let w = self.escrows.remove(&key).unwrap();
            *self.balances.entry(key.0).or_default() += w.amount;
        }
        self.epoch += 1;
    }
}

struct Side {
    ledger: Rc<RefCell<Ledger>>,
    me: Hash32,
}

impl Orien for Side {
    fn network(&self) -> String {
        "test".into()
    }
    fn me(&self) -> Hash32 {
        self.me
    }
    fn epoch(&self) -> Result<u64> {
        Ok(self.ledger.borrow().epoch)
    }
    fn latest_epoch(&self) -> Result<u64> {
        Ok(self.ledger.borrow().epoch)
    }
    fn epoch_seconds(&self) -> u64 {
        3_600
    }
    fn next_seq(&self) -> Result<u64> {
        Ok(self.ledger.borrow().next_seq.get(&self.me).copied().unwrap_or(0))
    }
    fn proven(&self, lock: &LockRef) -> Result<LockState> {
        let l = self.ledger.borrow();
        let key = (lock.from, lock.seq);
        let state = LockState { epoch: l.epoch, waiting: l.escrows.get(&key).cloned(), secret: l.revealed.get(&key).copied() };
        ensure!(state.waiting.is_some() || state.secret.is_some(), "nothing proven");
        Ok(state)
    }
    fn lock(&self, seq: u64, to: &Hash32, amount: u64, hash: [u8; 32], until: u64) -> Result<LockRef> {
        let mut l = self.ledger.borrow_mut();
        ensure!(l.next_seq.get(&self.me).copied().unwrap_or(0) == seq, "that order number is used");
        let balance = l.balances.entry(self.me).or_default();
        ensure!(*balance >= amount, "insufficient funds");
        *balance -= amount;
        l.next_seq.insert(self.me, seq + 1);
        l.escrows.insert((self.me, seq), Waiting { to: *to, amount, hash, until });
        Ok(LockRef { from: self.me, seq })
    }
    fn still_waiting(&self, lock: &LockRef) -> Result<Option<bool>> {
        Ok(Some(self.ledger.borrow().escrows.contains_key(&(lock.from, lock.seq))))
    }
    fn claim(&self, lock: &LockRef, payee: &Hash32, amount: u64, secret: [u8; 32]) -> Result<()> {
        let mut l = self.ledger.borrow_mut();
        let epoch = l.epoch;
        let w = l.escrows.get(&(lock.from, lock.seq)).cloned().ok_or_else(|| anyhow!("no such lock"))?;
        ensure!(w.to == *payee && w.amount == amount, "not that lock's payee");
        ensure!(epoch < w.until, "too late");
        ensure!(orien_ledger::types::lock_hash(&secret) == w.hash, "wrong secret");
        l.escrows.remove(&(lock.from, lock.seq));
        l.revealed.insert((lock.from, lock.seq), secret);
        *l.balances.entry(*payee).or_default() += amount;
        Ok(())
    }
}

// ---------------------------------------------------------------- Solana

const PROGRAM: Pubkey = [77; 32];

fn rent(len: u64) -> u64 {
    (len + 128) * 6_960
}

#[derive(Clone, Default)]
struct Chain {
    time: i64,
    balances: HashMap<Pubkey, u64>,
    /// orien-lock accounts' data, by address.
    locks: HashMap<Pubkey, Vec<u8>>,
    secrets: HashMap<Pubkey, [u8; 32]>,
    landed: HashMap<String, Landed>,
}

#[derive(Clone)]
struct FakeSol(Rc<RefCell<Chain>>);

fn compact(bytes: &[u8], at: &mut usize) -> Result<usize> {
    let (mut n, mut shift) = (0usize, 0);
    loop {
        let byte = *bytes.get(*at).ok_or_else(|| anyhow!("short"))?;
        *at += 1;
        n |= ((byte & 0x7f) as usize) << shift;
        if byte & 0x80 == 0 {
            return Ok(n);
        }
        shift += 7;
    }
}

fn take<'a>(bytes: &'a [u8], at: &mut usize, n: usize) -> Result<&'a [u8]> {
    let slice = bytes.get(*at..*at + n).ok_or_else(|| anyhow!("short"))?;
    *at += n;
    Ok(slice)
}

impl Chain {
    fn run(&mut self, tx: &[u8]) -> Result<String> {
        let mut at = 0;
        let signatures: Vec<[u8; 64]> =
            (0..compact(tx, &mut at)?).map(|_| take(tx, &mut at, 64).map(|s| s.try_into().unwrap())).collect::<Result<_>>()?;
        let message = &tx[at..];
        let header = take(tx, &mut at, 3)?.to_vec();
        let keys: Vec<Pubkey> = (0..compact(tx, &mut at)?).map(|_| take(tx, &mut at, 32).map(|k| k.try_into().unwrap())).collect::<Result<_>>()?;
        take(tx, &mut at, 32)?;
        ensure!(signatures.len() == header[0] as usize, "signature count");
        for (signature, key) in signatures.iter().zip(&keys) {
            VerifyingKey::from_bytes(key)?.verify_strict(message, &Signature::from_bytes(signature)).map_err(|_| anyhow!("bad signature"))?;
        }
        let signer = |i: usize| i < header[0] as usize;
        let mut next = self.clone();
        let payer = keys[0];
        let fee = FEE * signatures.len() as u64;
        let balance = next.balances.entry(payer).or_default();
        ensure!(*balance >= fee, "the payer can't pay the fee");
        *balance -= fee;
        for _ in 0..compact(tx, &mut at)? {
            let program = keys[take(tx, &mut at, 1)?[0] as usize];
            let accounts: Vec<usize> = (0..compact(tx, &mut at)?).map(|_| take(tx, &mut at, 1).map(|b| b[0] as usize)).collect::<Result<_>>()?;
            let n = compact(tx, &mut at)?;
            let data = take(tx, &mut at, n)?.to_vec();
            next.apply(&program, &keys, &accounts, &data, &signer)?;
        }
        // Solana's rule: every ordinary account holds nothing or at least the
        // rent minimum; a lock account at least its deposit.
        for (key, balance) in &next.balances {
            let minimum = if next.locks.contains_key(key) { rent(LOCK_LEN) } else { rent(0) };
            ensure!(*balance == 0 || *balance >= minimum, "{} would be left below the rent minimum", solana::text(key));
        }
        *self = next;
        let id = solana::text(&signatures[0]);
        self.landed.insert(id.clone(), Landed::Finalized);
        Ok(id)
    }

    fn apply(&mut self, program: &Pubkey, keys: &[Pubkey], accounts: &[usize], data: &[u8], signer: &dyn Fn(usize) -> bool) -> Result<()> {
        let key = |i: usize| keys[accounts[i]];
        let u64_at = |range: std::ops::Range<usize>| u64::from_le_bytes(data[range].try_into().unwrap());
        if *program == solana::SYSTEM_PROGRAM {
            ensure!(signer(accounts[0]), "the sender must sign");
            let from = key(0);
            match u32::from_le_bytes(data[..4].try_into().unwrap()) {
                0 => {
                    let (lamports, space) = (u64_at(4..12), u64_at(12..20));
                    let to = key(1);
                    ensure!(signer(accounts[1]), "a new account signs");
                    ensure!(self.balances.get(&to).copied().unwrap_or(0) == 0, "already in use");
                    ensure!(data[20..52] == PROGRAM && space == LOCK_LEN, "only lock accounts here");
                    self.debit(&from, lamports)?;
                    *self.balances.entry(to).or_default() += lamports;
                    self.locks.insert(to, vec![0; LOCK_LEN as usize]);
                }
                2 => {
                    let lamports = u64_at(4..12);
                    self.debit(&from, lamports)?;
                    *self.balances.entry(key(1)).or_default() += lamports;
                }
                other => bail!("system instruction {other}"),
            }
            return Ok(());
        }
        ensure!(*program == PROGRAM, "unknown program");
        // orien-lock, as programs/orien-lock does it.
        let lock = key(0);
        let stored = self.locks.get(&lock).cloned().ok_or_else(|| anyhow!("not a lock account"))?;
        let held = self.balances.get(&lock).copied().unwrap_or(0);
        match data[0] {
            0 => {
                ensure!(signer(accounts[0]) && data.len() == 113 && stored[0] == 0, "lock refused");
                ensure!(data[33..65] != data[65..97] && held >= u64_at(105..113), "lock refused");
                let mut written = data.to_vec();
                written[0] = 1;
                self.locks.insert(lock, written);
            }
            1 => {
                ensure!(stored[0] == 1 && data.len() == 33, "claim refused");
                let deadline = i64::from_le_bytes(stored[97..105].try_into().unwrap());
                ensure!(key(1).as_slice() == &stored[33..65] && key(2).as_slice() == &stored[65..97], "wrong accounts");
                ensure!(orien_ledger::types::lock_hash(data[1..33].try_into().unwrap()).as_slice() == &stored[1..33], "wrong secret");
                ensure!(self.time < deadline, "too late");
                let amount = u64::from_le_bytes(stored[105..113].try_into().unwrap());
                *self.balances.entry(key(1)).or_default() += amount;
                *self.balances.entry(key(2)).or_default() += held - amount;
                self.close(&lock);
                self.secrets.insert(lock, data[1..33].try_into().unwrap());
            }
            2 => {
                ensure!(stored[0] == 1 && key(1).as_slice() == &stored[65..97], "refund refused");
                ensure!(self.time >= i64::from_le_bytes(stored[97..105].try_into().unwrap()), "too early");
                *self.balances.entry(key(1)).or_default() += held;
                self.close(&lock);
            }
            _ => bail!("unknown instruction"),
        }
        Ok(())
    }

    fn debit(&mut self, from: &Pubkey, lamports: u64) -> Result<()> {
        let balance = self.balances.entry(*from).or_default();
        ensure!(*balance >= lamports, "insufficient funds");
        *balance -= lamports;
        Ok(())
    }

    fn close(&mut self, lock: &Pubkey) {
        self.balances.remove(lock);
        self.locks.remove(lock);
    }
}

impl Sol for FakeSol {
    fn lock_program(&self) -> Option<Pubkey> {
        Some(PROGRAM)
    }
    fn balance(&self, who: &Pubkey) -> Result<u64> {
        Ok(self.0.borrow().balances.get(who).copied().unwrap_or(0))
    }
    fn lock(&self, at: &Pubkey) -> Result<Option<Lock>> {
        let chain = self.0.borrow();
        let Some(data) = chain.locks.get(at) else { return Ok(None) };
        let account = solana::Account { lamports: chain.balances.get(at).copied().unwrap_or(0), owner: PROGRAM, data: data.clone() };
        Ok(Lock::read(&account, &PROGRAM))
    }
    fn time(&self) -> Result<i64> {
        Ok(self.0.borrow().time)
    }
    fn rent(&self, len: u64) -> Result<u64> {
        Ok(rent(len))
    }
    fn blockhash(&self) -> Result<[u8; 32]> {
        Ok([1; 32])
    }
    fn send(&self, tx: &[u8]) -> Result<String> {
        self.0.borrow_mut().run(tx)
    }
    fn landed(&self, signature: &str) -> Result<Option<Landed>> {
        Ok(self.0.borrow().landed.get(signature).copied())
    }
    fn secret(&self, lock: &Pubkey, hash: &[u8; 32]) -> Result<Option<[u8; 32]>> {
        Ok(self.0.borrow().secrets.get(lock).copied().filter(|s| &orien_ledger::types::lock_hash(s) == hash))
    }
}

// ---------------------------------------------------------------- setup

const ORN: u64 = 500;
const PRICE: u64 = 2 * LAMPORTS_PER_SOL / 10;

struct World {
    ledger: Rc<RefCell<Ledger>>,
    sol: FakeSol,
    seller: Side,
    buyer: Side,
    fee_secret: [u8; 32],
    payout: Keypair,
    /// The buyer's own wallet, which pays and is paid back.
    wallet: Keypair,
}

fn world() -> World {
    let ledger = Rc::new(RefCell::new(Ledger::default()));
    let (s, b) = (Hash32([1; 32]), Hash32([2; 32]));
    ledger.borrow_mut().balances.insert(s, 10_000);
    let sol = FakeSol(Rc::new(RefCell::new(Chain { time: 1_800_000_000, ..Chain::default() })));
    let (payout, wallet) = (Keypair::from_secret(&[11; 32]), Keypair::from_secret(&[12; 32]));
    sol.0.borrow_mut().balances.insert(wallet.pubkey(), 5 * LAMPORTS_PER_SOL);
    World {
        seller: Side { ledger: ledger.clone(), me: s },
        buyer: Side { ledger: ledger.clone(), me: b },
        ledger,
        sol,
        fee_secret: [13; 32],
        payout,
        wallet,
    }
}

impl World {
    fn sell(&self) -> SolSwap {
        let mut seller =
            SolSwap::sell(&self.seller, "test", self.buyer.me, ORN, PRICE, &solana::text(&self.payout.pubkey()), &self.fee_secret).unwrap();
        seller.step(&self.sol, &self.seller).unwrap();
        assert_eq!(seller.stage, SolStage::WaitingForReply);
        seller
    }

    fn take(&self, seller: &mut SolSwap) -> SolSwap {
        let offer = SolOffer::read(&seller.offer("test".into()).unwrap().code()).unwrap();
        let (buyer, reply) = SolSwap::buy(&self.buyer, &self.sol, "test", &offer, &solana::text(&self.wallet.pubkey())).unwrap();
        seller.accept(&SolReply::read(&reply.code()).unwrap()).unwrap();
        buyer
    }

    /// The buyer pays from their own wallet, as Phantom would.
    fn pay(&self, to: &str, lamports: u64) {
        let to = solana::address(to).unwrap();
        let tx = solana::transaction(&self.wallet, &[], &[solana::transfer(&self.wallet.pubkey(), &to, lamports)], [1; 32]).unwrap();
        self.sol.send(&tx).unwrap();
    }

    fn sol_balance(&self, who: &Pubkey) -> u64 {
        self.sol.balance(who).unwrap()
    }

    fn step(&self, seller: &mut SolSwap, buyer: &mut SolSwap, times: usize) {
        for _ in 0..times {
            let _ = seller.step(&self.sol, &self.seller);
            let _ = buyer.step(&self.sol, &self.buyer);
        }
    }

    fn pass_seconds(&self, seconds: i64) {
        let before = self.sol.0.borrow().time;
        self.sol.0.borrow_mut().time = before + seconds;
        let epochs = ((before + seconds) / 3_600 - before / 3_600) as u64;
        for _ in 0..epochs {
            self.ledger.borrow_mut().close();
        }
    }

    fn orn(&self, who: &Side) -> u64 {
        self.ledger.borrow().balances.get(&who.me).copied().unwrap_or(0)
    }
}

// ---------------------------------------------------------------- swaps

#[test]
fn a_whole_swap_pays_both_sides_and_sends_the_leftovers_home() {
    let w = world();
    let mut seller = w.sell();
    let mut buyer = w.take(&mut seller);
    let wallet_before = w.sol_balance(&w.wallet.pubkey());
    assert!(buyer.pay_link().unwrap().starts_with(&format!("solana:{}?amount=0.2", buyer.pay_address().unwrap())));

    w.pay(&buyer.pay_address().unwrap(), buyer.to_pay);
    w.step(&mut seller, &mut buyer, 6);
    assert_eq!(buyer.stage, SolStage::Done);
    assert_eq!(seller.stage, SolStage::Done);
    assert_eq!(w.orn(&w.buyer), ORN, "the buyer has the ORN");
    assert_eq!(w.sol_balance(&w.payout.pubkey()), PRICE, "the seller's wallet has exactly the price");

    // The buyer's wallet paid the price, the seller's first fee top-up, and a
    // handful of network fees; the deposit and the rest came back.
    w.step(&mut seller, &mut buyer, 2);
    assert!(buyer.finished(), "{:?}", buyer.stage);
    assert_eq!(w.sol_balance(&solana::address(&buyer.pay_address().unwrap()).unwrap()), 0, "the one-time address is empty");
    let fee_key = Keypair::from_secret(&w.fee_secret).pubkey();
    let topped = w.sol_balance(&fee_key) + FEE; // the seller's claim fee came out of it
    let cost = wallet_before - w.sol_balance(&w.wallet.pubkey());
    // Fees: the payment from the wallet, the lock (two signers), the sweep.
    assert_eq!(cost, PRICE + topped + 4 * FEE, "price {PRICE}, top-up {topped}, and fees");

    // A few minutes on, the seller's wallet sees the ORN already claimed.
    w.pass_seconds(400);
    w.step(&mut seller, &mut buyer, 1);
    assert!(seller.finished());
}

#[test]
fn a_short_payment_waits_for_the_rest() {
    let w = world();
    let mut seller = w.sell();
    let mut buyer = w.take(&mut seller);
    w.pay(&buyer.pay_address().unwrap(), PRICE);
    w.step(&mut seller, &mut buyer, 3);
    assert_eq!(buyer.stage, SolStage::WaitingForPayment, "not enough for the deposit and fees yet");
    w.pay(&buyer.pay_address().unwrap(), buyer.to_pay - PRICE);
    w.step(&mut seller, &mut buyer, 6);
    assert_eq!((buyer.stage.clone(), seller.stage.clone()), (SolStage::Done, SolStage::Done));
}

#[test]
fn if_the_seller_never_takes_the_sol_they_go_back_after_the_deadline() {
    let w = world();
    let mut seller = w.sell();
    let mut buyer = w.take(&mut seller);
    let wallet_before = w.sol_balance(&w.wallet.pubkey());
    w.pay(&buyer.pay_address().unwrap(), buyer.to_pay);
    // The seller's wallet is closed: only the buyer's steps.
    for _ in 0..3 {
        buyer.step(&w.sol, &w.buyer).unwrap();
    }
    assert_eq!(buyer.stage, SolStage::WaitingForSecret);
    w.pass_seconds(SOL_LOCK_SECONDS);
    for _ in 0..6 {
        let _ = buyer.step(&w.sol, &w.buyer);
    }
    assert!(matches!(buyer.stage, SolStage::Cancelled { .. }), "{:?}", buyer.stage);
    assert!(buyer.finished());
    let cost = wallet_before - w.sol_balance(&w.wallet.pubkey());
    let topped = w.sol_balance(&Keypair::from_secret(&w.fee_secret).pubkey());
    // Fees: the payment, the lock (two signers), the refund, the sweep.
    assert_eq!(cost, topped + 5 * FEE, "everything back but the top-up and the fees");
    // The seller's wallet comes back after the deadline: it must not claim.
    seller.step(&w.sol, &w.seller).unwrap();
    assert_eq!(w.sol_balance(&w.payout.pubkey()), 0);
    w.pass_seconds(ORN_EPOCHS as i64 * 3_600);
    seller.step(&w.sol, &w.seller).unwrap();
    assert!(matches!(seller.stage, SolStage::Cancelled { .. }));
    assert_eq!(w.orn(&w.seller), 10_000, "the ORN came back");
}

#[test]
fn if_no_sol_comes_both_sides_give_up_and_the_orn_come_back() {
    let w = world();
    let mut seller = w.sell();
    let mut buyer = w.take(&mut seller);
    w.step(&mut seller, &mut buyer, 2);
    w.pass_seconds((ORN_EPOCHS as i64) * 3_600);
    w.step(&mut seller, &mut buyer, 3);
    assert!(seller.finished() && buyer.finished(), "{:?} {:?}", seller.stage, buyer.stage);
    assert_eq!(w.orn(&w.seller), 10_000);
}

#[test]
fn money_sent_too_late_goes_back_without_a_lock() {
    let w = world();
    let mut seller = w.sell();
    let mut buyer = w.take(&mut seller);
    w.pass_seconds(2 * 3_600);
    w.pay(&buyer.pay_address().unwrap(), buyer.to_pay);
    let wallet_after_paying = w.sol_balance(&w.wallet.pubkey());
    w.step(&mut seller, &mut buyer, 4);
    assert!(matches!(buyer.stage, SolStage::Cancelled { .. }), "{:?}", buyer.stage);
    assert!(buyer.finished());
    assert_eq!(w.sol_balance(&w.wallet.pubkey()), wallet_after_paying + buyer.to_pay - FEE, "all of it back, less the sweep's fee");
}

#[test]
fn the_seller_refuses_a_lock_that_is_not_the_agreed_one() {
    let w = world();
    let mut seller = w.sell();
    let buyer = w.take(&mut seller);
    let program = PROGRAM;
    let at = Keypair::random();
    let hash = buyer.hash;
    let make = |seller_key: Pubkey, amount: u64, deadline: i64| {
        let at = Keypair::random();
        let terms = Lock { hash, seller: seller_key, buyer: w.wallet.pubkey(), deadline, amount, lamports: 0 };
        let ixs = solana::lock(&program, &w.wallet.pubkey(), &at.pubkey(), &terms, rent(LOCK_LEN));
        w.sol.send(&solana::transaction(&w.wallet, &[&at], &ixs, [1; 32]).unwrap()).unwrap();
        solana::text(&at.pubkey())
    };
    let now = w.sol.time().unwrap();
    let payout = w.payout.pubkey();
    for (why, lock) in [
        ("short", make(payout, PRICE - 1, now + SOL_LOCK_SECONDS)),
        ("paying someone else", make([99; 32], PRICE, now + SOL_LOCK_SECONDS)),
        ("ending too soon", make(payout, PRICE, now + SAFETY_SECONDS)),
        ("ending after the ORN", make(payout, PRICE, now + 4 * 3_600)),
    ] {
        let mut copy = seller.clone();
        copy.sol_lock = Some(lock);
        copy.step(&w.sol, &w.seller).unwrap();
        assert_eq!(copy.stage, SolStage::WaitingForSol, "{why}: not claimed");
        assert_eq!(w.sol_balance(&payout), 0, "{why}");
    }
    let _ = at;
}

#[test]
fn a_buyer_whose_wallet_crashed_after_locking_finds_the_lock_again() {
    let w = world();
    let mut seller = w.sell();
    let mut buyer = w.take(&mut seller);
    w.pay(&buyer.pay_address().unwrap(), buyer.to_pay);
    buyer.step(&w.sol, &w.buyer).unwrap();
    assert_eq!(buyer.stage, SolStage::Locking);
    // The saved file is from before the lock was sent.
    buyer.stage = SolStage::WaitingForPayment;
    buyer.sent = None;
    buyer.step(&w.sol, &w.buyer).unwrap();
    assert_eq!(buyer.stage, SolStage::WaitingForSecret, "found, not locked twice");
    w.step(&mut seller, &mut buyer, 6);
    assert_eq!(w.orn(&w.buyer), ORN);
}

#[test]
fn bad_offers_and_addresses_are_refused() {
    let w = world();
    let mut seller = w.sell();
    let offer = seller.offer("test".into()).unwrap();
    let wallet = solana::text(&w.wallet.pubkey());
    let unused = solana::text(&Keypair::random().pubkey());
    assert!(SolSwap::buy(&w.buyer, &w.sol, "test", &offer, &unused).is_err(), "a refund address that never held SOL");
    assert!(SolSwap::buy(&w.buyer, &w.sol, "test", &offer, "not an address").is_err());
    assert!(SolSwap::buy(&w.seller, &w.sol, "test", &offer, &wallet).is_err(), "for another wallet");
    let mut other = offer.clone();
    other.network = "elsewhere".into();
    assert!(SolSwap::buy(&w.buyer, &w.sol, "test", &other, &wallet).is_err());
    let mut more = offer.clone();
    more.orn += 1;
    assert!(SolSwap::buy(&w.buyer, &w.sol, "test", &more, &wallet).is_err(), "not what is locked");
    assert!(SolSwap::sell(&w.seller, "test", w.buyer.me, ORN, MIN_LAMPORTS - 1, &solana::text(&w.payout.pubkey()), &[1; 32]).is_err());
    assert!(seller.accept(&SolReply { hash: [0; 32], lock: wallet.clone(), refund: wallet }).is_err(), "another swap's reply");
    assert_eq!(seller.role, Role::Seller);
    let _ = CUSHION;
}
