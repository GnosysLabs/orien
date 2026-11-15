//! A pretend Bitcoin network, for testing swaps without real coins.

use crate::chain::Chain;
use crate::htlc::Coin;
use bitcoin::hashes::Hash;
use bitcoin::{Address, Amount, Network, OutPoint, Transaction, TxOut, Txid};
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;


/// The pretend Bitcoin network's state.
#[derive(Default)]
pub struct Bitcoin {
    pub height: u32,
    /// Unspent coins, by address, with the height they were mined at (None
    /// while waiting).
    pub coins: Vec<(Address, Coin, Option<u32>)>,
    pub spent: HashMap<OutPoint, Transaction>,
    /// What each address received from spends of swap locks.
    pub received: HashMap<String, Amount>,
    next: u8,
}

/// A pretend Bitcoin network for tests: it checks every transaction with
/// Bitcoin Core's own script rules and enforces lock times as nodes do.
#[derive(Clone, Default)]
pub struct TestChain(pub Rc<RefCell<Bitcoin>>);

impl TestChain {
    /// Someone pays `sats` to `address`, not yet mined.
    pub fn pay(&self, address: &Address, sats: u64) {
        let mut b = self.0.borrow_mut();
        b.next += 1;
        let txid = Txid::from_byte_array([b.next; 32]);
        b.coins.push((address.clone(), Coin { outpoint: OutPoint { txid, vout: 0 }, value: Amount::from_sat(sats) }, None));
    }
    /// Mines a block: everything waiting is confirmed.
    pub fn mine(&self, blocks: u32) {
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

impl Chain for TestChain {
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
            .filter(|(to, _, _)| to == address)
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

