//! Exhaustive LiteSVM tests for orien-order.
//! Build ../orien-order first with `cargo build-sbf`.

use litesvm::LiteSVM;
use solana_address::Address;
use solana_clock::Clock;
use solana_instruction::{AccountMeta, Instruction};
use solana_keypair::Keypair;
use solana_signer::Signer;
use solana_system_interface::instruction::{create_account, transfer};
use solana_transaction::Transaction;

const LEN: u64 = 217;
const SOL: u64 = 1_000_000_000;
const NOW: i64 = 1_800_000_000;
const EXPIRY: i64 = NOW + 7 * 24 * 3600;
const MAX_FILL_WINDOW: i64 = 48 * 3600;
const ORN_AMOUNT: u64 = 500_000_000_000; // 500 ORN
const BUYER_ORIEN: [u8; 32] = [0xab; 32];

struct World {
    svm: LiteSVM,
    program: Address,
    buyer: Keypair,
    authority: Keypair,
    seller: Keypair,
    other_seller: Keypair,
    /// A relayer / server paying fees to call fill, claim, reopen, or expire.
    relayer: Keypair,
    secret: [u8; 32],
}

fn world() -> World {
    let mut svm = LiteSVM::new();
    let program = Address::new_unique();
    let bytes = std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/../orien-order/target/deploy/orien_order.so"))
        .expect("build ../orien-order with cargo build-sbf first");
    svm.add_program(program, &bytes).unwrap();
    let (buyer, authority, seller, other_seller, relayer) =
        (Keypair::new(), Keypair::new(), Keypair::new(), Keypair::new(), Keypair::new());
    for who in [&buyer, &authority, &seller, &other_seller, &relayer] {
        svm.airdrop(&who.pubkey(), 10 * SOL).unwrap();
    }
    let mut w = World { svm, program, buyer, authority, seller, other_seller, relayer, secret: [0x42; 32] };
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

    fn deposit(&self) -> u64 {
        self.svm.minimum_balance_for_rent_exemption(LEN as usize)
    }

    fn open_data(
        &self,
        buyer: &Address,
        authority: &Address,
        orn_amount: u64,
        amount: u64,
        expiry: i64,
        min_bond: u64,
        max_fill_window: i64,
    ) -> Vec<u8> {
        let hash = solana_sha256_hasher::hash(&self.secret).to_bytes();
        let mut data = vec![0u8];
        data.extend_from_slice(&hash);
        data.extend_from_slice(buyer.as_ref());
        data.extend_from_slice(authority.as_ref());
        data.extend_from_slice(&BUYER_ORIEN);
        data.extend_from_slice(&orn_amount.to_le_bytes());
        data.extend_from_slice(&amount.to_le_bytes());
        data.extend_from_slice(&expiry.to_le_bytes());
        data.extend_from_slice(&min_bond.to_le_bytes());
        data.extend_from_slice(&max_fill_window.to_le_bytes());
        data
    }

    fn open_raw(&mut self, order: &Keypair, funded: u64, data: Vec<u8>) -> Result<(), String> {
        self.svm.expire_blockhash();
        let create = create_account(&self.buyer.pubkey(), &order.pubkey(), funded, LEN, &self.program);
        let ix = Instruction::new_with_bytes(self.program, &data, vec![AccountMeta::new(order.pubkey(), true)]);
        let tx = Transaction::new_signed_with_payer(
            &[create, ix],
            Some(&self.buyer.pubkey()),
            &[&self.buyer, order],
            self.svm.latest_blockhash(),
        );
        self.svm.send_transaction(tx).map(|_| ()).map_err(|e| format!("{:?}", e.err))
    }

    fn open_order(&mut self, amount: u64, min_bond: u64) -> Keypair {
        let order = Keypair::new();
        let data = self.open_data(
            &self.buyer.pubkey(),
            &self.authority.pubkey(),
            ORN_AMOUNT,
            amount,
            EXPIRY,
            min_bond,
            MAX_FILL_WINDOW,
        );
        let funded = amount + self.deposit();
        self.open_raw(&order, funded, data).expect("open_order");
        order
    }

    fn send_as(&mut self, ixs: &[Instruction], payer: &Keypair, signers: &[&Keypair]) -> Result<(), String> {
        self.svm.expire_blockhash();
        let tx = Transaction::new_signed_with_payer(ixs, Some(&payer.pubkey()), signers, self.svm.latest_blockhash());
        self.svm.send_transaction(tx).map(|_| ()).map_err(|e| format!("{:?}", e.err))
    }

    fn fill(&mut self, order: &Address, seller: &Address, fill_window: i64, bond: u64) -> Result<(), String> {
        let mut data = vec![2u8];
        data.extend_from_slice(seller.as_ref());
        data.extend_from_slice(&fill_window.to_le_bytes());
        let mut ixs = Vec::new();
        if bond > 0 {
            ixs.push(transfer(&self.relayer.pubkey(), order, bond));
        }
        ixs.push(Instruction::new_with_bytes(self.program, &data, vec![AccountMeta::new(*order, false)]));
        self.svm.expire_blockhash();
        let tx = Transaction::new_signed_with_payer(&ixs, Some(&self.relayer.pubkey()), &[&self.relayer], self.svm.latest_blockhash());
        self.svm.send_transaction(tx).map(|_| ()).map_err(|e| format!("{:?}", e.err))
    }

    fn claim(&mut self, order: &Address, secret: [u8; 32], to_seller: &Address, to_buyer: &Address) -> Result<(), String> {
        let mut data = vec![3u8];
        data.extend_from_slice(&secret);
        let ix = Instruction::new_with_bytes(
            self.program,
            &data,
            vec![
                AccountMeta::new(*order, false),
                AccountMeta::new(*to_seller, false),
                AccountMeta::new(*to_buyer, false),
            ],
        );
        self.svm.expire_blockhash();
        let tx = Transaction::new_signed_with_payer(&[ix], Some(&self.relayer.pubkey()), &[&self.relayer], self.svm.latest_blockhash());
        self.svm.send_transaction(tx).map(|_| ()).map_err(|e| format!("{:?}", e.err))
    }

    fn cancel(&mut self, order: &Address, signer: &Keypair, to_buyer: &Address) -> Result<(), String> {
        let ix = Instruction::new_with_bytes(
            self.program,
            &[1],
            vec![
                AccountMeta::new(*order, false),
                AccountMeta::new_readonly(signer.pubkey(), true),
                AccountMeta::new(*to_buyer, false),
            ],
        );
        self.svm.expire_blockhash();
        let tx = Transaction::new_signed_with_payer(
            &[ix],
            Some(&self.relayer.pubkey()),
            &[&self.relayer, signer],
            self.svm.latest_blockhash(),
        );
        self.svm.send_transaction(tx).map(|_| ()).map_err(|e| format!("{:?}", e.err))
    }

    fn cancel_same_account(&mut self, order: &Address, buyer_and_auth: &Keypair) -> Result<(), String> {
        let ix = Instruction::new_with_bytes(
            self.program,
            &[1],
            vec![
                AccountMeta::new(*order, false),
                AccountMeta::new(buyer_and_auth.pubkey(), true),
                AccountMeta::new(buyer_and_auth.pubkey(), false),
            ],
        );
        self.svm.expire_blockhash();
        let tx = Transaction::new_signed_with_payer(
            &[ix],
            Some(&self.relayer.pubkey()),
            &[&self.relayer, buyer_and_auth],
            self.svm.latest_blockhash(),
        );
        self.svm.send_transaction(tx).map(|_| ()).map_err(|e| format!("{:?}", e.err))
    }

    fn reopen(&mut self, order: &Address, to_buyer: &Address) -> Result<(), String> {
        let ix = Instruction::new_with_bytes(
            self.program,
            &[4],
            vec![AccountMeta::new(*order, false), AccountMeta::new(*to_buyer, false)],
        );
        self.svm.expire_blockhash();
        let tx = Transaction::new_signed_with_payer(&[ix], Some(&self.relayer.pubkey()), &[&self.relayer], self.svm.latest_blockhash());
        self.svm.send_transaction(tx).map(|_| ()).map_err(|e| format!("{:?}", e.err))
    }

    fn expire(&mut self, order: &Address, to_buyer: &Address) -> Result<(), String> {
        let ix = Instruction::new_with_bytes(
            self.program,
            &[5],
            vec![AccountMeta::new(*order, false), AccountMeta::new(*to_buyer, false)],
        );
        self.svm.expire_blockhash();
        let tx = Transaction::new_signed_with_payer(&[ix], Some(&self.relayer.pubkey()), &[&self.relayer], self.svm.latest_blockhash());
        self.svm.send_transaction(tx).map(|_| ()).map_err(|e| format!("{:?}", e.err))
    }
}

