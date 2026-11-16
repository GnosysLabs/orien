//! The built program, run in LiteSVM: everything it pays out, and everything
//! it refuses. Build it first: `cargo build-sbf` in ../orien-lock.

use litesvm::LiteSVM;
use solana_address::Address;
use solana_clock::Clock;
use solana_instruction::{AccountMeta, Instruction};
use solana_keypair::Keypair;
use solana_signer::Signer;
use solana_system_interface::instruction::create_account;
use solana_transaction::Transaction;

const LEN: u64 = 113;
const SOL: u64 = 1_000_000_000;
const NOW: i64 = 1_800_000_000;
const DEADLINE: i64 = NOW + 3600;

struct World {
    svm: LiteSVM,
    program: Address,
    buyer: Keypair,
    seller: Keypair,
    /// A stranger, who pays the fee to send a claim or a refund.
    stranger: Keypair,
    secret: [u8; 32],
}

fn world() -> World {
    let mut svm = LiteSVM::new();
    let program = Address::new_unique();
    let bytes = std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/../orien-lock/target/deploy/orien_lock.so"))
        .expect("build ../orien-lock with cargo build-sbf first");
    svm.add_program(program, &bytes).unwrap();
    let (buyer, seller, stranger) = (Keypair::new(), Keypair::new(), Keypair::new());
    for who in [&buyer, &seller, &stranger] {
        svm.airdrop(&who.pubkey(), 10 * SOL).unwrap();
    }
    let mut world = World { svm, program, buyer, seller, stranger, secret: [7; 32] };
    world.set_time(NOW);
    world
}

impl World {
    fn set_time(&mut self, unix: i64) {
        let mut clock: Clock = self.svm.get_sysvar();
        clock.unix_timestamp = unix;
        self.svm.set_sysvar(&clock);
    }

    fn balance(&self, who: &Address) -> u64 {
        self.svm.get_balance(who).unwrap_or(0)
    }

    fn terms(&self, seller: &Address, buyer: &Address, amount: u64) -> Vec<u8> {
        let hash = solana_sha256_hasher::hash(&self.secret).to_bytes();
        let mut data = vec![0u8];
        data.extend_from_slice(&hash);
        data.extend_from_slice(seller.as_ref());
        data.extend_from_slice(buyer.as_ref());
        data.extend_from_slice(&DEADLINE.to_le_bytes());
        data.extend_from_slice(&amount.to_le_bytes());
        data
    }

    /// The buyer creates the lock account, funds it with `funded` lamports
    /// (the deposit included) and locks `amount` of them for the seller.
    fn lock(&mut self, lock: &Keypair, funded: u64, data: Vec<u8>) -> Result<(), String> {
        let create = create_account(&self.buyer.pubkey(), &lock.pubkey(), funded, LEN, &self.program);
        let ix = Instruction::new_with_bytes(self.program, &data, vec![AccountMeta::new(lock.pubkey(), true)]);
        let tx = Transaction::new_signed_with_payer(
            &[create, ix],
            Some(&self.buyer.pubkey()),
            &[&self.buyer, lock],
            self.svm.latest_blockhash(),
        );
        self.svm.send_transaction(tx).map(|_| ()).map_err(|e| format!("{:?}", e.err))
    }

    fn deposit(&self) -> u64 {
        self.svm.minimum_balance_for_rent_exemption(LEN as usize)
    }

    fn locked(&mut self, amount: u64) -> Keypair {
        let lock = Keypair::new();
        let data = self.terms(&self.seller.pubkey(), &self.buyer.pubkey(), amount);
        let funded = amount + self.deposit();
        self.lock(&lock, funded, data).expect("lock");
        lock
    }

    fn send(&mut self, ix: Instruction) -> Result<(), String> {
        self.svm.expire_blockhash();
        let tx = Transaction::new_signed_with_payer(&[ix], Some(&self.stranger.pubkey()), &[&self.stranger], self.svm.latest_blockhash());
        self.svm.send_transaction(tx).map(|_| ()).map_err(|e| format!("{:?}", e.err))
    }

    fn claim(&mut self, lock: &Address, secret: [u8; 32], to_seller: &Address, to_buyer: &Address) -> Result<(), String> {
        let mut data = vec![1u8];
        data.extend_from_slice(&secret);
        let accounts = vec![AccountMeta::new(*lock, false), AccountMeta::new(*to_seller, false), AccountMeta::new(*to_buyer, false)];
        self.send(Instruction::new_with_bytes(self.program, &data, accounts))
    }

    fn refund(&mut self, lock: &Address, to_buyer: &Address) -> Result<(), String> {
        let accounts = vec![AccountMeta::new(*lock, false), AccountMeta::new(*to_buyer, false)];
        self.send(Instruction::new_with_bytes(self.program, &[2], accounts))
    }
}

