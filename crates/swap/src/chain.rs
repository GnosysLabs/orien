//! What a swap needs to know about the Bitcoin network, and a connection to
//! it through public Esplora explorers.
//!
//! No explorer is trusted. What a transaction says is checked against its
//! own bytes, which hash to its id. What cannot be checked that way, how
//! deep a transaction lies and how high the chain is, is asked of several
//! independent explorers, with a rule chosen for each decision:
//!
//! - Decisions that could cost the seller (revealing the secret, calling a
//!   claim settled) need most explorers (two of three) to answer and agree,
//!   and take the least favourable answer: one explorer that is down, busy
//!   or lying can neither stall a swap nor fool it.
//!
//! Explorers are asked gently: an answer is reused for half a minute (the
//! chain's height for a minute), since nothing on Bitcoin changes faster,
//! and an explorer that says it's busy is left alone for a few minutes.
//! - What keeps the buyer safe (seeing a payment, finding the secret,
//!   refunding) works as long as any one explorer answers, since what it
//!   says is checked or harmless if wrong.

use crate::htlc::Coin;
use anyhow::{Context, Result, anyhow, bail};
use bitcoin::consensus::encode;
use bitcoin::{Address, Amount, OutPoint, Transaction, Txid};
use serde::Deserialize;
use std::collections::HashMap;
use std::str::FromStr;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// The fee rates a swap accepts, in satoshis per virtual byte, whatever an
/// explorer says.
pub const FEE_RATES: std::ops::RangeInclusive<u64> = 1..=500;

/// How far apart explorers' heights may be before the lower one is doubted.
pub const HEIGHT_SPREAD: u32 = 2;

/// What one explorer says about the Bitcoin network.
pub trait Chain {
    /// The height of the newest block.
    fn height(&self) -> Result<u32>;
    /// The fee rate, in satoshis per virtual byte, for a transaction to be
    /// in one of the next few blocks.
    fn fee_rate(&self) -> Result<u64>;
    /// Every coin said to have been paid to `address`, spent or not, with
    /// how many blocks confirm it (0 while waiting to be mined). Unchecked.
    fn coins(&self, address: &Address) -> Result<Vec<(Coin, u32)>>;
    /// A transaction by its id, if the explorer has it.
    fn transaction(&self, txid: &Txid) -> Result<Option<Transaction>>;
    /// How many blocks confirm a transaction: `None` when the explorer does
    /// not have it, `Some(0)` while it waits to be mined.
    fn confirmations(&self, txid: &Txid) -> Result<Option<u32>>;
    /// The transaction that spent `outpoint`, if one has, mined or not.
    fn spender(&self, outpoint: &OutPoint) -> Result<Option<Transaction>>;
    /// Sends a transaction to the network.
    fn broadcast(&self, tx: &Transaction) -> Result<()>;
}

/// Checks a reported coin against its own transaction: the transaction
/// hashes to its id and pays this address; the value is the transaction's.
fn check(chain: &dyn Chain, coin: &Coin, address: &Address) -> Result<Option<Coin>> {
    let Some(tx) = chain.transaction(&coin.outpoint.txid)? else { return Ok(None) };
    if tx.compute_txid() != coin.outpoint.txid {
        return Ok(None);
    }
    Ok(tx
        .output
        .get(coin.outpoint.vout as usize)
        .filter(|output| output.script_pubkey == address.script_pubkey())
        .map(|output| Coin { outpoint: coin.outpoint, value: output.value }))
}

/// Several independent explorers, asked together.
pub struct Explorers(pub Vec<Box<dyn Chain>>);

impl Explorers {
    /// Bitcoin itself, through three independently run explorers.
    pub fn bitcoin() -> Explorers {
        Explorers(vec![
            Box::new(Esplora::at("https://mempool.space/api")),
            Box::new(Esplora::at("https://blockstream.info/api")),
            Box::new(Esplora::at("https://mempool.emzy.de/api")),
        ])
    }

