//! LiteSVM tests for orien-book: buy orders that fill in parts.
//! Build ../orien-book first with `cargo build-sbf`.

use litesvm::LiteSVM;
use solana_address::Address;
use solana_clock::Clock;
use solana_instruction::{AccountMeta, Instruction};
use solana_keypair::Keypair;
use solana_signer::Signer;
use solana_system_interface::instruction::create_account;
use solana_transaction::Transaction;

const LEN: u64 = 585;
const SOL: u64 = 1_000_000_000;
const NOW: i64 = 1_800_000_000;
const EXPIRY: i64 = NOW + 7 * 24 * 3600;
const WINDOW: i64 = 4 * 3600;
const ORN: u64 = 1_000_000_000;
const MIN_PART: u64 = 10_000_000; // 0.01 SOL
const BUYER_ORIEN: [u8; 32] = [0xab; 32];

fn sha(parts: &[&[u8]]) -> [u8; 32] {
    solana_sha256_hasher::hashv(parts).to_bytes()
}

/// 64 secrets, their hashes, and the Merkle tree over them (levels, leaves first).
struct Secrets {
    secrets: Vec<[u8; 32]>,
    levels: Vec<Vec<[u8; 32]>>,
}

impl Secrets {
    fn new(seed: u8) -> Self {
        let secrets: Vec<[u8; 32]> = (0..64u8).map(|i| { let mut s = [seed; 32]; s[0] = i; s }).collect();
        let mut levels = vec![secrets.iter().map(|s| sha(&[s])).collect::<Vec<_>>()];
        while levels.last().unwrap().len() > 1 {
            let below = levels.last().unwrap();
            levels.push(below.chunks(2).map(|pair| sha(&[&[1u8], &pair[0], &pair[1]])).collect());
        }
        Secrets { secrets, levels }
    }
    fn root(&self) -> [u8; 32] {
        self.levels.last().unwrap()[0]
    }
    fn hash(&self, index: usize) -> [u8; 32] {
        self.levels[0][index]
    }
    fn proof(&self, index: usize) -> Vec<[u8; 32]> {
        let mut at = index;
        (0..6).map(|level| { let sibling = self.levels[level][at ^ 1]; at >>= 1; sibling }).collect()
    }
}

struct World {
    svm: LiteSVM,
    program: Address,
    buyer: Keypair,
    authority: Keypair,
    filler: Keypair,
    sellers: Vec<Keypair>,
    secrets: Secrets,
}

