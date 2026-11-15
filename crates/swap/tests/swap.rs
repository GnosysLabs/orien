//! Whole swaps between a seller and a buyer, on a pretend Bitcoin network
//! that applies Bitcoin Core's own rules, lock times and replace-by-fee, and
//! a pretend Orien that keeps locked payments as the ledger does. Each attack
//! a dishonest counterparty or a lying explorer could try has its test.

use anyhow::{Result, anyhow, ensure};
use bitcoin::hashes::{Hash, sha256};
use bitcoin::{Address, CompressedPublicKey, Network, OutPoint, Transaction, Txid};
use orien_crypto::Hash32;
use orien_swap::chain::Chain;
use orien_swap::htlc::{Coin, new_key};
use orien_swap::swap::{
    BTC_BLOCKS, CONFIRMATIONS, LockRef, LockState, ORN_HOURS, Offer, Orien, Reply, Stage, Swap, Waiting,
};
use orien_swap::testing::TestChain;
use std::cell::RefCell;
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
    fn epoch_seconds(&self) -> u64 {
        3_600
    }
    fn locked(&self, lock: &LockRef, hash: &[u8; 32]) -> Result<LockState> {
        let l = self.ledger.borrow();
        let key = (lock.from, lock.seq);
        let secret = l.revealed.get(&key).copied().filter(|s| sha256::Hash::hash(s).to_byte_array() == *hash);
        Ok(LockState { epoch: l.epoch, waiting: l.escrows.get(&key).cloned(), secret })
    }
    fn lock(&self, to: &Hash32, amount: u64, hash: [u8; 32], until: u64) -> Result<LockRef> {
        let mut l = self.ledger.borrow_mut();
        let balance = l.balances.entry(self.me).or_default();
        ensure!(*balance >= amount, "insufficient funds");
        *balance -= amount;
        let seq = *l.next_seq.entry(self.me).and_modify(|n| *n += 1).or_insert(0);
        l.escrows.insert((self.me, seq), Waiting { to: *to, amount, hash, until });
        Ok(LockRef { from: self.me, seq })
    }
    fn claim(&self, lock: &LockRef, amount: u64, secret: [u8; 32]) -> Result<()> {
        let mut l = self.ledger.borrow_mut();
        let epoch = l.epoch;
        let w = l.escrows.get(&(lock.from, lock.seq)).cloned().ok_or_else(|| anyhow!("no such lock"))?;
        ensure!(w.to == self.me && w.amount == amount, "not ours");
        ensure!(epoch < w.until, "too late");
        ensure!(sha256::Hash::hash(&secret).to_byte_array() == w.hash, "wrong secret");
        l.escrows.remove(&(lock.from, lock.seq));
        l.revealed.insert((lock.from, lock.seq), secret);
        *l.balances.entry(self.me).or_default() += amount;
        Ok(())
    }
}

// ---------------------------------------------------------------- setup

const ORN: u64 = 500;
const SATS: u64 = 1_000_000;

struct World {
    chain: TestChain,
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
    World {
        chain: TestChain::at(100),
        seller: Side { ledger: ledger.clone(), me: alice },
        buyer: Side { ledger: ledger.clone(), me: bob },
        ledger,
        seller_payout: btc_address(),
        buyer_refund: btc_address(),
    }
}

/// The seller saves the swap and locks the ORN, offers, the buyer replies,
/// the seller accepts: as the codes would travel between them.
fn set_up(w: &World) -> (Swap, Swap) {
    let mut seller = Swap::sell(&w.seller, Network::Regtest, w.buyer.me, ORN, SATS, &w.seller_payout.to_string()).unwrap();
    assert_eq!(seller.stage, Stage::Preparing, "nothing is locked before the secret is saved");
    assert_eq!(w.ledger.borrow().balances[&w.seller.me], 1_000);
    seller.advance(&w.chain, &w.seller).unwrap();
    assert_eq!(seller.stage, Stage::WaitingForReply);
    let offer = Offer::read(&seller.offer(w.seller.network()).unwrap().code()).unwrap();
    let (buyer, reply) = Swap::buy(&w.buyer, &w.chain, Network::Regtest, &offer, &w.buyer_refund.to_string()).unwrap();
    seller.accept(&w.chain, &w.seller, &Reply::read(&reply.code()).unwrap()).unwrap();
    assert_eq!(seller.bitcoin_address().unwrap(), buyer.bitcoin_address().unwrap());
    (seller, buyer)
}

