//! Whole swaps between a seller and a buyer, on a pretend Bitcoin network
//! that applies Bitcoin Core's own rules, lock times and replace-by-fee, and
//! a pretend Orien that keeps locked payments as the ledger does. Each attack
//! a dishonest counterparty, a lying explorer or a crash could bring has its
//! test.

use anyhow::{Result, anyhow, bail, ensure};
use bitcoin::hashes::{Hash, sha256};
use bitcoin::{Address, CompressedPublicKey, Network, OutPoint, Transaction, Txid};
use orien_crypto::Hash32;
use orien_swap::chain::{Chain, Explorers};
use orien_swap::htlc::{Coin, new_key};
use orien_swap::swap::{
    BTC_BLOCKS, CONFIRMATIONS, LockRef, LockState, ORN_HOURS, ORN_HOURS_LEFT, Offer, Orien, Reply, SETTLED, Stage, Swap, Waiting,
};
use orien_swap::testing::TestChain;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;
use std::str::FromStr;

// ---------------------------------------------------------------- Orien

#[derive(Default)]
struct Ledger {
    epoch: u64,
    escrows: HashMap<(Hash32, u64), Waiting>,
    revealed: HashMap<(Hash32, u64), [u8; 32]>,
    balances: HashMap<Hash32, u64>,
    next_seq: HashMap<Hash32, u64>,
    /// How many locks were made, to catch a lock made twice.
    locks_made: u32,
    /// How many claims were signed and accepted.
    claims: u32,
}

impl Ledger {
    /// The checkpoint closing the current epoch: expired locks come back.
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
    /// Whether validators can be reached to prove anything, or to claim.
    reachable: Rc<Cell<bool>>,
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
        ensure!(self.reachable.get(), "no validator answers");
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
        let seq = l.next_seq.get(&self.me).copied().unwrap_or(0);
        l.next_seq.insert(self.me, seq + 1);
        l.escrows.insert((self.me, seq), Waiting { to: *to, amount, hash, until });
        l.locks_made += 1;
        Ok(LockRef { from: self.me, seq })
    }
    fn still_waiting(&self, lock: &LockRef) -> Result<Option<bool>> {
        ensure!(self.reachable.get(), "no validator answers");
        Ok(Some(self.ledger.borrow().escrows.contains_key(&(lock.from, lock.seq))))
    }
    fn claim(&self, lock: &LockRef, payee: &Hash32, amount: u64, secret: [u8; 32]) -> Result<()> {
        ensure!(self.reachable.get(), "no validator answers");
        let mut l = self.ledger.borrow_mut();
        let epoch = l.epoch;
        let w = l.escrows.get(&(lock.from, lock.seq)).cloned().ok_or_else(|| anyhow!("no such lock"))?;
        // Like the network once claims are open: anyone may claim, for the
        // lock's payee only.
        ensure!(w.to == *payee && w.amount == amount, "not that lock's payee");
        l.claims += 1;
        ensure!(epoch < w.until, "too late");
        ensure!(sha256::Hash::hash(&secret).to_byte_array() == w.hash, "wrong secret");
        l.escrows.remove(&(lock.from, lock.seq));
        l.revealed.insert((lock.from, lock.seq), secret);
        *l.balances.entry(*payee).or_default() += amount;
        Ok(())
    }
}

// ---------------------------------------------------------------- explorers

/// An explorer that reports payments that never happened, spends that never
/// happened, and any height it likes.
struct Liar {
    honest: TestChain,
    fake: Vec<(Coin, u32)>,
    fake_spend: Option<Transaction>,
    lift: u32,
}

impl Liar {
    fn new(honest: &TestChain) -> Liar {
        Liar { honest: honest.clone(), fake: Vec::new(), fake_spend: None, lift: 0 }
    }
}

impl Chain for Liar {
    fn height(&self) -> Result<u32> {
        Ok(self.honest.height()? + self.lift)
    }
    fn fee_rate(&self) -> Result<u64> {
        self.honest.fee_rate()
    }
    fn coins(&self, address: &Address) -> Result<Vec<(Coin, u32)>> {
        let mut coins = self.honest.coins(address)?;
        coins.extend(self.fake.iter().copied());
        Ok(coins)
    }
    fn transaction(&self, txid: &Txid) -> Result<Option<Transaction>> {
        if let Some(tx) = self.fake_spend.as_ref().filter(|tx| tx.compute_txid() == *txid) {
            return Ok(Some(tx.clone()));
        }
        self.honest.transaction(txid)
    }
    fn confirmations(&self, txid: &Txid) -> Result<Option<u32>> {
        if self.fake_spend.as_ref().is_some_and(|tx| tx.compute_txid() == *txid) {
            return Ok(Some(10));
        }
        self.honest.confirmations(txid)
    }
    fn spender(&self, outpoint: &OutPoint) -> Result<Option<Transaction>> {
        if let Some(tx) = self.fake_spend.as_ref().filter(|tx| tx.input.iter().any(|i| i.previous_output == *outpoint)) {
            return Ok(Some(tx.clone()));
        }
        self.honest.spender(outpoint)
    }
    fn broadcast(&self, tx: &Transaction) -> Result<()> {
        self.honest.broadcast(tx)
    }
}

/// An explorer that cannot be reached.
struct Down;

