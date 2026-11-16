//! Sending email through Resend: sign-in codes, notices, and messages from
//! the site's contact form.

/// Where the contact form's messages go.
pub const CONTACT: &str = "hello@orien.network";

use anyhow::{Context, Result, bail};
use serde_json::json;

pub struct Mailer {
    /// The Resend key, or none to print codes to the server's log instead,
    /// which is for trying the marketplace on one's own computer only.
    key: Option<String>,
    from: String,
}

impl Mailer {
    pub fn resend(key: String, from: &str) -> Mailer {
        Mailer { key: Some(key), from: from.into() }
    }

    pub fn log_only() -> Mailer {
        Mailer { key: None, from: String::new() }
    }

    /// Tells someone they have a message, with a link to it. The message
    /// itself is not in the email: it is read on the site.
    pub fn send_notice(&self, notice: &crate::messages::Notice, origin: &str) -> Result<()> {
        let link = format!("{origin}/marketplace/messages/?c={}", notice.conversation);
        let subject = format!("{} sent you a message about {}", notice.from_name, notice.listing_title);
        let Some(key) = &self.key else {
            println!("would email {}: {subject} ({link})", notice.to_email);
            return Ok(());
        };
        let escape = |text: &str| text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;");
        let text = format!("{subject} on the Orien Market.\n\nRead it and reply: {link}\n");
        let html = format!(
            "<p>{} on the Orien Market.</p><p><a href=\"{link}\">Read it and reply</a></p>",
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

    /// Sends a message from the contact form to CONTACT, with the sender's
    /// address to reply to.
    pub fn send_contact(&self, name: &str, email: &str, message: &str) -> Result<()> {
        let subject = format!("Message from {name} on orien.network");
        let text = format!("From: {name} <{email}>\n\n{message}\n\nReply to this email to answer them.\n");
        let Some(key) = &self.key else {
            println!("would email {CONTACT}: {subject}\n{text}");
            return Ok(());
        };
        let escape = |t: &str| t.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;");
        let html = format!(
            "<p><strong>From:</strong> {} &lt;{}&gt;</p><p style=\"white-space:pre-wrap\">{}</p><p style=\"color:#888\">Reply to this email to answer them.</p>",
            escape(name),
            escape(email),
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
        if !response.status().is_success() {
            let status = response.status();
            let detail = response.into_body().read_to_string().unwrap_or_default();
            bail!("Resend refused the email ({status}): {detail}");
        }
        Ok(())
    }

    /// Sends `code` to `to`. Blocks while Resend answers.
    pub fn send_code(&self, to: &str, code: &str) -> Result<()> {
        let Some(key) = &self.key else {
            println!("sign-in code for {to}: {code}");
            return Ok(());
        };
        let text = format!(
            "Your code for the Orien Market is {code}\n\n\
             It works for 15 minutes. If you did not ask for it, someone typed your email by mistake, \
             and you can ignore this.\n"
        );
        let html = format!(
            "<p>Your code for the Orien Market is</p>\
             <p style=\"font-size:28px;font-weight:700;letter-spacing:6px\">{code}</p>\
             <p>It works for 15 minutes. If you did not ask for it, someone typed your email by mistake, \
             and you can ignore this.</p>"
        );
        self.deliver(key, to, &format!("{code} is your Orien Market code"), &text, &html)
    }
}
