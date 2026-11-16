//! Bitcoin's and SOL's prices in dollars, for showing what swap offers are worth.
//! Fetched by the server at most every few minutes, so visitors' browsers
//! never ask anyone else, and kept if a fetch fails.

use anyhow::{Context, Result};
use std::sync::Mutex;
use std::time::{Duration, Instant};

const SOURCE: &str = "https://mempool.space/api/v1/prices";
const FRESH: Duration = Duration::from_secs(300);

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Price {
    pub usd: f64,
    /// When it was fetched, in seconds since 1970.
    pub at: u64,
}

pub struct Prices {
    last: Mutex<Option<(Price, Instant)>>,
    last_sol: Mutex<Option<(Price, Instant)>>,
    /// For tests: a fixed price, never fetched.
    fixed: Option<f64>,
}

impl Prices {
    pub fn live() -> Prices {
        Prices { last: Mutex::new(None), last_sol: Mutex::new(None), fixed: None }
    }

    #[doc(hidden)]
    pub fn fixed_for_tests(usd: f64) -> Prices {
        Prices { last: Mutex::new(None), last_sol: Mutex::new(None), fixed: Some(usd) }
    }

    /// The latest price: fetched again when the one kept is over five
    /// minutes old, and the one kept if fetching fails.
    pub fn bitcoin(&self) -> Option<Price> {
        if let Some(usd) = self.fixed {
            return Some(Price { usd, at: crate::store::now() });
        }
        let kept = *self.last.lock().unwrap();
        if let Some((price, fetched)) = kept
            && fetched.elapsed() < FRESH
        {
            return Some(price);
        }
        match fetch() {
            Ok(usd) => {
                let price = Price { usd, at: crate::store::now() };
                *self.last.lock().unwrap() = Some((price, Instant::now()));
                Some(price)
            }
            Err(error) => {
                eprintln!("cannot fetch Bitcoin's price: {error:#}");
                kept.map(|(price, _)| price)
            }
        }
    }
}

impl Prices {
    /// SOL's latest price, kept and refreshed the same way.
    pub fn solana(&self) -> Option<Price> {
        if self.fixed.is_some() {
            return Some(Price { usd: 70.0, at: crate::store::now() });
        }
        let kept = *self.last_sol.lock().unwrap();
        if let Some((price, fetched)) = kept
            && fetched.elapsed() < FRESH
        {
            return Some(price);
        }
        match fetch_sol() {
            Ok(usd) => {
                let price = Price { usd, at: crate::store::now() };
                *self.last_sol.lock().unwrap() = Some((price, Instant::now()));
                Some(price)
            }
            Err(error) => {
                eprintln!("cannot fetch SOL's price: {error:#}");
                kept.map(|(price, _)| price)
            }
        }
    }
}

/// SOL's price from Coinbase's public spot price.
fn fetch_sol() -> Result<f64> {
    let answer: serde_json::Value = ureq::get("https://api.coinbase.com/v2/prices/SOL-USD/spot")
        .config()
        .timeout_global(Some(Duration::from_secs(10)))
        .build()
        .call()
        .context("cannot reach Coinbase")?
        .into_body()
        .read_json()
        .context("Coinbase's answer is not JSON")?;
    let usd: f64 = answer["data"]["amount"].as_str().context("no SOL price")?.parse()?;
    anyhow::ensure!(usd > 0.0, "a price of {usd}");
    Ok(usd)
}

fn fetch() -> Result<f64> {
    let answer: serde_json::Value = ureq::get(SOURCE)
        .config()
        .timeout_global(Some(Duration::from_secs(10)))
        .build()
        .call()
        .context("cannot reach mempool.space")?
        .into_body()
        .read_json()
        .context("mempool.space's answer is not JSON")?;
    let usd = answer["USD"].as_f64().context("no USD price")?;
    anyhow::ensure!(usd > 0.0, "a price of {usd}");
    Ok(usd)
}
