//! Spotting a recovery phrase posted in the open, and telling a question
//! from chatter, without asking the model.

use std::collections::HashSet;
use std::sync::OnceLock;

/// The wallet's own list of recovery-phrase words.
fn words() -> &'static HashSet<&'static str> {
    static WORDS: OnceLock<HashSet<&'static str>> = OnceLock::new();
    WORDS.get_or_init(|| include_str!("../../crates/net/src/words.txt").lines().map(str::trim).filter(|w| !w.is_empty()).collect())
}

/// Whether a message holds what looks like a recovery phrase, or a good
/// part of one: twelve or more of the list's words in a row. Ordinary
/// sentences never manage that, since "the", "a", "is" and "to" are not on
/// the list.
pub fn holds_recovery_phrase(text: &str) -> bool {
    let list = words();
    let mut run = 0;
    for token in text.split(|c: char| c.is_whitespace() || c == ',' || c == ';') {
        // "1." and "12)" in a numbered phrase are not words.
        let word: String = token.chars().filter(|c| c.is_alphabetic()).flat_map(char::to_lowercase).collect();
        if word.is_empty() {
            continue;
        }
        if list.contains(word.as_str()) {
            run += 1;
            if run >= 12 {
                return true;
            }
        } else {
            run = 0;
        }
    }
    false
}

/// Whether a message reads like a question, so worth showing to the model
/// when nobody addressed the bot.
pub fn looks_like_question(text: &str) -> bool {
    let text = text.trim();
    if text.chars().count() < 8 {
        return false;
    }
    if text.contains('?') || text.contains('？') {
        return true;
    }
    const OPENERS: [&str; 22] = [
        "how", "what", "why", "where", "when", "which", "who", "can", "could", "does", "do", "is", "are", "will", "should",
        "anyone", "help", "i can't", "i cant", "i cannot", "it won't", "it wont",
    ];
    let lower = text.to_lowercase();
    OPENERS.iter().any(|opener| lower.strip_prefix(opener).is_some_and(|rest| rest.starts_with(' ')))
}

/// Whether a message carries a link to anywhere but Orien's own sites. A
/// masked link like [MINT HERE](https://...) still holds its address, and
/// Discord turns bare "www." and invite addresses into links too.
pub fn has_outside_link(text: &str) -> bool {
    let lower = text.to_lowercase();
    if lower.contains("discord.gg/") || lower.contains("discord.com/invite") {
        return true;
    }
    let ours = |host: &str| host == "orien.network" || host.ends_with(".orien.network");
    let mut rest = lower.as_str();
    while let Some(at) = ["http://", "https://", "www."].iter().filter_map(|start| rest.find(start).map(|i| (i, start.len()))).min() {
        let after = &rest[at.0 + at.1..];
        let host: String = after.chars().take_while(|c| c.is_alphanumeric() || *c == '.' || *c == '-').collect();
        let host = host.trim_start_matches("www.").trim_end_matches('.');
        if !ours(host) {
            return true;
        }
        rest = &after[host.len().min(after.len())..];
    }
    false
}

/// Whether a name dresses up as an announcement or as staff, like
/// "🎉|community giveaway" or "Support Team": the bot gives such a name no
/// public welcome, so its hello cannot make a scam look vouched for.
pub fn announcement_like_name(names: &[&str]) -> bool {
    const WORDS: [&str; 22] = [
        "giveaway", "giveaways", "airdrop", "airdrops", "announcement", "announcements", "mint", "minting", "claim", "reward",
        "rewards", "support", "helpdesk", "admin", "admins", "moderator", "mod", "mods", "official", "staff", "ticket", "verify",
    ];
    names.iter().any(|name| {
        let lower = name.to_lowercase();
        let words: Vec<&str> = lower.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).collect();
        words.iter().any(|w| WORDS.contains(w)) || lower.contains("community giveaway") || words.windows(2).any(|p| p == ["orien", "team"] || p == ["the", "team"])
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_phrase_is_spotted_however_it_is_laid_out() {
        let list: Vec<&str> = include_str!("../../crates/net/src/words.txt").lines().collect();
        let phrase: Vec<&str> = list.iter().step_by(85).take(24).copied().collect();
        assert!(holds_recovery_phrase(&phrase.join(" ")));
        assert!(holds_recovery_phrase(&format!("my wallet is broken here it is: {}", phrase.join(", "))));
        let numbered: Vec<String> = phrase.iter().enumerate().map(|(i, w)| format!("{}. {}", i + 1, w.to_uppercase())).collect();
        assert!(holds_recovery_phrase(&numbered.join("\n")));
        // Half a phrase is still worth removing.
        assert!(holds_recovery_phrase(&phrase[..12].join(" ")));
    }

    #[test]
    fn ordinary_talk_is_left_alone() {
        for text in [
            "how do I start mining on my laptop, the button does nothing and I am stuck",
            "I wrote my recovery phrase on paper and put it in a drawer, is that enough?",
            "abandon ability able about above absent absorb abstract absurd abuse",
            "",
        ] {
            assert!(!holds_recovery_phrase(text), "{text}");
        }
    }

    #[test]
    fn questions_and_chatter() {
        assert!(looks_like_question("how do i mine"));
        assert!(looks_like_question("mining shows 0 ORN after an hour?"));
        assert!(looks_like_question("Help my wallet says no connection"));
        assert!(!looks_like_question("gm"));
        assert!(!looks_like_question("nice, got my first coins today"));
        assert!(!looks_like_question("however you like"));
    }

    #[test]
    fn outside_links_and_dressed_up_names() {
        assert!(has_outside_link("🎉 free Mint with OpenSea [MINT HERE](https://opensea-mint.xyz/claim)"));
        assert!(has_outside_link("go to www.free-orn.com now"));
        assert!(has_outside_link("join discord.gg/abc"));
        assert!(has_outside_link("see https://orien.network.evil.com/x"));
        assert!(!has_outside_link("my wallet: https://explorer.orien.network/address/abc/ and https://orien.network/try/"));
        assert!(!has_outside_link("how do I start mining?"));
        assert!(announcement_like_name(&["🎉|community giveaway"]));
        assert!(announcement_like_name(&["Orien Support"]));
        assert!(announcement_like_name(&["Luna", "airdrop_bot"]));
        assert!(!announcement_like_name(&["Luna", "minty_fresh", "Steamboat Willie"]));
    }
}
