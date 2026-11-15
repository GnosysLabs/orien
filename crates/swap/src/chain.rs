//! What a swap needs to know about the Bitcoin network, and a connection to
//! it through public Esplora explorers.
//!
//! An explorer is not trusted. What a transaction says is checked against
//! its own bytes, which hash to its id. What cannot be checked that way,
//! whether a transaction is in a block and how deep, is asked of two
//! independent explorers, and the less favourable answer is used.

use crate::htlc::Coin;
use anyhow::{Context, Result, anyhow, bail};
use bitcoin::consensus::encode;
use bitcoin::{Address, Amount, OutPoint, Transaction, Txid};
use serde::Deserialize;
use std::str::FromStr;
use std::time::Duration;

/// The fee rates a swap accepts, in satoshis per virtual byte, whatever an
/// explorer says.
pub const FEE_RATES: std::ops::RangeInclusive<u64> = 1..=500;

/// What the Bitcoin network says.
pub trait Chain {
    /// The height of the newest block.
    fn height(&self) -> Result<u32>;
    /// The fee rate, in satoshis per virtual byte, for a transaction to be
    /// in one of the next few blocks.
    fn fee_rate(&self) -> Result<u64>;
    /// Every coin said to have been paid to `address`, spent or not, with
    /// how many blocks confirm it (0 while waiting to be mined). Unchecked:
    /// use `paid_to`.
    fn coins(&self, address: &Address) -> Result<Vec<(Coin, u32)>>;
    /// A transaction by its id, if the network has it.
    fn transaction(&self, txid: &Txid) -> Result<Option<Transaction>>;
    /// How many blocks confirm a transaction: `None` when the network does
    /// not have it, `Some(0)` while it waits to be mined.
    fn confirmations(&self, txid: &Txid) -> Result<Option<u32>>;
    /// The transaction that spent `outpoint`, if one has, mined or not.
    fn spender(&self, outpoint: &OutPoint) -> Result<Option<Transaction>>;
    /// Sends a transaction to the network.
    fn broadcast(&self, tx: &Transaction) -> Result<()>;
}

/// The coins paid to `address`, each checked against its own transaction:
/// the transaction exists, hashes to its id, and pays this address this
/// amount. Coins an explorer made up, or misreported, are left out.
pub fn paid_to(chain: &dyn Chain, address: &Address) -> Result<Vec<(Coin, u32)>> {
    let script = address.script_pubkey();
    let mut checked = Vec::new();
    for (coin, confirmations) in chain.coins(address)? {
        let Some(tx) = chain.transaction(&coin.outpoint.txid)? else { continue };
        if tx.compute_txid() != coin.outpoint.txid {
            continue;
        }
        let Some(output) = tx.output.get(coin.outpoint.vout as usize) else { continue };
        if output.script_pubkey != script {
            continue;
        }
        let coin = Coin { outpoint: coin.outpoint, value: output.value };
        if !checked.iter().any(|(known, _): &(Coin, u32)| known.outpoint == coin.outpoint) {
            checked.push((coin, confirmations));
        }
    }
    Ok(checked)
}

/// A Bitcoin network reached through one Esplora explorer's public API.
pub struct Esplora {
    base: String,
    agent: ureq::Agent,
}

impl Esplora {
    pub fn at(base: &str) -> Esplora {
        let agent = ureq::AgentBuilder::new().timeout(Duration::from_secs(20)).user_agent("orien-swap").build();
        Esplora { base: base.trim_end_matches('/').to_string(), agent }
    }

    fn get(&self, path: &str) -> Result<String> {
        let url = format!("{}{path}", self.base);
        self.agent.get(&url).call().with_context(|| format!("cannot reach {url}"))?.into_string().map_err(Into::into)
    }

    /// Like `get`, but a 404 is an answer: "not there".
    fn find(&self, path: &str) -> Result<Option<String>> {
        let url = format!("{}{path}", self.base);
        match self.agent.get(&url).call() {
            Ok(response) => Ok(Some(response.into_string()?)),
            Err(ureq::Error::Status(404, _)) => Ok(None),
            Err(error) => Err(error).with_context(|| format!("cannot reach {url}")),
        }
    }
}

#[derive(Deserialize)]
struct Tx {
    txid: String,
    vout: Vec<Output>,
    status: Status,
}

#[derive(Deserialize)]
struct Output {
    scriptpubkey_address: Option<String>,
    value: u64,
}

#[derive(Deserialize)]
struct Status {
    confirmed: bool,
    block_height: Option<u32>,
}