fn world() -> World {
    let mut svm = LiteSVM::new();
    let program = Address::new_unique();
    let bytes = std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/../orien-book/target/deploy/orien_book.so"))
        .expect("build ../orien-book with cargo build-sbf first");
    svm.add_program(program, &bytes).unwrap();
    let (buyer, authority, filler) = (Keypair::new(), Keypair::new(), Keypair::new());
    let sellers: Vec<Keypair> = (0..5).map(|_| Keypair::new()).collect();
    for who in [&buyer, &authority, &filler].into_iter().chain(sellers.iter()) {
        svm.airdrop(&who.pubkey(), 10 * SOL).unwrap();
    }
    let mut w = World { svm, program, buyer, authority, filler, sellers, secrets: Secrets::new(0x42) };
    w.set_time(NOW);
    w
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
    fn rent(&self) -> u64 {
        self.svm.minimum_balance_for_rent_exemption(LEN as usize)
    }
    fn send(&mut self, ixs: &[Instruction], payer: &Keypair, signers: &[&Keypair]) -> Result<(), String> {
        self.svm.expire_blockhash();
        let tx = Transaction::new_signed_with_payer(ixs, Some(&payer.pubkey()), signers, self.svm.latest_blockhash());
        self.svm.send_transaction(tx).map(|_| ()).map_err(|e| format!("{:?}", e.err))
    }

    fn open_with(&mut self, orn: u64, lamports: u64, root: [u8; 32]) -> Result<Keypair, String> {
        let order = Keypair::new();
        let mut data = vec![0u8];
        data.extend_from_slice(&root);
        data.extend_from_slice(self.buyer.pubkey().as_ref());
        data.extend_from_slice(self.authority.pubkey().as_ref());
        data.extend_from_slice(self.filler.pubkey().as_ref());
        data.extend_from_slice(&BUYER_ORIEN);
        data.extend_from_slice(&orn.to_le_bytes());
        data.extend_from_slice(&lamports.to_le_bytes());
        data.extend_from_slice(&EXPIRY.to_le_bytes());
        data.extend_from_slice(&WINDOW.to_le_bytes());
        data.extend_from_slice(&MIN_PART.to_le_bytes());
        let create = create_account(&self.buyer.pubkey(), &order.pubkey(), lamports + self.rent(), LEN, &self.program);
        let ix = Instruction::new_with_bytes(self.program, &data, vec![AccountMeta::new(order.pubkey(), true)]);
        let buyer = self.buyer.insecure_clone();
        self.send(&[create, ix], &buyer, &[&buyer, &order])?;
        Ok(order)
    }

    fn open(&mut self, orn: u64, lamports: u64) -> Keypair {
        let root = self.secrets.root();
        self.open_with(orn, lamports, root).expect("open")
    }

    fn fill_as(&mut self, order: &Address, index: usize, hash: [u8; 32], proof: Vec<[u8; 32]>, seller: usize, orn: u64, signer: &Keypair) -> Result<(), String> {
        let mut data = vec![1u8, index as u8];
        data.extend_from_slice(&hash);
        for p in &proof {
            data.extend_from_slice(p);
        }
        data.extend_from_slice(self.sellers[seller].pubkey().as_ref());
        data.extend_from_slice(&orn.to_le_bytes());
        data.extend_from_slice(&WINDOW.to_le_bytes());
        let ix = Instruction::new_with_bytes(self.program, &data, vec![AccountMeta::new(*order, false), AccountMeta::new_readonly(signer.pubkey(), true)]);
        let signer = signer.insecure_clone();
        self.send(&[ix], &signer, &[&signer])
    }

    fn fill(&mut self, order: &Address, index: usize, seller: usize, orn: u64) -> Result<(), String> {
        let (hash, proof) = (self.secrets.hash(index), self.secrets.proof(index));
        let filler = self.filler.insecure_clone();
        self.fill_as(order, index, hash, proof, seller, orn, &filler)
    }

    fn claim(&mut self, order: &Address, slot: u8, secret: [u8; 32], seller: usize) -> Result<(), String> {
        let mut data = vec![2u8, slot];
        data.extend_from_slice(&secret);
        let ix = Instruction::new_with_bytes(
            self.program,
            &data,
            vec![AccountMeta::new(*order, false), AccountMeta::new(self.sellers[seller].pubkey(), false), AccountMeta::new(self.buyer.pubkey(), false)],
        );
        let payer = self.filler.insecure_clone();
        self.send(&[ix], &payer, &[&payer])
    }

    fn release(&mut self, order: &Address, slot: u8) -> Result<(), String> {
        let ix = Instruction::new_with_bytes(self.program, &[3u8, slot], vec![AccountMeta::new(*order, false)]);
        let payer = self.sellers[4].insecure_clone();
        self.send(&[ix], &payer, &[&payer])
    }

    fn cancel(&mut self, order: &Address) -> Result<(), String> {
        let ix = Instruction::new_with_bytes(
            self.program,
            &[4u8],
            vec![AccountMeta::new(*order, false), AccountMeta::new_readonly(self.authority.pubkey(), true), AccountMeta::new(self.buyer.pubkey(), false)],
        );
        let authority = self.authority.insecure_clone();
        self.send(&[ix], &authority, &[&authority])
    }

    fn expire(&mut self, order: &Address) -> Result<(), String> {
        let ix = Instruction::new_with_bytes(self.program, &[5u8], vec![AccountMeta::new(*order, false), AccountMeta::new(self.buyer.pubkey(), false)]);
        let payer = self.sellers[4].insecure_clone();
        self.send(&[ix], &payer, &[&payer])
    }

    fn left(&self, order: &Address) -> (u64, u64) {
        let data = self.svm.get_account(order).unwrap().data;
        // state 1 + open 200, then orn_left, lamports_left.
        (u64::from_le_bytes(data[201..209].try_into().unwrap()), u64::from_le_bytes(data[209..217].try_into().unwrap()))
    }
}

