//! Link previews on community posts, as Discord shows them: the first link
//! in a post (YouTube aside, which plays in place) is fetched here, once per
//! address, and its title, description, site and picture kept.
//!
//! Fetching other people's addresses from the server needs care. Every
//! connection, redirects included, goes through a resolver that only lets
//! public internet addresses through, so a link can't reach this server,
//! the tailnet or anything private. Only HTML pages and pictures are read,
//! to a size cap, within a few seconds. The words are screened like a
//! post, and the picture like an uploaded photo; a preview that fails
//! either check is not shown, and an unchecked picture never is.

use crate::screen::{Screener, Verdict};
use anyhow::{Context, Result, bail};
use std::io::Read;
use std::net::{IpAddr, Ipv4Addr};
use std::time::Duration;
use ureq::unversioned::resolver::{DefaultResolver, ResolvedSocketAddrs, Resolver};
use ureq::unversioned::transport::{DefaultConnector, NextTimeout};

/// How much of a page is read for its tags.
const PAGE_BYTES: u64 = 512 << 10;
/// How long a whole fetch may take.
const TIMEOUT: Duration = Duration::from_secs(6);
/// A title's and a description's longest, in characters.
pub const TITLE: usize = 100;
pub const DESCRIPTION: usize = 200;

/// What a page says about itself.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Page {
    pub site: String,
    pub title: String,
    pub description: String,
    /// Its picture's address, made whole.
    pub image: Option<String>,
}

/// The first link in a post's words worth a preview: http or https, not
/// YouTube (which plays in place), at most 2,000 characters.
pub fn first_link(body: &str) -> Option<String> {
    body.split_whitespace()
        .filter_map(|word| {
            let start = word.find("https://").or_else(|| word.find("http://"))?;
            let link = word[start..].trim_end_matches(|c: char| ").,!?;:'\"]>".contains(c));
            let parsed = url::Url::parse(link).ok()?;
            let host = parsed.host_str()?.trim_start_matches("www.").to_ascii_lowercase();
            let youtube = host == "youtu.be" || host == "youtube.com" || host.ends_with(".youtube.com");
            (link.len() <= 2000 && !youtube && matches!(parsed.scheme(), "http" | "https")).then(|| link.to_string())
        })
        .next()
}

/// Whether an address is on the public internet.
pub fn public(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => public_v4(v4),
        IpAddr::V6(v6) => {
            if let Some(v4) = v6.to_ipv4_mapped() {
                return public_v4(v4);
            }
            let s = v6.segments();
            !(v6.is_loopback()
                || v6.is_unspecified()
                || v6.is_multicast()
                || (s[0] & 0xfe00) == 0xfc00 // unique local
                || (s[0] & 0xffc0) == 0xfe80 // link-local
                || (s[0] == 0x2001 && s[1] == 0x0db8) // documentation
                || (s[0] == 0x0064 && s[1] == 0xff9b) // NAT64
                || s[..6] == [0, 0, 0, 0, 0, 0]) // IPv4-compatible
        }
    }
}

fn public_v4(ip: Ipv4Addr) -> bool {
    let [a, b, c, _] = ip.octets();
    !(ip.is_private()
        || ip.is_loopback()
        || ip.is_link_local()
        || ip.is_broadcast()
        || ip.is_multicast()
        || ip.is_unspecified()
        || ip.is_documentation()
        || a == 0
        || (a == 100 && (64..128).contains(&b)) // shared address space, and the tailnet
        || (a == 192 && b == 0 && c == 0)
        || (a == 198 && (b == 18 || b == 19))
        || a >= 240)
}

/// Looks names up as usual, then keeps only public addresses.
struct PublicOnly(DefaultResolver);

impl std::fmt::Debug for PublicOnly {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("PublicOnly")
    }
}

impl Resolver for PublicOnly {
    fn resolve(&self, uri: &ureq::http::Uri, config: &ureq::config::Config, timeout: NextTimeout) -> Result<ResolvedSocketAddrs, ureq::Error> {
        let found = self.0.resolve(uri, config, timeout)?;
        let mut kept = self.empty();
        for addr in found.iter().filter(|a| public(a.ip())) {
            kept.push(*addr);
        }
        if kept.is_empty() {
            return Err(ureq::Error::HostNotFound);
        }
        Ok(kept)
    }
}

fn agent() -> ureq::Agent {
    let config = ureq::Agent::config_builder()
        .timeout_global(Some(TIMEOUT))
        .max_redirects(3)
        .user_agent("OrienNetworkBot/1.0 (link previews; +https://orien.network)")
        .build();
    ureq::Agent::with_parts(config, DefaultConnector::new(), PublicOnly(DefaultResolver::default()))
}