fn round(w: &World, seller: &mut Swap, buyer: &mut Swap) {
    let _ = seller.advance(&w.chain, &w.seller);
    let _ = buyer.advance(&w.chain, &w.buyer);
}

fn orn(w: &World, side: &Side) -> u64 {
    w.ledger.borrow().balances.get(&side.me).copied().unwrap_or(0)
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
    w.chain.mine(3);
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
    seller.advance(&w.chain, &w.seller).unwrap();
    w.chain.mine(3);
    seller.advance(&w.chain, &w.seller).unwrap();
    assert_eq!(seller.stage, Stage::Done);
    for _ in 0..3 {
        let _ = buyer.advance(&w.chain, &w.buyer);
    }
    assert_eq!(buyer.stage, Stage::Done);
    assert_eq!(orn(&w, &w.buyer), ORN);
}

#[test]
fn if_the_buyer_never_pays_the_orn_come_back() {
    let w = world();
    let (mut seller, mut buyer) = set_up(&w);
    w.chain.mine(BTC_BLOCKS);
    round(&w, &mut seller, &mut buyer);
    assert!(matches!(seller.stage, Stage::Cancelled { .. }), "{:?}", seller.stage);
    assert!(matches!(buyer.stage, Stage::Cancelled { .. }), "{:?}", buyer.stage);
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
    buyer.advance(&w.chain, &w.buyer).unwrap();
    w.chain.mine(BTC_BLOCKS - 1);
    buyer.advance(&w.chain, &w.buyer).unwrap();
    assert_eq!(w.chain.received(&w.buyer_refund), 0, "no refund before the deadline");
    w.chain.mine(1);
    buyer.advance(&w.chain, &w.buyer).unwrap();
    assert_eq!(buyer.stage, Stage::Refunding);
    w.chain.mine(3);
    buyer.advance(&w.chain, &w.buyer).unwrap();
    assert!(matches!(buyer.stage, Stage::Cancelled { .. }), "{:?}", buyer.stage);
    assert!(w.chain.received(&w.buyer_refund) > SATS * 99 / 100);
    // The seller's wallet, opened too late, takes nothing.
    seller.advance(&w.chain, &w.seller).unwrap();
    assert!(matches!(seller.stage, Stage::Cancelled { .. }), "{:?}", seller.stage);
    assert_eq!(w.chain.received(&w.seller_payout), 0);
}

// ---------------------------------------------------------------- attacks

/// The review's worst case: a seller waits for the buyer's refund, then
/// replaces it with a claim paying a higher fee. The claim shows the secret,
/// and the buyer's wallet claims the ORN with it.
#[test]
fn a_seller_who_replaces_the_refund_with_a_claim_still_pays_the_orn() {
    let w = world();
    let (seller, mut buyer) = set_up(&w);
    let coin = w.chain.pay(&buyer.bitcoin_address().unwrap(), SATS);
    w.chain.mine(BTC_BLOCKS);
    buyer.advance(&w.chain, &w.buyer).unwrap();
    buyer.advance(&w.chain, &w.buyer).unwrap();
    assert_eq!(buyer.stage, Stage::Refunding);
    // The seller claims over the refund with ten times the fee rate.
    let lock = seller.bitcoin_lock().unwrap();
    let key = bitcoin::secp256k1::SecretKey::from_str(&seller.key).unwrap();
    let claim = lock.claim(&[coin], &seller.secret.unwrap(), &key, &w.seller_payout, 50).unwrap();
    w.chain.broadcast(&claim).unwrap();
    buyer.advance(&w.chain, &w.buyer).unwrap();
    assert_eq!(buyer.stage, Stage::ClaimingOrn);
    buyer.advance(&w.chain, &w.buyer).unwrap();
    assert_eq!((buyer.stage.clone(), orn(&w, &w.buyer)), (Stage::Done, ORN));
}