#[test]
fn fill_then_claim_pays_only_recorded_seller_and_returns_deposit_to_buyer() {
    let mut w = world();
    let bond = SOL / 200; // 0.005 SOL bond
    let order = w.open_order(SOL, bond);
    let (seller, other, buyer) = (w.seller.pubkey(), w.other_seller.pubkey(), w.buyer.pubkey());

    // Claiming before fill is refused.
    assert!(w.claim(&order.pubkey(), w.secret, &seller, &buyer).is_err(), "cannot claim unfilled");
    // Filling with insufficient bond is refused.
    assert!(w.fill(&order.pubkey(), &seller, 3600, bond - 1).is_err(), "bond too small");
    // Filling with window > max_fill_window is refused.
    assert!(w.fill(&order.pubkey(), &seller, MAX_FILL_WINDOW + 1, bond).is_err(), "window too long");

    w.fill(&order.pubkey(), &seller, 3600, bond).expect("fill");
    // Double fill is strictly refused (first come, first served).
    assert!(w.fill(&order.pubkey(), &other, 3600, bond).is_err(), "double fill refused");
    // Wrong secret is refused.
    assert!(w.claim(&order.pubkey(), [0x99; 32], &seller, &buyer).is_err(), "wrong secret");
    // Claiming to someone other than the recorded seller is refused.
    assert!(w.claim(&order.pubkey(), w.secret, &other, &buyer).is_err(), "wrong seller");
    // Claiming with wrong buyer deposit address is refused.
    assert!(w.claim(&order.pubkey(), w.secret, &seller, &other).is_err(), "wrong buyer");

    let (seller_before, buyer_before) = (w.balance(&seller), w.balance(&buyer));
    w.claim(&order.pubkey(), w.secret, &seller, &buyer).expect("claim");
    assert_eq!(w.balance(&seller), seller_before + SOL + bond, "seller gets SOL + bond back");
    assert_eq!(w.balance(&buyer), buyer_before + w.deposit(), "buyer gets rent deposit back");
    assert_eq!(w.balance(&order.pubkey()), 0, "order closed");
    assert!(w.claim(&order.pubkey(), w.secret, &seller, &buyer).is_err(), "cannot claim twice");
}