/// Fetches a page and reads what it says about itself.
pub fn fetch_page(link: &str) -> Result<Page> {
    let mut response = agent().get(link).header("Accept", "text/html").call().context("the page didn't answer")?;
    let kind = response.headers().get("content-type").and_then(|v| v.to_str().ok()).unwrap_or("").to_ascii_lowercase();
    if !kind.contains("text/html") {
        bail!("not a web page");
    }
    let mut bytes = Vec::new();
    response.body_mut().as_reader().take(PAGE_BYTES).read_to_end(&mut bytes)?;
    let base = url::Url::parse(link)?;
    Ok(read_page(&String::from_utf8_lossy(&bytes), &base))
}

/// Fetches a picture, up to `most` bytes.
pub fn fetch_picture(link: &str, most: usize) -> Result<Vec<u8>> {
    let mut response = agent().get(link).header("Accept", "image/*").call().context("the picture didn't answer")?;
    let kind = response.headers().get("content-type").and_then(|v| v.to_str().ok()).unwrap_or("").to_ascii_lowercase();
    if !kind.starts_with("image/") {
        bail!("not a picture");
    }
    let mut bytes = Vec::new();
    response.body_mut().as_reader().take(most as u64 + 1).read_to_end(&mut bytes)?;
    if bytes.len() > most {
        bail!("the picture is too big");
    }
    Ok(bytes)
}

/// The tags of a page: og: ones first, then its <title> and description,
/// then its host for the site.
pub fn read_page(html: &str, base: &url::Url) -> Page {
    let head = match html.to_ascii_lowercase().find("</head") {
        Some(end) => &html[..end],
        None => html,
    };
    let mut tags: Vec<(String, String)> = Vec::new();
    let lower = head.to_ascii_lowercase();
    let mut at = 0;
    while let Some(start) = lower[at..].find("<meta") {
        let start = at + start;
        let Some(end) = lower[start..].find('>') else { break };
        let tag = &head[start..start + end];
        let attrs = attributes(tag);
        let key = attrs.iter().find(|(k, _)| k == "property" || k == "name").map(|(_, v)| v.to_ascii_lowercase());
        let content = attrs.iter().find(|(k, _)| k == "content").map(|(_, v)| v.clone());
        if let (Some(key), Some(content)) = (key, content) {
            tags.push((key, decode(&content)));
        }
        at = start + end;
    }
    let tag = |names: &[&str]| names.iter().find_map(|n| tags.iter().find(|(k, v)| k == n && !v.trim().is_empty()).map(|(_, v)| v.clone()));
    let title_tag = lower.find("<title").and_then(|s| {
        let open = lower[s..].find('>')? + s + 1;
        let close = lower[open..].find("</title")? + open;
        Some(decode(&head[open..close]))
    });
    let host = base.host_str().unwrap_or_default().trim_start_matches("www.").to_string();
    let site = tag(&["og:site_name", "application-name"]).unwrap_or(host);
    let title = tag(&["og:title", "twitter:title"]).or(title_tag).unwrap_or_default();
    let description = tag(&["og:description", "twitter:description", "description"]).unwrap_or_default();
    let image = tag(&["og:image:secure_url", "og:image", "og:image:url", "twitter:image", "twitter:image:src"])
        .and_then(|src| base.join(src.trim()).ok())
        .filter(|u| matches!(u.scheme(), "http" | "https"))
        .map(|u| u.to_string());
    Page { site: tidy(&site, 60), title: tidy(&title, TITLE), description: tidy(&description, DESCRIPTION), image }
}

/// A tag's name="value" pairs, names in lowercase.
fn attributes(tag: &str) -> Vec<(String, String)> {
    let mut found = Vec::new();
    let chars: Vec<char> = tag.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        while i < chars.len() && !chars[i].is_alphanumeric() {
            i += 1;
        }
        let start = i;
        while i < chars.len() && (chars[i].is_alphanumeric() || matches!(chars[i], '-' | ':' | '_')) {
            i += 1;
        }
        let name: String = chars[start..i].iter().collect::<String>().to_ascii_lowercase();
        while i < chars.len() && chars[i].is_whitespace() {
            i += 1;
        }
        if i < chars.len() && chars[i] == '=' {
            i += 1;
            while i < chars.len() && chars[i].is_whitespace() {
                i += 1;
            }
            let value = if i < chars.len() && (chars[i] == '"' || chars[i] == '\'') {
                let quote = chars[i];
                i += 1;
                let begin = i;
                while i < chars.len() && chars[i] != quote {
                    i += 1;
                }
                let v: String = chars[begin..i].iter().collect();
                i += 1;
                v
            } else {
                let begin = i;
                while i < chars.len() && !chars[i].is_whitespace() {
                    i += 1;
                }
                chars[begin..i].iter().collect()
            };
            if !name.is_empty() {
                found.push((name, value));
            }
        } else if name.is_empty() {
            i += 1;
        }
    }
    found
}

