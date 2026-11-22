//! Checkpoint summaries: after every hourly checkpoint the bot posts, in
//! #checkpoints, who mined, how the hour's ORN were shared, and how the
//! network is doing. Every figure comes from the explorer's public data;
//! nothing here is written by a model.
//!
//! To say how many wallets were paid for the first time, and whether an
//! hour set a record, it remembers every wallet ever paid in one small
//! file, which it fills once by reading every checkpoint so far.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use serenity::all::{ChannelId, ChannelType, CreateAllowedMentions, CreateMessage, MessageFlags};
use serenity::http::Http;

const API: &str = "https://explorer.orien.network/api/explorer";
const EXPLORER: &str = "https://explorer.orien.network";

/// How often the explorer is asked whether an hour has closed.
const EVERY: Duration = Duration::from_secs(45);
/// The pause between two questions to the explorer.
const PAUSE: Duration = Duration::from_millis(1600);

/// What is remembered between checkpoints.
#[derive(Default, Serialize, Deserialize)]
pub struct Memory {
    /// The last checkpoint taken in.
    last: Option<u64>,
    /// Every wallet ever paid for mining.
    seen: BTreeSet<String>,
    /// The most miners paid in one hour before `last`'s, and in `last`'s.
    record: usize,
    last_miners: usize,
}

async fn ask(client: &reqwest::Client, path: &str) -> Result<Value> {
    let response = client.get(format!("{API}{path}")).header("User-Agent", "orien-discord").send().await.context("cannot reach the explorer")?;
    if !response.status().is_success() {
        bail!("the explorer answered {} for {path}", response.status());
    }
    Ok(response.json().await?)
}

/// A whole number the explorer wrote as a number or as text.
fn whole(value: &Value) -> u128 {
    value.as_u64().map(u128::from).or_else(|| value.as_str().and_then(|text| text.parse().ok())).unwrap_or(0)
}

/// 1971593983140 billionths as "1,971.59".
fn orn(units: u128) -> String {
    let cents = (units + 5_000_000) / 10_000_000;
    format!("{}.{:02}", thousands(cents / 100), cents % 100)
}

fn thousands(n: u128) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    out
}

fn short(address: &str) -> String {
    if address.len() < 12 { address.to_string() } else { format!("{}…{}", &address[..6], &address[address.len() - 4..]) }
}

/// How many shares the hour's miners found: the hour's ORN divided by what
/// one share earned. The smallest miner may hold several shares, so the
/// share is the largest amount that goes into every payout evenly.
fn shares(amounts: &[u128]) -> Option<u128> {
    let smallest = *amounts.iter().min()? as f64;
    let total: u128 = amounts.iter().sum();
    (1..=60).map(|k| smallest / k as f64).find(|unit| amounts.iter().all(|a| ((*a as f64 / unit) - (*a as f64 / unit).round()).abs() < 0.02)).map(|unit| (total as f64 / unit).round() as u128)
}