#[test]
fn cancel_works_when_open_or_fill_lapsed_and_refused_during_active_fill() {
    let mut w = world();
    let order = w.open_order(SOL, 0);
    let (buyer, seller) = (w.buyer.pubkey(), w.seller.pubkey());
    let wrong_signer = Keypair::new();
    let auth_clone = w.authority.insecure_clone();

    // Wrong signer cannot cancel.
    assert!(w.cancel(&order.pubkey(), &wrong_signer, &buyer).is_err(), "wrong authority");
    // Authority cannot redirect funds to another address.
    assert!(w.cancel(&order.pubkey(), &auth_clone, &seller).is_err(), "only pays buyer");

    // Cancel succeeds while open.
    let before = w.balance(&buyer);
    w.cancel(&order.pubkey(), &auth_clone, &buyer).expect("cancel open");
    assert_eq!(w.balance(&buyer), before + SOL + w.deposit());
    assert_eq!(w.balance(&order.pubkey()), 0);

    // Authority == buyer (same Solana account) also works cleanly!
    let order2 = Keypair::new();
    let buyer_clone = w.buyer.insecure_clone();
    let data = w.open_data(&buyer, &buyer, ORN_AMOUNT, SOL, EXPIRY, 0, MAX_FILL_WINDOW);
    w.open_raw(&order2, SOL + w.deposit(), data).expect("open same auth");
    let before2 = w.balance(&buyer);
    w.cancel_same_account(&order2.pubkey(), &buyer_clone).expect("cancel same account");
    assert_eq!(w.balance(&buyer), before2 + SOL + w.deposit());

    // Cannot cancel while an active fill is within its window; CAN cancel once fill lapses.
    let bond = SOL / 100;
    let order3 = w.open_order(SOL, bond);
    w.fill(&order3.pubkey(), &seller, 3600, bond).expect("fill");
    assert!(w.cancel(&order3.pubkey(), &auth_clone, &buyer).is_err(), "active fill blocks cancel");
    w.set_time(NOW + 3599);
    assert!(w.cancel(&order3.pubkey(), &auth_clone, &buyer).is_err(), "one second before lapse");
    w.set_time(NOW + 3600);
    let before3 = w.balance(&buyer);
    w.cancel(&order3.pubkey(), &auth_clone, &buyer).expect("cancel after fill lapse");
    assert_eq!(w.balance(&buyer), before3 + SOL + w.deposit() + bond, "buyer gets SOL + deposit + forfeited bond");
    assert_eq!(w.balance(&order3.pubkey()), 0);
}