#[derive(Deserialize)]
struct Outspend {
    spent: bool,
    txid: Option<String>,
}

#[derive(Deserialize)]
struct Fees {
    #[serde(rename = "fastestFee")]
    fastest: u64,
}

fn depth(status: &Status, tip: u32) -> u32 {
    match (status.confirmed, status.block_height) {
        (true, Some(height)) => tip.saturating_sub(height) + 1,
        _ => 0,
    }
}

impl Chain for Esplora {
    fn height(&self) -> Result<u32> {
        self.get("/blocks/tip/height")?.trim().parse().context("the explorer gave no block height")
    }

    fn fee_rate(&self) -> Result<u64> {
        if let Ok(text) = self.get("/v1/fees/recommended")
            && let Ok(fees) = serde_json::from_str::<Fees>(&text)
        {
            return Ok(fees.fastest);
        }
        let estimates: std::collections::HashMap<String, f64> = serde_json::from_str(&self.get("/fee-estimates")?)?;
        let rate = estimates.get("2").or_else(|| estimates.get("1")).copied().unwrap_or(2.0);
        Ok(rate.ceil().max(1.0) as u64)
    }

    fn coins(&self, address: &Address) -> Result<Vec<(Coin, u32)>> {
        // A swap address sees a handful of transactions, far fewer than the
        // 25 one answer holds.
        let txs: Vec<Tx> = serde_json::from_str(&self.get(&format!("/address/{address}/txs"))?)?;
        let tip = self.height()?;
        let wanted = address.to_string();
        let mut coins = Vec::new();
        for tx in txs {
            let txid = Txid::from_str(&tx.txid).map_err(|_| anyhow!("the explorer gave a bad txid"))?;
            for (vout, output) in tx.vout.iter().enumerate() {
                if output.scriptpubkey_address.as_deref() == Some(wanted.as_str()) {
                    let outpoint = OutPoint { txid, vout: vout as u32 };
                    coins.push((Coin { outpoint, value: Amount::from_sat(output.value) }, depth(&tx.status, tip)));
                }
            }
        }
        Ok(coins)
    }

    fn transaction(&self, txid: &Txid) -> Result<Option<Transaction>> {
        let Some(hex) = self.find(&format!("/tx/{txid}/hex"))? else { return Ok(None) };
        let tx: Transaction = encode::deserialize_hex(hex.trim()).context("the explorer gave a bad transaction")?;
        Ok((tx.compute_txid() == *txid).then_some(tx))
    }

    fn confirmations(&self, txid: &Txid) -> Result<Option<u32>> {
        let Some(text) = self.find(&format!("/tx/{txid}/status"))? else { return Ok(None) };
        let status: Status = serde_json::from_str(&text)?;
        Ok(Some(depth(&status, self.height()?)))
    }

    fn spender(&self, outpoint: &OutPoint) -> Result<Option<Transaction>> {
        let outspend: Outspend =
            serde_json::from_str(&self.get(&format!("/tx/{}/outspend/{}", outpoint.txid, outpoint.vout))?)?;
        let Some(txid) = outspend.txid.filter(|_| outspend.spent) else { return Ok(None) };
        let txid = Txid::from_str(&txid).map_err(|_| anyhow!("the explorer gave a bad txid"))?;
        // The transaction must really spend this coin.
        Ok(self.transaction(&txid)?.filter(|tx| tx.input.iter().any(|input| input.previous_output == *outpoint)))
    }

    fn broadcast(&self, tx: &Transaction) -> Result<()> {
        let url = format!("{}/tx", self.base);
        match self.agent.post(&url).send_string(&encode::serialize_hex(tx)) {
            Ok(_) => Ok(()),
            Err(ureq::Error::Status(_, response)) => {
                bail!("Bitcoin refused the transaction: {}", response.into_string().unwrap_or_default())
            }
            Err(error) => Err(error).context("cannot reach the explorer"),
        }
    }
}

/// Several independent explorers, asked together. A fact matters only if
/// all of them back it: a coin is counted only if every explorer reports it,
/// with the smallest number of confirmations any of them gives; the height
/// is the highest any reports, so deadlines are never thought further off
/// than they are. Any explorer's checked transaction or spender is enough,
/// since it proves itself.
pub struct Explorers(pub Vec<Esplora>);

impl Explorers {
    /// Bitcoin itself, through mempool.space and blockstream.info.
    pub fn bitcoin() -> Explorers {
        Explorers(vec![Esplora::at("https://mempool.space/api"), Esplora::at("https://blockstream.info/api")])
    }
}