#[test]
fn an_order_fills_in_parts_and_each_seller_is_paid_their_share() {
    let mut w = world();
    let order = w.open(1_000 * ORN, SOL / 10); // 1,000 ORN for 0.1 SOL
    let buyer_before = w.balance(&w.buyer.pubkey());
    let s0 = w.balance(&w.sellers[0].pubkey());
    // 300 ORN: 0.03 SOL.
    w.fill(&order.pubkey(), 0, 0, 300 * ORN).unwrap();
    assert_eq!(w.left(&order.pubkey()), (700 * ORN, SOL / 10 - 30_000_000));
    w.claim(&order.pubkey(), 0, w.secrets.secrets[0], 0).unwrap();
    assert_eq!(w.balance(&w.sellers[0].pubkey()) - s0, 30_000_000);
    // Two more at once, in two slots; then the rest.
    w.fill(&order.pubkey(), 1, 1, 200 * ORN).unwrap();
    w.fill(&order.pubkey(), 2, 2, 100 * ORN).unwrap();
    w.claim(&order.pubkey(), 1, w.secrets.secrets[2], 2).unwrap();
    w.claim(&order.pubkey(), 0, w.secrets.secrets[1], 1).unwrap();
    w.fill(&order.pubkey(), 3, 3, 400 * ORN).unwrap();
    w.claim(&order.pubkey(), 0, w.secrets.secrets[3], 3).unwrap();
    // All filled: the account is gone, its rent back with the buyer.
    assert!(w.svm.get_account(&order.pubkey()).is_none_or(|a| a.lamports == 0));
    assert_eq!(w.balance(&w.buyer.pubkey()) - buyer_before, w.rent());
}

#[test]
fn parts_add_up_to_the_whole_exactly() {
    let mut w = world();
    let lamports = 99_999_997; // an awkward total
    let order = w.open(3 * ORN, lamports);
    let before: u64 = (0..3).map(|i| w.balance(&w.sellers[i].pubkey())).sum();
    for i in 0..3 {
        w.fill(&order.pubkey(), i, i, ORN).unwrap();
        w.claim(&order.pubkey(), 0, w.secrets.secrets[i], i).unwrap();
    }
    let after: u64 = (0..3).map(|i| w.balance(&w.sellers[i].pubkey())).sum();
    assert_eq!(after - before, lamports);
}

#[test]
fn only_the_filler_fills_and_only_with_a_proven_unused_secret() {
    let mut w = world();
    let order = w.open(1_000 * ORN, SOL / 10);
    let o = order.pubkey();
    // Someone else can't fill.
    let stranger = Keypair::new();
    w.svm.airdrop(&stranger.pubkey(), SOL).unwrap();
    let (h, p) = (w.secrets.hash(0), w.secrets.proof(0));
    assert!(w.fill_as(&o, 0, h, p.clone(), 0, 100 * ORN, &stranger).is_err());
    // A hash the buyer never committed to.
    let fake = sha(&[&[9u8; 32]]);
    let filler = w.filler.insecure_clone();
    assert!(w.fill_as(&o, 0, fake, p.clone(), 0, 100 * ORN, &filler).is_err());
    // The right hash at the wrong index.
    assert!(w.fill_as(&o, 1, h, p.clone(), 0, 100 * ORN, &filler).is_err());
    // Proven: fine. Then never again.
    w.fill(&o, 0, 0, 100 * ORN).unwrap();
    w.set_time(NOW + WINDOW + 1);
    w.release(&o, 0).unwrap();
    assert!(w.fill(&o, 0, 0, 100 * ORN).is_err(), "a used secret");
    w.fill(&o, 1, 0, 100 * ORN).unwrap();
}