/// HTML's common entities, decoded.
fn decode(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(amp) = rest.find('&') {
        out.push_str(&rest[..amp]);
        rest = &rest[amp..];
        let Some(semi) = rest[..rest.len().min(12)].find(';') else {
            out.push('&');
            rest = &rest[1..];
            continue;
        };
        let entity = &rest[1..semi];
        let ch = match entity {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            "nbsp" => Some(' '),
            _ if entity.starts_with("#x") || entity.starts_with("#X") => u32::from_str_radix(&entity[2..], 16).ok().and_then(char::from_u32),
            _ if entity.starts_with('#') => entity[1..].parse().ok().and_then(char::from_u32),
            _ => None,
        };
        match ch {
            Some(c) => {
                out.push(c);
                rest = &rest[semi + 1..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// One line of plain words, at most `most` characters (with … when cut).
fn tidy(text: &str, most: usize) -> String {
    let flat: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let flat: String = flat.chars().filter(|c| !c.is_control()).collect();
    if flat.chars().count() <= most {
        return flat;
    }
    let cut: String = flat.chars().take(most - 1).collect();
    format!("{}…", cut.trim_end())
}

/// Whether a preview's words may show, read as a post is.
pub fn words_allowed(screener: &Screener, page: &Page) -> Result<bool> {
    let text = format!("{}\n{}\n{}", page.site, page.title, page.description);
    Ok(matches!(screener.message(&text)?, Verdict::Allowed))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_first_link_worth_a_preview() {
        assert_eq!(first_link("look https://example.com/a?b=1, nice").as_deref(), Some("https://example.com/a?b=1"));
        assert_eq!(first_link("https://youtu.be/abc then (https://news.example.org/story)").as_deref(), Some("https://news.example.org/story"));
        assert_eq!(first_link("https://www.youtube.com/watch?v=abc only"), None);
        assert_eq!(first_link("no links, ftp://x.example"), None);
    }

    #[test]
    fn only_public_addresses_are_reached() {
        for private in ["127.0.0.1", "10.1.2.3", "172.16.0.1", "192.168.1.1", "100.99.59.98", "169.254.169.254", "0.0.0.0", "::1", "fd7a:115c:a1e0::1", "fe80::1", "::ffff:127.0.0.1"] {
            assert!(!public(private.parse().unwrap()), "{private}");
        }
        for open in ["1.1.1.1", "93.184.216.34", "2606:4700:4700::1111"] {
            assert!(public(open.parse().unwrap()), "{open}");
        }
    }

    #[test]
    fn a_page_says_what_it_is() {
        let base = url::Url::parse("https://www.example.com/news/story").unwrap();
        let html = r#"<html><head><title>Fallback &amp; title</title>
            <meta property="og:title" content="Orien hits 500,000 ORN &#8212; mined">
            <meta name="description" content="A   long
              description">
            <META PROPERTY='og:image' CONTENT='/pics/cover.jpg'>
            </head><body><meta property="og:title" content="not in the head"></body></html>"#;
        let page = read_page(html, &base);
        assert_eq!(page.title, "Orien hits 500,000 ORN — mined");
        assert_eq!(page.description, "A long description");
        assert_eq!(page.site, "example.com");
        assert_eq!(page.image.as_deref(), Some("https://www.example.com/pics/cover.jpg"));
        let bare = read_page("<title>Just a title</title>", &base);
        assert_eq!((bare.title.as_str(), bare.image), ("Just a title", None));
        assert_eq!(tidy(&"x".repeat(150), TITLE).chars().count(), TITLE);
    }
}

#[cfg(test)]
mod live {
    use super::*;

    /// Asks the internet: `cargo test -p orien-market links::live -- --ignored`.
    #[test]
    #[ignore]
    fn a_real_page_and_a_private_address() {
        let page = fetch_page("https://orien.network/").unwrap();
        assert!(!page.title.is_empty(), "{page:?}");
        println!("{page:?}");
        assert!(fetch_page("http://127.0.0.1:7200/").is_err(), "this server's own Market");
        assert!(fetch_page("http://100.99.59.98/").is_err(), "the tailnet");
        assert!(fetch_page("http://localhost/").is_err());
    }
}
