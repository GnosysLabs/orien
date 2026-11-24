//! Announcements: the bot reads the website's changelog page, and what it
//! has not announced yet it posts in #announcements: not straight away, but
//! two hours after it first sees something new, so the changes made in
//! that time go out together in one post.
//!
//! What has been announced is remembered in one small file, so a restart
//! never repeats a post. The very first time, everything already on the
//! page counts as announced: the catch-up posts are made by hand, once,
//! with `orien-discord catch-up`.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use serenity::all::{ChannelId, ChannelType, CreateAllowedMentions, CreateMessage, MessageFlags};
use serenity::http::Http;

use crate::expert::Expert;

/// How often the page is read.
const EVERY: Duration = Duration::from_secs(600);

/// How long new entries wait, from when the first of them is seen, so the
/// ones that follow go out in the same post.
const GATHER: Duration = Duration::from_secs(2 * 3600);

/// More new entries than this at once is a changed page, not news: they
/// are noted and not posted, so the channel is never flooded.
const MOST_AT_ONCE: usize = 12;

/// One entry of the changelog, as the page shows it.
#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    /// As in the page's markup: 2026-11-16.
    pub date: String,
    /// As people read it: 16 November 2026.
    pub day: String,
    /// "Orien Wallet", "orien", "Validators", "Network", "Community" (older entries say "Market").
    pub products: Vec<String>,
    pub version: Option<String>,
    pub title: String,
    pub changes: Vec<String>,
}

impl Entry {
    /// What tells one entry from another.
    pub fn key(&self) -> String {
        format!("{}|{}|{}", self.date, self.version.as_deref().unwrap_or(""), self.title)
    }

    /// The entry as plain text, for the model.
    pub fn plain(&self) -> String {
        let mut text = format!("Date: {}\nAbout: {}\n", self.day, self.products.join(", "));
        if let Some(version) = &self.version {
            text.push_str(&format!("Version: {version}\n"));
        }
        text.push_str(&format!("Title: {}\nChanges:\n", self.title));
        for change in &self.changes {
            text.push_str(&format!("- {change}\n"));
        }
        text
    }

    /// The finished post: the entry's own title and date, then the body
    /// the model wrote, then where the full notes are. The emoji before
    /// the title is one the model picked to fit the entry, if it did.
    pub fn post(&self, emoji: Option<&str>, body: &str) -> String {
        let about = match &self.version {
            Some(version) => format!("{} {version}", self.products.join(" and ")),
            None => self.products.join(" and "),
        };
        let title = match emoji {
            Some(emoji) => format!("{emoji} {}", self.title),
            None => self.title.clone(),
        };
        format!("## {title}\n**{about}** · {}\n\n{}\n\nFull notes: <{CHANGELOG}>", self.day, body.trim())
    }
}

/// Splits what the model wrote into the emoji on its first line, if that
/// line is one fitting emoji and nothing else, and the body after it.
pub fn emoji_and_body(text: &str) -> (Option<&str>, &str) {
    let (first, rest) = text.trim().split_once('\n').unwrap_or(("", text.trim()));
    let first = first.trim();
    // Purses, wallets, coins and banknotes say nothing about a change.
    const NOT_THESE: [&str; 9] = ["👛", "👜", "💰", "💵", "💸", "🪙", "💳", "💲", "🤑"];
    let is_emoji = !first.is_empty()
        && first.chars().count() <= 4
        && first.chars().all(|c| !c.is_ascii() && !c.is_alphanumeric())
        && !NOT_THESE.iter().any(|not| first.contains(not));
    if is_emoji {
        (Some(first), rest.trim())
    } else if first.chars().count() <= 4 && !rest.is_empty() && first.chars().all(|c| !c.is_ascii()) {
        // An emoji we do not want: drop the line, keep the body.
        (None, rest.trim())
    } else {
        (None, text.trim())
    }
}

pub const CHANGELOG: &str = "https://orien.network/changelog/";

