//! Verified Holder: a role for people who show they hold ORN.
//!
//! Someone presses "Verify my ORN" and is shown a tiny, exact amount to
//! send from their wallet to the Market's wallet. The bot watches that
//! wallet's incoming payments; the one with that amount tells it which
//! wallet is theirs, and if the wallet still holds ORN they get the role.
//! Once a day every verified wallet is looked at again, and the role is
//! taken back from those that hold nothing.
//!
//! Nobody types an address, a password or a recovery phrase anywhere.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{Context, Result};
use orien_ledger::{AccountId, Income};
use orien_net::client::Client;
use serde::{Deserialize, Serialize};
use serenity::all::{ComponentInteraction, CreateInteractionResponseFollowup, GuildId, UserId};
use serenity::http::Http;

/// The smallest and largest amounts asked for, in ORN's smallest units:
/// 0.0001 to 0.000999 ORN, in steps that read cleanly (0.000417).
const LOWEST: u64 = 100_000;
const HIGHEST: u64 = 999_000;
const STEP: u64 = 1_000;

/// How long people are told they have, and how long the bot really waits.
pub const MINUTES: u64 = 30;
const PATIENCE: u64 = 3600;

const LOOK_EVERY: Duration = Duration::from_secs(30);
const RECHECK_EVERY: u64 = 24 * 3600;

pub const ROLE: &str = "Verified Holder";

/// An amount as people read it: 0.000417.
pub fn orn(units: u64) -> String {
    format!("{}.{:06}", units / 1_000_000_000, units % 1_000_000_000 / 1_000)
}

fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Debug)]
struct Waiting {
    amount: u64,
    /// The network's epoch when it was asked for: earlier payments don't count.
    epoch: u64,
    asked: u64,
}

#[derive(Default, Serialize, Deserialize)]
struct Book {
    /// By Discord user.
    #[serde(default)]
    waiting: BTreeMap<u64, Waiting>,
    /// By Discord user: the wallet they proved.
    #[serde(default)]
    verified: BTreeMap<u64, String>,
    /// Payments already counted for someone, so none counts twice.
    #[serde(default)]
    used: BTreeSet<String>,
    /// When every verified wallet was last looked at again.
    #[serde(default)]
    rechecked: u64,
}

/// A payment that came into the wallet being watched.
#[derive(Clone, Debug, PartialEq)]
pub struct Paid {
    pub from: String,
    pub amount: u64,
    pub epoch: u64,
    /// What tells this payment from every other.
    pub key: String,
}

/// What a payment means for the people waiting.
#[derive(Debug, PartialEq)]
enum Outcome {
    Verified { user: u64, address: String },
    /// The wallet already belongs to another verified person.
    Taken { user: u64 },
}

impl Book {
    /// The amount for `user` to send: the one already asked for, or a new
    /// one nobody else is waiting on. None when every amount is taken.
    fn ask(&mut self, user: u64, epoch: u64, now: u64, pick: u64) -> Option<u64> {
        self.waiting.retain(|_, w| now.saturating_sub(w.asked) < PATIENCE);
        if let Some(waiting) = self.waiting.get(&user) {
            return Some(waiting.amount);
        }
        let taken: BTreeSet<u64> = self.waiting.values().map(|w| w.amount).collect();
        let count = (HIGHEST - LOWEST) / STEP + 1;
        let amount = (0..count).map(|i| LOWEST + (pick + i) % count * STEP).find(|amount| !taken.contains(amount))?;
        self.waiting.insert(user, Waiting { amount, epoch, asked: now });
        Some(amount)
    }

    /// Matches payments to the people waiting.
    fn settle(&mut self, payments: &[Paid]) -> Vec<Outcome> {
        let mut outcomes = Vec::new();
        for paid in payments {
            if self.used.contains(&paid.key) {
                continue;
            }
            let Some(user) = self.waiting.iter().find(|(_, w)| w.amount == paid.amount && paid.epoch >= w.epoch).map(|(user, _)| *user) else {
                continue;
            };
            self.waiting.remove(&user);
            self.used.insert(paid.key.clone());
            if self.verified.values().any(|address| *address == paid.from) {
                outcomes.push(Outcome::Taken { user });
            } else {
                outcomes.push(Outcome::Verified { user, address: paid.from.clone() });
            }
        }
        outcomes
    }
}

