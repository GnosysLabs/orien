//! Keeping what the bot knows current: it reads the website's own pages
//! and the changelog again every half hour, so a published change reaches
//! its answers without anyone rebuilding or restarting it.
//!
//! Only the live website is read, so the bot learns of a feature when the
//! website tells the public about it, and not before.
//!
//! The knowledge file shipped with the bot stays as it is for two things:
//! the parts that are not on the website (the design notes and the orien
//! program's help, after the line below), and as a whole when the website
//! cannot be read at start.

use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result, bail};

use crate::expert::Expert;
use crate::news;

/// In the shipped knowledge file, what follows this line is not on the
/// website. build-knowledge.py writes it.
pub const NOT_ON_THE_WEBSITE: &str = "<!-- not on the website -->";

const EVERY: Duration = Duration::from_secs(1800);

/// The pages read, with the heading each gets. The changelog is read
/// entry by entry instead, with the announcements' own reader.
const PAGES: [(&str, &str); 6] = [
    ("Home page", "https://orien.network/"),
    ("Get started (download the wallet, mine, send, run a validator)", "https://orien.network/try/"),
    ("How it works", "https://orien.network/how-it-works/"),
    ("News: mining with RandomX", "https://orien.network/news/randomx/"),
    ("Contact", "https://orien.network/contact/"),
    ("The Orien paper", "https://orien.network/paper/"),
];

/// Removes every `<tag ...>...</tag>` block of one kind.
fn without(markup: &str, tag: &str) -> String {
    let (open, close) = (format!("<{tag}"), format!("</{tag}>"));
    let mut kept = String::new();
    let mut rest = markup;
    while let Some(start) = rest.find(&open) {
        kept.push_str(&rest[..start]);
        match rest[start..].find(&close) {
            Some(end) => rest = &rest[start + end + close.len()..],
            None => return kept,
        }
    }
    kept.push_str(rest);
    kept
}

/// A page's words, from the markup of its main part: headings, paragraphs
/// and list items each on their own line.
pub fn page_text(page: &str) -> Option<String> {
    let start = page.find("<main")?;
    let end = page.rfind("</main>")?;
    let mut main = page.get(start..end)?.to_string();
    for tag in ["script", "style", "svg", "noscript", "button"] {
        main = without(&main, tag);
    }
    let mut text = String::new();
    let mut rest = main.as_str();
    while let Some(open) = rest.find('<') {
        text.push_str(&rest[..open]);
        let Some(close) = rest[open..].find('>') else { break };
        let tag = &rest[open + 1..open + close];
        let name: String = tag.trim_start_matches('/').chars().take_while(|c| c.is_ascii_alphanumeric()).collect();
        let closing = tag.starts_with('/');
        match (name.as_str(), closing) {
            ("h1" | "h2" | "h3" | "h4", false) => text.push_str("\n\n### "),
            ("li", false) => text.push_str("\n- "),
            ("td" | "th", true) => text.push_str(" | "),
            ("p" | "h1" | "h2" | "h3" | "h4" | "li" | "tr" | "section" | "header" | "div" | "ul" | "ol" | "table" | "pre", true) | ("br", _) => {
                text.push('\n')
            }
            _ => {}
        }
        rest = &rest[open + close + 1..];
    }
    text.push_str(rest);
    for (entity, shown) in [("&#x27;", "'"), ("&#39;", "'"), ("&quot;", "\""), ("&lt;", "<"), ("&gt;", ">"), ("&nbsp;", " "), ("&amp;", "&")] {
        text = text.replace(entity, shown);
    }
    let lines: Vec<String> = text.lines().map(|line| line.split_whitespace().collect::<Vec<_>>().join(" ")).collect();
    let mut tidy = String::new();
    for line in lines {
        if line.is_empty() || line == "-" || line == "###" {
            continue;
        }
        // A blank line before each heading, and nowhere else.
        if line.starts_with("### ") && !tidy.is_empty() {
            tidy.push('\n');
        }
        tidy.push_str(&line);
        tidy.push('\n');
    }
    Some(tidy.trim().to_string())
}

/// Everything the bot should know now: the live website and changelog,
/// then the shipped parts that are not on the website.
pub async fn gather(client: &reqwest::Client, shipped: &str) -> Result<String> {
    let mut sections = Vec::new();
    for (heading, url) in PAGES {
        let response = client.get(url).header("User-Agent", "orien-discord").send().await.with_context(|| format!("cannot reach {url}"))?;
        if !response.status().is_success() {
            bail!("{url} answered {}", response.status());
        }
        let text = page_text(&response.text().await?).with_context(|| format!("{url} has no main part to read"))?;
        if text.chars().count() < 200 {
            bail!("{url} came back nearly empty");
        }
        sections.push(format!("# {heading}\nSource: {url}\n\n{text}"));
    }
    let entries = news::read_page(client, news::CHANGELOG).await?;
    let changelog: Vec<String> = entries.iter().map(news::Entry::plain).collect();
    sections.push(format!("# Changelog, newest first\nSource: {}\n\n{}", news::CHANGELOG, changelog.join("\n")));
    if let Some((_, rest)) = shipped.split_once(NOT_ON_THE_WEBSITE) {
        sections.push(rest.trim().to_string());
    }
    Ok(sections.join("\n\n\n"))
}

/// Reads the website again and again for as long as the bot runs, and
/// hands the expert what it found whenever that has changed.
pub async fn keep_current(expert: Arc<Expert>, shipped: String) {
    let client = reqwest::Client::builder().timeout(Duration::from_secs(30)).build().expect("a plain web client");
    let mut known = String::new();
    let mut last_trouble = String::new();
    loop {
        match gather(&client, &shipped).await {
            // A website that suddenly says far less is being published or is broken: wait.
            Ok(fresh) if !known.is_empty() && fresh.len() < known.len() / 2 => {
                eprintln!("knowledge: the website came back less than half its size; keeping what the bot knows");
            }
            Ok(fresh) => {
                last_trouble.clear();
                if fresh != known {
                    expert.learn(&fresh);
                    eprintln!("knowledge: read from the live website, {} KB", fresh.len() / 1024);
                    known = fresh;
                }
            }
            Err(error) => {
                let trouble = format!("{error:#}");
                if trouble != last_trouble {
                    eprintln!("knowledge: {trouble}; keeping what the bot knows and trying again every {} minutes", EVERY.as_secs() / 60);
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

    #[test]
    fn a_page_s_words_are_kept_and_its_code_dropped() {
        let page = r#"<html><head><title>x</title></head><body><nav><a href="/">Home</a></nav><main class="m"><header><p class="k">Get started</p><h1 class="t">From nothing to your first ORN</h1></header><section><h2>Mine</h2><p>Press <strong>Mine</strong>, then Start.&nbsp;It&#x27;s easy.</p><ul><li>About 2 GB</li><li><svg><path d="M0"/></svg>Any computer</li></ul><button>Copy</button><script>self.__next_f.push([1,"junk"])</script></section></main><footer>Footer</footer></body></html>"#;
        let text = page_text(page).unwrap();
        assert_eq!(text, "Get started\n\n### From nothing to your first ORN\n\n### Mine\nPress Mine, then Start. It's easy.\n- About 2 GB\n- Any computer");
        assert!(page_text("<html><body>Not found</body></html>").is_none());
    }
}
