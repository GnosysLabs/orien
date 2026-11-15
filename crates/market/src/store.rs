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
pub const MAX_TITLE: usize = 100;
pub const MAX_DESCRIPTION: usize = 5000;
pub const MAX_UNIT: usize = 30;
pub const MAX_CONTACT: usize = 300;
/// Listings one account may have open at a time.
pub const MAX_OPEN_LISTINGS: i64 = 50;

pub const CATEGORIES: &[&str] = &["design", "writing", "software", "teaching", "goods", "other"];

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
    /// The Orien address payments go to.
    pub pay_to: String,
    /// How to reach the seller, for now.
    #[serde(default)]
    pub contact: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Listing {
    pub id: i64,
    pub seller: i64,
    pub seller_name: String,
    pub title: String,
    pub description: String,
    pub category: String,
    pub price: u64,
    pub unit: String,
    pub pay_to: String,
    pub contact: String,
    pub created: u64,
    pub updated: u64,
    pub open: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Account {
    pub id: i64,
    pub name: String,
    pub created: u64,
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
            contact: clean(&self.contact),
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
        if draft.unit.chars().count() > MAX_UNIT || draft.contact.chars().count() > MAX_CONTACT {
            bail!("the price's unit is at most {MAX_UNIT} characters, and contact details at most {MAX_CONTACT}");
        }
        if draft.pay_to.len() != 64 || !draft.pay_to.bytes().all(|b| b.is_ascii_hexdigit()) {
            bail!("an Orien address is 64 letters and digits, 0-9 and a-f");
        }
        Ok(draft)
    }
}

pub struct Store {
    db: Connection,
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
                 created INTEGER NOT NULL
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
                 contact TEXT NOT NULL,
                 created INTEGER NOT NULL,
                 updated INTEGER NOT NULL,
                 open INTEGER NOT NULL
             );
             CREATE INDEX IF NOT EXISTS listings_open ON listings(open, updated);",
        )?;
        Ok(Store { db })
    }

    pub fn create_account(&self, name: &str) -> Result<Account> {
        let name = clean(name);
        if name.is_empty() || name.chars().count() > MAX_NAME {
            bail!("a name is 1 to {MAX_NAME} characters");
        }
        let created = now();
        self.db.execute("INSERT INTO accounts (name, created) VALUES (?1, ?2)", params![name, created])?;
        Ok(Account { id: self.db.last_insert_rowid(), name, created })
    }

    pub fn account(&self, id: i64) -> Result<Option<Account>> {
        Ok(self
            .db
            .query_row("SELECT id, name, created FROM accounts WHERE id = ?1", [id], |row| {
                Ok(Account { id: row.get(0)?, name: row.get(1)?, created: row.get(2)? })
            })
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
        let mut secret = [0u8; 32];
        getrandom::fill(&mut secret).map_err(|e| anyhow::anyhow!("no randomness: {e}"))?;
        let token = hex::encode(secret);
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

    pub fn post(&self, seller: i64, draft: &Draft) -> Result<Listing> {
        let draft = draft.checked()?;
        let open: i64 =
            self.db.query_row("SELECT COUNT(*) FROM listings WHERE seller = ?1 AND open = 1", [seller], |row| row.get(0))?;
        if open >= MAX_OPEN_LISTINGS {
            bail!("an account can have {MAX_OPEN_LISTINGS} listings open at once");
        }
        let at = now();
        self.db.execute(
            "INSERT INTO listings (seller, title, description, category, price, unit, pay_to, contact, created, updated, open)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?9, 1)",
            params![seller, draft.title, draft.description, draft.category, draft.price as i64, draft.unit, draft.pay_to, draft.contact, at],
        )?;
        Ok(self.listing(self.db.last_insert_rowid())?.expect("just written"))
    }

    /// Changes a listing; only its seller may.
    pub fn edit(&self, seller: i64, id: i64, draft: &Draft) -> Result<Listing> {
        let draft = draft.checked()?;
        let changed = self.db.execute(
            "UPDATE listings SET title = ?3, description = ?4, category = ?5, price = ?6, unit = ?7, pay_to = ?8,
                 contact = ?9, updated = ?10
             WHERE id = ?1 AND seller = ?2",
            params![id, seller, draft.title, draft.description, draft.category, draft.price as i64, draft.unit, draft.pay_to, draft.contact, now()],
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

    const LISTING: &'static str = "SELECT l.id, l.seller, a.name, l.title, l.description, l.category, l.price, l.unit,
             l.pay_to, l.contact, l.created, l.updated, l.open
         FROM listings l JOIN accounts a ON a.id = l.seller";

    fn row(row: &rusqlite::Row) -> rusqlite::Result<Listing> {
        Ok(Listing {
            id: row.get(0)?,
            seller: row.get(1)?,
            seller_name: row.get(2)?,
            title: row.get(3)?,
            description: row.get(4)?,
            category: row.get(5)?,
            price: row.get::<_, i64>(6)? as u64,
            unit: row.get(7)?,
            pay_to: row.get(8)?,
            contact: row.get(9)?,
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
            contact: "maya@example.com".into(),
        }
    }

    #[test]
    fn a_seller_posts_edits_and_closes_a_listing() {
        let store = Store::in_memory().unwrap();
        let maya = store.create_account("Maya").unwrap();
        let sam = store.create_account("Sam").unwrap();
        let posted = store.post(maya.id, &draft("Logo design")).unwrap();
        assert_eq!((posted.seller_name.as_str(), posted.open), ("Maya", true));
        assert_eq!(store.browse(None, None, 10, 0).unwrap(), vec![posted.clone()]);
        assert_eq!(store.browse(Some("writing"), None, 10, 0).unwrap(), vec![]);
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
        let maya = store.create_account("Maya").unwrap();
        let bad = |change: fn(&mut Draft)| {
            let mut d = draft("Logo design");
            change(&mut d);
            store.post(maya.id, &d).is_err()
        };
        assert!(bad(|d| d.title = "   ".into()));
        assert!(bad(|d| d.title = "x".repeat(101)));
        assert!(bad(|d| d.category = "weapons".into()));
        assert!(bad(|d| d.price = 0));
        assert!(bad(|d| d.pay_to = "not an address".into()));
        assert!(!bad(|d| d.pay_to = d.pay_to.to_uppercase()), "an address in capitals is the same address");
    }

    #[test]
    fn a_session_signs_in_until_it_ends_and_only_its_hash_is_kept() {
        let store = Store::in_memory().unwrap();
        let maya = store.create_account("Maya").unwrap();
        let token = store.start_session(maya.id).unwrap();
        assert_eq!(store.session(&token).unwrap(), Some(maya.id));
        assert_eq!(store.session("guess").unwrap(), None);
        let kept: String = store.db.query_row("SELECT token FROM sessions", [], |row| row.get(0)).unwrap();
        assert_ne!(kept, token);
        store.end_session(&token).unwrap();
        assert_eq!(store.session(&token).unwrap(), None);
    }
}
