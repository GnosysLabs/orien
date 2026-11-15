//! Everything the marketplace keeps, in one SQLite file: accounts, their
//! passkeys, signed-in sessions and listings.

use anyhow::{Result, bail};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

/// How long signing in lasts on one browser.
pub const SESSION_SECONDS: u64 = 30 * 24 * 3600;

pub const MAX_NAME: usize = 50;
pub const MAX_BIO: usize = 1000;
pub const MAX_TITLE: usize = 100;
pub const MAX_DESCRIPTION: usize = 5000;
pub const MAX_UNIT: usize = 30;
/// Listings one account may have open at a time.
pub const MAX_OPEN_LISTINGS: i64 = 50;

/// How long a sign-in code works, how many tries it allows, and how long
/// before another can be sent.
pub const CODE_SECONDS: u64 = 15 * 60;
pub const CODE_TRIES: u32 = 5;
pub const CODE_GAP: u64 = 60;
/// How long a seller has to send the payment that proves their wallet.
pub const VERIFY_SECONDS: u64 = 30 * 60;

pub const CATEGORIES: &[&str] = &["design", "software", "teaching", "goods", "other"];

pub(crate) fn clean_text(text: &str) -> String {
    clean(text)
}

pub fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

/// What someone fills in to post or change a listing.
#[derive(Clone, Debug, Deserialize)]
pub struct Draft {
    pub title: String,
    pub description: String,
    pub category: String,
    /// The price in ORN's smallest units (a billionth of an ORN).
    pub price: u64,
    /// What the price is for, such as "per hour"; empty for a fixed price.
    #[serde(default)]
    pub unit: String,
    /// The Orien address payments go to: always the seller's verified
    /// wallet, whatever is sent.
    #[serde(default)]
    pub pay_to: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Listing {
    pub id: i64,
    pub seller: i64,
    pub seller_name: String,
    pub seller_picture: Option<String>,
    pub title: String,
    pub description: String,
    pub category: String,
    pub price: u64,
    pub unit: String,
    pub pay_to: String,
    pub created: u64,
    pub updated: u64,
    pub open: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Account {
    pub id: i64,
    pub name: String,
    /// A few lines about the seller, shown on their profile.
    pub bio: String,
    /// The file name of their picture, under /api/market/images/.
    pub picture: Option<String>,
    /// The Orien address they proved they hold, which their listings are
    /// paid to.
    pub wallet: Option<String>,
    /// Other ways buyers can reach them, besides messages. Empty fields
    /// are left out.
    pub contact: Contact,
    pub created: u64,
}

/// How buyers reach a seller: any of a public email, a Telegram username
/// and an X username.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Contact {
    #[serde(default)]
    pub email: String,
    #[serde(default)]
    pub telegram: String,
    #[serde(default)]
    pub x: String,
}

impl Contact {
    pub fn is_empty(&self) -> bool {
        self.email.is_empty() && self.telegram.is_empty() && self.x.is_empty()
    }