#[test]
fn a_part_pays_only_its_seller_with_its_own_secret_before_its_deadline() {
    let mut w = world();
    let order = w.open(1_000 * ORN, SOL / 10);
    let o = order.pubkey();
    w.fill(&o, 0, 0, 100 * ORN).unwrap();
    w.fill(&o, 1, 1, 100 * ORN).unwrap();
    // Seller 1 can't take part 0's SOL, nor use part 0's secret for part 1.
    assert!(w.claim(&o, 0, w.secrets.secrets[0], 1).is_err());
    assert!(w.claim(&o, 1, w.secrets.secrets[0], 1).is_err());
    w.claim(&o, 0, w.secrets.secrets[0], 0).unwrap();
    assert!(w.claim(&o, 0, w.secrets.secrets[0], 0).is_err(), "paid once");
    // Too late: part 1 goes back to the order.
    w.set_time(NOW + WINDOW);
    assert!(w.claim(&o, 1, w.secrets.secrets[1], 1).is_err());
    w.release(&o, 1).unwrap();
    assert_eq!(w.left(&o), (900 * ORN, SOL / 10 - 10_000_000));
}

#[test]
fn release_waits_for_the_deadline() {
    let mut w = world();
    let order = w.open(1_000 * ORN, SOL / 10);
    w.fill(&order.pubkey(), 0, 0, 100 * ORN).unwrap();
    assert!(w.release(&order.pubkey(), 0).is_err());
    assert!(w.release(&order.pubkey(), 1).is_err(), "an empty slot");
}

#[test]
fn parts_have_a_smallest_size_and_leave_no_dust() {
    let mut w = world();
    let order = w.open(1_000 * ORN, SOL / 10);
    let o = order.pubkey();
    assert!(w.fill(&o, 0, 0, 50 * ORN).is_err(), "0.005 SOL is too small");
    assert!(w.fill(&o, 0, 0, 950 * ORN).is_err(), "would leave 0.005 SOL");
    assert!(w.fill(&o, 0, 0, 1_001 * ORN).is_err(), "more than the order");
    w.fill(&o, 0, 0, 1_000 * ORN).unwrap();
}

#[test]
fn at_most_four_parts_at_once() {
    let mut w = world();
    let order = w.open(1_000 * ORN, SOL / 10);
    let o = order.pubkey();
    for i in 0..4 {
        w.fill(&o, i, i, 100 * ORN).unwrap();
    }
    assert!(w.fill(&o, 4, 4, 100 * ORN).is_err());
    w.claim(&o, 2, w.secrets.secrets[2], 2).unwrap();
    w.fill(&o, 4, 4, 100 * ORN).unwrap();
}

#[test]
fn cancel_returns_whats_left_but_waits_for_parts_in_progress() {
    let mut w = world();
    let order = w.open(1_000 * ORN, SOL / 10);
    let o = order.pubkey();
    w.fill(&o, 0, 0, 300 * ORN).unwrap();
    assert!(w.cancel(&o).is_err());
    w.claim(&o, 0, w.secrets.secrets[0], 0).unwrap();
    let before = w.balance(&w.buyer.pubkey());
    w.cancel(&o).unwrap();
    assert_eq!(w.balance(&w.buyer.pubkey()) - before, 70_000_000 + w.rent());
}

