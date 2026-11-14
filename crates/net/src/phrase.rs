//! Recovery phrases: 24 ordinary words that hold a wallet's master secret,
//! written on paper so that a lost computer does not mean lost coins.
//!
//! The words come from the standard list of 2,048 English words most
//! cryptocurrency wallets use, in which no two words share their first four
//! letters. 24 words carry 264 bits: the 256-bit master secret and an 8-bit
//! check. The check is BLAKE3's, not Bitcoin's SHA-256, on purpose, so that
//! an Orien phrase is refused by Bitcoin wallets and the other way round.
//!
//! One phrase can also make further accounts, each with its own address, for
//! anyone who wants their coins kept apart. Account 1 is the wallet itself.

use anyhow::{Result, anyhow, bail};
use orien_crypto::{Tag, hash};

/// How many words a recovery phrase has.
pub const WORDS: usize = 24;

fn list() -> Vec<&'static str> {
    include_str!("words.txt").lines().collect()
}

/// The check that ends a phrase: 8 bits of BLAKE3 over the master secret.
fn check(master: &[u8; 32]) -> u8 {
    hash(Tag::Phrase, &[master]).0[0]
}

/// The recovery phrase for a master secret.
pub fn to_words(master: &[u8; 32]) -> Vec<&'static str> {
    let list = list();
    // 256 bits of secret, then 8 bits of check, read 11 bits at a time.
    let mut bits: Vec<bool> = master.iter().flat_map(|byte| (0..8).rev().map(move |i| byte >> i & 1 == 1)).collect();
    bits.extend((0..8).rev().map(|i| check(master) >> i & 1 == 1));
    bits.chunks(11).map(|chunk| list[chunk.iter().fold(0, |n, bit| n << 1 | *bit as usize)]).collect()
}

/// Reads a recovery phrase back into its master secret. Words may be typed
/// in any case, and by their first four letters alone.
pub fn from_words(text: &str) -> Result<[u8; 32]> {
    let list = list();
    let typed: Vec<String> = text.split_whitespace().map(str::to_lowercase).collect();
    if typed.len() != WORDS {
        bail!("a recovery phrase has {WORDS} words; this has {}", typed.len());
    }
    let mut bits = Vec::with_capacity(WORDS * 11);
    for (place, word) in typed.iter().enumerate() {
        let index = list
            .iter()
            .position(|known| known == word || (word.len() >= 4 && known.starts_with(word.as_str())))
            .ok_or_else(|| anyhow!("word {} of the phrase, \"{word}\", is not in the word list", place + 1))?;
        bits.extend((0..11).rev().map(|i| index >> i & 1 == 1));
    }
    let byte = |chunk: &[bool]| chunk.iter().fold(0u8, |n, bit| n << 1 | *bit as u8);
    let mut master = [0u8; 32];
    for (slot, chunk) in master.iter_mut().zip(bits.chunks(8)) {
        *slot = byte(chunk);
    }
    if byte(&bits[256..264]) != check(&master) {
        bail!("the phrase does not check out: a word is wrong, or two are swapped");
    }
    Ok(master)
}

/// The secret of account `number` made from a master secret. Accounts are
/// numbered from 1, and account 1 is the wallet itself.
pub fn account_seed(master: &[u8; 32], number: u32) -> [u8; 32] {
    hash(Tag::WalletAccount, &[master, &number.to_le_bytes()]).0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_list_is_the_standard_one() {
        let list = list();
        assert_eq!(list.len(), 2048);
        assert_eq!((list[0], list[2047]), ("abandon", "zoo"));
        let mut prefixes: Vec<&str> = list.iter().map(|w| &w[..w.len().min(4)]).collect();
        prefixes.sort_unstable();
        prefixes.dedup();
        assert_eq!(prefixes.len(), 2048, "no two words share their first four letters");
    }

    #[test]
    fn a_phrase_reads_back_into_its_secret() {
        for master in [[0u8; 32], [0xff; 32], *hash(Tag::Phrase, &[b"example"]).as_bytes()] {
            let words = to_words(&master);
            assert_eq!(words.len(), 24);
            assert_eq!(from_words(&words.join(" ")).unwrap(), master);
            // Upper case, extra spaces and four-letter prefixes are fine.
            let short: Vec<String> = words.iter().map(|w| w[..w.len().min(4)].to_uppercase()).collect();
            assert_eq!(from_words(&format!("  {}  ", short.join("   "))).unwrap(), master);
        }
    }

    #[test]
    fn a_mistake_in_the_phrase_is_caught() {
        let master = *hash(Tag::Phrase, &[b"mistakes"]).as_bytes();
        let words = to_words(&master);
        // One wrong word.
        let mut wrong = words.clone();
        wrong[5] = if wrong[5] == "zoo" { "abandon" } else { "zoo" };
        assert!(from_words(&wrong.join(" ")).is_err());
        // Two words swapped.
        let mut swapped = words.clone();
        swapped.swap(3, 4);
        if swapped != words {
            assert!(from_words(&swapped.join(" ")).is_err());
        }
        // A word that is not on the list, or one too few.
        let mut unknown = words.clone();
        unknown[0] = "qqqq";
        assert!(from_words(&unknown.join(" ")).unwrap_err().to_string().contains("word 1"));
        assert!(from_words(&words[1..].join(" ")).is_err());
    }

    #[test]
    fn each_account_has_its_own_secret() {
        let master = [7u8; 32];
        assert_ne!(account_seed(&master, 1), account_seed(&master, 2));
        assert_eq!(account_seed(&master, 1), account_seed(&master, 1));
        assert_ne!(account_seed(&master, 1), account_seed(&[8u8; 32], 1));
    }
}