    /// Tidied: an email in lower case, and bare usernames, whether typed with
    /// an @ or as a link. Or why one cannot be used.
    pub fn checked(&self) -> Result<Contact> {
        let handle = |text: &str, hosts: &[&str]| {
            let mut handle = text.trim();
            for host in hosts {
                for scheme in ["https://", "http://", ""] {
                    for www in ["www.", ""] {
                        if let Some(rest) = handle.strip_prefix(&format!("{scheme}{www}{host}/")) {
                            handle = rest;
                        }
                    }
                }
            }
            handle.trim_start_matches('@').trim_end_matches('/').to_string()
        };
        let telegram = handle(&self.telegram, &["t.me", "telegram.me"]);
        let x = handle(&self.x, &["x.com", "twitter.com"]);
        let word = |h: &str| h.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_');
        if !telegram.is_empty() && (!(5..=32).contains(&telegram.len()) || !word(&telegram)) {
            bail!("a Telegram username is 5 to 32 letters, digits and underscores");
        }
        if !x.is_empty() && (x.len() > 15 || !word(&x)) {
            bail!("an X username is up to 15 letters, digits and underscores");
        }
        let email = if self.email.trim().is_empty() { String::new() } else { email(&self.email)? };
        Ok(Contact { email, telegram, x })
    }
}

fn clean(text: &str) -> String {
    text.trim().chars().filter(|c| !c.is_control() || *c == '\n').collect()
}

impl Draft {
    /// The draft tidied, or why it cannot be posted.
    pub fn checked(&self) -> Result<Draft> {
        let draft = Draft {
            title: clean(&self.title),
            description: clean(&self.description),
            category: self.category.trim().to_lowercase(),
            price: self.price,
            unit: clean(&self.unit),
            pay_to: self.pay_to.trim().to_lowercase(),
        };
        if draft.title.is_empty() || draft.title.chars().count() > MAX_TITLE {
            bail!("a title is 1 to {MAX_TITLE} characters");
        }
        if draft.description.chars().count() > MAX_DESCRIPTION {
            bail!("a description is at most {MAX_DESCRIPTION} characters");
        }
        if !CATEGORIES.contains(&draft.category.as_str()) {
            bail!("pick one of the categories: {}", CATEGORIES.join(", "));
        }
        if draft.price == 0 {
            bail!("a price is more than nothing");
        }
        if draft.unit.chars().count() > MAX_UNIT {
            bail!("the price's unit is at most {MAX_UNIT} characters");
        }
        orien_address(&draft.pay_to)?;
        Ok(draft)
    }
}

pub struct Store {
    pub(crate) db: Connection,
}

impl Store {
    pub fn open(path: &Path) -> Result<Store> {
        Store::with(Connection::open(path)?)
    }

    pub fn in_memory() -> Result<Store> {
        Store::with(Connection::open_in_memory()?)
    }

    fn with(db: Connection) -> Result<Store> {
        db.execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA foreign_keys = ON;
             CREATE TABLE IF NOT EXISTS accounts (
                 id INTEGER PRIMARY KEY,
                 name TEXT NOT NULL,
                 -- The id passkeys know the account by, which says nothing about it.
                 handle TEXT NOT NULL UNIQUE,
                 -- How the account signs in, lowercase; none for one that uses passkeys only.
                 email TEXT UNIQUE,
                 bio TEXT NOT NULL DEFAULT '',
                 picture TEXT,
                 wallet TEXT UNIQUE,
                 contact_email TEXT NOT NULL DEFAULT '',
                 telegram TEXT NOT NULL DEFAULT '',
                 x TEXT NOT NULL DEFAULT '',
                 created INTEGER NOT NULL
             );
             CREATE TABLE IF NOT EXISTS verifications (
                 account INTEGER PRIMARY KEY REFERENCES accounts(id) ON DELETE CASCADE,
                 address TEXT NOT NULL,
                 amount INTEGER NOT NULL UNIQUE,
                 epoch INTEGER NOT NULL,
                 expires INTEGER NOT NULL
             );
             CREATE TABLE IF NOT EXISTS codes (
                 email TEXT PRIMARY KEY,
                 code TEXT NOT NULL,
                 sent INTEGER NOT NULL,
                 tries INTEGER NOT NULL
             );
             CREATE TABLE IF NOT EXISTS passkeys (
                 credential TEXT PRIMARY KEY,
                 account INTEGER NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
                 key TEXT NOT NULL,
                 created INTEGER NOT NULL
             );
             CREATE TABLE IF NOT EXISTS sessions (
                 token TEXT PRIMARY KEY,
                 account INTEGER NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
                 expires INTEGER NOT NULL
             );
             CREATE TABLE IF NOT EXISTS listings (
                 id INTEGER PRIMARY KEY,
                 seller INTEGER NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
                 title TEXT NOT NULL,
                 description TEXT NOT NULL,
                 category TEXT NOT NULL,
                 price INTEGER NOT NULL,
                 unit TEXT NOT NULL,
                 pay_to TEXT NOT NULL,
                 created INTEGER NOT NULL,
                 updated INTEGER NOT NULL,
                 open INTEGER NOT NULL
             );
             CREATE INDEX IF NOT EXISTS listings_open ON listings(open, updated);
             CREATE TABLE IF NOT EXISTS conversations (
                 id INTEGER PRIMARY KEY,
                 listing INTEGER NOT NULL REFERENCES listings(id) ON DELETE CASCADE,
                 buyer INTEGER NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
                 seller INTEGER NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
                 created INTEGER NOT NULL,
                 updated INTEGER NOT NULL,
                 -- The last message each side has read.
                 buyer_read INTEGER NOT NULL DEFAULT 0,
                 seller_read INTEGER NOT NULL DEFAULT 0,
                 -- Whether each side has been emailed since they last read.
                 buyer_told INTEGER NOT NULL DEFAULT 0,
                 seller_told INTEGER NOT NULL DEFAULT 0,
                 UNIQUE (listing, buyer)
             );
             CREATE TABLE IF NOT EXISTS messages (
                 id INTEGER PRIMARY KEY,
                 conversation INTEGER NOT NULL REFERENCES conversations(id) ON DELETE CASCADE,
                 sender INTEGER NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
                 body TEXT NOT NULL,
                 created INTEGER NOT NULL
             );
             CREATE INDEX IF NOT EXISTS messages_by_conversation ON messages(conversation, id);
             CREATE INDEX IF NOT EXISTS messages_by_sender ON messages(sender, created);
             CREATE TABLE IF NOT EXISTS blocks (
                 blocker INTEGER NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
                 blocked INTEGER NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
                 PRIMARY KEY (blocker, blocked)
             );",
        )?;
        Ok(Store { db })
    }