/// The text between `open` and `close`, after `from`, and where it ended.
fn between<'a>(text: &'a str, from: usize, open: &str, close: &str) -> Option<(&'a str, usize)> {
    let start = from + text[from..].find(open)? + open.len();
    let end = start + text[start..].find(close)?;
    Some((&text[start..end], end + close.len()))
}

/// A piece of markup as the words it shows.
fn words(markup: &str) -> String {
    let mut text = String::new();
    let mut inside = false;
    for c in markup.chars() {
        match c {
            '<' => inside = true,
            '>' => inside = false,
            _ if !inside => text.push(c),
            _ => {}
        }
    }
    for (entity, shown) in [("&#x27;", "'"), ("&#39;", "'"), ("&quot;", "\""), ("&lt;", "<"), ("&gt;", ">"), ("&nbsp;", " "), ("&amp;", "&")] {
        text = text.replace(entity, shown);
    }
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The page's entries, newest first, as the page lists them.
pub fn parse(page: &str) -> Vec<Entry> {
    let mut entries = Vec::new();
    for article in page.split("<article").skip(1) {
        let article = article.split("</article>").next().unwrap_or(article);
        let Some((date, _)) = between(article, 0, "dateTime=\"", "\"") else { continue };
        let Some((day, after_day)) = between(article, 0, "<time", "</time>") else { continue };
        let Some((title, after_title)) = between(article, 0, "<h2", "</h2>") else { continue };
        // Between the date and the title: one label for each product, then the version.
        let labels = &article[after_day..article.find("<h2").unwrap_or(after_day)];
        let mut products = Vec::new();
        let mut version = None;
        for span in labels.split("<span").skip(1) {
            let class = between(span, 0, "class=\"", "\"").map(|(class, _)| class).unwrap_or("");
            let Some((inner, _)) = between(span, 0, ">", "</span>") else { continue };
            let label = words(inner);
            if class.split(' ').any(|c| c == "num") {
                version = Some(label);
            } else if !label.is_empty() {
                products.push(label);
            }
        }
        let mut changes = Vec::new();
        let mut at = after_title;
        while let Some((item, next)) = between(article, at, "<li", "</li>") {
            changes.push(words(&format!("<li{item}")));
            at = next;
        }
        let entry = Entry {
            date: date.to_string(),
            day: words(&format!("<time{day}")),
            products,
            version,
            title: words(&format!("<h2{title}")),
            changes,
        };
        if !entry.title.is_empty() && !entry.changes.is_empty() {
            entries.push(entry);
        }
    }
    entries
}

pub async fn read_page(client: &reqwest::Client, url: &str) -> Result<Vec<Entry>> {
    let response = client.get(url).header("User-Agent", "orien-discord").send().await.context("cannot reach the changelog page")?;
    if !response.status().is_success() {
        bail!("the changelog page answered {}", response.status());
    }
    let entries = parse(&response.text().await?);
    if entries.is_empty() {
        bail!("the changelog page has no entries the bot can read; has its layout changed?");
    }
    Ok(entries)
}

/// The entries already announced, kept in a file.
struct Announced {
    path: PathBuf,
    keys: BTreeSet<String>,
}

impl Announced {
    /// None when nothing has ever been recorded.
    fn open(path: &Path) -> Result<Option<Announced>> {
        if !path.exists() {
            return Ok(None);
        }
        let keys = serde_json::from_str(&std::fs::read_to_string(path)?).with_context(|| format!("{} is damaged", path.display()))?;
        Ok(Some(Announced { path: path.to_owned(), keys }))
    }

    fn save(&self) -> Result<()> {
        let fresh = self.path.with_extension("new");
        std::fs::write(&fresh, serde_json::to_string_pretty(&self.keys)?)?;
        std::fs::rename(&fresh, &self.path)?;
        Ok(())
    }
}

/// The server's #announcements channel.
pub async fn announcements(http: &Http) -> Result<ChannelId> {
    let guilds = http.get_guilds(None, None).await?;
    let [guild] = guilds.as_slice() else {
        bail!("the bot is in {} servers; it should be in exactly one", guilds.len());
    };
    let channels = guild.id.channels(http).await?;
    channels
        .values()
        .find(|c| matches!(c.kind, ChannelType::Text | ChannelType::News) && crate::bot::plain(&c.name) == "announcements")
        .map(|c| c.id)
        .context("the server has no #announcements channel")
}

/// Posts one announcement. Nothing in it can ping anyone, and the link
/// shows no preview card.
pub async fn publish(http: &Http, channel: ChannelId, text: &str) -> Result<()> {
    let message = CreateMessage::new().content(text).allowed_mentions(CreateAllowedMentions::new()).flags(MessageFlags::SUPPRESS_EMBEDS);
    channel.send_message(http, message).await?;
    Ok(())
}

/// Reads the page now and announces what is new.
async fn look(http: &Http, expert: &Expert, client: &reqwest::Client, url: &str, file: &Path) -> Result<()> {
    let entries = read_page(client, url).await?;
    let Some(mut announced) = Announced::open(file)? else {
        // The first look: what is already there is old news.
        let announced = Announced { path: file.to_owned(), keys: entries.iter().map(Entry::key).collect() };
        announced.save()?;
        eprintln!("announcements: watching the changelog from here on; its {} entries so far count as announced", entries.len());
        return Ok(());
    };
    // Oldest first, so the channel reads in order.
    let new: Vec<&Entry> = entries.iter().rev().filter(|entry| !announced.keys.contains(&entry.key())).collect();
    // When the first of the waiting entries was seen: kept in a file beside
    // the announced ones, so a restart doesn't start the wait again.
    let waiting = file.with_extension("waiting");
    if new.is_empty() {
        let _ = std::fs::remove_file(&waiting);
        return Ok(());
    }
    if new.len() > MOST_AT_ONCE {
        eprintln!("announcements: {} new entries at once looks like a changed page, not news; none posted", new.len());
        announced.keys.extend(new.iter().map(|entry| entry.key()));
        let _ = std::fs::remove_file(&waiting);
        return announced.save();
    }
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)?.as_secs();
    let since = std::fs::read_to_string(&waiting).ok().and_then(|text| text.trim().parse::<u64>().ok());
    let Some(since) = since else {
        std::fs::write(&waiting, now.to_string())?;
        eprintln!("announcements: {} new, posting them together in {} minutes", new.len(), GATHER.as_secs() / 60);
        return Ok(());
    };
    if now < since + GATHER.as_secs() {
        return Ok(());
    }
    let channel = announcements(http).await?;
    let post = match new.as_slice() {
        [entry] => {
            let written = expert.announce(entry).await?;
            let (emoji, body) = emoji_and_body(&written);
            entry.post(emoji, body)
        }
        several => {
            let owned: Vec<Entry> = several.iter().map(|entry| (*entry).clone()).collect();
            let body = expert.roundup(&owned).await?;
            roundup_post(&owned, &body)
        }
    };
    publish(http, channel, &post).await.context("could not post in #announcements")?;
    announced.keys.extend(new.iter().map(|entry| entry.key()));
    announced.save()?;
    let _ = std::fs::remove_file(&waiting);
    eprintln!("announcements: posted {} entries together", new.len());
    Ok(())
}