impl Chain for Down {
    fn height(&self) -> Result<u32> {
        bail!("down")
    }
    fn fee_rate(&self) -> Result<u64> {
        bail!("down")
    }
    fn coins(&self, _: &Address) -> Result<Vec<(Coin, u32)>> {
        bail!("down")
    }
    fn transaction(&self, _: &Txid) -> Result<Option<Transaction>> {
        bail!("down")
    }
    fn confirmations(&self, _: &Txid) -> Result<Option<u32>> {
        bail!("down")
    }
    fn spender(&self, _: &OutPoint) -> Result<Option<Transaction>> {
        bail!("down")
    }
    fn broadcast(&self, _: &Transaction) -> Result<()> {
        bail!("down")
    }
}

// ---------------------------------------------------------------- setup

const ORN: u64 = 500;
const SATS: u64 = 1_000_000;

struct World {
    chain: TestChain,
    btc: Explorers,
    ledger: Rc<RefCell<Ledger>>,
    seller: Side,
    buyer: Side,
    seller_payout: Address,
    buyer_refund: Address,
}

fn btc_address() -> Address {
    let (_, key) = new_key();
    Address::p2wpkh(&CompressedPublicKey(key.inner), Network::Regtest)
}

fn world() -> World {
    let ledger = Rc::new(RefCell::new(Ledger { epoch: 10, ..Ledger::default() }));
    let (alice, bob) = (Hash32([1; 32]), Hash32([2; 32]));
    ledger.borrow_mut().balances.insert(alice, 1_000);
    let chain = TestChain::at(100);
    let reachable = Rc::new(Cell::new(true));
    World {
        btc: Explorers::one(chain.clone()),
        chain,
        seller: Side { ledger: ledger.clone(), me: alice, reachable: reachable.clone() },
        buyer: Side { ledger: ledger.clone(), me: bob, reachable },
        ledger,
        seller_payout: btc_address(),
        buyer_refund: btc_address(),
    }
}

/// Two explorers: the honest one and another.
fn with(w: &World, other: impl Chain + 'static) -> Explorers {
    Explorers(vec![Box::new(w.chain.clone()), Box::new(other)])
}

/// The seller saves the swap and locks the ORN, offers, the buyer replies,
/// the seller accepts: as the codes would travel between them.
fn set_up(w: &World) -> (Swap, Swap) {
    let mut seller = Swap::sell(&w.seller, Network::Regtest, w.buyer.me, ORN, SATS, &w.seller_payout.to_string()).unwrap();
    assert_eq!(seller.stage, Stage::Preparing, "nothing is locked before the secret is saved");
    assert_eq!(w.ledger.borrow().balances[&w.seller.me], 1_000);
    seller.advance(&w.btc, &w.seller).unwrap();
    assert_eq!(seller.stage, Stage::WaitingForReply);
    let offer = Offer::read(&seller.offer(w.seller.network()).unwrap().code()).unwrap();
    let (buyer, reply) = Swap::buy(&w.buyer, &w.btc, Network::Regtest, &offer, &w.buyer_refund.to_string()).unwrap();
    seller.accept(&w.btc, &w.seller, &Reply::read(&reply.code()).unwrap()).unwrap();
    assert_eq!(seller.bitcoin_address().unwrap(), buyer.bitcoin_address().unwrap());
    (seller, buyer)
}

fn round(w: &World, seller: &mut Swap, buyer: &mut Swap) {
    let _ = seller.advance(&w.btc, &w.seller);
    let _ = buyer.advance(&w.btc, &w.buyer);
}

fn orn(w: &World, side: &Side) -> u64 {
    w.ledger.borrow().balances.get(&side.me).copied().unwrap_or(0)
}

fn cancelled(swap: &Swap) -> bool {
    matches!(swap.stage, Stage::Cancelled { .. })
}

// ---------------------------------------------------------------- the usual

#[test]
fn a_swap_pays_both_sides() {
    let w = world();
    let (mut seller, mut buyer) = set_up(&w);
    w.chain.pay(&buyer.bitcoin_address().unwrap(), SATS);
    round(&w, &mut seller, &mut buyer);
    assert_eq!(buyer.stage, Stage::WaitingForSecret);
    // Not taken until buried three blocks deep.
    w.chain.mine(CONFIRMATIONS - 1);
    round(&w, &mut seller, &mut buyer);
    assert_eq!(seller.stage, Stage::WaitingForBitcoin);
    w.chain.mine(1);
    round(&w, &mut seller, &mut buyer);
    assert_eq!(seller.stage, Stage::Claiming);
    round(&w, &mut seller, &mut buyer);
    assert_eq!(buyer.stage, Stage::Done, "the buyer read the secret and claimed the ORN");
    assert_eq!(orn(&w, &w.buyer), ORN);
    // The seller is done once the claim is settled.
    assert_eq!(seller.stage, Stage::Claiming);
    w.chain.mine(SETTLED);
    round(&w, &mut seller, &mut buyer);
    assert_eq!(seller.stage, Stage::Done);
    assert!(w.chain.received(&w.seller_payout) > SATS * 99 / 100, "the seller has the Bitcoin, less its fee");
}