pub struct Holders {
    client: Arc<Client>,
    to: AccountId,
    /// The address people send to, written out.
    pub address: String,
    path: PathBuf,
    book: Mutex<Book>,
    /// The network's epoch, as last seen. 0 until the network has answered.
    epoch: AtomicU64,
    /// The button presses of people waiting, to tell them how it went.
    /// Discord lets a press be answered for 15 minutes.
    presses: Mutex<HashMap<u64, ComponentInteraction>>,
}

impl Holders {
    pub fn open(genesis: &Path, address: &str, path: PathBuf) -> Result<Holders> {
        let genesis = orien_net::keys::load_genesis(genesis)?;
        let to: AccountId = address.parse().map_err(|_| anyhow::anyhow!("{address} is not an Orien address"))?;
        let book = match std::fs::read_to_string(&path) {
            Ok(text) => serde_json::from_str(&text).with_context(|| format!("{} is damaged", path.display()))?,
            Err(_) => Book::default(),
        };
        Ok(Holders {
            client: Arc::new(Client::new(&genesis)),
            to,
            address: address.to_string(),
            path,
            book: Mutex::new(book),
            epoch: AtomicU64::new(0),
            presses: Mutex::new(HashMap::new()),
        })
    }

    fn save(&self, book: &Book) {
        let fresh = self.path.with_extension("new");
        let written = serde_json::to_string_pretty(book).map_err(anyhow::Error::from).and_then(|text| {
            std::fs::write(&fresh, text)?;
            std::fs::rename(&fresh, &self.path)?;
            Ok(())
        });
        if let Err(error) = written {
            eprintln!("holders: could not save {}: {error:#}", self.path.display());
        }
    }