impl Chain for Explorers {
    fn height(&self) -> Result<u32> {
        let mut highest = None;
        for explorer in &self.0 {
            highest = highest.max(Some(explorer.height()?));
        }
        highest.ok_or_else(|| anyhow!("no Bitcoin explorer is set"))
    }

    fn fee_rate(&self) -> Result<u64> {
        let mut rates = Vec::new();
        for explorer in &self.0 {
            if let Ok(rate) = explorer.fee_rate() {
                rates.push(rate.clamp(*FEE_RATES.start(), *FEE_RATES.end()));
            }
        }
        rates.into_iter().max().ok_or_else(|| anyhow!("no Bitcoin explorer gave a fee rate"))
    }

    fn coins(&self, address: &Address) -> Result<Vec<(Coin, u32)>> {
        let mut agreed: Option<Vec<(Coin, u32)>> = None;
        for explorer in &self.0 {
            let said = explorer.coins(address)?;
            agreed = Some(match agreed {
                None => said,
                Some(before) => before
                    .into_iter()
                    .filter_map(|(coin, depth)| {
                        said.iter().find(|(other, _)| *other == coin).map(|(_, other)| (coin, depth.min(*other)))
                    })
                    .collect(),
            });
        }
        Ok(agreed.unwrap_or_default())
    }

    fn transaction(&self, txid: &Txid) -> Result<Option<Transaction>> {
        for explorer in &self.0 {
            if let Ok(Some(tx)) = explorer.transaction(txid) {
                return Ok(Some(tx));
            }
        }
        Ok(None)
    }

    fn confirmations(&self, txid: &Txid) -> Result<Option<u32>> {
        let mut least: Option<Option<u32>> = None;
        for explorer in &self.0 {
            let said = explorer.confirmations(txid)?;
            least = Some(match least {
                None => said,
                Some(before) => match (before, said) {
                    (Some(a), Some(b)) => Some(a.min(b)),
                    _ => None,
                },
            });
        }
        Ok(least.flatten())
    }

    fn spender(&self, outpoint: &OutPoint) -> Result<Option<Transaction>> {
        let mut last = Ok(None);
        for explorer in &self.0 {
            match explorer.spender(outpoint) {
                Ok(Some(tx)) => return Ok(Some(tx)),
                other => last = other,
            }
        }
        last
    }

    fn broadcast(&self, tx: &Transaction) -> Result<()> {
        let mut outcome = Err(anyhow!("no Bitcoin explorer is set"));
        for explorer in &self.0 {
            match explorer.broadcast(tx) {
                Ok(()) => outcome = Ok(()),
                Err(error) if outcome.is_err() => outcome = Err(error),
                Err(_) => {}
            }
        }
        outcome
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Reads the real Bitcoin network through both explorers. Run with
    /// `--ignored`; it needs the internet.
    #[test]
    #[ignore]
    fn reads_bitcoin_through_two_explorers() {
        let chain = Explorers::bitcoin();
        let height = chain.height().unwrap();
        assert!(height > 900_000, "{height}");
        let rate = chain.fee_rate().unwrap();
        assert!(FEE_RATES.contains(&rate), "{rate}");
        // The first output of the genesis block's coinbase can never be spent.
        let genesis = OutPoint {
            txid: Txid::from_str("4a5e1e4baab89f3a32518a88c31bc87f618f76673e2cc77ab2127b7afdeda33b").unwrap(),
            vout: 0,
        };
        assert!(chain.spender(&genesis).unwrap().is_none());
        // Block 9's coinbase, which Satoshi sent to Hal Finney in block 170:
        // spent, its spender fetched and checked.
        let first = OutPoint {
            txid: Txid::from_str("0437cd7f8525ceed2324359c2d0ba26006d92d856a9c20fa0241106ee5a597c9").unwrap(),
            vout: 0,
        };
        let spender = chain.spender(&first).unwrap().expect("spent");
        // That spending transaction, deeply confirmed.
        assert!(chain.confirmations(&spender.compute_txid()).unwrap().unwrap() > 900_000);
        // Hal Finney's address: the coins it was paid, each checked against
        // its own transaction.
        let hal = Address::from_str("1Q2TWHE3GMdB6BZKafqwxXtWAWgFt5Jvm3").unwrap().assume_checked();
        assert!(!paid_to(&chain, &hal).unwrap().is_empty());
        println!("height {height}, fee {rate} sat/vB");
    }
}
