//! How many coins exist at a given moment.
//!
//! No coins exist at the start. The ones still to come are released
//! continuously, half of them every four years: after `t` seconds,
//! `CAP × (1 − 2^(−t / HALF_LIFE))` have been released. A checkpoint pays
//! out what was released between its time and the previous checkpoint's,
//! so closing an epoch early or late changes nothing about how many coins
//! exist.
//!
//! Every validator must reach exactly the same number, so this uses only
//! whole-number arithmetic: no floating point, whose rounding can differ
//! from one machine to another.

use crate::types::COIN;

/// The most coins there will ever be, in units: 100 million coins.
pub const CAP: u64 = 100_000_000 * COIN;

/// Four years of 365.25 days, in seconds. Half of what remains is released
/// in each such stretch.
pub const HALF_LIFE: u64 = 4 * 365 * 86_400 + 86_400;

/// 2^(−1 / HALF_LIFE), the share of what remains that is still unreleased
/// a second later, as a fraction of 2^64, rounded to nearest. Worked out to
/// 100 digits; the test below checks it against the half-life.
const PER_SECOND: u128 = 18_446_743_972_416_134_102;

/// `a × b` for two fractions of 2^64, rounded to nearest. Rounding to
/// nearest rather than down keeps the result within a ten-thousandth of a
/// coin of the exact curve over the first years.
fn times(a: u128, b: u128) -> u128 {
    (a * b + (1 << 63)) >> 64
}

/// 2^(−seconds / HALF_LIFE) as a fraction of 2^64, to within rounding: the share
/// of the cap not yet released. `None` stands for exactly 1, at second 0.
fn unreleased(seconds: u64) -> Option<u128> {
    // Whole half-lives halve it outright; the rest is PER_SECOND raised to
    // the leftover seconds, by repeated squaring.
    let halvings = seconds / HALF_LIFE;
    if halvings >= 64 {
        return Some(0);
    }
    let (mut left, mut factor, mut result) = (seconds % HALF_LIFE, PER_SECOND, None::<u128>);
    while left > 0 {
        if left & 1 == 1 {
            result = Some(result.map_or(factor, |r| times(r, factor)));
        }
        factor = times(factor, factor);
        left >>= 1;
    }
    match (result, halvings) {
        (None, 0) => None,
        (None, n) => Some(1u128 << (64 - n)),
        (Some(r), n) => Some(r >> n),
    }
}

/// How many units have been released `seconds` after the network started.
pub fn released(seconds: u64) -> u64 {
    match unreleased(seconds) {
        None => 0,
        Some(share) => CAP - ((CAP as u128 * share) >> 64) as u64,
    }
}

/// How many units were released between two moments, each in seconds since
/// the network started. Nothing for a stretch that runs backwards.
pub fn released_between(from: u64, to: u64) -> u64 {
    released(to).saturating_sub(released(from))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_constant_halves_in_one_half_life() {
        // One half-life of PER_SECOND, raised step by step the long way
        // round, is a half to within rounding.
        let mut left = HALF_LIFE;
        let (mut factor, mut result) = (PER_SECOND, 1u128 << 64);
        while left > 0 {
            if left & 1 == 1 {
                result = times(result, factor);
            }
            factor = times(factor, factor);
            left >>= 1;
        }
        let half = 1u128 << 63;
        // Within a hundred-billionth.
        assert!(result.abs_diff(half) < 1 << 27, "{result} against {half}");
    }
}