    /// How many explorers must answer a decision that could cost the
    /// seller: most of them (two of three; both of two; the one of one).
    fn quorum(&self) -> usize {
        self.0.len() / 2 + 1
    }

    /// One explorer, for a test network.
    pub fn one(chain: impl Chain + 'static) -> Explorers {
        Explorers(vec![Box::new(chain)])
    }

    /// The answers of the explorers that answer; an error only if none does.
    fn answers<T>(&self, ask: impl Fn(&dyn Chain) -> Result<T>) -> Result<Vec<T>> {
        let mut answers = Vec::new();
        let mut last = None;
        for explorer in &self.0 {
            match ask(explorer.as_ref()) {
                Ok(answer) => answers.push(answer),
                Err(error) => last = Some(error),
            }
        }
        if answers.is_empty() {
            return Err(last.unwrap_or_else(|| anyhow!("no Bitcoin explorer is set")));
        }
        Ok(answers)
    }

    /// The answers of most explorers; an error if too few answer.
    fn most<T>(&self, ask: impl Fn(&dyn Chain) -> Result<T>) -> Result<Vec<T>> {
        let mut answers = Vec::new();
        let mut last = None;
        for explorer in &self.0 {
            match ask(explorer.as_ref()) {
                Ok(answer) => answers.push(answer),
                Err(error) => last = Some(error),
            }
        }
        if answers.len() < self.quorum() {
            return Err(last.unwrap_or_else(|| anyhow!("no Bitcoin explorer is set"))
                .context("too few Bitcoin explorers answered; trying again shortly"));
        }
        Ok(answers)
    }

    /// The highest height any explorer reports: deadlines are never thought
    /// further off than they are.
    pub fn height(&self) -> Result<u32> {
        Ok(self.answers(|c| c.height())?.into_iter().max().expect("at least one"))
    }

    /// The lowest height the explorers report, most of them answering,
    /// refused if they disagree by more than `HEIGHT_SPREAD`: for setting
    /// or calling a deadline, so that no one explorer can move it.
    pub fn agreed_height(&self) -> Result<u32> {
        let heights = self.most(|c| c.height())?;
        let (low, high) = (*heights.iter().min().expect("one"), *heights.iter().max().expect("one"));
        if high - low > HEIGHT_SPREAD {
            bail!("the Bitcoin explorers disagree about the chain's height ({low} and {high}); try again shortly");
        }
        Ok(low)
    }

    /// The highest sane fee rate any explorer gives, so that none can hold
    /// a claim or refund below what the network needs. What it costs is
    /// held in check by the fee cap.
    pub fn fee_rate(&self) -> Result<u64> {
        let rates = self.answers(|c| c.fee_rate())?;
        Ok(rates.into_iter().map(|r| r.clamp(*FEE_RATES.start(), *FEE_RATES.end())).max().expect("one"))
    }

    /// The coins every answering explorer reports and checks out, most of
    /// them answering, at the smallest depth any gives: for the seller,
    /// before revealing the secret.
    pub fn coins_all(&self, address: &Address) -> Result<Vec<(Coin, u32)>> {
        let said = self.most(|explorer| {
            let mut checked = Vec::new();
            for (coin, depth) in explorer.coins(address)? {
                if let Some(coin) = check(explorer, &coin, address)? {
                    checked.push((coin, depth));
                }
            }
            Ok(checked)
        })?;
        let mut agreed: Option<Vec<(Coin, u32)>> = None;
        for checked in said {
            agreed = Some(match agreed {
                None => checked,
                Some(before) => before
                    .into_iter()
                    .filter_map(|(coin, depth)| {
                        checked.iter().find(|(other, _)| other.outpoint == coin.outpoint).map(|(_, d)| (coin, depth.min(*d)))
                    })
                    .collect(),
            });
        }
        Ok(agreed.unwrap_or_default())
    }