/// One checkpoint's post.
fn render(checkpoint: &Value, overview: &Value, memory: &Memory, new_wallets: usize) -> String {
    let head = &checkpoint["checkpoint"];
    let epoch = head["epoch"].as_u64().unwrap_or(0);
    let miners = checkpoint["rewards"]["miners"].as_array().cloned().unwrap_or_default();
    let amounts: Vec<u128> = miners.iter().map(|m| whole(&m["amount"])).collect();
    let mined = whole(&checkpoint["rewards"]["mined"]);
    let count = miners.len();

    let validators = checkpoint["validators"].as_array().cloned().unwrap_or_default();
    let agreed = validators.iter().filter(|v| v["agreed"] == true).count();
    let agreement = if agreed == validators.len() { format!("all {agreed} validators agreed") } else { format!("{agreed} of {} validators agreed", validators.len()) };
    let round = match head["round"].as_u64().unwrap_or(0) {
        0 => "on the first try".to_string(),
        1 => "on the second try".to_string(),
        n => format!("after {} tries", n + 1),
    };

    let change = match count as i64 - memory.last_miners as i64 {
        _ if memory.last.is_none() => String::new(),
        0 => ", the same as the hour before".to_string(),
        n if n > 0 => format!(", {n} more than the hour before"),
        n => format!(", {} fewer than the hour before", -n),
    };
    let record = if memory.last.is_none() || count < memory.record {
        ""
    } else if count > memory.record {
        " 🏆 A new record."
    } else {
        " That ties the record."
    };

    let mut post = format!(
        "## ⛏️ Checkpoint {epoch}\nClosed <t:{}:f> · {agreement}, {round}\n\n**{count} miners** shared **{} ORN** this hour{change}.{record}\n",
        head["time"].as_u64().unwrap_or(0),
        orn(mined)
    );
    post.push_str(&format!("- **New miners:** {new_wallets} paid for the first time\n"));
    if let Some(shares) = shares(&amounts) {
        let next = match checkpoint["rewards"]["harder"].as_f64().unwrap_or(0.0) * 100.0 {
            p if p >= 0.05 => format!("so mining gets {p:.1}% harder next hour"),
            p if p <= -0.05 => format!("so mining gets {:.1}% easier next hour", -p),
            _ => "so mining stays as hard next hour".to_string(),
        };
        post.push_str(&format!("- **Work done:** {} shares against a target of 50,000, {next}\n", thousands(shares)));
    }
    if let Some(rate) = overview["hashrate"]["per_second"].as_f64() {
        let speed = if rate >= 1e6 { format!("{:.2} million", rate / 1e6) } else { thousands(rate.round() as u128) };
        post.push_str(&format!("- **Network speed:** about {speed} tries a second, averaged over the last {} hours\n", overview["hashrate"]["hours"].as_u64().unwrap_or(6)));
    }

    post.push_str("\n**Largest miners this hour**\n");
    for (i, miner) in miners.iter().take(3).enumerate() {
        let address = miner["address"].as_str().unwrap_or("");
        post.push_str(&format!(
            "{}. [`{}`](<{EXPLORER}/address/{address}/>) · {} ORN · {:.1}% of the hour\n",
            i + 1,
            short(address),
            orn(whole(&miner["amount"])),
            miner["part"].as_f64().unwrap_or(0.0) * 100.0
        ));
    }

    let unit = 1_000_000_000u128;
    let between = |low: u128, high: u128| amounts.iter().filter(|a| **a >= low * unit && **a < high * unit).count();
    post.push_str(&format!(
        "\n**How the hour was shared:** {} earned over 20 ORN · {} earned 5 to 20 · {} earned 1 to 5 · {} earned under 1\n",
        between(20, u128::MAX / unit),
        between(5, 20),
        between(1, 5),
        between(0, 1)
    ));
    post.push_str(&format!(
        "\n**The network:** {} wallets hold ORN · {} wallets mined in the last 24 hours · {} ORN mined so far, of 100 million\n",
        overview["supply"]["holding"].as_u64().unwrap_or(0),
        overview["miners"]["last_day"].as_u64().unwrap_or(0),
        thousands((whole(&overview["supply"]["released"]) + unit / 2) / unit)
    ));
    post.push_str(&format!("\nEvery payout: <{EXPLORER}/checkpoint/{epoch}/>"));
    post
}

fn all_agreed(checkpoint: &Value) -> bool {
    let validators = checkpoint["validators"].as_array().cloned().unwrap_or_default();
    !validators.is_empty() && validators.iter().all(|v| v["agreed"] == true)
}