#[test]
fn the_secret_pays_the_seller_and_the_deposit_goes_back_to_the_buyer() {
    let mut w = world();
    let lock = w.locked(SOL);
    let (seller, buyer) = (w.seller.pubkey(), w.buyer.pubkey());
    let (seller_before, buyer_before) = (w.balance(&seller), w.balance(&buyer));
    // A stranger sends it, paying the fee: the coins still only go where the lock says.
    w.claim(&lock.pubkey(), w.secret, &seller, &buyer).expect("claim");
    assert_eq!(w.balance(&seller), seller_before + SOL, "the seller gets exactly the amount");
    assert_eq!(w.balance(&buyer), buyer_before + w.deposit(), "the buyer gets the deposit back");
    assert_eq!(w.balance(&lock.pubkey()), 0, "the lock is closed");
    assert!(w.claim(&lock.pubkey(), w.secret, &seller, &buyer).is_err(), "and cannot pay twice");
}

#[test]
fn a_wrong_secret_another_payee_or_a_late_claim_is_refused() {
    let mut w = world();
    let lock = w.locked(SOL);
    let (seller, buyer, stranger) = (w.seller.pubkey(), w.buyer.pubkey(), w.stranger.pubkey());
    assert!(w.claim(&lock.pubkey(), [8; 32], &seller, &buyer).is_err(), "wrong secret");
    assert!(w.claim(&lock.pubkey(), w.secret, &stranger, &buyer).is_err(), "someone else as the seller");
    assert!(w.claim(&lock.pubkey(), w.secret, &seller, &stranger).is_err(), "someone else as the buyer");
    assert!(w.claim(&lock.pubkey(), w.secret, &seller, &seller).is_err(), "the seller twice");
    w.set_time(DEADLINE);
    assert!(w.claim(&lock.pubkey(), w.secret, &seller, &buyer).is_err(), "at the deadline it is too late");
    assert_eq!(w.balance(&lock.pubkey()), SOL + w.deposit(), "nothing moved");
}

#[test]
fn after_the_deadline_everything_goes_back_to_the_buyer_and_not_before() {
    let mut w = world();
    let lock = w.locked(SOL);
    let (buyer, stranger) = (w.buyer.pubkey(), w.stranger.pubkey());
    assert!(w.refund(&lock.pubkey(), &buyer).is_err(), "before the deadline");
    w.set_time(DEADLINE - 1);
    assert!(w.refund(&lock.pubkey(), &buyer).is_err(), "a second before");
    w.set_time(DEADLINE);
    assert!(w.refund(&lock.pubkey(), &stranger).is_err(), "only to the buyer");
    let before = w.balance(&buyer);
    w.refund(&lock.pubkey(), &buyer).expect("refund");
    assert_eq!(w.balance(&buyer), before + SOL + w.deposit());
    assert_eq!(w.balance(&lock.pubkey()), 0);
}

#[test]
fn a_lock_is_written_once_and_must_hold_what_it_promises() {
    let mut w = world();
    let (seller, buyer) = (w.seller.pubkey(), w.buyer.pubkey());
    let deposit = w.deposit();
    // More promised than the account holds.
    let lock = Keypair::new();
    assert!(w.lock(&lock, SOL + deposit, w.terms(&seller, &buyer, 2 * SOL)).is_err());
    // The same address as seller and buyer.
    let lock = Keypair::new();
    assert!(w.lock(&lock, SOL + deposit, w.terms(&buyer, &buyer, SOL)).is_err());
    // Too short.
    let lock = Keypair::new();
    let mut short = w.terms(&seller, &buyer, SOL);
    short.pop();
    assert!(w.lock(&lock, SOL + deposit, short).is_err());

    // A lock in place cannot be written over, even to change the payee.
    let lock = w.locked(SOL);
    let rewrite = w.terms(&w.stranger.pubkey(), &buyer, SOL);
    let ix = Instruction::new_with_bytes(w.program, &rewrite, vec![AccountMeta::new(lock.pubkey(), true)]);
    w.svm.expire_blockhash();
    let tx = Transaction::new_signed_with_payer(&[ix], Some(&w.buyer.pubkey()), &[&w.buyer, &lock], w.svm.latest_blockhash());
    assert!(w.svm.send_transaction(tx).is_err());
    w.claim(&lock.pubkey(), w.secret, &seller, &buyer).expect("the original terms still hold");
}

#[test]
fn locking_needs_the_lock_accounts_own_signature() {
    let mut w = world();
    let lock = Keypair::new();
    let create = create_account(&w.buyer.pubkey(), &lock.pubkey(), SOL + w.deposit(), LEN, &w.program);
    let data = w.terms(&w.seller.pubkey(), &w.buyer.pubkey(), SOL);
    // Created in one transaction, then locked by someone else without its key.
    let tx = Transaction::new_signed_with_payer(&[create], Some(&w.buyer.pubkey()), &[&w.buyer, &lock], w.svm.latest_blockhash());
    w.svm.send_transaction(tx).unwrap();
    let ix = Instruction::new_with_bytes(w.program, &data, vec![AccountMeta::new(lock.pubkey(), false)]);
    assert!(w.send(ix).is_err());
}

#[test]
fn it_is_small() {
    let size = std::fs::metadata(concat!(env!("CARGO_MANIFEST_DIR"), "/../orien-lock/target/deploy/orien_lock.so")).unwrap().len();
    println!("orien_lock.so: {size} bytes");
    assert!(size <= 4096, "{size} bytes");
}
