//! Sending email through Resend: sign-in codes, notices, and messages from
//! the site's contact form.

/// Where the contact form's messages go.
pub const CONTACT: &str = "hello@orien.network";

use std::path::PathBuf;
use std::sync::Mutex;

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::json;

/// Resend's free plan: 100 emails a day (until midnight UTC), 3,000 a month.
pub const DAILY_LIMIT: u64 = 100;
pub const MONTHLY_LIMIT: u64 = 3000;

/// How many emails Resend says the account has used, as of its last answer.
/// Resend says so on every email it sends: our key can only send, so these
/// are only as fresh as the last email.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Usage {
    /// Today's emails, as of `at`; the count starts again at midnight UTC.
    pub daily: Option<u64>,
    pub monthly: Option<u64>,
    /// When Resend last said, in seconds since 1970.
    pub at: u64,
    /// When Resend last refused an email for the daily or monthly quota.
    pub refused: Option<Refused>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Refused {
    /// "daily_quota_exceeded" or "monthly_quota_exceeded".
    pub why: String,
    pub at: u64,
}

pub struct Mailer {
    /// The Resend key, or none to print codes to the server's log instead,
    /// which is for trying the marketplace on one's own computer only.
    key: Option<String>,
    from: String,
    usage: Mutex<Usage>,
    /// Where the usage is kept between restarts.
    file: Option<PathBuf>,
}

impl Mailer {
    pub fn resend(key: String, from: &str) -> Mailer {
        Mailer { key: Some(key), from: from.into(), usage: Mutex::new(Usage::default()), file: None }
    }

    pub fn log_only() -> Mailer {
        Mailer { key: None, from: String::new(), usage: Mutex::new(Usage::default()), file: None }
    }

    /// Keeps the usage in `file`, and starts from what it holds.
    pub fn keeping_usage_in(mut self, file: PathBuf) -> Mailer {
        if let Some(kept) = std::fs::read_to_string(&file).ok().and_then(|text| serde_json::from_str::<Usage>(&text).ok()) {
            self.usage = Mutex::new(kept);
        }
        self.file = Some(file);
        self
    }

    /// The usage as Resend last told it.
    pub fn usage(&self) -> Usage {
        self.usage.lock().unwrap().clone()
    }

    /// Notes what one of Resend's answers says about the usage.
    pub fn note_usage(&self, daily: Option<u64>, monthly: Option<u64>, refused: Option<&str>, at: u64) {
        if daily.is_none() && monthly.is_none() && refused.is_none() {
            return;
        }
        let mut usage = self.usage.lock().unwrap();
        if daily.is_some() || monthly.is_some() {
            usage.daily = daily.or(usage.daily);
            usage.monthly = monthly.or(usage.monthly);
            usage.at = at;
        }
        if let Some(why) = refused {
            usage.refused = Some(Refused { why: why.into(), at });
        }
        if let (Some(file), Ok(text)) = (&self.file, serde_json::to_string(&*usage)) {
            let fresh = file.with_extension("new");
            if std::fs::write(&fresh, text).is_ok() {
                let _ = std::fs::rename(&fresh, file);
            }
        }
    }

    /// Tells someone they have a message, with a link to it. The message
    /// itself is not in the email: it is read on the site.
    pub fn send_notice(&self, notice: &crate::messages::Notice, origin: &str) -> Result<()> {
        let link = format!("{origin}/messages/?c={}", notice.conversation);
        let subject = match &notice.listing_title {
            Some(title) => format!("{} sent you a message about {}", notice.from_name, title),
            None => format!("{} sent you a message", notice.from_name),
        };
        let Some(key) = &self.key else {
            println!("would email {}: {subject} ({link})", notice.to_email);
            return Ok(());
        };
        let escape = |text: &str| text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;");
        let text = format!("{subject} on Orien Network.\n\nRead it and reply: {link}\n");
        let html = format!(
            "<p>{} on Orien Network.</p><p><a href=\"{link}\">Read it and reply</a></p>",
            escape(&subject)
        );
        self.deliver(key, &notice.to_email, &subject, &text, &html)
    }

    /// Tells a seller someone took their swap offer: their wallet must be
    /// open for the swap to start.
    pub fn send_swap_taken(&self, to: &str, taker: &str, origin: &str) -> Result<()> {
        let subject = format!("{taker} took your swap offer");
        let text = format!(
            "{taker} took your offer on the Orien swap board.\n\nOpen Orien Wallet within 30 minutes so it can start the swap; \
             otherwise the offer goes back on the board.\n\n{origin}/swap/\n"
        );
        let Some(key) = &self.key else {
            println!("would email {to}: {subject}");
            return Ok(());
        };
        let escape = |t: &str| t.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;");
        let html = format!(
            "<p>{} took your offer on the Orien swap board.</p><p>Open Orien Wallet within 30 minutes so it can start the swap; \
             otherwise the offer goes back on the board.</p>",
            escape(taker)
        );
        self.deliver(key, to, &subject, &text, &html)
    }