#[test]
fn lapsed_fill_can_reopen_and_be_filled_and_claimed_by_next_seller() {
    let mut w = world();
    let bond = SOL / 200;
    let order = w.open_order(SOL, bond);
    let (seller1, seller2, buyer) = (w.seller.pubkey(), w.other_seller.pubkey(), w.buyer.pubkey());

    w.fill(&order.pubkey(), &seller1, 1800, bond).expect("first fill");
    // Cannot reopen before fill_deadline.
    assert!(w.reopen(&order.pubkey(), &buyer).is_err(), "before lapse");
    w.set_time(NOW + 1800);
    // Late claim by seller1 is refused at deadline.
    assert!(w.claim(&order.pubkey(), w.secret, &seller1, &buyer).is_err(), "late claim refused");
    // Reopen forfeits seller1's bond to buyer and reopens order.
    let buyer_before = w.balance(&buyer);
    w.reopen(&order.pubkey(), &buyer).expect("reopen");
    assert_eq!(w.balance(&buyer), buyer_before + bond, "forfeited bond paid to buyer");
    assert_eq!(w.balance(&order.pubkey()), SOL + w.deposit(), "order still holds SOL + rent");

    // Second seller fills and claims cleanly.
    w.fill(&order.pubkey(), &seller2, 1800, bond).expect("second fill");
    let (s2_before, b_before) = (w.balance(&seller2), w.balance(&buyer));
    w.claim(&order.pubkey(), w.secret, &seller2, &buyer).expect("second claim");
    assert_eq!(w.balance(&seller2), s2_before + SOL + bond);
    assert_eq!(w.balance(&buyer), b_before + w.deposit());
    assert_eq!(w.balance(&order.pubkey()), 0);
}