/// Takes in every checkpoint from where the memory stops up to `upto`, and
/// gives the post for `upto`. None while the explorer has not worked out
/// `upto`'s rewards yet.
async fn take_in(client: &reqwest::Client, memory: &mut Memory, upto: u64, overview: &Value) -> Result<Option<String>> {
    let mut next = match memory.last {
        Some(last) => last + 1,
        // From the first hour that paid miners.
        None => ask(client, &format!("/checkpoints/{upto}")).await?["rewards"]["since"].as_u64().unwrap_or(upto),
    };
    let mut post = None;
    while next <= upto {
        tokio::time::sleep(PAUSE).await;
        let mut checkpoint = ask(client, &format!("/checkpoints/{next}")).await?;
        if checkpoint["rewards"]["known"] == false && next == upto {
            return Ok(None);
        }
        // The close needs only three votes, and a far-away validator stores
        // the checkpoint a moment later: give every validator up to about
        // three minutes to show it before the post counts who agreed.
        if next == upto {
            for _ in 0..12 {
                if all_agreed(&checkpoint) {
                    break;
                }
                tokio::time::sleep(Duration::from_secs(15)).await;
                checkpoint = ask(client, &format!("/checkpoints/{next}")).await?;
            }
        }
        let addresses: Vec<String> = checkpoint["rewards"]["miners"].as_array().into_iter().flatten().filter_map(|m| m["address"].as_str().map(str::to_string)).collect();
        let new_wallets = addresses.iter().filter(|a| !memory.seen.contains(*a)).count();
        if next == upto {
            post = Some(render(&checkpoint, overview, memory, new_wallets));
        }
        memory.record = memory.record.max(memory.last_miners);
        memory.last_miners = addresses.len();
        memory.seen.extend(addresses);
        memory.last = Some(next);
        next += 1;
    }
    Ok(post)
}

/// The post for the checkpoint that closed last, for reading before
/// anything is posted. Reads every checkpoint so far, which takes a few
/// minutes.
pub async fn preview() -> Result<String> {
    let client = reqwest::Client::builder().timeout(Duration::from_secs(30)).build()?;
    let overview = ask(&client, "/overview").await?;
    let closed = overview["epoch"].as_u64().context("the explorer names no epoch")?.saturating_sub(1);
    let mut memory = Memory::default();
    take_in(&client, &mut memory, closed, &overview).await?.context("the explorer has not worked out the last hour's rewards yet")
}

async fn channel(http: &Http) -> Result<ChannelId> {
    let guilds = http.get_guilds(None, None).await?;
    let [guild] = guilds.as_slice() else {
        bail!("the bot is in {} servers; it should be in exactly one", guilds.len());
    };
    let channels = guild.id.channels(http).await?;
    channels
        .values()
        .find(|c| matches!(c.kind, ChannelType::Text | ChannelType::News) && crate::bot::plain(&c.name) == "checkpoints")
        .map(|c| c.id)
        .context("the server has no #checkpoints channel")
}

fn save(file: &Path, memory: &Memory) -> Result<()> {
    let fresh = file.with_extension("new");
    std::fs::write(&fresh, serde_json::to_string(memory)?)?;
    std::fs::rename(&fresh, file)?;
    Ok(())
}

async fn look(http: &Http, client: &reqwest::Client, file: &Path, to: &mut Option<ChannelId>) -> Result<()> {
    let overview = ask(client, "/overview").await?;
    let closed = overview["epoch"].as_u64().context("the explorer names no epoch")?.saturating_sub(1);
    let first_time = !file.exists();
    let mut memory: Memory = if first_time { Memory::default() } else { serde_json::from_str(&std::fs::read_to_string(file)?).with_context(|| format!("{} is damaged", file.display()))? };
    if memory.last.is_some_and(|last| last >= closed) {
        return Ok(());
    }
    let Some(post) = take_in(client, &mut memory, closed, &overview).await? else { return Ok(()) };
    if first_time {
        eprintln!("checkpoints: read every checkpoint up to {closed}");
    }
    let channel = match to {
        Some(channel) => *channel,
        None => *to.insert(channel(http).await?),
    };
    let message = CreateMessage::new().content(post).allowed_mentions(CreateAllowedMentions::new()).flags(MessageFlags::SUPPRESS_EMBEDS);
    channel.send_message(http, message).await.with_context(|| format!("could not post checkpoint {closed}"))?;
    save(file, &memory)?;
    eprintln!("checkpoints: posted checkpoint {closed}");
    Ok(())
}

