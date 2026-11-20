//! New videos: the bot reads the feed of Orien's YouTube channel, and
//! posts a link to each video it has not posted yet. Discord shows the
//! video's own preview, playable in place.
//!
//! They go in #videos (or #youtube) if the server has such a channel, and
//! in #announcements otherwise. What has been posted is remembered in one
//! small file. Videos already on the channel the first time the bot looks
//! count as posted.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use serenity::all::{ChannelId, ChannelType, CreateAllowedMentions, CreateMessage};
use serenity::http::Http;

/// Orien Network's channel, youtube.com/@OrienNetwork.
pub const FEED: &str = "https://www.youtube.com/feeds/videos.xml?channel_id=UCyzIILxPmYH94f15MBIXHog";

const EVERY: Duration = Duration::from_secs(600);

/// More new videos than this at once is a changed feed, not news.
const MOST_AT_ONCE: usize = 3;

#[derive(Clone, Debug, PartialEq)]
pub struct Video {
    pub id: String,
    pub title: String,
}

fn between<'a>(text: &'a str, open: &str, close: &str) -> Option<&'a str> {
    let start = text.find(open)? + open.len();
    let end = start + text[start..].find(close)?;
    Some(&text[start..end])
}

/// The feed's videos, newest first, as the feed lists them. None when the
/// text is not the channel's feed at all.
pub fn parse(feed: &str) -> Option<Vec<Video>> {
    if !feed.contains("<feed") || !feed.contains("yt:channelId") {
        return None;
    }
    let mut videos = Vec::new();
    for entry in feed.split("<entry>").skip(1) {
        let (Some(id), Some(title)) = (between(entry, "<yt:videoId>", "</yt:videoId>"), between(entry, "<title>", "</title>")) else { continue };
        // Only what a video's address can hold.
        if id.is_empty() || id.len() > 20 || !id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
            continue;
        }
        let mut title = title.to_string();
        for (entity, shown) in [("&quot;", "\""), ("&apos;", "'"), ("&#39;", "'"), ("&lt;", "<"), ("&gt;", ">"), ("&amp;", "&")] {
            title = title.replace(entity, shown);
        }
        videos.push(Video { id: id.to_string(), title: title.split_whitespace().collect::<Vec<_>>().join(" ") });
    }
    Some(videos)
}

/// The post for one video.
pub fn post(video: &Video) -> String {
    // The title is plain words: nothing in it may turn into a heading, a link or a mention.
    let title: String = video.title.chars().filter(|c| !matches!(c, '@' | '#' | '<' | '>' | '`' | '*' | '_' | '[' | ']')).take(120).collect();
    format!("📺 **New video: {}**\nhttps://www.youtube.com/watch?v={}", title.trim(), video.id)
}

/// Where videos are posted.
async fn channel(http: &Http) -> Result<ChannelId> {
    let guilds = http.get_guilds(None, None).await?;
    let [guild] = guilds.as_slice() else {
        bail!("the bot is in {} servers; it should be in exactly one", guilds.len());
    };
    let channels = guild.id.channels(http).await?;
    let find = |name: &str| {
        channels.values().find(|c| matches!(c.kind, ChannelType::Text | ChannelType::News) && crate::bot::plain(&c.name) == name).map(|c| c.id)
    };
    ["videos", "youtube", "announcements"].into_iter().find_map(find).context("the server has no channel for videos")
}

async fn look(http: &Http, client: &reqwest::Client, file: &Path) -> Result<()> {
    let response = client.get(FEED).header("User-Agent", "orien-discord").send().await.context("cannot reach YouTube")?;
    if !response.status().is_success() {
        bail!("YouTube's feed answered {}", response.status());
    }
    let videos = parse(&response.text().await?).context("YouTube's answer is not the channel's feed")?;
    if !file.exists() {
        let seen: BTreeSet<&str> = videos.iter().map(|v| v.id.as_str()).collect();
        std::fs::write(file, serde_json::to_string_pretty(&seen)?)?;
        eprintln!("videos: watching the YouTube channel from here on; its {} videos so far count as posted", videos.len());
        return Ok(());
    }
    let mut seen: BTreeSet<String> = serde_json::from_str(&std::fs::read_to_string(file)?).with_context(|| format!("{} is damaged", file.display()))?;
    let save = |seen: &BTreeSet<String>| -> Result<()> {
        let fresh = file.with_extension("new");
        std::fs::write(&fresh, serde_json::to_string_pretty(seen)?)?;
        std::fs::rename(&fresh, file)?;
        Ok(())
    };
    // Oldest first, so the channel reads in order.
    let new: Vec<&Video> = videos.iter().rev().filter(|v| !seen.contains(&v.id)).collect();
    if new.is_empty() {
        return Ok(());
    }
    if new.len() > MOST_AT_ONCE {
        eprintln!("videos: {} new videos at once looks like a changed feed, not news; none posted", new.len());
        seen.extend(new.iter().map(|v| v.id.clone()));
        return save(&seen);
    }
    let channel = channel(http).await?;
    for video in new {
        // The link keeps its preview: that is the video.
        let message = CreateMessage::new().content(post(video)).allowed_mentions(CreateAllowedMentions::new());
        channel.send_message(http, message).await.with_context(|| format!("could not post the video \"{}\"", video.title))?;
        seen.insert(video.id.clone());
        save(&seen)?;
        eprintln!("videos: posted \"{}\"", video.title);
    }
    Ok(())
}

/// Watches the channel's feed for as long as the bot runs.
pub async fn watch(http: Arc<Http>, file: PathBuf) {
    let client = reqwest::Client::builder().timeout(Duration::from_secs(30)).build().expect("a plain web client");
    tokio::time::sleep(Duration::from_secs(40)).await;
    let mut last_trouble = String::new();
    loop {
        match look(&http, &client, &file).await {
            Ok(()) => last_trouble.clear(),
            Err(error) => {
                let trouble = format!("{error:#}");
                if trouble != last_trouble {
                    eprintln!("videos: {trouble}; trying again every {} minutes", EVERY.as_secs() / 60);
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

    const EMPTY: &str = r#"<?xml version="1.0"?><feed xmlns:yt="http://www.youtube.com/xml/schemas/2015"><yt:channelId>yzIILxPmYH94f15MBIXHog</yt:channelId><title>Orien Network</title></feed>"#;

    #[test]
    fn the_feed_is_read_video_by_video() {
        assert_eq!(parse(EMPTY), Some(vec![]));
        assert_eq!(parse("<html>Sorry, something went wrong</html>"), None);
        let feed = EMPTY.replace(
            "</feed>",
            "<entry><id>yt:video:abc-DEF_123</id><yt:videoId>abc-DEF_123</yt:videoId><title>What is Orien? Mining &amp; more</title></entry>\
             <entry><yt:videoId>bad id&quot;</yt:videoId><title>x</title></entry></feed>",
        );
        let videos = parse(&feed).unwrap();
        assert_eq!(videos, [Video { id: "abc-DEF_123".into(), title: "What is Orien? Mining & more".into() }]);
        assert_eq!(post(&videos[0]), "📺 **New video: What is Orien? Mining & more**\nhttps://www.youtube.com/watch?v=abc-DEF_123");
        // A title cannot ping anyone or break the line's bold.
        let tricky = Video { id: "a".into(), title: "@everyone **free** <#1> ORN".into() };
        assert_eq!(post(&tricky), "📺 **New video: everyone free 1 ORN**\nhttps://www.youtube.com/watch?v=a");
    }
}