    /// Every coin any answering explorer reports and checks out, at the
    /// greatest depth any gives: for the buyer, who must not miss one.
    pub fn coins_any(&self, address: &Address) -> Result<Vec<(Coin, u32)>> {
        let mut found: Vec<(Coin, u32)> = Vec::new();
        for said in self.answers(|explorer| {
            let mut checked = Vec::new();
            for (coin, depth) in explorer.coins(address)? {
                if let Some(coin) = check(explorer, &coin, address)? {
                    checked.push((coin, depth));
                }
            }
            Ok(checked)
        })? {
            for (coin, depth) in said {
                match found.iter_mut().find(|(known, _)| known.outpoint == coin.outpoint) {
                    Some((_, known)) => *known = (*known).max(depth),
                    None => found.push((coin, depth)),
                }
            }
        }
        Ok(found)
    }

    /// How deep the answering explorers agree a transaction is, most of
    /// them answering: `None` unless each of them has it. For calling a
    /// claim settled.
    pub fn depth_all(&self, txid: &Txid) -> Result<Option<u32>> {
        let depths = self.most(|c| c.confirmations(txid))?;
        Ok(depths.into_iter().try_fold(u32::MAX, |least, depth| depth.map(|d| least.min(d))))
    }

    /// How deep every answering explorer agrees a transaction is: `None` if
    /// any of them does not have it. For deciding whether to send it again,
    /// which no explorer can talk a wallet out of.
    pub fn depth_least(&self, txid: &Txid) -> Result<Option<u32>> {
        let depths = self.answers(|c| c.confirmations(txid))?;
        Ok(depths.into_iter().try_fold(u32::MAX, |least, depth| depth.map(|d| least.min(d))))
    }

    /// How deep any answering explorer says a transaction is; `None` if
    /// none has it.
    pub fn depth_any(&self, txid: &Txid) -> Result<Option<u32>> {
        Ok(self.answers(|c| c.confirmations(txid))?.into_iter().flatten().max())
    }

    /// A transaction any explorer has, checked against its id.
    pub fn transaction(&self, txid: &Txid) -> Result<Option<Transaction>> {
        let found = self.answers(|c| c.transaction(txid))?;
        Ok(found.into_iter().flatten().find(|tx| tx.compute_txid() == *txid))
    }

    /// Every transaction any explorer says spent `outpoint`, each checked to
    /// spend it.
    pub fn spenders(&self, outpoint: &OutPoint) -> Result<Vec<Transaction>> {
        let mut found: Vec<Transaction> = Vec::new();
        for tx in self.answers(|c| c.spender(outpoint))?.into_iter().flatten() {
            if tx.input.iter().any(|input| input.previous_output == *outpoint)
                && !found.iter().any(|known| known.compute_txid() == tx.compute_txid())
            {
                found.push(tx);
            }
        }
        Ok(found)
    }

    /// Sends to every explorer; fine if any accepts it.
    pub fn broadcast(&self, tx: &Transaction) -> Result<()> {
        self.answers(|c| c.broadcast(tx)).map(|_| ())
    }
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
        self.find(path)?.ok_or_else(|| anyhow!("{}{path} was not found", self.base))
    }

    /// Like `get`, but a 404 is an answer: "not there". Answers are reused
    /// while fresh, and an explorer that says it's busy (429) or failing
    /// (5xx) is left alone for as long as it asks, a few minutes at most.
    fn find(&self, path: &str) -> Result<Option<String>> {
        let url = format!("{}{path}", self.base);
        let fresh = if path == "/blocks/tip/height" { TIP_FRESH } else { FRESH };
        if let Some(answer) = remembered(&url, fresh) {
            return Ok(Some(answer));
        }
        if let Some(until) = resting(&self.base) {
            bail!("{} is busy; asking again in {} seconds", self.base, until.saturating_duration_since(Instant::now()).as_secs());
        }
        match self.agent.get(&url).call() {
            Ok(response) => {
                let answer = response.into_string()?;
                remember(&url, &answer);
                Ok(Some(answer))
            }
            Err(ureq::Error::Status(404, _)) => Ok(None),
            Err(ureq::Error::Status(code, response)) if code == 429 || code >= 500 => {
                let asked = response.header("retry-after").and_then(|after| after.trim().parse::<u64>().ok());
                rest(&self.base, Duration::from_secs(asked.unwrap_or(REST.as_secs()).clamp(10, MAX_REST.as_secs())));
                bail!("{} is busy ({code}); asking the other explorers", self.base)
            }
            Err(error) => Err(error).with_context(|| format!("cannot reach {url}")),
        }
    }
}