    /// A new account. `handle` is the id its passkeys know it by.
    pub fn create_account(&self, name: &str, handle: &str) -> Result<Account> {
        self.insert_account(name, handle, None)
    }

    fn insert_account(&self, name: &str, handle: &str, email: Option<&str>) -> Result<Account> {
        let name = clean(name);
        if name.is_empty() || name.chars().count() > MAX_NAME {
            bail!("a name is 1 to {MAX_NAME} characters");
        }
        let created = now();
        self.db.execute(
            "INSERT INTO accounts (name, handle, email, created) VALUES (?1, ?2, ?3, ?4)",
            params![name, handle, email, created],
        )?;
        Ok(Account { id: self.db.last_insert_rowid(), name, bio: String::new(), picture: None, wallet: None, contact: Contact::default(), created })
    }

    /// A new account that signs in by email. The email must be one a code
    /// has just proven.
    pub fn create_account_with_email(&self, name: &str, email: &str) -> Result<Account> {
        self.insert_account(name, &hex::encode(random::<16>()?), Some(email))
    }

    pub fn account_by_email(&self, email: &str) -> Result<Option<i64>> {
        Ok(self.db.query_row("SELECT id FROM accounts WHERE email = ?1", [email], |row| row.get(0)).optional()?)
    }

    /// A new sign-in code for `email`, to send there; none when one was sent
    /// in the last minute, which still works.
    pub fn issue_code(&self, email: &str) -> Result<Option<String>> {
        let at = now();
        let last: Option<u64> =
            self.db.query_row("SELECT sent FROM codes WHERE email = ?1", [email], |row| row.get(0)).optional()?;
        if last.is_some_and(|sent| at < sent + CODE_GAP) {
            return Ok(None);
        }
        let number = u32::from_le_bytes(random::<4>()?) % 1_000_000;
        let code = format!("{number:06}");
        self.db.execute(
            "INSERT OR REPLACE INTO codes (email, code, sent, tries) VALUES (?1, ?2, ?3, 0)",
            params![email, code_hash(email, &code), at],
        )?;
        Ok(Some(code))
    }