#[test]
fn a_buyer_whose_wallet_was_closed_still_gets_the_orn() {
    let w = world();
    let (mut seller, mut buyer) = set_up(&w);
    w.chain.pay(&buyer.bitcoin_address().unwrap(), SATS);
    w.chain.mine(CONFIRMATIONS);
    seller.advance(&w.btc, &w.seller).unwrap();
    w.chain.mine(SETTLED);
    seller.advance(&w.btc, &w.seller).unwrap();
    assert_eq!(seller.stage, Stage::Done);
    for _ in 0..3 {
        let _ = buyer.advance(&w.btc, &w.buyer);
    }
    assert_eq!(buyer.stage, Stage::Done);
    assert_eq!(orn(&w, &w.buyer), ORN);
}

#[test]
fn a_buyer_whose_wallet_never_opens_again_gets_the_orn_from_the_seller() {
    let w = world();
    let (mut seller, buyer) = set_up(&w);
    // The buyer pays, then never opens their wallet again.
    w.chain.pay(&buyer.bitcoin_address().unwrap(), SATS);
    drop(buyer);
    w.chain.mine(CONFIRMATIONS);
    seller.advance(&w.btc, &w.seller).unwrap();
    assert_eq!(seller.stage, Stage::Claiming);
    w.chain.mine(SETTLED);
    seller.advance(&w.btc, &w.seller).unwrap();
    assert_eq!(seller.stage, Stage::Done, "the seller has its Bitcoin");
    assert!(!seller.finished(), "but is not finished until the buyer has their ORN");
    // An epoch on, the seller's wallet claims the ORN for the buyer, with
    // its own secret.
    seller.advance(&w.btc, &w.seller).unwrap();
    assert!(!seller.finished(), "it gives the buyer's own wallet an epoch first");
    w.ledger.borrow_mut().close();
    seller.advance(&w.btc, &w.seller).unwrap();
    assert!(seller.finished());
    assert_eq!(orn(&w, &w.buyer), ORN, "the buyer is paid without lifting a finger");
    assert!(w.chain.received(&w.seller_payout) > SATS * 99 / 100);
}

#[test]
fn a_seller_that_cannot_claim_for_the_buyer_yet_keeps_trying_quietly() {
    let w = world();
    let (mut seller, buyer) = set_up(&w);
    w.chain.pay(&buyer.bitcoin_address().unwrap(), SATS);
    w.chain.mine(CONFIRMATIONS);
    seller.advance(&w.btc, &w.seller).unwrap();
    w.chain.mine(SETTLED);
    seller.advance(&w.btc, &w.seller).unwrap();
    w.ledger.borrow_mut().close();
    // Validators out of reach: no error, still waiting to deliver.
    w.seller.reachable.set(false);
    assert!(seller.advance(&w.btc, &w.seller).is_ok());
    assert!(!seller.finished());
    w.seller.reachable.set(true);
    seller.advance(&w.btc, &w.seller).unwrap();
    assert!(seller.finished());
    assert_eq!(orn(&w, &w.buyer), ORN);
}

#[test]
fn a_seller_never_signs_a_claim_the_buyer_already_made() {
    let w = world();
    let (mut seller, mut buyer) = set_up(&w);
    w.chain.pay(&buyer.bitcoin_address().unwrap(), SATS);
    w.chain.mine(CONFIRMATIONS);
    round(&w, &mut seller, &mut buyer);
    round(&w, &mut seller, &mut buyer);
    assert_eq!(buyer.stage, Stage::Done, "the buyer's open wallet claimed the ORN itself");
    w.chain.mine(SETTLED);
    seller.advance(&w.btc, &w.seller).unwrap();
    assert_eq!(seller.stage, Stage::Done);
    w.ledger.borrow_mut().close();
    seller.advance(&w.btc, &w.seller).unwrap();
    assert!(seller.finished());
    assert_eq!(w.ledger.borrow().claims, 1, "only the buyer's claim was signed");
    assert_eq!(orn(&w, &w.buyer), ORN);
}

#[test]
fn if_the_buyer_never_pays_the_orn_come_back() {
    let w = world();
    let (mut seller, mut buyer) = set_up(&w);
    w.chain.mine(BTC_BLOCKS);
    round(&w, &mut seller, &mut buyer);
    assert!(cancelled(&seller), "{:?}", seller.stage);
    // The buyer keeps watching a while in case a payment is late.
    assert_eq!(buyer.stage, Stage::WaitingForBitcoin);
    w.chain.mine(144);
    round(&w, &mut seller, &mut buyer);
    assert!(cancelled(&buyer), "{:?}", buyer.stage);
    for _ in 0..ORN_HOURS {
        w.ledger.borrow_mut().close();
    }
    assert_eq!(orn(&w, &w.seller), 1_000, "the seller has every ORN back");
}

#[test]
fn if_the_seller_never_takes_the_bitcoin_it_comes_back() {
    let w = world();
    let (mut seller, mut buyer) = set_up(&w);
    w.chain.pay(&buyer.bitcoin_address().unwrap(), SATS);
    buyer.advance(&w.btc, &w.buyer).unwrap();
    w.chain.mine(BTC_BLOCKS - 1);
    buyer.advance(&w.btc, &w.buyer).unwrap();
    assert_eq!(w.chain.received(&w.buyer_refund), 0, "no refund before the deadline");
    w.chain.mine(1);
    buyer.advance(&w.btc, &w.buyer).unwrap();
    assert_eq!(buyer.stage, Stage::Refunding);
    w.chain.mine(SETTLED);
    buyer.advance(&w.btc, &w.buyer).unwrap();
    assert!(cancelled(&buyer), "{:?}", buyer.stage);
    assert!(w.chain.received(&w.buyer_refund) > SATS * 99 / 100);
    // The seller's wallet, opened too late, takes nothing.
    seller.advance(&w.btc, &w.seller).unwrap();
    assert!(cancelled(&seller), "{:?}", seller.stage);
    assert_eq!(w.chain.received(&w.seller_payout), 0);
}