/// How long an answer is reused: nothing on Bitcoin changes much faster.
pub(crate) const FRESH: Duration = Duration::from_secs(30);
/// The chain's height, reused a little longer: a block comes every ten
/// minutes or so.
const TIP_FRESH: Duration = Duration::from_secs(60);
/// How long a busy explorer is left alone when it doesn't say.
pub(crate) const REST: Duration = Duration::from_secs(120);
/// The longest it's left alone, whatever it says.
pub(crate) const MAX_REST: Duration = Duration::from_secs(600);

/// Recent answers by address, shared by every `Esplora`.
static ANSWERS: Mutex<Option<HashMap<String, (Instant, String)>>> = Mutex::new(None);
/// Explorers resting, and until when.
static RESTING: Mutex<Option<HashMap<String, Instant>>> = Mutex::new(None);

pub(crate) fn remembered(url: &str, fresh: Duration) -> Option<String> {
    let guard = ANSWERS.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let (at, answer) = guard.as_ref()?.get(url)?;
    (at.elapsed() < fresh).then(|| answer.clone())
}

pub(crate) fn remember(url: &str, answer: &str) {
    let mut guard = ANSWERS.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let all = guard.get_or_insert_with(HashMap::new);
    all.retain(|_, (at, _)| at.elapsed() < MAX_REST);
    all.insert(url.to_string(), (Instant::now(), answer.to_string()));
}

pub(crate) fn resting(base: &str) -> Option<Instant> {
    let guard = RESTING.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    guard.as_ref()?.get(base).copied().filter(|until| *until > Instant::now())
}