    /// The amount for this person to send, or none while the network has
    /// not answered yet.
    pub fn ask(&self, press: &ComponentInteraction) -> Option<u64> {
        let epoch = self.epoch.load(Ordering::Relaxed);
        if epoch == 0 {
            return None;
        }
        let user = press.user.id.get();
        let mut book = self.book.lock().unwrap();
        let pick = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.subsec_nanos() as u64 / 1000);
        let amount = book.ask(user, epoch, now(), pick)?;
        self.save(&book);
        self.presses.lock().unwrap().insert(user, press.clone());
        Some(amount)
    }

    pub fn is_verified(&self, user: UserId) -> bool {
        self.book.lock().unwrap().verified.contains_key(&user.get())
    }

    /// The network's epoch now.
    fn network_epoch(client: &Client) -> Option<u64> {
        client.info().into_iter().filter_map(|info| info.ok().map(|i| i.epoch)).max()
    }

    /// Payments into the watched wallet from epoch `from` on.
    fn received(client: &Client, to: &AccountId, from: u64) -> Option<Vec<Paid>> {
        let mut found = Vec::new();
        let mut next = Some(from);
        for _ in 0..5 {
            let Some(at) = next else { break };
            let page = client.history(to, at, 1000)?;
            for (i, entry) in page.entries.iter().enumerate() {
                if entry.income != Income::Payment {
                    continue;
                }
                let Some(payer) = entry.counterparty else { continue };
                let from = hex::encode(payer.as_bytes());
                let key = match entry.order {
                    Some((sender, seq)) => format!("{}:{seq}", hex::encode(sender.as_bytes())),
                    None => format!("{from}:{}:{}:{i}", entry.epoch, entry.amount),
                };
                found.push(Paid { from, amount: entry.amount, epoch: entry.epoch, key });
            }
            next = page.next;
        }
        Some(found)
    }

    /// Whether a wallet holds any ORN. None when no validator answers.
    fn holds(client: &Client, address: &str) -> Option<bool> {
        let id: AccountId = address.parse().ok()?;
        let status = client.account(&id, 0);
        let views: Vec<_> = status.views.iter().flatten().collect();
        if views.is_empty() {
            return None;
        }
        Some(views.iter().any(|view| view.balance > 0) || status.spendable.is_some_and(|spendable| spendable > 0))
    }

    /// What the network says right now, for checking by hand.
    pub fn report(&self, wallet: Option<&str>) -> String {
        let epoch = Holders::network_epoch(&self.client);
        let recent = epoch.and_then(|epoch| Holders::received(&self.client, &self.to, epoch.saturating_sub(48)));
        let mut text = format!(
            "epoch now: {epoch:?}\npayments into {}… in the last 48 epochs: {}",
            &self.address[..8],
            recent.map_or("no validator keeps history".to_string(), |paid| paid.len().to_string())
        );
        if let Some(wallet) = wallet {
            text.push_str(&format!("\n{}… holds ORN: {:?}", &wallet[..8.min(wallet.len())], Holders::holds(&self.client, wallet)));
        }
        text
    }

    async fn tell(&self, http: &Http, user: u64, text: &str) {
        let press = self.presses.lock().unwrap().remove(&user);
        if let Some(press) = press {
            // Fails quietly once Discord's 15 minutes are over.
            let _ = press.create_followup(http, CreateInteractionResponseFollowup::new().content(text).ephemeral(true)).await;
        }
    }

    /// One look at the network: who has paid, and once a day, who still holds.
    async fn look(self: &Arc<Self>, http: &Http, guild: GuildId) -> Result<()> {
        let client = self.client.clone();
        let epoch = tokio::task::spawn_blocking(move || Holders::network_epoch(&client)).await?.context("no validator answers")?;
        self.epoch.store(epoch, Ordering::Relaxed);

        let oldest = {
            let mut book = self.book.lock().unwrap();
            let now = now();
            book.waiting.retain(|_, w| now.saturating_sub(w.asked) < PATIENCE);
            book.waiting.values().map(|w| w.epoch).min()
        };
        if let Some(from) = oldest {
            let (client, to) = (self.client.clone(), self.to);
            let payments = tokio::task::spawn_blocking(move || Holders::received(&client, &to, from)).await?.context("no validator keeps history")?;
            let outcomes = self.book.lock().unwrap().settle(&payments);
            for outcome in outcomes {
                match outcome {
                    Outcome::Taken { user } => {
                        self.tell(http, user, "That wallet already made someone else a Verified Holder, so it can't be used again.").await;
                    }
                    Outcome::Verified { user, address } => {
                        let (client, wallet) = (self.client.clone(), address.clone());
                        let holds = tokio::task::spawn_blocking(move || Holders::holds(&client, &wallet)).await?;
                        if holds == Some(false) {
                            self.tell(http, user, "Your payment arrived, but that wallet holds no ORN now, so I can't give you the role. Try again once it holds some.").await;
                            continue;
                        }
                        let role = role(http, guild).await?;
                        match http.add_member_role(guild, UserId::new(user), role, Some("proved they hold ORN")).await {
                            Ok(()) => {
                                self.book.lock().unwrap().verified.insert(user, address);
                                eprintln!("holders: gave {ROLE} to a member");
                                self.tell(http, user, "✅ Your payment arrived. You're a **Verified Holder** now.").await;
                            }
                            Err(error) => {
                                eprintln!("holders: could not give the {ROLE} role: {error} (is the bot's own role above it?)");
                                self.tell(http, user, "Your payment arrived, but I couldn't give you the role just now. Please tell the founder.").await;
                            }
                        }
                    }
                }
            }
            self.save(&self.book.lock().unwrap());
        }

        let due: Vec<(u64, String)> = {
            let book = self.book.lock().unwrap();
            if now().saturating_sub(book.rechecked) < RECHECK_EVERY { Vec::new() } else { book.verified.iter().map(|(u, a)| (*u, a.clone())).collect() }
        };
        if !due.is_empty() || self.book.lock().unwrap().rechecked == 0 {
            for (user, address) in due {
                let client = self.client.clone();
                // Only a clear "holds nothing" takes the role back.
                if tokio::task::spawn_blocking(move || Holders::holds(&client, &address)).await? == Some(false) {
                    let role = role(http, guild).await?;
                    if http.remove_member_role(guild, UserId::new(user), role, Some("their wallet holds no ORN now")).await.is_ok() {
                        self.book.lock().unwrap().verified.remove(&user);
                        eprintln!("holders: took {ROLE} back from a member whose wallet is empty");
                    }
                }
            }
            let mut book = self.book.lock().unwrap();
            book.rechecked = now();
            self.save(&book);
        }
        Ok(())
    }

    /// Watches for as long as the bot runs.
    pub async fn watch(self: Arc<Self>, http: Arc<Http>) {
        tokio::time::sleep(Duration::from_secs(5)).await;
        let mut last_trouble = String::new();
        loop {
            let looked = match http.get_guilds(None, None).await {
                Ok(guilds) if guilds.len() == 1 => self.look(&http, guilds[0].id).await,
                Ok(guilds) => Err(anyhow::anyhow!("the bot is in {} servers; it should be in exactly one", guilds.len())),
                Err(error) => Err(error.into()),
            };
            match looked {
                Ok(()) => last_trouble.clear(),
                Err(error) => {
                    let trouble = format!("{error:#}");
                    if trouble != last_trouble {
                        eprintln!("holders: {trouble}; trying again");
                        last_trouble = trouble;
                    }
                }
            }
            tokio::time::sleep(LOOK_EVERY).await;
        }
    }
}