// ---------------------------------------------------------------- a dishonest other side

/// A seller waits for the buyer's refund, then replaces it with a claim
/// paying a higher fee. The claim shows the secret, and the buyer's wallet
/// claims the ORN with it.
#[test]
fn a_seller_who_replaces_the_refund_with_a_claim_still_pays_the_orn() {
    let w = world();
    let (seller, mut buyer) = set_up(&w);
    let coin = w.chain.pay(&buyer.bitcoin_address().unwrap(), SATS);
    w.chain.mine(BTC_BLOCKS);
    buyer.advance(&w.btc, &w.buyer).unwrap();
    buyer.advance(&w.btc, &w.buyer).unwrap();
    assert_eq!(buyer.stage, Stage::Refunding);
    // The seller claims over the refund with ten times the fee rate.
    let lock = seller.bitcoin_lock().unwrap();
    let key = bitcoin::secp256k1::SecretKey::from_str(&seller.key).unwrap();
    let claim = lock.claim(&[coin], &seller.secret.unwrap(), &key, &w.seller_payout, 50, 10).unwrap();
    w.chain.broadcast(&claim).unwrap();
    // The buyer learns the secret; its refund, outbid, cannot be sent again
    // for now, and it says so.
    assert!(buyer.advance(&w.btc, &w.buyer).is_err());
    assert_eq!(buyer.stage, Stage::ClaimingOrn);
    buyer.advance(&w.btc, &w.buyer).unwrap();
    assert_eq!(orn(&w, &w.buyer), ORN);
    w.chain.mine(SETTLED);
    buyer.advance(&w.btc, &w.buyer).unwrap();
    assert_eq!(buyer.stage, Stage::Done);
}

#[test]
fn a_payment_split_in_two_is_taken_whole() {
    let w = world();
    let (mut seller, mut buyer) = set_up(&w);
    let address = buyer.bitcoin_address().unwrap();
    w.chain.pay(&address, SATS / 2);
    w.chain.pay(&address, SATS / 2);
    w.chain.mine(CONFIRMATIONS);
    for _ in 0..3 {
        round(&w, &mut seller, &mut buyer);
    }
    assert_eq!(buyer.stage, Stage::Done);
    assert_eq!(seller.stage, Stage::Claiming);
    assert!(w.chain.received(&w.seller_payout) > SATS * 99 / 100);
}

#[test]
fn a_short_payment_is_not_taken_and_comes_back() {
    let w = world();
    let (mut seller, mut buyer) = set_up(&w);
    // An exchange took its fee out of the payment.
    w.chain.pay(&buyer.bitcoin_address().unwrap(), SATS - 5_000);
    w.chain.mine(BTC_BLOCKS);
    round(&w, &mut seller, &mut buyer);
    round(&w, &mut seller, &mut buyer);
    assert!(cancelled(&seller), "{:?}", seller.stage);
    assert_eq!(buyer.stage, Stage::Refunding);
    w.chain.mine(SETTLED);
    round(&w, &mut seller, &mut buyer);
    assert!(w.chain.received(&w.buyer_refund) > (SATS - 5_000) * 99 / 100);
}

#[test]
fn bitcoin_left_in_the_lock_after_the_claim_comes_back() {
    let w = world();
    let (mut seller, mut buyer) = set_up(&w);
    let address = buyer.bitcoin_address().unwrap();
    w.chain.pay(&address, SATS);
    w.chain.mine(CONFIRMATIONS);
    seller.advance(&w.btc, &w.seller).unwrap();
    assert_eq!(seller.stage, Stage::Claiming);
    // The buyer pays again by mistake, after the seller's claim.
    w.chain.pay(&address, 300_000);
    w.chain.mine(1);
    for _ in 0..3 {
        buyer.advance(&w.btc, &w.buyer).unwrap();
    }
    assert_eq!(orn(&w, &w.buyer), ORN);
    assert_eq!(buyer.stage, Stage::WaitingForSecret, "still owed the second payment");
    w.chain.mine(BTC_BLOCKS);
    buyer.advance(&w.btc, &w.buyer).unwrap();
    w.chain.mine(SETTLED);
    buyer.advance(&w.btc, &w.buyer).unwrap();
    assert_eq!(buyer.stage, Stage::Done);
    assert!(w.chain.received(&w.buyer_refund) > 290_000);
}

#[test]
fn a_strangers_unconfirmed_payment_cannot_hold_up_the_refund() {
    let w = world();
    let (_, mut buyer) = set_up(&w);
    let address = buyer.bitcoin_address().unwrap();
    w.chain.pay(&address, SATS);
    w.chain.mine(BTC_BLOCKS);
    // A stranger pays a little into the lock and it sits unmined.
    w.chain.pay(&address, 30_000);
    buyer.advance(&w.btc, &w.buyer).unwrap();
    buyer.advance(&w.btc, &w.buyer).unwrap();
    assert_eq!(buyer.stage, Stage::Refunding);
    assert_eq!(buyer.sending.len(), 1, "only the mined coin is refunded");
    w.chain.mine(SETTLED);
    buyer.advance(&w.btc, &w.buyer).unwrap();
    assert!(w.chain.received(&w.buyer_refund) > SATS * 99 / 100);
    // Once mined, the stranger's coin comes back too.
    w.chain.mine(SETTLED);
    buyer.advance(&w.btc, &w.buyer).unwrap();
    assert!(cancelled(&buyer), "{:?}", buyer.stage);
}