/// An explorer that reports payments that never happened.
struct Liar {
    honest: TestChain,
    fake: Vec<(Coin, u32)>,
}

impl Chain for Liar {
    fn height(&self) -> Result<u32> {
        self.honest.height()
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
        self.honest.transaction(txid)
    }
    fn confirmations(&self, txid: &Txid) -> Result<Option<u32>> {
        self.honest.confirmations(txid)
    }
    fn spender(&self, outpoint: &OutPoint) -> Result<Option<Transaction>> {
        self.honest.spender(outpoint)
    }
    fn broadcast(&self, tx: &Transaction) -> Result<()> {
        self.honest.broadcast(tx)
    }
}

#[test]
fn a_seller_is_not_fooled_by_payments_that_never_happened() {
    let w = world();
    let (mut seller, _) = set_up(&w);
    // A coin with no transaction behind it, and a real one that pays
    // somewhere else, both reported as deeply confirmed payments.
    let elsewhere = w.chain.pay(&btc_address(), SATS);
    w.chain.mine(10);
    let made_up = Coin { outpoint: OutPoint { txid: Txid::from_byte_array([9; 32]), vout: 0 }, value: elsewhere.value };
    let liar = Liar { honest: w.chain.clone(), fake: vec![(made_up, 10), (elsewhere, 10)] };
    seller.advance(&liar, &w.seller).unwrap();
    assert_eq!(seller.stage, Stage::WaitingForBitcoin, "the secret was not revealed");
    assert!(seller.tx.is_none());
}

#[test]
fn a_stuck_claim_is_sent_again_with_a_higher_fee() {
    let w = world();
    let (mut seller, mut buyer) = set_up(&w);
    w.chain.pay(&buyer.bitcoin_address().unwrap(), SATS);
    w.chain.mine(CONFIRMATIONS);
    seller.advance(&w.chain, &w.seller).unwrap();
    let first = seller.tx.clone();
    // The network gets busier; the claim waits unmined.
    w.chain.0.borrow_mut().fee_rate = 40;
    seller.advance(&w.chain, &w.seller).unwrap();
    assert_ne!(seller.tx, first, "a new claim, with a higher fee");
    assert_eq!(seller.fee_rate, 40);
    // Only the new one is on the network, and the buyer still reads the
    // secret from it.
    buyer.advance(&w.chain, &w.buyer).unwrap();
    buyer.advance(&w.chain, &w.buyer).unwrap();
    buyer.advance(&w.chain, &w.buyer).unwrap();
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
    assert!(matches!(seller.stage, Stage::Cancelled { .. }), "{:?}", seller.stage);
    assert_eq!(buyer.stage, Stage::Refunding);
    w.chain.mine(3);
    round(&w, &mut seller, &mut buyer);
    assert!(w.chain.received(&w.buyer_refund) > (SATS - 5_000) * 99 / 100);
}

