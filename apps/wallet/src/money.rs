//! Amounts and fees. All arithmetic on money happens here, in whole units
//! (a billionth of a coin); the window only ever receives finished text.
//! JavaScript numbers are exact only up to about nine million coins'
//! worth of units, and the cap is a hundred million coins.

use orien_ledger::{Fees, Genesis, Kind, coins, parse_coins};
use serde::Serialize;

/// The fee rule of the network this app was built for.
pub fn fees(genesis: &str) -> Fees {
    serde_json::from_str::<Genesis>(genesis).map(|genesis| genesis.fee).unwrap_or(Fees::NONE)
}

/// An amount as people read it: "1,250.5".
pub fn show(units: i128) -> String {
    let text = coins(units);
    let (sign, text) = text.strip_prefix('-').map_or(("", text.as_str()), |rest| ("-", rest));
    let (whole, fraction) = text.split_once('.').map_or((text, None), |(whole, fraction)| (whole, Some(fraction)));
    let mut grouped = String::new();
    for (i, digit) in whole.chars().enumerate() {
        if i > 0 && (whole.len() - i) % 3 == 0 {
            grouped.push(',');
        }
        grouped.push(digit);
    }
    match fraction {
        Some(fraction) => format!("{sign}{grouped}.{fraction}"),
        None => format!("{sign}{grouped}"),
    }
}

/// Reads what someone typed: "1,250.5" or "0.25".
pub fn read(text: &str) -> Result<u64, String> {
    let clean: String = text.chars().filter(|c| !matches!(c, ',' | ' ')).collect();
    if clean.is_empty() {
        return Err("Enter an amount.".into());
    }
    let units = parse_coins(&clean).map_err(|_| "Enter an amount, such as 12 or 0.25.".to_string())?;
    if units == 0 {
        return Err("Enter an amount above zero.".into());
    }
    Ok(units)
}

/// The most that can be sent from `balance` once the fee is paid on top.
pub fn most(fees: &Fees, balance: i128) -> u64 {
    let balance = balance.clamp(0, u64::MAX as i128) as u64;
    let (mut low, mut high) = (0u64, balance);
    while low < high {
        let middle = low + (high - low).div_ceil(2);
        if middle.saturating_add(fees.on(&Kind::Payment, middle)) <= balance {
            low = middle;
        } else {
            high = middle - 1;
        }
    }
    low
}

/// What the Send screen shows while someone types an amount.
#[derive(Serialize)]
pub struct Quote {
    pub amount: Option<String>,
    pub fee: Option<String>,
    pub total: Option<String>,
    pub after: Option<String>,
    /// Why this amount cannot be sent, if it cannot.
    pub problem: Option<String>,
}

pub fn quote(fees: &Fees, balance: Option<i128>, text: &str) -> Quote {
    let amount = match read(text) {
        Ok(amount) => amount,
        Err(problem) => {
            let problem = (!text.trim().is_empty()).then_some(problem);
            return Quote { amount: None, fee: None, total: None, after: None, problem };
        }
    };
    let fee = fees.on(&Kind::Payment, amount);
    let total = amount as i128 + fee as i128;
    let problem = match balance {
        Some(balance) if total > balance => Some(format!(
            "That is more than this wallet holds once the fee of {} is added. The most you can send is {}.",
            show(fee as i128),
            show(most(fees, balance) as i128)
        )),
        _ => None,
    };
    Quote {
        amount: Some(show(amount as i128)),
        fee: Some(show(fee as i128)),
        total: Some(show(total)),
        after: balance.map(|balance| show(balance - total)),
        problem,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shows_and_reads_amounts() {
        assert_eq!(show(1_250_500_000_000), "1,250.5");
        assert_eq!(show(250_000_000), "0.25");
        assert_eq!(show(1_000_000_000_000_000), "1,000,000");
        assert_eq!(show(-5_000_000_000), "-5");
        assert_eq!(read("1,250.5").unwrap(), 1_250_500_000_000);
        assert_eq!(read(" 0.25 ").unwrap(), 250_000_000);
        assert!(read("0").is_err());
        assert!(read("abc").is_err());
        assert!(read("0.0000000001").is_err(), "more than nine decimals");
    }

    #[test]
    fn the_most_leaves_exactly_room_for_the_fee() {
        let fees = Fees::default();
        for balance in [0i128, 999, 1_000, 5_000_000, 1_234_567_890_123, 50_000_000_000_000] {
            let most = most(&fees, balance);
            let fee = fees.on(&Kind::Payment, most);
            // Nothing can be sent when the balance does not cover the smallest fee.
            assert!(most == 0 || most as i128 + fee as i128 <= balance, "{balance}");
            let one_more = most + 1;
            assert!(one_more as i128 + fees.on(&Kind::Payment, one_more) as i128 > balance, "{balance}");
        }
    }

    #[test]
    fn a_quote_adds_the_fee() {
        let fees = Fees::default(); // 0.1%, at least 0.000001, at most 1
        let quote = quote(&fees, Some(10 * 1_000_000_000), "5");
        assert_eq!(quote.fee.as_deref(), Some("0.005"));
        assert_eq!(quote.after.as_deref(), Some("4.995"));
        assert!(quote.problem.is_none());
        assert!(super::quote(&fees, Some(10 * 1_000_000_000), "10").problem.is_some());
    }
}