// ---------------------------------------------------------------- explorers

#[test]
fn a_seller_is_not_fooled_by_payments_that_never_happened() {
    let w = world();
    let (mut seller, _) = set_up(&w);
    // A coin with no transaction behind it, and a real one that pays
    // somewhere else, both reported as deeply confirmed payments.
    let elsewhere = w.chain.pay(&btc_address(), SATS);
    w.chain.mine(10);
    let made_up = Coin { outpoint: OutPoint { txid: Txid::from_byte_array([9; 32]), vout: 0 }, value: elsewhere.value };
    let lying = || Liar { fake: vec![(made_up, 10), (elsewhere, 10)], ..Liar::new(&w.chain) };
    seller.advance(&Explorers::one(lying()), &w.seller).unwrap();
    seller.advance(&with(&w, lying()), &w.seller).unwrap();
    assert_eq!(seller.stage, Stage::WaitingForBitcoin, "the secret was not revealed");
    assert!(seller.sending.is_empty());
}

#[test]
fn a_seller_needs_every_explorer_to_see_the_payment() {
    let w = world();
    let (mut seller, buyer) = set_up(&w);
    w.chain.pay(&buyer.bitcoin_address().unwrap(), SATS);
    w.chain.mine(CONFIRMATIONS);
    // One explorer is down: the seller waits rather than trust the other.
    assert!(seller.advance(&with(&w, Down), &w.seller).is_err());
    assert_eq!(seller.stage, Stage::WaitingForBitcoin);
    seller.advance(&w.btc, &w.seller).unwrap();
    assert_eq!(seller.stage, Stage::Claiming);
}

#[test]
fn a_buyer_refunds_with_one_explorer_down_and_another_lying() {
    let w = world();
    let (_, mut buyer) = set_up(&w);
    let coin = w.chain.pay(&buyer.bitcoin_address().unwrap(), SATS);
    buyer.advance(&with(&w, Down), &w.buyer).unwrap();
    w.chain.mine(BTC_BLOCKS);
    buyer.advance(&with(&w, Down), &w.buyer).unwrap();
    buyer.advance(&with(&w, Down), &w.buyer).unwrap();
    assert_eq!(buyer.stage, Stage::Refunding);
    // A liar claims the coin was already spent, deep in the chain; the
    // refund is kept going all the same.
    let mut fake = Transaction { version: bitcoin::transaction::Version::TWO, lock_time: bitcoin::absolute::LockTime::ZERO, input: Vec::new(), output: Vec::new() };
    fake.input.push(bitcoin::TxIn { previous_output: coin.outpoint, ..Default::default() });
    w.chain.drop_waiting();
    let liar = Liar { fake_spend: Some(fake), ..Liar::new(&w.chain) };
    w.chain.busy(1);
    buyer.advance(&with(&w, liar), &w.buyer).unwrap();
    w.chain.mine(SETTLED);
    buyer.advance(&w.btc, &w.buyer).unwrap();
    assert!(cancelled(&buyer), "{:?}", buyer.stage);
    assert!(w.chain.received(&w.buyer_refund) > SATS * 99 / 100);
}

#[test]
fn a_buyer_sets_its_deadline_by_the_lower_height() {
    let w = world();
    let mut seller = Swap::sell(&w.seller, Network::Regtest, w.buyer.me, ORN, SATS, &w.seller_payout.to_string()).unwrap();
    seller.advance(&w.btc, &w.seller).unwrap();
    let offer = seller.offer(w.seller.network()).unwrap();
    let refund = w.buyer_refund.to_string();
    // A little ahead: the lower height is used.
    let ahead = Liar { lift: 2, ..Liar::new(&w.chain) };
    let (_, reply) = Swap::buy(&w.buyer, &with(&w, ahead), Network::Regtest, &offer, &refund).unwrap();
    assert_eq!(reply.height, 100 + BTC_BLOCKS);
    // Far ahead: refused.
    let far = Liar { lift: 50, ..Liar::new(&w.chain) };
    assert!(Swap::buy(&w.buyer, &with(&w, far), Network::Regtest, &offer, &refund).is_err());
}

// ---------------------------------------------------------------- fees

#[test]
fn a_stuck_claim_is_sent_again_with_a_higher_fee_once_a_block() {
    let w = world();
    let (mut seller, mut buyer) = set_up(&w);
    w.chain.pay(&buyer.bitcoin_address().unwrap(), SATS);
    w.chain.mine(CONFIRMATIONS);
    seller.advance(&w.btc, &w.seller).unwrap();
    let first = seller.sending.clone();
    // The network gets busier; the claim waits unmined.
    w.chain.0.borrow_mut().fee_rate = 40;
    seller.advance(&w.btc, &w.seller).unwrap();
    assert_eq!(seller.sending, first, "no new fee before a new block");
    w.chain.busy(1);
    seller.advance(&w.btc, &w.seller).unwrap();
    seller.advance(&w.btc, &w.seller).unwrap();
    assert_ne!(seller.sending, first, "a new claim, with a higher fee");
    assert_eq!(seller.sending[0].fee_rate, 40);
    // Only the new one is on the network, and the buyer still reads the
    // secret from it.
    for _ in 0..3 {
        buyer.advance(&w.btc, &w.buyer).unwrap();
    }
    assert_eq!(orn(&w, &w.buyer), ORN);
    w.chain.mine(SETTLED);
    round(&w, &mut seller, &mut buyer);
    assert_eq!((seller.stage.clone(), buyer.stage.clone()), (Stage::Done, Stage::Done));
}

