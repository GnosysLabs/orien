//! Whole swaps between a seller and a buyer, on a pretend Bitcoin network
//! that checks every transaction with Bitcoin Core's own rules and enforces
//! lock times as real nodes do, and a pretend Orien that keeps locked
//! payments as the ledger does.

use bitcoin::hashes::{Hash, sha256};
use bitcoin::{Address, Amount, CompressedPublicKey, Network, OutPoint, Transaction, TxOut, Txid};
use orien_crypto::Hash32;
use orien_swap::chain::Chain;
use orien_swap::htlc::{Coin, new_key};
use orien_swap::swap::{BTC_BLOCKS, LockRef, ORN_EPOCHS, Offer, Orien, Reply, Role, Stage, Swap, Waiting};
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

// ---------------------------------------------------------------- Bitcoin

#[derive(Default)]
struct Bitcoin {
    height: u32,
    /// Unspent coins, by address, with the height they were mined at (None
    /// while waiting).
    coins: Vec<(Address, Coin, Option<u32>)>,
    spent: HashMap<OutPoint, Transaction>,
    /// What each address received from spends of swap locks.
    received: HashMap<String, Amount>,
    next: u8,
}

#[derive(Clone, Default)]
struct Chain_(Rc<RefCell<Bitcoin>>);

impl Chain_ {
    fn pay(&self, address: &Address, sats: u64) {
        let mut b = self.0.borrow_mut();
        b.next += 1;
        let txid = Txid::from_byte_array([b.next; 32]);
        b.coins.push((address.clone(), Coin { outpoint: OutPoint { txid, vout: 0 }, value: Amount::from_sat(sats) }, None));
    }
    /// Mines a block: everything waiting is confirmed.
    fn mine(&self, blocks: u32) {
        let mut b = self.0.borrow_mut();
        for _ in 0..blocks {
            b.height += 1;
            let height = b.height;
            for (_, _, mined) in b.coins.iter_mut() {
                mined.get_or_insert(height);
            }
        }
    }
}

impl Chain for Chain_ {
    fn height(&self) -> anyhow::Result<u32> {
        Ok(self.0.borrow().height)
    }
    fn fee_rate(&self) -> anyhow::Result<u64> {
        Ok(5)
    }
    fn coins(&self, address: &Address) -> anyhow::Result<Vec<(Coin, u32)>> {
        let b = self.0.borrow();
        Ok(b.coins
            .iter()
            .filter(|(to, coin, _)| to == address && !b.spent.contains_key(&coin.outpoint))
            .map(|(_, coin, mined)| (*coin, mined.map_or(0, |h| b.height - h + 1)))
            .collect())
    }
    fn spender(&self, outpoint: &OutPoint) -> anyhow::Result<Option<Transaction>> {
        Ok(self.0.borrow().spent.get(outpoint).cloned())
    }
    fn broadcast(&self, tx: &Transaction) -> anyhow::Result<Txid> {
        let mut b = self.0.borrow_mut();
        let input = &tx.input[0];
        let (address, coin, _) = b
            .coins
            .iter()
            .find(|(_, coin, _)| coin.outpoint == input.previous_output)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("spends a coin that does not exist"))?;
        if b.spent.contains_key(&coin.outpoint) {
            anyhow::bail!("double spend");
        }
        // A lock time must have passed, as real nodes require.
        if tx.lock_time.to_consensus_u32() > b.height {
            anyhow::bail!("non-final transaction");
        }
        let spent = TxOut { value: coin.value, script_pubkey: address.script_pubkey() };
        tx.verify(|_| Some(spent.clone())).map_err(|e| anyhow::anyhow!("Bitcoin's rules refuse it: {e:?}"))?;
        b.spent.insert(coin.outpoint, tx.clone());
        for output in &tx.output {
            let to = Address::from_script(&output.script_pubkey, Network::Regtest).unwrap().to_string();
            *b.received.entry(to).or_default() += output.value;
        }
        Ok(tx.compute_txid())
    }
}

// ---------------------------------------------------------------- Orien

