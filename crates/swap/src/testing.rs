//! A pretend Bitcoin network, for testing swaps without real coins. It
//! checks every transaction with Bitcoin Core's own script rules, enforces
//! lock times as nodes do, and lets a waiting transaction be replaced by one
//! paying a higher fee rate, as nodes with full replace-by-fee do.

use crate::chain::Chain;
use crate::htlc::Coin;
use bitcoin::absolute::LockTime;
use bitcoin::hashes::Hash;
use bitcoin::transaction::Version;
use bitcoin::{Address, Amount, OutPoint, ScriptBuf, Sequence, Transaction, TxIn, TxOut, Txid, Witness};
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

/// The pretend Bitcoin network's state.
#[derive(Default)]
pub struct Bitcoin {
    pub height: u32,
    /// Every transaction it has, with the height it was mined at (None while
    /// waiting).
    pub txs: HashMap<Txid, (Transaction, Option<u32>)>,
    /// Which transaction spends each spent output.
    pub spent: HashMap<OutPoint, Txid>,
    pub fee_rate: u64,
    next: u8,
}

/// A pretend Bitcoin network for tests.
#[derive(Clone, Default)]
pub struct TestChain(pub Rc<RefCell<Bitcoin>>);

impl TestChain {
    pub fn at(height: u32) -> TestChain {
        let chain = TestChain::default();
        chain.0.borrow_mut().height = height;
        chain.0.borrow_mut().fee_rate = 5;
        chain
    }

    /// Someone pays `sats` to `address`, not yet mined. Returns the coin.
    pub fn pay(&self, address: &Address, sats: u64) -> Coin {
        let mut b = self.0.borrow_mut();
        b.next += 1;
        let tx = Transaction {
            version: Version::TWO,
            lock_time: LockTime::ZERO,
            input: vec![TxIn {
                previous_output: OutPoint { txid: Txid::from_byte_array([b.next; 32]), vout: 0 },
                script_sig: ScriptBuf::new(),
                sequence: Sequence::MAX,
                witness: Witness::new(),
            }],
            output: vec![TxOut { value: Amount::from_sat(sats), script_pubkey: address.script_pubkey() }],
        };
        let txid = tx.compute_txid();
        b.txs.insert(txid, (tx, None));
        Coin { outpoint: OutPoint { txid, vout: 0 }, value: Amount::from_sat(sats) }
    }

    /// Mines `blocks` blocks: everything waiting is in the first.
    pub fn mine(&self, blocks: u32) {
        let mut b = self.0.borrow_mut();
        for _ in 0..blocks {
            b.height += 1;
            let height = b.height;
            for (_, mined) in b.txs.values_mut() {
                mined.get_or_insert(height);
            }
        }
    }

    /// What `address` has received from transactions the network holds.
    pub fn received(&self, address: &Address) -> u64 {
        let b = self.0.borrow();
        let script = address.script_pubkey();
        b.txs
            .values()
            .flat_map(|(tx, _)| tx.output.iter())
            .filter(|output| output.script_pubkey == script)
            .map(|output| output.value.to_sat())
            .sum()
    }

    fn value_of(b: &Bitcoin, outpoint: &OutPoint) -> Option<TxOut> {
        b.txs.get(&outpoint.txid).and_then(|(tx, _)| tx.output.get(outpoint.vout as usize).cloned())
    }

    fn fee_rate_of(b: &Bitcoin, tx: &Transaction) -> u64 {
        let inputs: u64 = tx.input.iter().filter_map(|i| Self::value_of(b, &i.previous_output)).map(|o| o.value.to_sat()).sum();
        let outputs: u64 = tx.output.iter().map(|o| o.value.to_sat()).sum();
        inputs.saturating_sub(outputs) / tx.vsize() as u64
    }
}

impl Chain for TestChain {
    fn height(&self) -> anyhow::Result<u32> {
        Ok(self.0.borrow().height)
    }
    fn fee_rate(&self) -> anyhow::Result<u64> {
        Ok(self.0.borrow().fee_rate)
    }
    fn coins(&self, address: &Address) -> anyhow::Result<Vec<(Coin, u32)>> {
        let b = self.0.borrow();
        let script = address.script_pubkey();
        let mut coins = Vec::new();
        for (txid, (tx, mined)) in &b.txs {
            for (vout, output) in tx.output.iter().enumerate() {
                if output.script_pubkey == script {
                    let coin = Coin { outpoint: OutPoint { txid: *txid, vout: vout as u32 }, value: output.value };
                    coins.push((coin, mined.map_or(0, |h| b.height - h + 1)));
                }
            }
        }
        Ok(coins)
    }
    fn transaction(&self, txid: &Txid) -> anyhow::Result<Option<Transaction>> {
        Ok(self.0.borrow().txs.get(txid).map(|(tx, _)| tx.clone()))
    }
    fn confirmations(&self, txid: &Txid) -> anyhow::Result<Option<u32>> {
        let b = self.0.borrow();
        Ok(b.txs.get(txid).map(|(_, mined)| mined.map_or(0, |h| b.height - h + 1)))
    }
    fn spender(&self, outpoint: &OutPoint) -> anyhow::Result<Option<Transaction>> {
        let b = self.0.borrow();
        Ok(b.spent.get(outpoint).and_then(|txid| b.txs.get(txid)).map(|(tx, _)| tx.clone()))
    }
    fn broadcast(&self, tx: &Transaction) -> anyhow::Result<()> {
        let mut b = self.0.borrow_mut();
        let txid = tx.compute_txid();
        if b.txs.contains_key(&txid) {
            return Ok(());
        }
        // A lock time must have passed, as real nodes require.
        if tx.lock_time.to_consensus_u32() > b.height {
            anyhow::bail!("non-final transaction");
        }
        let lookup = |outpoint: &OutPoint| TestChain::value_of(&b, outpoint);
        tx.verify(lookup).map_err(|e| anyhow::anyhow!("Bitcoin's rules refuse it: {e:?}"))?;
        // A spend of an output already spent: refused if that spend is mined,
        // replaced if it is waiting and pays a lower fee rate.
        let mut replaced = Vec::new();
        for input in &tx.input {
            if let Some(other) = b.spent.get(&input.previous_output).copied() {
                let (old, mined) = b.txs[&other].clone();
                if mined.is_some() || TestChain::fee_rate_of(&b, &old) >= TestChain::fee_rate_of(&b, tx) {
                    anyhow::bail!("conflicts with a transaction already sent");
                }
                replaced.push(other);
            }
        }
        for old in replaced {
            if let Some((old_tx, _)) = b.txs.remove(&old) {
                for input in &old_tx.input {
                    b.spent.remove(&input.previous_output);
                }
            }
        }
        for input in &tx.input {
            b.spent.insert(input.previous_output, txid);
        }
        b.txs.insert(txid, (tx.clone(), None));
        Ok(())
    }
}