async fn role(http: &Http, guild: GuildId) -> Result<serenity::all::RoleId> {
    let roles = guild.roles(http).await?;
    roles.values().find(|r| r.name.trim().eq_ignore_ascii_case(ROLE)).map(|r| r.id).with_context(|| format!("the server has no {ROLE} role"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paid(from: &str, amount: u64, epoch: u64, seq: u64) -> Paid {
        Paid { from: from.into(), amount, epoch, key: format!("{from}:{seq}") }
    }

    #[test]
    fn amounts_read_cleanly_and_differ_between_people() {
        assert_eq!(orn(417_000), "0.000417");
        assert_eq!(orn(100_000), "0.000100");
        let mut book = Book::default();
        let a = book.ask(1, 50, 1000, 317).unwrap();
        let b = book.ask(2, 50, 1000, 317).unwrap();
        assert_eq!(a, 417_000);
        assert_ne!(a, b);
        assert!((LOWEST..=HIGHEST).contains(&b) && b % STEP == 0);
        // Asking again gives the same amount; much later, a new wait starts.
        assert_eq!(book.ask(1, 51, 1500, 5), Some(a));
        assert_eq!(book.ask(1, 60, 1000 + PATIENCE, 5), Some(105_000));
    }

    #[test]
    fn the_right_payment_verifies_the_right_person_once() {
        let mut book = Book::default();
        let a = book.ask(1, 50, 1000, 0).unwrap();
        let b = book.ask(2, 50, 1000, 0).unwrap();
        // Too early, the wrong amount, and then the real one.
        let payments = [paid("aa", a, 49, 1), paid("cc", 555, 50, 1), paid("bb", b, 50, 7)];
        assert_eq!(book.settle(&payments), [Outcome::Verified { user: 2, address: "bb".into() }]);
        book.verified.insert(2, "bb".into());
        // The same payment never counts again, even for a new wait on that amount.
        book.waiting.insert(3, Waiting { amount: b, epoch: 50, asked: 1000 });
        assert_eq!(book.settle(&payments), []);
        // A wallet that already verified someone can't verify another.
        assert_eq!(book.settle(&[paid("bb", b, 51, 8)]), [Outcome::Taken { user: 3 }]);
        assert!(book.waiting.contains_key(&1) && !book.waiting.contains_key(&3));
    }
}