/// One post for several entries, oldest first: a title, how many and
/// when, the body the model wrote, and where the full notes are.
pub fn roundup_post(entries: &[Entry], body: &str) -> String {
    let (first, last) = (&entries[0], &entries[entries.len() - 1]);
    let when = if first.day == last.day { last.day.clone() } else { format!("{} to {}", first.day, last.day) };
    format!("## ✨ What's new\n**{} changes** · {when}\n\n{}\n\nFull notes: <{CHANGELOG}>", entries.len(), body.trim())
}

/// Watches the changelog page for as long as the bot runs.
pub async fn watch(http: Arc<Http>, expert: Arc<Expert>, url: String, file: PathBuf) {
    let client = reqwest::Client::builder().timeout(Duration::from_secs(30)).build().expect("a plain web client");
    // Let the bot sign in first.
    tokio::time::sleep(Duration::from_secs(20)).await;
    let mut last_trouble = String::new();
    loop {
        match look(&http, &expert, &client, &url, &file).await {
            Ok(()) => last_trouble.clear(),
            Err(error) => {
                // Said once, not every ten minutes.
                let trouble = format!("{error:#}");
                if trouble != last_trouble {
                    eprintln!("announcements: {trouble}; trying again every {} minutes", EVERY.as_secs() / 60);
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

    const PAGE: &str = r#"<ol><li class="relative"><span class="absolute" aria-hidden="true"></span><article class="rounded-2xl border"><div class="mb-3 flex"><time dateTime="2026-11-16" class="text-sm text-muted">16 November 2026</time><span class="rounded-full bg-raised px-2.5 text-xs">Orien Wallet</span><span class="num ml-auto text-sm font-semibold text-pearl">1.8.10</span></div><h2 class="mb-3 text-xl">Smoother Market chats</h2><ul class="list-disc"><li>Chats open complete straight away, the listing they&#x27;re about shines at the top.</li><li>Report &amp; block are in a tidy ⋯ menu.</li></ul></article></li><li class="relative"><article class="rounded-2xl"><div><time dateTime="2026-11-15" class="text-sm">15 November 2026</time><span class="rounded-full">Network</span><span class="rounded-full">Validators</span></div><h2 class="mb-3">Mining moves to RandomX</h2><ul><li>From epoch 21, mining uses RandomX.</li></ul></article></li></ol>"#;

    #[test]
    fn several_entries_go_out_as_one_post() {
        let entries = parse(PAGE);
        let oldest_first: Vec<Entry> = entries.into_iter().rev().collect();
        let post = roundup_post(&oldest_first, "Two things changed.\n- **Mining** RandomX.");
        assert!(post.starts_with("## ✨ What's new\n**2 changes** · 15 November 2026 to 16 November 2026\n\nTwo things changed."), "{post}");
        assert!(post.ends_with("Full notes: <https://orien.network/changelog/>"));
    }

    #[test]
    fn the_page_is_read_entry_by_entry() {
        let entries = parse(PAGE);
        assert_eq!(entries.len(), 2);
        assert_eq!(
            entries[0],
            Entry {
                date: "2026-11-16".into(),
                day: "16 November 2026".into(),
                products: vec!["Orien Wallet".into()],
                version: Some("1.8.10".into()),
                title: "Smoother Market chats".into(),
                changes: vec![
                    "Chats open complete straight away, the listing they're about shines at the top.".into(),
                    "Report & block are in a tidy ⋯ menu.".into(),
                ],
            }
        );
        assert_eq!(entries[1].products, ["Network", "Validators"]);
        assert_eq!(entries[1].version, None);
        assert_eq!(entries[1].key(), "2026-11-15||Mining moves to RandomX");
    }

    #[test]
    fn a_post_carries_the_entry_s_own_title_and_the_link() {
        let (emoji, body) = emoji_and_body("💬\n\nChats in the Market got smoother.\n- One\n- Two\n");
        let post = parse(PAGE)[0].post(emoji, body);
        assert!(post.starts_with("## 💬 Smoother Market chats\n**Orien Wallet 1.8.10** · 16 November 2026\n\nChats in"));
        assert!(post.ends_with("Full notes: <https://orien.network/changelog/>"));
        // No emoji, or a purse: the title stands alone and the body is whole.
        assert_eq!(emoji_and_body("Chats got smoother.\n- One"), (None, "Chats got smoother.\n- One"));
        assert_eq!(emoji_and_body("👛\n\nChats got smoother."), (None, "Chats got smoother."));
        assert!(parse(PAGE)[0].post(None, "Body").starts_with("## Smoother Market chats\n"));
    }

    #[test]
    fn a_page_with_nothing_to_read_gives_nothing() {
        assert!(parse("<html><body>Not found</body></html>").is_empty());
    }
}
