//! Checking listings and account names before anyone sees them, with Liquid
//! AI's d1.1 decision model through OpenRouter's Decisions API, as
//! synthrise.xyz checks coins. Each question is answered with a probability;
//! anything that reaches a question's threshold is refused, and the seller is
//! told why in plain words.
//!
//! Docs: https://openrouter.ai/liquid/d1.1/api

use anyhow::{Context, Result, bail};
use serde_json::{Value, json};
use std::collections::BTreeMap;

const ENDPOINT: &str = "https://openrouter.ai/api/alpha/decisions";
const MODEL: &str = "liquid/d1.1";

struct Check {
    id: &'static str,
    question: &'static str,
    /// Asked instead for an account's name.
    name_question: Option<&'static str>,
    threshold: f64,
    reason: &'static str,
}

/// Child safety blocks at a much lower probability than the rest.
const CHECKS: &[Check] = &[
    Check {
        id: "minors",
        question: "Does the text sexualize anyone who appears to be a minor, or offer sexual or suggestive content involving a minor?",
        name_question: None,
        threshold: 0.2,
        reason: "content that sexualizes minors",
    },
    Check {
        id: "sexual",
        question: "Does the text contain or offer nudity, pornography, sexually explicit content, or sexual services?",
        name_question: None,
        threshold: 0.7,
        reason: "sexually explicit content or services",
    },
    Check {
        id: "hate",
        question: "Does the text contain hate speech, slurs, hateful symbols, or harassment targeting people because of race, ethnicity, religion, gender, sexual orientation, disability, or nationality?",
        name_question: None,
        threshold: 0.7,
        reason: "hateful content or slurs",
    },
    Check {
        id: "violence",
        question: "Does the text contain graphic violence, real threats against people, or promotion of terrorism, violent extremism, or self-harm?",
        name_question: None,
        threshold: 0.7,
        reason: "violence, threats or extremism",
    },
    Check {
        id: "impersonation",
        question: "Does this listing falsely present the seller as a real company, brand, celebrity, public figure, or crypto project, or as its official account or staff?",
        name_question: Some(
            "Does this name falsely present the user as a real company, brand, celebrity, public figure, or crypto project, or as its official account or staff?",
        ),
        threshold: 0.8,
        reason: "a false claim to be someone else or their official account",
    },
    Check {
        id: "scam",
        question: "Does the text promise guaranteed profits or investment returns, offer to multiply or double money or crypto sent, or include phishing or fake airdrop links?",
        name_question: None,
        threshold: 0.8,
        reason: "scam-like promises or links",
    },
    Check {
        id: "illegal",
        question: "Does the text offer illegal drugs, weapons, stolen goods or accounts, fake documents, hacking of other people's accounts or systems, or other illegal goods or services?",
        name_question: None,
        threshold: 0.8,
        reason: "illegal goods or services",
    },
];

/// The context every listing is read in: ordinary listings must not be
/// refused for being informal.
const LISTING_CONTEXT: &str = "A user wants to post a listing on a marketplace where people offer their work, services \
     and goods for a cryptocurrency called ORN. Freelance work, crafts, tutoring, software, design, writing and other \
     ordinary offers are normal and allowed, as are informal tone, jokes and asking to be paid in ORN. Only flag clear \
     violations.";

const NAME_CONTEXT: &str = "A user is choosing the public name shown on their profile on a marketplace where people offer \
     their work and goods for a cryptocurrency. Personal names, business names, nicknames and jokes are normal and \
     allowed. Only flag clear violations.";

pub enum Verdict {
    Allowed,
    /// Refused, with the plain reasons to show.
    Refused(Vec<&'static str>),
}

pub struct Screener {
    /// The OpenRouter key, or none to let everything through: for trying
    /// the marketplace on one's own computer only.
    key: Option<String>,
    /// For tests: refuses text with this word in it, and fails on "fail".
    test_word: Option<String>,
}

impl Screener {
    pub fn openrouter(key: String) -> Screener {
        Screener { key: Some(key), test_word: None }
    }

    pub fn off() -> Screener {
        Screener { key: None, test_word: None }
    }

    #[doc(hidden)]
    pub fn refusing_for_tests(word: &str) -> Screener {
        Screener { key: None, test_word: Some(word.into()) }
    }

    /// A listing's text. Blocks while OpenRouter answers. An error means it
    /// could not be checked: the caller must not post it.
    pub fn listing(&self, title: &str, description: &str, category: &str, unit: &str, contact: &str) -> Result<Verdict> {
        let details = json!({
            "context": LISTING_CONTEXT,
            "title": title,
            "category": category,
            "price_unit": unit,
            "description": description,
            "contact": contact,
        });
        self.screen(details, false)
    }

    /// An account's public name.
    pub fn name(&self, name: &str) -> Result<Verdict> {
        self.screen(json!({ "context": NAME_CONTEXT, "name": name }), true)
    }

    fn screen(&self, details: Value, name: bool) -> Result<Verdict> {
        if let Some(word) = &self.test_word {
            let text = details.to_string();
            if text.contains("fail") {
                bail!("the screening service is down");
            }
            return Ok(if text.contains(word.as_str()) { Verdict::Refused(vec![CHECKS[6].reason]) } else { Verdict::Allowed });
        }
        let Some(key) = &self.key else { return Ok(Verdict::Allowed) };
        let questions: serde_json::Map<String, Value> = CHECKS
            .iter()
            .map(|check| {
                let question = if name { check.name_question.unwrap_or(check.question) } else { check.question };
                (check.id.to_string(), json!({ "type": "noul", "instructions": question }))
            })
            .collect();
        let body = json!({ "model": MODEL, "state": [details.to_string()], "questions": questions });
        let response = ureq::post(ENDPOINT)
            .header("Authorization", &format!("Bearer {key}"))
            .header("HTTP-Referer", "https://orien.network")
            .header("X-Title", "Orien marketplace")
            .config()
            .http_status_as_error(false)
            .timeout_global(Some(std::time::Duration::from_secs(15)))
            .build()
            .send_json(&body)
            .context("cannot reach OpenRouter")?;
        let status = response.status();
        let text = response.into_body().read_to_string().context("cannot read OpenRouter's answer")?;
        if !status.is_success() {
            bail!("OpenRouter answered {status}: {}", text.chars().take(200).collect::<String>());
        }
        let answer: Value = serde_json::from_str(&text).context("OpenRouter's answer is not JSON")?;
        let mut scores = BTreeMap::new();
        for check in CHECKS {
            // A missing answer is an error, never a pass.
            let Some(p) = answer["answers"][check.id]["noul"].as_f64() else {
                bail!("no answer for {}: {}", check.id, text.chars().take(200).collect::<String>());
            };
            scores.insert(check.id, p);
        }
        let refused: Vec<&'static str> =
            CHECKS.iter().filter(|check| scores[check.id] >= check.threshold).map(|check| check.reason).collect();
        println!(
            "screened {}: {} {:?}",
            if name { "a name" } else { "a listing" },
            if refused.is_empty() { "allowed" } else { "refused" },
            scores
        );
        Ok(if refused.is_empty() { Verdict::Allowed } else { Verdict::Refused(refused) })
    }
}