#[test]
fn an_expired_order_returns_everything_to_the_buyer() {
    let mut w = world();
    let order = w.open(1_000 * ORN, SOL / 10);
    let o = order.pubkey();
    w.fill(&o, 0, 0, 300 * ORN).unwrap();
    assert!(w.expire(&o).is_err());
    w.set_time(EXPIRY);
    assert!(w.fill(&o, 1, 1, 100 * ORN).is_err(), "no parts once expired");
    let before = w.balance(&w.buyer.pubkey());
    w.expire(&o).unwrap();
    assert_eq!(w.balance(&w.buyer.pubkey()) - before, SOL / 10 + w.rent());
}

#[test]
fn an_order_is_opened_once_and_only_properly() {
    let mut w = world();
    let root = w.secrets.root();
    assert!(w.open_with(0, SOL / 10, root).is_err());
    assert!(w.open_with(1_000 * ORN, 0, root).is_err());
    let order = w.open(1_000 * ORN, SOL / 10);
    // Opening again over a live order is refused.
    let mut data = vec![0u8];
    data.extend_from_slice(&[0u8; 200]);
    let ix = Instruction::new_with_bytes(w.program, &data, vec![AccountMeta::new(order.pubkey(), true)]);
    let buyer = w.buyer.insecure_clone();
    assert!(w.send(&[ix], &buyer, &[&buyer, &order]).is_err());
}

#[test]
fn the_program_stays_small() {
    let size = std::fs::metadata(concat!(env!("CARGO_MANIFEST_DIR"), "/../orien-book/target/deploy/orien_book.so")).unwrap().len();
    assert!(size <= 20_480, "{size} bytes");
}

#[test]
fn money_only_goes_where_the_order_says() {
    let mut w = world();
    let order = w.open(1_000 * ORN, SOL / 10);
    let o = order.pubkey();
    // The buyer can't sell into their own order.
    let buyer_as_seller = {
        let mut data = vec![1u8, 0];
        data.extend_from_slice(&w.secrets.hash(0));
        for p in w.secrets.proof(0) { data.extend_from_slice(&p); }
        data.extend_from_slice(w.buyer.pubkey().as_ref());
        data.extend_from_slice(&(100 * ORN).to_le_bytes());
        data.extend_from_slice(&WINDOW.to_le_bytes());
        Instruction::new_with_bytes(w.program, &data, vec![AccountMeta::new(o, false), AccountMeta::new_readonly(w.filler.pubkey(), true)])
    };
    let filler = w.filler.insecure_clone();
    assert!(w.send(&[buyer_as_seller], &filler, &[&filler]).is_err());
    w.fill(&o, 0, 0, 100 * ORN).unwrap();
    // A claim naming another buyer account is refused.
    let mut data = vec![2u8, 0];
    data.extend_from_slice(&w.secrets.secrets[0]);
    let stranger = Keypair::new();
    let bad = Instruction::new_with_bytes(w.program, &data, vec![AccountMeta::new(o, false), AccountMeta::new(w.sellers[0].pubkey(), false), AccountMeta::new(stranger.pubkey(), false)]);
    assert!(w.send(&[bad], &filler, &[&filler]).is_err());
    // A slot that doesn't exist.
    assert!(w.claim(&o, 9, w.secrets.secrets[0], 0).is_err());
    // A stranger can't cancel, and expiry can't pay a stranger.
    w.claim(&o, 0, w.secrets.secrets[0], 0).unwrap();
    let cancel = Instruction::new_with_bytes(w.program, &[4u8], vec![AccountMeta::new(o, false), AccountMeta::new_readonly(stranger.pubkey(), true), AccountMeta::new(stranger.pubkey(), false)]);
    w.svm.airdrop(&stranger.pubkey(), SOL).unwrap();
    assert!(w.send(&[cancel], &stranger, &[&stranger]).is_err());
    w.set_time(EXPIRY);
    let expire = Instruction::new_with_bytes(w.program, &[5u8], vec![AccountMeta::new(o, false), AccountMeta::new(stranger.pubkey(), false)]);
    assert!(w.send(&[expire], &stranger, &[&stranger]).is_err());
    w.expire(&o).unwrap();
}