#[test]
fn a_refund_near_the_orn_deadline_pays_what_it_takes_once_a_block() {
    let w = world();
    let (_, mut buyer) = set_up(&w);
    w.chain.pay(&buyer.bitcoin_address().unwrap(), SATS);
    w.chain.mine(BTC_BLOCKS);
    buyer.advance(&w.btc, &w.buyer).unwrap();
    buyer.advance(&w.btc, &w.buyer).unwrap();
    let first = buyer.sending[0].fee_rate;
    // Hours pass with the refund stuck, until little ORN time is left.
    while buyer.until - w.ledger.borrow().epoch > 6 {
        w.ledger.borrow_mut().close();
    }
    let mut rates = Vec::new();
    for _ in 0..4 {
        w.chain.busy(1);
        for _ in 0..5 {
            buyer.advance(&w.btc, &w.buyer).unwrap();
        }
        rates.push(buyer.sending[0].fee_rate);
    }
    assert_eq!(rates, vec![first * 2, first * 4, first * 8, first * 16], "doubled once a block, not once a step");
    w.chain.mine(SETTLED);
    buyer.advance(&w.btc, &w.buyer).unwrap();
    assert!(cancelled(&buyer), "{:?}", buyer.stage);
}

#[test]
fn a_refund_that_never_reached_the_network_is_sent_again() {
    let w = world();
    let (_, mut buyer) = set_up(&w);
    w.chain.pay(&buyer.bitcoin_address().unwrap(), SATS);
    w.chain.mine(BTC_BLOCKS);
    buyer.advance(&w.btc, &w.buyer).unwrap();
    buyer.advance(&w.btc, &w.buyer).unwrap();
    assert_eq!(buyer.stage, Stage::Refunding);
    w.chain.drop_waiting();
    buyer.advance(&w.btc, &w.buyer).unwrap();
    w.chain.mine(SETTLED);
    buyer.advance(&w.btc, &w.buyer).unwrap();
    assert!(cancelled(&buyer), "{:?}", buyer.stage);
    assert!(w.chain.received(&w.buyer_refund) > SATS * 99 / 100);
}

// ---------------------------------------------------------------- crashes

#[test]
fn a_seller_that_crashed_after_locking_does_not_lock_twice() {
    let w = world();
    let saved = Swap::sell(&w.seller, Network::Regtest, w.buyer.me, ORN, SATS, &w.seller_payout.to_string()).unwrap();
    let mut first = saved.clone();
    first.advance(&w.btc, &w.seller).unwrap();
    // The app stopped before saving: it starts again from the saved swap.
    let mut again = saved;
    again.advance(&w.btc, &w.seller).unwrap();
    assert_eq!(again.stage, Stage::WaitingForReply);
    assert_eq!(again.lock, first.lock, "the first lock was found");
    assert_eq!(w.ledger.borrow().locks_made, 1);
}

#[test]
fn a_seller_that_crashed_after_claiming_settles_the_claim() {
    let w = world();
    let (mut seller, mut buyer) = set_up(&w);
    w.chain.pay(&buyer.bitcoin_address().unwrap(), SATS);
    w.chain.mine(CONFIRMATIONS);
    let saved = seller.clone();
    seller.advance(&w.btc, &w.seller).unwrap();
    let mut seller = saved;
    seller.advance(&w.btc, &w.seller).unwrap();
    assert_eq!(seller.stage, Stage::Claiming, "it found its own claim");
    w.chain.mine(SETTLED);
    round(&w, &mut seller, &mut buyer);
    assert_eq!(seller.stage, Stage::Done);
}

#[test]
fn a_buyer_that_crashed_after_refunding_finishes() {
    let w = world();
    let (_, mut buyer) = set_up(&w);
    w.chain.pay(&buyer.bitcoin_address().unwrap(), SATS);
    w.chain.mine(BTC_BLOCKS);
    buyer.advance(&w.btc, &w.buyer).unwrap();
    let saved = buyer.clone();
    buyer.advance(&w.btc, &w.buyer).unwrap();
    assert_eq!(buyer.sending.len(), 1);
    let mut buyer = saved;
    buyer.advance(&w.btc, &w.buyer).unwrap();
    assert_eq!(buyer.sending.len(), 1, "it found its own refund");
    w.chain.mine(SETTLED);
    buyer.advance(&w.btc, &w.buyer).unwrap();
    assert!(cancelled(&buyer), "{:?}", buyer.stage);
}