#[derive(Default)]
struct Ledger {
    epoch: u64,
    escrows: HashMap<(Hash32, u64), Waiting>,
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
    fn epoch(&self) -> anyhow::Result<u64> {
        Ok(self.ledger.borrow().epoch)
    }
    fn waiting(&self, lock: &LockRef) -> anyhow::Result<Option<Waiting>> {
        Ok(self.ledger.borrow().escrows.get(&(lock.from, lock.seq)).cloned())
    }
    fn lock(&self, to: &Hash32, amount: u64, hash: [u8; 32], until: u64) -> anyhow::Result<LockRef> {
        let mut l = self.ledger.borrow_mut();
        let balance = l.balances.entry(self.me).or_default();
        anyhow::ensure!(*balance >= amount, "insufficient funds");
        *balance -= amount;
        let seq = *l.next_seq.entry(self.me).and_modify(|n| *n += 1).or_insert(0);
        l.escrows.insert((self.me, seq), Waiting { to: *to, amount, hash, until });
        Ok(LockRef { from: self.me, seq })
    }
    fn claim(&self, lock: &LockRef, amount: u64, secret: [u8; 32]) -> anyhow::Result<()> {
        let mut l = self.ledger.borrow_mut();
        let epoch = l.epoch;
        let w = l.escrows.get(&(lock.from, lock.seq)).cloned().ok_or_else(|| anyhow::anyhow!("no such lock"))?;
        anyhow::ensure!(w.to == self.me && w.amount == amount, "not ours");
        anyhow::ensure!(epoch < w.until, "too late");
        anyhow::ensure!(sha256::Hash::hash(&secret).to_byte_array() == w.hash, "wrong secret");
        l.escrows.remove(&(lock.from, lock.seq));
        *l.balances.entry(self.me).or_default() += amount;
        Ok(())
    }
}

// ---------------------------------------------------------------- setup

const ORN: u64 = 500;
const SATS: u64 = 1_000_000;

struct World {
    chain: Chain_,
    ledger: Rc<RefCell<Ledger>>,
    seller: Side,
    buyer: Side,
    seller_payout: String,
    buyer_refund: String,
}

fn btc_address() -> String {
    let (_, key) = new_key();
    Address::p2wpkh(&CompressedPublicKey(key.inner), Network::Regtest).to_string()
}

fn world() -> World {
    let chain = Chain_::default();
    chain.0.borrow_mut().height = 100;
    let ledger = Rc::new(RefCell::new(Ledger { epoch: 10, ..Ledger::default() }));
    let (alice, bob) = (Hash32([1; 32]), Hash32([2; 32]));
    ledger.borrow_mut().balances.insert(alice, 1_000);
    World {
        chain,
        seller: Side { ledger: ledger.clone(), me: alice },
        buyer: Side { ledger: ledger.clone(), me: bob },
        ledger,
        seller_payout: btc_address(),
        buyer_refund: btc_address(),
    }
}

/// The seller offers, the buyer replies, the seller accepts: as the codes
/// would travel between them.
fn set_up(w: &World) -> (Swap, Swap) {
    let (seller, offer) = Swap::sell(&w.seller, Network::Regtest, w.buyer.me, ORN, SATS, &w.seller_payout).unwrap();
    let offer = Offer::read(&offer.code()).unwrap();
    let (buyer, reply) = Swap::buy(&w.buyer, &w.chain, Network::Regtest, &offer, &w.buyer_refund).unwrap();
    let reply = Reply::read(&reply.code()).unwrap();
    let mut seller = seller;
    seller.accept(&w.chain, &reply).unwrap();
    // Both sides work out the same Bitcoin address.
    assert_eq!(seller.bitcoin_address().unwrap(), buyer.bitcoin_address().unwrap());
    (seller, buyer)
}

fn round(w: &World, seller: &mut Swap, buyer: &mut Swap) {
    seller.advance(&w.chain, &w.seller).unwrap();
    buyer.advance(&w.chain, &w.buyer).unwrap();
}

fn received(w: &World, address: &str) -> u64 {
    w.chain.0.borrow().received.get(address).map_or(0, |a| a.to_sat())
}

// ---------------------------------------------------------------- tests

#[test]
fn a_swap_pays_both_sides() {
    let w = world();
    let (mut seller, mut buyer) = set_up(&w);
    assert_eq!(w.ledger.borrow().balances[&w.seller.me], 500, "the ORN are locked");

    // The buyer pays the Bitcoin. Unconfirmed, the seller does not take it.
    w.chain.pay(&buyer.bitcoin_address().unwrap(), SATS);
    round(&w, &mut seller, &mut buyer);
    assert_eq!(buyer.stage, Stage::WaitingForSecret);
    assert_eq!(seller.stage, Stage::WaitingForBitcoin);

    // Confirmed: the seller takes it, which shows the secret; the buyer
    // reads it and claims the ORN.
    w.chain.mine(1);
    round(&w, &mut seller, &mut buyer);
    round(&w, &mut seller, &mut buyer);
    round(&w, &mut seller, &mut buyer);
    assert_eq!((seller.stage.clone(), buyer.stage.clone()), (Stage::Done, Stage::Done));
    assert_eq!(w.ledger.borrow().balances[&w.buyer.me], ORN, "the buyer has the ORN");
    assert_eq!(received(&w, &w.seller_payout), SATS - 5 * 180, "the seller has the Bitcoin, less its fee");

    // Running on changes nothing and sends nothing again.
    round(&w, &mut seller, &mut buyer);
    assert_eq!(w.ledger.borrow().balances[&w.buyer.me], ORN);
}