    /// Checks a sign-in code. Each code allows a few tries and lasts a few
    /// minutes. A right code is used up when `consume` is set.
    pub fn check_code(&self, email: &str, code: &str, consume: bool) -> Result<()> {
        let found: Option<(String, u64, u32)> = self
            .db
            .query_row("SELECT code, sent, tries FROM codes WHERE email = ?1", [email], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?))
            })
            .optional()?;
        let Some((kept, sent, tries)) = found else { bail!("ask for a code first") };
        if now() > sent + CODE_SECONDS || tries >= CODE_TRIES {
            self.db.execute("DELETE FROM codes WHERE email = ?1", [email])?;
            bail!("that code has run out; ask for a new one");
        }
        if kept != code_hash(email, code.trim()) {
            self.db.execute("UPDATE codes SET tries = tries + 1 WHERE email = ?1", [email])?;
            bail!("that code is not right; check it and try again");
        }
        if consume {
            self.db.execute("DELETE FROM codes WHERE email = ?1", [email])?;
        }
        Ok(())
    }

    /// Changes an account's name and bio, tidied, or says why not.
    pub fn set_profile(&self, id: i64, name: &str, bio: &str, contact: &Contact) -> Result<Account> {
        let (name, bio) = profile_text(name, bio)?;
        let contact = contact.checked()?;
        self.db.execute(
            "UPDATE accounts SET name = ?2, bio = ?3, contact_email = ?4, telegram = ?5, x = ?6 WHERE id = ?1",
            params![id, name, bio, contact.email, contact.telegram, contact.x],
        )?;
        self.account(id)?.ok_or_else(|| anyhow::anyhow!("no such account"))
    }

    /// Sets an account's picture, and returns the one it replaces, whose
    /// file can go.
    pub fn set_picture(&self, id: i64, picture: &str) -> Result<Option<String>> {
        let old: Option<String> =
            self.db.query_row("SELECT picture FROM accounts WHERE id = ?1", [id], |row| row.get(0))?;
        self.db.execute("UPDATE accounts SET picture = ?2 WHERE id = ?1", params![id, picture])?;
        Ok(old)
    }

    /// A seller's open listings, newest first, for their profile.
    pub fn open_listings_of(&self, seller: i64) -> Result<Vec<Listing>> {
        let sql = format!("{} WHERE l.seller = ?1 AND l.open = 1 ORDER BY l.updated DESC", Store::LISTING);
        let mut statement = self.db.prepare(&sql)?;
        let rows = statement.query_map([seller], Store::row)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// Starts checking that `account` holds `address`: returns the exact
    /// amount, in ORN's smallest units, to send from it. Each waiting check
    /// has its own amount, so a payment names one check only.
    pub fn start_verification(&self, account: i64, address: &str, epoch: u64) -> Result<u64> {
        let address = orien_address(address)?;
        let taken: Option<i64> =
            self.db.query_row("SELECT id FROM accounts WHERE wallet = ?1", [&address], |row| row.get(0)).optional()?;
        if taken.is_some_and(|other| other != account) {
            bail!("that wallet is already linked to another marketplace account");
        }
        self.db.execute("DELETE FROM verifications WHERE expires < ?1 OR account = ?2", params![now(), account])?;
        loop {
            // Between 0.001 and 0.01 ORN.
            let amount = 1_000_000 + u64::from_le_bytes(random::<8>()?) % 9_000_000;
            let added = self.db.execute(
                "INSERT OR IGNORE INTO verifications (account, address, amount, epoch, expires) VALUES (?1, ?2, ?3, ?4, ?5)",
                params![account, address, amount as i64, epoch, now() + VERIFY_SECONDS],
            )?;
            if added == 1 {
                return Ok(amount);
            }
        }
    }

    /// The check under way for `account`: the address, the amount to send,
    /// and the epoch it began in.
    pub fn verification(&self, account: i64) -> Result<Option<(String, u64, u64)>> {
        Ok(self
            .db
            .query_row(
                "SELECT address, amount, epoch FROM verifications WHERE account = ?1 AND expires >= ?2",
                params![account, now()],
                |row| Ok((row.get(0)?, row.get::<_, i64>(1)? as u64, row.get(2)?)),
            )
            .optional()?)
    }

    /// Links a proven wallet to `account`. Its listings are paid there from
    /// now on.
    pub fn set_wallet(&self, account: i64, address: &str) -> Result<()> {
        let address = orien_address(address)?;
        self.db.execute("UPDATE accounts SET wallet = ?2 WHERE id = ?1", params![account, address])?;
        self.db.execute("UPDATE listings SET pay_to = ?2 WHERE seller = ?1", params![account, address])?;
        self.db.execute("DELETE FROM verifications WHERE account = ?1", [account])?;
        Ok(())
    }

    fn wallet_of(&self, account: i64) -> Result<String> {
        let wallet: Option<String> =
            self.db.query_row("SELECT wallet FROM accounts WHERE id = ?1", [account], |row| row.get(0))?;
        wallet.ok_or_else(|| anyhow::anyhow!("verify your Orien wallet first, under Your profile, so buyers' payments reach you"))
    }

    #[doc(hidden)]
    pub fn forget_codes_for_tests(&self) {
        self.db.execute("DELETE FROM codes", []).unwrap();
    }

    pub fn account(&self, id: i64) -> Result<Option<Account>> {
        Ok(self
            .db
            .query_row(
                "SELECT id, name, bio, picture, wallet, created, contact_email, telegram, x FROM accounts WHERE id = ?1",
                [id],
                |row| {
                    Ok(Account {
                        id: row.get(0)?,
                        name: row.get(1)?,
                        bio: row.get(2)?,
                        picture: row.get(3)?,
                        wallet: row.get(4)?,
                        contact: Contact { email: row.get(6)?, telegram: row.get(7)?, x: row.get(8)? },
                        created: row.get(5)?,
                    })
                },
            )
            .optional()?)
    }

    /// Keeps a passkey for an account: `credential` is its id, `key` the
    /// stored public part, both as the passkey library writes them.
    pub fn add_passkey(&self, account: i64, credential: &str, key: &str) -> Result<()> {
        self.db.execute(
            "INSERT INTO passkeys (credential, account, key, created) VALUES (?1, ?2, ?3, ?4)",
            params![credential, account, key, now()],
        )?;
        Ok(())
    }

    /// The account a passkey belongs to, with the key as stored.
    pub fn passkey(&self, credential: &str) -> Result<Option<(i64, String)>> {
        Ok(self
            .db
            .query_row("SELECT account, key FROM passkeys WHERE credential = ?1", [credential], |row| {
                Ok((row.get(0)?, row.get(1)?))
            })
            .optional()?)
    }

    /// Stores a passkey again after use: its counter moves on.
    pub fn update_passkey(&self, credential: &str, key: &str) -> Result<()> {
        self.db.execute("UPDATE passkeys SET key = ?2 WHERE credential = ?1", params![credential, key])?;
        Ok(())
    }

    /// Signs an account in: returns the secret token the browser keeps.
    /// Only its hash is stored, so a copy of the database signs nobody in.
    pub fn start_session(&self, account: i64) -> Result<String> {
        let token = hex::encode(random::<32>()?);
        self.db.execute(
            "INSERT INTO sessions (token, account, expires) VALUES (?1, ?2, ?3)",
            params![token_hash(&token), account, now() + SESSION_SECONDS],
        )?;
        self.db.execute("DELETE FROM sessions WHERE expires < ?1", [now()])?;
        Ok(token)
    }

    /// The account a browser's token signs in, if it is still good.
    pub fn session(&self, token: &str) -> Result<Option<i64>> {
        Ok(self
            .db
            .query_row(
                "SELECT account FROM sessions WHERE token = ?1 AND expires >= ?2",
                params![token_hash(token), now()],
                |row| row.get(0),
            )
            .optional()?)
    }

    pub fn end_session(&self, token: &str) -> Result<()> {
        self.db.execute("DELETE FROM sessions WHERE token = ?1", [token_hash(token)])?;
        Ok(())
    }

    /// Posts a listing, paid to the seller's verified wallet.
    pub fn post(&self, seller: i64, draft: &Draft) -> Result<Listing> {
        let draft = Draft { pay_to: self.wallet_of(seller)?, ..draft.clone() }.checked()?;
        let open: i64 =
            self.db.query_row("SELECT COUNT(*) FROM listings WHERE seller = ?1 AND open = 1", [seller], |row| row.get(0))?;
        if open >= MAX_OPEN_LISTINGS {
            bail!("an account can have {MAX_OPEN_LISTINGS} listings open at once");
        }
        let at = now();
        self.db.execute(
            "INSERT INTO listings (seller, title, description, category, price, unit, pay_to, created, updated, open)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?8, 1)",
            params![seller, draft.title, draft.description, draft.category, draft.price as i64, draft.unit, draft.pay_to, at],
        )?;
        Ok(self.listing(self.db.last_insert_rowid())?.expect("just written"))
    }

    /// Changes a listing; only its seller may.
    pub fn edit(&self, seller: i64, id: i64, draft: &Draft) -> Result<Listing> {
        let draft = Draft { pay_to: self.wallet_of(seller)?, ..draft.clone() }.checked()?;
        let changed = self.db.execute(
            "UPDATE listings SET title = ?3, description = ?4, category = ?5, price = ?6, unit = ?7, pay_to = ?8,
                 updated = ?9
             WHERE id = ?1 AND seller = ?2",
            params![id, seller, draft.title, draft.description, draft.category, draft.price as i64, draft.unit, draft.pay_to, now()],
        )?;
        if changed == 0 {
            bail!("no such listing of yours");
        }
        Ok(self.listing(id)?.expect("just written"))
    }

    /// Opens or closes a listing; only its seller may. A closed listing
    /// stays, for its reviews, but is not shown in the marketplace.
    pub fn set_open(&self, seller: i64, id: i64, open: bool) -> Result<()> {
        let changed = self.db.execute(
            "UPDATE listings SET open = ?3, updated = ?4 WHERE id = ?1 AND seller = ?2",
            params![id, seller, open, now()],
        )?;
        if changed == 0 {
            bail!("no such listing of yours");
        }
        Ok(())
    }

    const LISTING: &'static str = "SELECT l.id, l.seller, a.name, a.picture, l.title, l.description, l.category, l.price, l.unit,
             l.pay_to, l.created, l.updated, l.open
         FROM listings l JOIN accounts a ON a.id = l.seller";

    fn row(row: &rusqlite::Row) -> rusqlite::Result<Listing> {
        Ok(Listing {
            id: row.get(0)?,
            seller: row.get(1)?,
            seller_name: row.get(2)?,
            seller_picture: row.get(3)?,
            title: row.get(4)?,
            description: row.get(5)?,
            category: row.get(6)?,
            price: row.get::<_, i64>(7)? as u64,
            unit: row.get(8)?,
            pay_to: row.get(9)?,
            created: row.get(10)?,
            updated: row.get(11)?,
            open: row.get(12)?,
        })
    }

    pub fn listing(&self, id: i64) -> Result<Option<Listing>> {
        let sql = format!("{} WHERE l.id = ?1", Store::LISTING);
        Ok(self.db.query_row(&sql, [id], Store::row).optional()?)
    }

    /// Open listings, newest first: in one category or all, matching a
    /// search in the title or description if one is given.
    pub fn browse(&self, category: Option<&str>, search: Option<&str>, limit: u32, offset: u32) -> Result<Vec<Listing>> {
        let sql = format!(
            "{} WHERE l.open = 1 AND (?1 IS NULL OR l.category = ?1)
                 AND (?2 IS NULL OR l.title LIKE ?2 ESCAPE '\\' OR l.description LIKE ?2 ESCAPE '\\')
             ORDER BY l.updated DESC, l.id DESC LIMIT ?3 OFFSET ?4",
            Store::LISTING
        );
        let pattern = search.map(str::trim).filter(|s| !s.is_empty()).map(|s| {
            let escaped = s.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_");
            format!("%{escaped}%")
        });
        let mut statement = self.db.prepare(&sql)?;
        let rows = statement.query_map(params![category, pattern, limit.min(100), offset], Store::row)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// Every listing of one seller, open ones first.
    pub fn listings_of(&self, seller: i64) -> Result<Vec<Listing>> {
        let sql = format!("{} WHERE l.seller = ?1 ORDER BY l.open DESC, l.updated DESC", Store::LISTING);
        let mut statement = self.db.prepare(&sql)?;
        let rows = statement.query_map([seller], Store::row)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }
}

fn random<const N: usize>() -> Result<[u8; N]> {
    let mut bytes = [0u8; N];
    getrandom::fill(&mut bytes).map_err(|e| anyhow::anyhow!("no randomness: {e}"))?;
    Ok(bytes)
}

fn code_hash(email: &str, code: &str) -> String {
    blake3::hash(format!("{email}\n{code}").as_bytes()).to_hex().to_string()
}

/// An Orien address tidied, or why it is not one.
pub fn orien_address(text: &str) -> Result<String> {
    let address = text.trim().to_lowercase();
    if address.len() != 64 || !address.bytes().all(|b| b.is_ascii_hexdigit()) {
        bail!("an Orien address is 64 letters and digits, 0-9 and a-f");
    }
    Ok(address)
}

/// A name and bio tidied, or why they cannot be used.
pub fn profile_text(name: &str, bio: &str) -> Result<(String, String)> {
    let (name, bio) = (clean(name), clean(bio));
    if name.is_empty() || name.chars().count() > MAX_NAME {
        bail!("a name is 1 to {MAX_NAME} characters");
    }
    if bio.chars().count() > MAX_BIO {
        bail!("a bio is at most {MAX_BIO} characters");
    }
    Ok((name, bio))
}

/// An email address, tidied, or why it is not one.
pub fn email(text: &str) -> Result<String> {
    let email = text.trim().to_lowercase();
    let good = email.len() <= 254
        && !email.chars().any(|c| c.is_whitespace() || c.is_control())
        && email.split_once('@').is_some_and(|(local, domain)| {
            !local.is_empty() && !domain.contains('@') && domain.contains('.') && !domain.starts_with('.') && !domain.ends_with('.')
        });
    if !good {
        bail!("that does not look like an email address");
    }
    Ok(email)
}

fn token_hash(token: &str) -> String {
    blake3::hash(token.as_bytes()).to_hex().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn draft(title: &str) -> Draft {
        Draft {
            title: title.into(),
            description: "Clean, simple logos.".into(),
            category: "design".into(),
            price: 200_000_000_000,
            unit: String::new(),
            pay_to: "be5b7cc81cbe475fe002bee2597c30b0ae4f030434e93f12ade7f1a266c6258b".into(),
        }
    }

    #[test]
    fn a_seller_posts_edits_and_closes_a_listing() {
        let store = Store::in_memory().unwrap();
        let maya = store.create_account("Maya", "maya").unwrap();
        let sam = store.create_account("Sam", "sam").unwrap();
        assert!(store.post(maya.id, &draft("Logo design")).is_err(), "a seller verifies a wallet first");
        store.set_wallet(maya.id, &"a".repeat(64)).unwrap();
        store.set_wallet(sam.id, &"b".repeat(64)).unwrap();

        let posted = store.post(maya.id, &draft("Logo design")).unwrap();
        assert_eq!((posted.seller_name.as_str(), posted.open), ("Maya", true));
        assert_eq!(store.browse(None, None, 10, 0).unwrap(), vec![posted.clone()]);
        assert_eq!(store.browse(Some("teaching"), None, 10, 0).unwrap(), vec![]);
        assert_eq!(store.browse(None, Some("LOGO"), 10, 0).unwrap().len(), 1);
        assert_eq!(store.browse(None, Some("%"), 10, 0).unwrap().len(), 0, "wildcards are taken literally");

        assert!(store.edit(sam.id, posted.id, &draft("Mine now")).is_err(), "only the seller edits");
        assert!(store.set_open(sam.id, posted.id, false).is_err());
        let edited = store.edit(maya.id, posted.id, &draft("Logo and icon design")).unwrap();
        assert_eq!(edited.title, "Logo and icon design");

        store.set_open(maya.id, posted.id, false).unwrap();
        assert!(store.browse(None, None, 10, 0).unwrap().is_empty());
        assert_eq!(store.listings_of(maya.id).unwrap().len(), 1, "a closed listing stays for its seller");
    }

    #[test]
    fn listings_are_checked() {
        let store = Store::in_memory().unwrap();
        let maya = store.create_account("Maya", "maya").unwrap();
        store.set_wallet(maya.id, &"a".repeat(64)).unwrap();
        store.set_profile(maya.id, "Maya", "", &Contact { x: "maya".into(), ..Contact::default() }).unwrap();
        let bad = |change: fn(&mut Draft)| {
            let mut d = draft("Logo design");
            change(&mut d);
            store.post(maya.id, &d).is_err()
        };
        assert!(bad(|d| d.title = "   ".into()));
        assert!(bad(|d| d.title = "x".repeat(101)));
        assert!(bad(|d| d.category = "weapons".into()));
        assert!(bad(|d| d.price = 0));
        assert!(!bad(|d| d.pay_to = "somewhere else".into()), "listings are paid to the verified wallet, whatever is sent");
    }

    #[test]
    fn a_code_signs_in_once_within_its_tries() {
        let store = Store::in_memory().unwrap();
        let maya = email(" Maya@Example.com ").unwrap();
        assert_eq!(maya, "maya@example.com");
        assert!(email("not an email").is_err() && email("a@b").is_err() && email("a b@c.com").is_err());
        assert!(store.check_code(&maya, "123456", true).is_err(), "no code asked for");
        let code = store.issue_code(&maya).unwrap().unwrap();
        assert_eq!(code.len(), 6);
        assert_eq!(store.issue_code(&maya).unwrap(), None, "one code a minute");
        let wrong = if code == "000000" { "111111" } else { "000000" };
        assert!(store.check_code(&maya, wrong, true).is_err());
        store.check_code(&maya, &code, false).unwrap();
        store.check_code(&maya, &code, true).unwrap();
        assert!(store.check_code(&maya, &code, true).is_err(), "a code works once");

        store.db.execute("DELETE FROM codes", []).unwrap();
        let code = store.issue_code(&maya).unwrap().unwrap();
        for _ in 0..CODE_TRIES {
            let _ = store.check_code(&maya, wrong, true);
        }
        assert!(store.check_code(&maya, &code, true).is_err(), "too many wrong tries use the code up");

        let account = store.create_account_with_email("Maya", &maya).unwrap();
        assert_eq!(store.account_by_email(&maya).unwrap(), Some(account.id));
        assert!(store.create_account_with_email("Other", &maya).is_err(), "one account per email");
    }

    #[test]
    fn a_wallet_is_linked_once_proven_and_to_one_account_only() {
        let store = Store::in_memory().unwrap();
        let maya = store.create_account("Maya", "maya").unwrap();
        let sam = store.create_account("Sam", "sam").unwrap();
        let wallet = "c".repeat(64);
        assert!(store.start_verification(maya.id, "nonsense", 3).is_err());
        let amount = store.start_verification(maya.id, &wallet.to_uppercase(), 3).unwrap();
        assert!((1_000_000..10_000_000).contains(&amount));
        assert_eq!(store.verification(maya.id).unwrap(), Some((wallet.clone(), amount, 3)));
        let other = store.start_verification(sam.id, &wallet, 3).unwrap();
        assert_ne!(other, amount, "each waiting check has its own amount");
        store.set_wallet(maya.id, &wallet).unwrap();
        assert_eq!(store.account(maya.id).unwrap().unwrap().wallet, Some(wallet.clone()));
        assert_eq!(store.verification(maya.id).unwrap(), None);
        assert!(store.start_verification(sam.id, &wallet, 4).is_err(), "a wallet belongs to one account");
        assert!(store.set_wallet(sam.id, &wallet).is_err());
    }

    #[test]
    fn contact_details_are_tidied_or_refused() {
        let typed = Contact {
            email: " Maya@Example.com ".into(),
            telegram: "https://t.me/maya_designs".into(),
            x: "@MayaLin".into(),
        };
        let tidy = typed.checked().unwrap();
        assert_eq!((tidy.email.as_str(), tidy.telegram.as_str(), tidy.x.as_str()), ("maya@example.com", "maya_designs", "MayaLin"));
        assert_eq!(Contact { x: "https://x.com/maya/".into(), ..Contact::default() }.checked().unwrap().x, "maya");
        assert!(Contact { telegram: "abc".into(), ..Contact::default() }.checked().is_err());
        assert!(Contact { x: "way_too_long_for_x_names".into(), ..Contact::default() }.checked().is_err());
        assert!(Contact { x: "no spaces".into(), ..Contact::default() }.checked().is_err());
        assert!(Contact { email: "nope".into(), ..Contact::default() }.checked().is_err());
        assert!(Contact::default().checked().unwrap().is_empty());
    }

    #[test]
    fn a_session_signs_in_until_it_ends_and_only_its_hash_is_kept() {
        let store = Store::in_memory().unwrap();
        let maya = store.create_account("Maya", "maya").unwrap();
        let token = store.start_session(maya.id).unwrap();
        assert_eq!(store.session(&token).unwrap(), Some(maya.id));
        assert_eq!(store.session("guess").unwrap(), None);
        let kept: String = store.db.query_row("SELECT token FROM sessions", [], |row| row.get(0)).unwrap();
        assert_ne!(kept, token);
        store.end_session(&token).unwrap();
        assert_eq!(store.session(&token).unwrap(), None);
    }
}