/// Watches for closed hours for as long as the bot runs.
pub async fn watch(http: Arc<Http>, file: PathBuf) {
    let client = reqwest::Client::builder().timeout(Duration::from_secs(30)).build().expect("a plain web client");
    tokio::time::sleep(Duration::from_secs(60)).await;
    let mut to = None;
    let mut last_trouble = String::new();
    loop {
        match look(&http, &client, &file, &mut to).await {
            Ok(()) => last_trouble.clear(),
            Err(error) => {
                let trouble = format!("{error:#}");
                if trouble != last_trouble {
                    eprintln!("checkpoints: {trouble}; trying again");
                    last_trouble = trouble;
                }
            }
        }
        tokio::time::sleep(EVERY).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn amounts_and_shares() {
        assert_eq!(orn(1_971_593_983_140), "1,971.59");
        assert_eq!(orn(796_440_333_071), "796.44");
        assert_eq!(orn(4_000_000), "0.00");
        assert_eq!(thousands(264633), "264,633");
        assert_eq!(short("2b6381f5ca71660bd93c19f8a5cbd2d675a7146546d43b18573734aca9a23ccd"), "2b6381…3ccd");
        // One share is 10: the smallest miner holds two.
        assert_eq!(shares(&[20, 30, 50, 1000]), Some(110));
        assert_eq!(shares(&[10, 30]), Some(4));
        assert_eq!(shares(&[]), None);
    }

    #[test]
    fn a_post_says_what_happened() {
        let checkpoint = json!({
            "checkpoint": { "epoch": 133, "round": 0, "time": 1795173233u64 },
            "validators": [{ "agreed": true }, { "agreed": true }, { "agreed": true }, { "agreed": true }],
            "rewards": { "known": true, "harder": -0.04386, "mined": "100000000000", "miners": [
                { "address": "2b6381f5ca71660bd93c19f8a5cbd2d675a7146546d43b18573734aca9a23ccd", "amount": "70000000000", "part": 0.7 },
                { "address": "fb7334aa71660bd93c19f8a5cbd2d675a7146546d43b18573734aca9a2beef", "amount": "29500000000", "part": 0.295 },
                { "address": "186164aa71660bd93c19f8a5cbd2d675a7146546d43b18573734aca9a2cafe", "amount": "500000000", "part": 0.005 },
            ] },
        });
        let overview = json!({ "supply": { "holding": 125, "released": "264633452868453" }, "miners": { "last_day": 107 }, "hashrate": { "per_second": 3631640.5, "hours": 6 } });
        let memory = Memory { last: Some(132), seen: BTreeSet::new(), record: 3, last_miners: 2 };
        let post = render(&checkpoint, &overview, &memory, 1);
        for part in [
            "## ⛏️ Checkpoint 133\nClosed <t:1795173233:f> · all 4 validators agreed, on the first try",
            "**3 miners** shared **100.00 ORN** this hour, 1 more than the hour before. That ties the record.",
            "- **New miners:** 1 paid for the first time",
            "- **Work done:** 200 shares against a target of 50,000, so mining gets 4.4% easier next hour",
            "about 3.63 million tries a second, averaged over the last 6 hours",
            "1. [`2b6381…3ccd`](<https://explorer.orien.network/address/2b6381f5ca71660bd93c19f8a5cbd2d675a7146546d43b18573734aca9a23ccd/>) · 70.00 ORN · 70.0% of the hour",
            "2 earned over 20 ORN · 0 earned 5 to 20 · 0 earned 1 to 5 · 1 earned under 1",
            "125 wallets hold ORN · 107 wallets mined in the last 24 hours · 264,633 ORN mined so far, of 100 million",
            "Every payout: <https://explorer.orien.network/checkpoint/133/>",
        ] {
            assert!(post.contains(part), "missing: {part}\n\n{post}");
        }
        assert!(post.chars().count() < 2000);
    }
}