#[test]
fn a_buyer_is_done_only_when_the_claim_is_proven() {
    let w = world();
    let (mut seller, mut buyer) = set_up(&w);
    w.chain.pay(&buyer.bitcoin_address().unwrap(), SATS);
    w.chain.mine(CONFIRMATIONS);
    seller.advance(&w.btc, &w.seller).unwrap();
    buyer.advance(&w.btc, &w.buyer).unwrap();
    assert_eq!(buyer.stage, Stage::ClaimingOrn);
    // The validators cannot be reached: the claim waits, and is not taken
    // as made.
    w.buyer.reachable.set(false);
    assert!(buyer.advance(&w.btc, &w.buyer).is_err());
    assert_eq!(buyer.stage, Stage::ClaimingOrn);
    w.buyer.reachable.set(true);
    buyer.advance(&w.btc, &w.buyer).unwrap();
    assert_eq!(orn(&w, &w.buyer), ORN);
}

// ---------------------------------------------------------------- refusals

#[test]
fn a_buyer_refuses_an_offer_orien_does_not_back() {
    let w = world();
    let mut seller = Swap::sell(&w.seller, Network::Regtest, w.buyer.me, ORN, SATS, &w.seller_payout.to_string()).unwrap();
    seller.advance(&w.btc, &w.seller).unwrap();
    let offer = seller.offer(w.seller.network()).unwrap();
    let refund = w.buyer_refund.to_string();
    let buy = |offer: &Offer| Swap::buy(&w.buyer, &w.btc, Network::Regtest, offer, &refund).map(|_| ());
    assert!(buy(&Offer { orn: ORN + 1, ..offer.clone() }).is_err());
    assert!(buy(&Offer { hash: [0; 32], ..offer.clone() }).is_err());
    assert!(buy(&Offer { network: "other".into(), ..offer.clone() }).is_err());
    assert!(buy(&Offer { lock: LockRef { seq: 7, ..offer.lock }, ..offer.clone() }).is_err());
    // An uncompressed Bitcoin key, which P2WSH cannot use.
    let (_, key) = new_key();
    let uncompressed = bitcoin::PublicKey::new_uncompressed(key.inner).to_string();
    assert!(buy(&Offer { key: uncompressed, ..offer.clone() }).is_err());
    // An offer meant for someone else.
    let carol = Side { ledger: w.ledger.clone(), me: Hash32([3; 32]), reachable: w.buyer.reachable.clone() };
    assert!(Swap::buy(&carol, &w.btc, Network::Regtest, &offer, &refund).is_err());
    // Nothing can be proven: no deal.
    w.buyer.reachable.set(false);
    assert!(buy(&offer).is_err());
    w.buyer.reachable.set(true);
    // A lock that ends too soon to swap safely.
    for _ in 0..(ORN_HOURS - ORN_HOURS_LEFT + 2) {
        w.ledger.borrow_mut().close();
    }
    assert!(buy(&offer).is_err());
}

#[test]
fn a_seller_refuses_a_reply_that_would_outlast_the_orn() {
    let w = world();
    let mut seller = Swap::sell(&w.seller, Network::Regtest, w.buyer.me, ORN, SATS, &w.seller_payout.to_string()).unwrap();
    seller.advance(&w.btc, &w.seller).unwrap();
    let offer = seller.offer(w.seller.network()).unwrap();
    let (_, reply) = Swap::buy(&w.buyer, &w.btc, Network::Regtest, &offer, &w.buyer_refund.to_string()).unwrap();
    // A day passes before the reply arrives: too little ORN time is left.
    for _ in 0..24 {
        w.ledger.borrow_mut().close();
    }
    w.chain.0.borrow_mut().height = reply.height - BTC_BLOCKS;
    assert!(seller.accept(&w.btc, &w.seller, &reply).is_err());
}

#[test]
fn codes_survive_the_trip_and_damage_is_caught() {
    let w = world();
    let mut seller = Swap::sell(&w.seller, Network::Regtest, w.buyer.me, ORN, SATS, &w.seller_payout.to_string()).unwrap();
    assert!(seller.offer(w.seller.network()).is_err(), "no offer before the ORN are locked");
    seller.advance(&w.btc, &w.seller).unwrap();
    let offer = seller.offer(w.seller.network()).unwrap();
    let code = offer.code();
    assert!(code.starts_with("orien-offer:"));
    assert_eq!(Offer::read(&format!("  {code}\n")).unwrap(), offer);
    assert!(Offer::read(&code[..code.len() - 5]).is_err());
    assert!(Reply::read(&code).is_err(), "an offer is not a reply");
}

// ---------------------------------------------------------------- the final review's attacks

#[test]
fn tiny_coins_paid_in_cannot_make_the_claim_too_dear_to_mine() {
    let w = world();
    let (mut seller, buyer) = set_up(&w);
    let address = buyer.bitcoin_address().unwrap();
    w.chain.pay(&address, SATS);
    for _ in 0..60 {
        w.chain.pay(&address, 600);
    }
    w.chain.mine(CONFIRMATIONS);
    seller.advance(&w.btc, &w.seller).unwrap();
    assert_eq!(seller.stage, Stage::Claiming);
    let claim = &seller.transactions()[0];
    assert_eq!(claim.input.len(), 1, "only the coin the price needs");
    assert!(seller.sending[0].fee_rate >= 5, "at the network's rate");
}