pub(crate) fn rest(base: &str, length: Duration) {
    let mut guard = RESTING.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    guard.get_or_insert_with(HashMap::new).insert(base.to_string(), Instant::now() + length);
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

/// The most pages of an address's history read: 50 waiting transactions and
/// 25 per page of mined ones, so a swap address flooded with payments to
/// hide the real one is still read to the end.
const PAGES: usize = 40;

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
        let tip = self.height()?;
        let wanted = address.to_string();
        let mut coins = Vec::new();
        // The first answer holds the waiting transactions and the newest
        // mined ones; each later page, older mined ones.
        let mut page: Vec<Tx> = serde_json::from_str(&self.get(&format!("/address/{address}/txs"))?)?;
        for _ in 0..PAGES {
            let mut last_mined = None;
            for tx in &page {
                let txid = Txid::from_str(&tx.txid).map_err(|_| anyhow!("the explorer gave a bad txid"))?;
                if tx.status.confirmed {
                    last_mined = Some(tx.txid.clone());
                }
                for (vout, output) in tx.vout.iter().enumerate() {
                    if output.scriptpubkey_address.as_deref() == Some(wanted.as_str()) {
                        let outpoint = OutPoint { txid, vout: vout as u32 };
                        coins.push((Coin { outpoint, value: Amount::from_sat(output.value) }, depth(&tx.status, tip)));
                    }
                }
            }
            let mined = page.iter().filter(|tx| tx.status.confirmed).count();
            let Some(last) = last_mined.filter(|_| mined >= 25) else { break };
            page = serde_json::from_str(&self.get(&format!("/address/{address}/txs/chain/{last}"))?)?;
            if page.is_empty() {
                break;
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

#[cfg(test)]
mod tests {
    use super::*;

    /// An explorer that knows only the chain's height, or is down.
    struct Height(Option<u32>);

    impl Chain for Height {
        fn height(&self) -> Result<u32> {
            self.0.ok_or_else(|| anyhow!("busy"))
        }
        fn fee_rate(&self) -> Result<u64> {
            bail!("not asked")
        }
        fn coins(&self, _: &Address) -> Result<Vec<(Coin, u32)>> {
            bail!("not asked")
        }
        fn transaction(&self, _: &Txid) -> Result<Option<Transaction>> {
            bail!("not asked")
        }
        fn confirmations(&self, _: &Txid) -> Result<Option<u32>> {
            bail!("not asked")
        }
        fn spender(&self, _: &OutPoint) -> Result<Option<Transaction>> {
            bail!("not asked")
        }
        fn broadcast(&self, _: &Transaction) -> Result<()> {
            bail!("not asked")
        }
    }

    fn explorers(heights: &[Option<u32>]) -> Explorers {
        Explorers(heights.iter().map(|h| Box::new(Height(*h)) as Box<dyn Chain>).collect())
    }

    #[test]
    fn one_explorer_down_of_three_stalls_nothing_but_two_do() {
        assert_eq!(explorers(&[Some(100), None, Some(101)]).agreed_height().unwrap(), 100);
        assert!(explorers(&[Some(100), None, None]).agreed_height().is_err());
        // Two explorers: both must answer, as before.
        assert!(explorers(&[Some(100), None]).agreed_height().is_err());
        assert_eq!(explorers(&[Some(100)]).agreed_height().unwrap(), 100);
        // The answering ones must still agree.
        assert!(explorers(&[Some(100), None, Some(110)]).agreed_height().is_err());
    }

    #[test]
    fn a_busy_explorer_is_left_alone_for_a_while() {
        let base = "https://busy.example/api";
        assert!(resting(base).is_none());
        rest(base, Duration::from_secs(60));
        assert!(resting(base).is_some());
        let explorer = Esplora::at(base);
        let error = explorer.height().unwrap_err().to_string();
        assert!(error.contains("busy"), "{error}");
    }

    /// Reads the real Bitcoin network through the three explorers. Run with
    /// `--ignored`; it needs the internet.
    #[test]
    #[ignore]
    fn reads_bitcoin_through_three_explorers() {
        let chain = Explorers::bitcoin();
        let height = chain.agreed_height().unwrap();
        assert!(height > 900_000, "{height}");
        let rate = chain.fee_rate().unwrap();
        assert!(FEE_RATES.contains(&rate), "{rate}");
        let genesis = OutPoint {
            txid: Txid::from_str("4a5e1e4baab89f3a32518a88c31bc87f618f76673e2cc77ab2127b7afdeda33b").unwrap(),
            vout: 0,
        };
        assert!(chain.spenders(&genesis).unwrap().is_empty());
        // Block 9's coinbase, which Satoshi sent to Hal Finney in block 170.
        let first = OutPoint {
            txid: Txid::from_str("0437cd7f8525ceed2324359c2d0ba26006d92d856a9c20fa0241106ee5a597c9").unwrap(),
            vout: 0,
        };
        let spender = chain.spenders(&first).unwrap().pop().expect("spent");
        assert!(chain.depth_all(&spender.compute_txid()).unwrap().unwrap() > 900_000);
        // Hal Finney's address: more than 25 mined transactions, read page
        // after page, every coin checked against its transaction. Far more
        // than any swap's address, so public explorers may ask for a rest
        // part way; then most must still have answered.
        let hal = Address::from_str("1Q2TWHE3GMdB6BZKafqwxXtWAWgFt5Jvm3").unwrap().assume_checked();
        let any = chain.coins_any(&hal).unwrap();
        assert!(!any.is_empty());
        match chain.coins_all(&hal) {
            Ok(all) => assert!(all.len() <= any.len()),
            Err(error) => println!("coins_all waited, as it should with explorers resting: {error:#}"),
        }
        println!("height {height}, fee {rate} sat/vB, Hal's address paid {} times", any.len());
    }
}