#[test]
fn a_refund_that_never_reached_the_network_is_sent_again() {
    let w = world();
    let (_, mut buyer) = set_up(&w);
    w.chain.pay(&buyer.bitcoin_address().unwrap(), SATS);
    w.chain.mine(BTC_BLOCKS);
    buyer.advance(&w.chain, &w.buyer).unwrap();
    buyer.advance(&w.chain, &w.buyer).unwrap();
    assert_eq!(buyer.stage, Stage::Refunding);
    // The network dropped it.
    {
        let mut b = w.chain.0.borrow_mut();
        let refund: Vec<_> = b.spent.values().copied().collect();
        for txid in refund {
            b.txs.remove(&txid);
        }
        b.spent.clear();
    }
    buyer.advance(&w.chain, &w.buyer).unwrap();
    w.chain.mine(3);
    buyer.advance(&w.chain, &w.buyer).unwrap();
    assert!(matches!(buyer.stage, Stage::Cancelled { .. }), "{:?}", buyer.stage);
    assert!(w.chain.received(&w.buyer_refund) > SATS * 99 / 100);
}

// ---------------------------------------------------------------- refusals

#[test]
fn a_buyer_refuses_an_offer_orien_does_not_back() {
    let w = world();
    let mut seller = Swap::sell(&w.seller, Network::Regtest, w.buyer.me, ORN, SATS, &w.seller_payout.to_string()).unwrap();
    seller.advance(&w.chain, &w.seller).unwrap();
    let offer = seller.offer(w.seller.network()).unwrap();
    let refund = w.buyer_refund.to_string();
    let buy = |offer: &Offer| Swap::buy(&w.buyer, &w.chain, Network::Regtest, offer, &refund).map(|_| ());
    assert!(buy(&Offer { orn: ORN + 1, ..offer.clone() }).is_err());
    assert!(buy(&Offer { hash: [0; 32], ..offer.clone() }).is_err());
    assert!(buy(&Offer { network: "other".into(), ..offer.clone() }).is_err());
    // An uncompressed Bitcoin key, which P2WSH cannot use.
    let (_, key) = new_key();
    let uncompressed = bitcoin::PublicKey::new_uncompressed(key.inner).to_string();
    assert!(buy(&Offer { key: uncompressed, ..offer.clone() }).is_err());
    // An offer meant for someone else.
    let carol = Side { ledger: w.ledger.clone(), me: Hash32([3; 32]) };
    assert!(Swap::buy(&carol, &w.chain, Network::Regtest, &offer, &refund).is_err());
    // A lock that ends too soon to swap safely.
    for _ in 0..(ORN_HOURS - 20) {
        w.ledger.borrow_mut().close();
    }
    assert!(buy(&offer).is_err());
}

#[test]
fn a_seller_refuses_a_reply_that_would_outlast_the_orn() {
    let w = world();
    let mut seller = Swap::sell(&w.seller, Network::Regtest, w.buyer.me, ORN, SATS, &w.seller_payout.to_string()).unwrap();
    seller.advance(&w.chain, &w.seller).unwrap();
    let offer = seller.offer(w.seller.network()).unwrap();
    let (_, reply) = Swap::buy(&w.buyer, &w.chain, Network::Regtest, &offer, &w.buyer_refund.to_string()).unwrap();
    // A day passes before the reply arrives: too little ORN time is left.
    for _ in 0..24 {
        w.ledger.borrow_mut().close();
    }
    w.chain.0.borrow_mut().height = reply.height - BTC_BLOCKS;
    assert!(seller.accept(&w.chain, &w.seller, &reply).is_err());
}

#[test]
fn codes_survive_the_trip_and_damage_is_caught() {
    let w = world();
    let mut seller = Swap::sell(&w.seller, Network::Regtest, w.buyer.me, ORN, SATS, &w.seller_payout.to_string()).unwrap();
    assert!(seller.offer(w.seller.network()).is_err(), "no offer before the ORN are locked");
    seller.advance(&w.chain, &w.seller).unwrap();
    let offer = seller.offer(w.seller.network()).unwrap();
    let code = offer.code();
    assert!(code.starts_with("orien-offer:"));
    assert_eq!(Offer::read(&format!("  {code}\n")).unwrap(), offer);
    assert!(Offer::read(&code[..code.len() - 5]).is_err());
    assert!(Reply::read(&code).is_err(), "an offer is not a reply");
}