    /// Tells a buyer the seller's wallet accepted their swap: their ORN are
    /// locked for them, and it is their turn to pay the Bitcoin.
    /// `amount` includes its coin: "0.2 SOL", "0.00015 BTC".
    pub fn send_swap_accepted(&self, to: &str, seller: &str, btc: &str, origin: &str) -> Result<()> {
        let subject = format!("{seller} accepted your swap: pay {btc} now");
        let text = format!(
            "{seller} accepted your swap, and your ORN are locked for you.\n\n\
             Your turn: open Orien Wallet and pay exactly what it shows ({btc}) to the address it shows, from your own wallet, \
             as soon as you can. Your ORN arrive once the seller takes the payment.\n\n{origin}/swap/\n"
        );
        let Some(key) = &self.key else {
            println!("would email {to}: {subject}");
            return Ok(());
        };
        let escape = |t: &str| t.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;");
        let html = format!(
            "<p>{} accepted your swap, and your ORN are locked for you.</p>\
             <p><strong>Your turn:</strong> open Orien Wallet and pay exactly what it shows (<strong>{btc}</strong>) to the address it shows, \
             from your own wallet, as soon as you can. Your ORN arrive once the seller takes the payment.</p>",
            escape(seller)
        );
        self.deliver(key, to, &subject, &text, &html)
    }

    /// Sends a message from the contact form to CONTACT, with the sender's
    /// address to reply to and their business, if they gave one.
    pub fn send_contact(&self, name: &str, business: &str, email: &str, message: &str) -> Result<()> {
        let at = if business.is_empty() { String::new() } else { format!(" at {business}") };
        let subject = format!("Message from {name}{at} on orien.network");
        let line = if business.is_empty() { String::new() } else { format!("Business: {business}\n") };
        let text = format!("From: {name} <{email}>\n{line}\n{message}\n\nReply to this email to answer them.\n");
        let Some(key) = &self.key else {
            println!("would email {CONTACT}: {subject}\n{text}");
            return Ok(());
        };
        let escape = |t: &str| t.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;");
        let html = format!(
            "<p><strong>From:</strong> {} &lt;{}&gt;</p>{}<p style=\"white-space:pre-wrap\">{}</p><p style=\"color:#888\">Reply to this email to answer them.</p>",
            escape(name),
            escape(email),
            if business.is_empty() { String::new() } else { format!("<p><strong>Business:</strong> {}</p>", escape(business)) },
            escape(message)
        );
        self.send(key, CONTACT, Some(email), &subject, &text, &html)
    }

    fn deliver(&self, key: &str, to: &str, subject: &str, text: &str, html: &str) -> Result<()> {
        self.send(key, to, None, subject, text, html)
    }

    fn send(&self, key: &str, to: &str, reply_to: Option<&str>, subject: &str, text: &str, html: &str) -> Result<()> {
        let mut body = json!({ "from": self.from, "to": [to], "subject": subject, "text": text, "html": html });
        if let Some(reply_to) = reply_to {
            body["reply_to"] = json!(reply_to);
        }
        let response = ureq::post("https://api.resend.com/emails")
            .header("Authorization", &format!("Bearer {key}"))
            .config()
            .http_status_as_error(false)
            .build()
            .send_json(&body)
            .context("cannot reach Resend")?;
        let count = |name: &str| response.headers().get(name).and_then(|v| v.to_str().ok()).and_then(|v| v.trim().parse::<u64>().ok());
        let (daily, monthly) = (count("x-resend-daily-quota"), count("x-resend-monthly-quota"));
        let at = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
        if !response.status().is_success() {
            let status = response.status();
            let detail = response.into_body().read_to_string().unwrap_or_default();
            let why = serde_json::from_str::<serde_json::Value>(&detail)
                .ok()
                .and_then(|answer| answer["name"].as_str().map(str::to_string))
                .filter(|name| name.ends_with("_quota_exceeded"));
            self.note_usage(daily, monthly, why.as_deref(), at);
            bail!("Resend refused the email ({status}): {detail}");
        }
        self.note_usage(daily, monthly, None, at);
        Ok(())
    }

    /// Sends `code` to `to`. Blocks while Resend answers.
    pub fn send_code(&self, to: &str, code: &str) -> Result<()> {
        let Some(key) = &self.key else {
            println!("sign-in code for {to}: {code}");
            return Ok(());
        };
        let text = format!(
            "Your Orien Network code is {code}\n\n\
             It works for 15 minutes. If you did not ask for it, someone typed your email by mistake, \
             and you can ignore this.\n"
        );
        let html = format!(
            "<p>Your Orien Network code is</p>\
             <p style=\"font-size:28px;font-weight:700;letter-spacing:6px\">{code}</p>\
             <p>It works for 15 minutes. If you did not ask for it, someone typed your email by mistake, \
             and you can ignore this.</p>"
        );
        self.deliver(key, to, &format!("{code} is your Orien Network code"), &text, &html)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn usage_is_kept_across_restarts_and_refusals_are_noted() {
        let mut random = [0u8; 8];
        getrandom::fill(&mut random).unwrap();
        let file = std::env::temp_dir().join(format!("orien-resend-usage-{}.json", hex::encode(random)));
        let mailer = Mailer::log_only().keeping_usage_in(file.clone());
        assert_eq!(mailer.usage(), Usage::default());
        mailer.note_usage(None, None, None, 5);
        assert_eq!(mailer.usage().at, 0, "an answer without counts changes nothing");
        mailer.note_usage(Some(41), Some(1200), None, 10);
        mailer.note_usage(None, None, Some("daily_quota_exceeded"), 20);
        let again = Mailer::log_only().keeping_usage_in(file.clone());
        let usage = again.usage();
        assert_eq!((usage.daily, usage.monthly, usage.at), (Some(41), Some(1200), 10));
        assert_eq!(usage.refused, Some(Refused { why: "daily_quota_exceeded".into(), at: 20 }));
        let _ = std::fs::remove_file(file);
    }
}