#[test]
fn expire_refunds_buyer_at_expiry_and_blocks_late_fill_or_claim() {
    let mut w = world();
    let order = w.open_order(SOL, 0);
    let (buyer, seller) = (w.buyer.pubkey(), w.seller.pubkey());

    assert!(w.expire(&order.pubkey(), &buyer).is_err(), "before expiry");
    w.set_time(EXPIRY - 1);
    assert!(w.expire(&order.pubkey(), &buyer).is_err(), "one second before expiry");
    w.set_time(EXPIRY);
    assert!(w.fill(&order.pubkey(), &seller, 600, 0).is_err(), "cannot fill at expiry");
    assert!(w.expire(&order.pubkey(), &seller).is_err(), "expire only pays buyer");

    let before = w.balance(&buyer);
    w.expire(&order.pubkey(), &buyer).expect("expire");
    assert_eq!(w.balance(&buyer), before + SOL + w.deposit());
    assert_eq!(w.balance(&order.pubkey()), 0);

    // Fill near expiry is clamped to expiry, so expire works immediately at EXPIRY with no claim race.
    w.set_time(NOW);
    let order2 = w.open_order(SOL, 0);
    w.set_time(EXPIRY - 60);
    w.fill(&order2.pubkey(), &seller, 3600, 0).expect("fill clamped to expiry");
    w.set_time(EXPIRY);
    assert!(w.claim(&order2.pubkey(), w.secret, &seller, &buyer).is_err(), "clamped claim refused at expiry");
    assert!(w.reopen(&order2.pubkey(), &buyer).is_err(), "reopen refused at expiry");
    let before2 = w.balance(&buyer);
    w.expire(&order2.pubkey(), &buyer).expect("expire filled order at expiry");
    assert_eq!(w.balance(&buyer), before2 + SOL + w.deposit());
}

#[test]
fn open_order_validation_and_small_binary_size() {
    let mut w = world();
    let (buyer, auth) = (w.buyer.pubkey(), w.authority.pubkey());
    let dep = w.deposit();

    // Insufficient SOL funded.
    let o = Keypair::new();
    let d = w.open_data(&buyer, &auth, ORN_AMOUNT, 2 * SOL, EXPIRY, 0, MAX_FILL_WINDOW);
    assert!(w.open_raw(&o, SOL + dep, d).is_err(), "underfunded");
    // Zero ORN or zero SOL or past expiry refused.
    let o = Keypair::new();
    let d = w.open_data(&buyer, &auth, 0, SOL, EXPIRY, 0, MAX_FILL_WINDOW);
    assert!(w.open_raw(&o, SOL + dep, d).is_err(), "zero orn");
    let o = Keypair::new();
    let d = w.open_data(&buyer, &auth, ORN_AMOUNT, 0, EXPIRY, 0, MAX_FILL_WINDOW);
    assert!(w.open_raw(&o, SOL + dep, d).is_err(), "zero amount");
    let o = Keypair::new();
    let d = w.open_data(&buyer, &auth, ORN_AMOUNT, SOL, NOW, 0, MAX_FILL_WINDOW);
    assert!(w.open_raw(&o, SOL + dep, d).is_err(), "expiry in past");
    // Cannot overwrite an already-open order.
    let order = w.open_order(SOL, 0);
    let overwrite = w.open_data(&w.seller.pubkey(), &auth, ORN_AMOUNT, SOL, EXPIRY, 0, MAX_FILL_WINDOW);
    let ix = Instruction::new_with_bytes(w.program, &overwrite, vec![AccountMeta::new(order.pubkey(), true)]);
    let buyer_clone = w.buyer.insecure_clone();
    assert!(w.send_as(&[ix], &buyer_clone, &[&buyer_clone, &order]).is_err(), "cannot overwrite");

    let size = std::fs::metadata(concat!(env!("CARGO_MANIFEST_DIR"), "/../orien-order/target/deploy/orien_order.so"))
        .unwrap()
        .len();
    println!("orien_order.so: {size} bytes");
    assert!(size <= 10_240, "unexpectedly large binary: {size} bytes");
}
