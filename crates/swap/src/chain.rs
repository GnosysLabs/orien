//! What a swap needs to know about the Bitcoin network, and a connection to
//! it through a public Esplora explorer (mempool.space by default).

use crate::htlc::Coin;
use anyhow::{Context, Result, anyhow, bail};
use bitcoin::consensus::encode;
use bitcoin::{Address, Amount, OutPoint, Transaction, Txid};
use serde::Deserialize;
use std::str::FromStr;
use std::time::Duration;

/// What the Bitcoin network says. A swap only reads it and sends finished
/// transactions to it, so a lying explorer can delay a swap but cannot take
/// its coins: every transaction the swap acts on is checked here.
pub trait Chain {
    /// The height of the newest block.
    fn height(&self) -> Result<u32>;
    /// The fee rate, in satoshis per virtual byte, for a transaction to be
    /// in one of the next few blocks.
    fn fee_rate(&self) -> Result<u64>;
    /// Every coin paid to `address`, with how many blocks confirm it (0 while
    /// it waits to be mined).
    fn coins(&self, address: &Address) -> Result<Vec<(Coin, u32)>>;
    /// The transaction that spent `outpoint`, if one has, mined or not.
    fn spender(&self, outpoint: &OutPoint) -> Result<Option<Transaction>>;
    /// Sends a transaction to the network.
    fn broadcast(&self, tx: &Transaction) -> Result<Txid>;
}

/// A Bitcoin network reached through an Esplora explorer's public API.
pub struct Esplora {
    base: String,
    agent: ureq::Agent,
}

impl Esplora {
    /// Bitcoin itself, through mempool.space.
    pub fn bitcoin() -> Esplora {
        Esplora::at("https://mempool.space/api")
    }

    pub fn at(base: &str) -> Esplora {
        let agent = ureq::AgentBuilder::new().timeout(Duration::from_secs(20)).user_agent("orien-swap").build();
        Esplora { base: base.trim_end_matches('/').to_string(), agent }
    }

    fn get(&self, path: &str) -> Result<String> {
        let url = format!("{}{path}", self.base);
        self.agent.get(&url).call().with_context(|| format!("cannot reach {url}"))?.into_string().map_err(Into::into)
    }
}

#[derive(Deserialize)]
struct Utxo {
    txid: String,
    vout: u32,
    value: u64,
    status: Status,
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

impl Chain for Esplora {
    fn height(&self) -> Result<u32> {
        self.get("/blocks/tip/height")?.trim().parse().context("the explorer gave no block height")
    }

    fn fee_rate(&self) -> Result<u64> {
        // mempool.space's own estimate; plain Esplora servers have another
        // endpoint, so fall back to it.
        if let Ok(text) = self.get("/v1/fees/recommended")
            && let Ok(fees) = serde_json::from_str::<Fees>(&text)
        {
            return Ok(fees.fastest.max(1));
        }
        let estimates: std::collections::HashMap<String, f64> = serde_json::from_str(&self.get("/fee-estimates")?)?;
        let rate = estimates.get("2").or_else(|| estimates.get("1")).copied().unwrap_or(2.0);
        Ok(rate.ceil().max(1.0) as u64)
    }

    fn coins(&self, address: &Address) -> Result<Vec<(Coin, u32)>> {
        let utxos: Vec<Utxo> = serde_json::from_str(&self.get(&format!("/address/{address}/utxo"))?)?;
        let tip = self.height()?;
        utxos
            .into_iter()
            .map(|utxo| {
                let txid = Txid::from_str(&utxo.txid).map_err(|_| anyhow!("the explorer gave a bad txid"))?;
                let confirmations = match (utxo.status.confirmed, utxo.status.block_height) {
                    (true, Some(height)) => tip.saturating_sub(height) + 1,
                    _ => 0,
                };
                Ok((Coin { outpoint: OutPoint { txid, vout: utxo.vout }, value: Amount::from_sat(utxo.value) }, confirmations))
            })
            .collect()
    }

    fn spender(&self, outpoint: &OutPoint) -> Result<Option<Transaction>> {
        let outspend: Outspend =
            serde_json::from_str(&self.get(&format!("/tx/{}/outspend/{}", outpoint.txid, outpoint.vout))?)?;
        let Some(txid) = outspend.txid.filter(|_| outspend.spent) else { return Ok(None) };
        let hex = self.get(&format!("/tx/{txid}/hex"))?;
        let tx: Transaction = encode::deserialize_hex(hex.trim()).context("the explorer gave a bad transaction")?;
        // The transaction must really spend this coin.
        if !tx.input.iter().any(|input| input.previous_output == *outpoint) {
            bail!("the explorer named a transaction that does not spend the coin");
        }
        Ok(Some(tx))
    }

    fn broadcast(&self, tx: &Transaction) -> Result<Txid> {
        let url = format!("{}/tx", self.base);
        let answer = self.agent.post(&url).send_string(&encode::serialize_hex(tx));
        match answer {
            Ok(response) => Txid::from_str(response.into_string()?.trim()).map_err(|_| anyhow!("the explorer gave a bad txid")),
            Err(ureq::Error::Status(_, response)) => {
                bail!("Bitcoin refused the transaction: {}", response.into_string().unwrap_or_default())
            }
            Err(error) => Err(error).context("cannot reach the explorer"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Reads the real Bitcoin network. Run with `--ignored`; it needs the
    /// internet.
    #[test]
    #[ignore]
    fn reads_bitcoin_through_mempool_space() {
        let chain = Esplora::bitcoin();
        let height = chain.height().unwrap();
        assert!(height > 900_000, "{height}");
        let rate = chain.fee_rate().unwrap();
        assert!((1..10_000).contains(&rate), "{rate}");
        // The first output of the genesis block's coinbase can never be spent.
        let genesis = OutPoint {
            txid: Txid::from_str("4a5e1e4baab89f3a32518a88c31bc87f618f76673e2cc77ab2127b7afdeda33b").unwrap(),
            vout: 0,
        };
        assert!(chain.spender(&genesis).unwrap().is_none());
        // Block 170's coin, which Satoshi sent Hal Finney, was spent; its
        // spender is fetched and checked.
        let first = OutPoint {
            txid: Txid::from_str("0437cd7f8525ceed2324359c2d0ba26006d92d856a9c20fa0241106ee5a597c9").unwrap(),
            vout: 0,
        };
        assert!(chain.spender(&first).unwrap().is_some());
        println!("height {height}, fee {rate} sat/vB");
    }
}