#[test]
fn if_the_buyer_never_pays_the_orn_come_back() {
    let w = world();
    let (mut seller, mut buyer) = set_up(&w);
    w.chain.mine(BTC_BLOCKS);
    round(&w, &mut seller, &mut buyer);
    assert!(matches!(seller.stage, Stage::Cancelled { .. }), "{:?}", seller.stage);
    assert!(matches!(buyer.stage, Stage::Cancelled { .. }), "{:?}", buyer.stage);
    for _ in 0..ORN_EPOCHS {
        w.ledger.borrow_mut().close();
    }
    assert_eq!(w.ledger.borrow().balances[&w.seller.me], 1_000, "the seller has every ORN back");
}

#[test]
fn if_the_seller_never_takes_the_bitcoin_it_comes_back() {
    let w = world();
    let (mut seller, mut buyer) = set_up(&w);
    w.chain.pay(&buyer.bitcoin_address().unwrap(), SATS);
    buyer.advance(&w.chain, &w.buyer).unwrap();
    // The seller's wallet stays closed. Before the deadline, no refund is
    // possible; from it, the buyer's wallet takes the Bitcoin back.
    w.chain.mine(BTC_BLOCKS - 2);
    buyer.advance(&w.chain, &w.buyer).unwrap();
    assert_eq!(received(&w, &w.buyer_refund), 0);
    w.chain.mine(2);
    buyer.advance(&w.chain, &w.buyer).unwrap();
    buyer.advance(&w.chain, &w.buyer).unwrap();
    assert!(matches!(buyer.stage, Stage::Cancelled { .. }), "{:?}", buyer.stage);
    assert_eq!(received(&w, &w.buyer_refund), SATS - 5 * 180);
    // The seller's wallet, opened too late, does not take it.
    seller.advance(&w.chain, &w.seller).unwrap();
    assert!(matches!(seller.stage, Stage::Cancelled { .. }), "{:?}", seller.stage);
    assert_eq!(received(&w, &w.seller_payout), 0);
}

#[test]
fn a_buyer_refuses_an_offer_orien_does_not_back() {
    let w = world();
    let (_, offer) = Swap::sell(&w.seller, Network::Regtest, w.buyer.me, ORN, SATS, &w.seller_payout).unwrap();
    let buy = |offer: &Offer| Swap::buy(&w.buyer, &w.chain, Network::Regtest, offer, &w.buyer_refund).map(|_| ());

    // More ORN than are locked, another hash, another network.
    assert!(buy(&Offer { orn: ORN + 1, ..offer.clone() }).is_err());
    assert!(buy(&Offer { hash: [0; 32], ..offer.clone() }).is_err());
    assert!(buy(&Offer { network: "other".into(), ..offer.clone() }).is_err());
    // An offer meant for someone else.
    let carol = Side { ledger: w.ledger.clone(), me: Hash32([3; 32]) };
    assert!(Swap::buy(&carol, &w.chain, Network::Regtest, &offer, &w.buyer_refund).is_err());
    // A lock that ends too soon to swap safely.
    for _ in 0..(ORN_EPOCHS - 20) {
        w.ledger.borrow_mut().close();
    }
    assert!(buy(&offer).is_err());
}

#[test]
fn codes_survive_the_trip_and_damage_is_caught() {
    let w = world();
    let (swap, offer) = Swap::sell(&w.seller, Network::Regtest, w.buyer.me, ORN, SATS, &w.seller_payout).unwrap();
    assert_eq!(swap.role, Role::Seller);
    let code = offer.code();
    assert!(code.starts_with("orien-offer:"));
    assert_eq!(Offer::read(&format!("  {code}\n")).unwrap(), offer);
    assert!(Offer::read(&code[..code.len() - 5]).is_err());
    assert!(Reply::read(&code).is_err(), "an offer is not a reply");
}