#[test]
fn a_payment_made_only_of_tiny_coins_does_not_draw_out_the_secret() {
    let w = world();
    let (mut seller, buyer) = set_up(&w);
    let address = buyer.bitcoin_address().unwrap();
    // The price in 400 coins: a claim of them could not pay the network's
    // fee with room to raise it.
    for _ in 0..400 {
        w.chain.pay(&address, SATS / 400);
    }
    w.chain.mine(CONFIRMATIONS);
    w.chain.0.borrow_mut().fee_rate = 20;
    assert!(seller.advance(&w.btc, &w.seller).is_err());
    assert_eq!(seller.stage, Stage::WaitingForBitcoin);
    assert!(seller.sending.is_empty(), "the secret was not revealed");
}

#[test]
fn a_forged_spend_cannot_stand_in_for_the_buyers_refund() {
    let w = world();
    let (_, mut buyer) = set_up(&w);
    let coin = w.chain.pay(&buyer.bitcoin_address().unwrap(), SATS);
    w.chain.mine(BTC_BLOCKS);
    // A liar reports a spend paying the buyer's refund address 1 satoshi:
    // no one signed it, so it is not taken for the buyer's own.
    let forged = Transaction {
        version: bitcoin::transaction::Version::TWO,
        lock_time: bitcoin::absolute::LockTime::ZERO,
        input: vec![bitcoin::TxIn { previous_output: coin.outpoint, ..Default::default() }],
        output: vec![bitcoin::TxOut { value: bitcoin::Amount::from_sat(1), script_pubkey: w.buyer_refund.script_pubkey() }],
    };
    let lying = || Liar { fake_spend: Some(forged.clone()), ..Liar::new(&w.chain) };
    buyer.advance(&with(&w, lying()), &w.buyer).unwrap();
    buyer.advance(&with(&w, lying()), &w.buyer).unwrap();
    assert_eq!(buyer.sending.len(), 1);
    assert_ne!(buyer.transactions()[0], forged);
    w.chain.mine(SETTLED);
    buyer.advance(&w.btc, &w.buyer).unwrap();
    assert!(cancelled(&buyer), "{:?}", buyer.stage);
    assert!(w.chain.received(&w.buyer_refund) > SATS * 99 / 100);
}

#[test]
fn a_seller_that_cannot_prove_its_lock_does_not_lock_again() {
    let w = world();
    let saved = Swap::sell(&w.seller, Network::Regtest, w.buyer.me, ORN, SATS, &w.seller_payout.to_string()).unwrap();
    saved.clone().advance(&w.btc, &w.seller).unwrap();
    // Restarted from the saved swap, with no proof to be had: the lock's
    // order number is used, so nothing new is locked.
    w.seller.reachable.set(false);
    let mut again = saved;
    assert!(again.advance(&w.btc, &w.seller).is_err());
    assert_eq!(again.stage, Stage::Preparing);
    assert_eq!(w.ledger.borrow().locks_made, 1);
    w.seller.reachable.set(true);
    again.advance(&w.btc, &w.seller).unwrap();
    assert_eq!(again.stage, Stage::WaitingForReply);
    assert_eq!(w.ledger.borrow().locks_made, 1);
}

#[test]
fn a_buyer_that_misses_the_orn_deadline_still_wins_the_bitcoin_back() {
    let w = world();
    let (seller, mut buyer) = set_up(&w);
    let coin = w.chain.pay(&buyer.bitcoin_address().unwrap(), SATS);
    w.chain.mine(BTC_BLOCKS);
    buyer.advance(&w.btc, &w.buyer).unwrap();
    buyer.advance(&w.btc, &w.buyer).unwrap();
    assert_eq!(buyer.stage, Stage::Refunding);
    // The seller claims over the refund, and the buyer cannot reach Orien
    // until the ORN deadline has passed.
    let lock = seller.bitcoin_lock().unwrap();
    let key = bitcoin::secp256k1::SecretKey::from_str(&seller.key).unwrap();
    w.chain.broadcast(&lock.claim(&[coin], &seller.secret.unwrap(), &key, &w.seller_payout, 30, 10).unwrap()).unwrap();
    w.buyer.reachable.set(false);
    let _ = buyer.advance(&w.btc, &w.buyer);
    assert_eq!(buyer.stage, Stage::ClaimingOrn);
    while w.ledger.borrow().epoch < buyer.until {
        w.ledger.borrow_mut().close();
    }
    // The buyer keeps raising its refund's fee each block until it
    // displaces the seller's claim.
    for _ in 0..6 {
        w.chain.busy(1);
        let _ = buyer.advance(&w.btc, &w.buyer);
    }
    assert_eq!(buyer.stage, Stage::Refunding);
    w.chain.mine(SETTLED);
    let _ = buyer.advance(&w.btc, &w.buyer);
    assert!(cancelled(&buyer), "{:?}", buyer.stage);
    assert!(w.chain.received(&w.buyer_refund) > SATS / 2);
    assert_eq!(w.chain.received(&w.seller_payout), 0);
}

#[test]
fn a_claim_left_unmined_is_raised_even_if_the_network_looks_quiet() {
    let w = world();
    let (mut seller, buyer) = set_up(&w);
    w.chain.pay(&buyer.bitcoin_address().unwrap(), SATS);
    w.chain.mine(CONFIRMATIONS);
    seller.advance(&w.btc, &w.seller).unwrap();
    let first = seller.sending[0].fee_rate;
    w.chain.busy(3);
    seller.advance(&w.btc, &w.seller).unwrap();
    assert!(seller.sending[0].fee_rate > first, "raised after three blocks unmined");
}
