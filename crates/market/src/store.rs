//! Everything the marketplace keeps, in one SQLite file: accounts, their
//! signed-in sessions and listings.

use anyhow::{Result, bail};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

/// How long a sign-in lasts unused. Each use renews it, so a browser or
/// wallet in regular use stays signed in.
pub const SESSION_SECONDS: u64 = 30 * 24 * 3600;
/// How long the browser keeps the sign-in cookie: as long as browsers allow.
/// The server's own record, which lapses after a month unused, decides.
pub const COOKIE_SECONDS: u64 = 400 * 24 * 3600;

/// A name is a username: 3 to 20 letters, digits or underscores, unique
/// whatever the case, so it can be a profile's address (/u/Cyph3r) and
/// nobody can copy someone's with lookalike letters.
pub const MIN_NAME: usize = 3;
pub const MAX_NAME: usize = 20;
pub const MAX_BIO: usize = 1000;
pub const MAX_TITLE: usize = 100;
pub const MAX_DESCRIPTION: usize = 5000;
pub const MAX_UNIT: usize = 30;
/// Pictures one listing may have.
pub const MAX_LISTING_PICTURES: i64 = 4;
/// Listings one account may have open at a time.
pub const MAX_OPEN_LISTINGS: i64 = 50;

/// How long a sign-in code works, how many tries it allows, and how long
/// before another can be sent.
pub const CODE_SECONDS: u64 = 15 * 60;
pub const CODE_TRIES: u32 = 5;
pub const CODE_GAP: u64 = 60;

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
    /// File names of its pictures, the cover first.
    pub pictures: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Account {
    pub id: i64,
    pub name: String,
    /// A few lines about the seller, shown on their profile.
    pub bio: String,
    /// The file name of their picture, under /api/market/images/.
    pub picture: Option<String>,
    /// The file name of their profile's wide banner, the same way.
    pub banner: Option<String>,
    /// The Orien address their listings are paid to.
    pub wallet: Option<String>,
    /// Whether that wallet proved it is theirs, by a small payment to the
    /// Market's wallet.
    pub wallet_verified: bool,
    /// Other ways buyers can reach them, besides messages. Empty fields
    /// are left out.
    pub contact: Contact,
    pub created: u64,
    /// Who may start a direct conversation with them: one of MESSAGES_FROM.
    /// Messages about their listings always come through.
    pub messages_from: String,
}

/// Who may message an account: anyone, only people they have talked with
/// (a message either way, about a listing or not), or nobody.
pub const MESSAGES_FROM: [&str; 3] = ["everyone", "talked", "nobody"];

/// How buyers reach a seller besides messages: any of a Telegram, an X and
/// a Discord username. Messages are built in, so no email is shown.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Contact {
    #[serde(default)]
    pub telegram: String,
    #[serde(default)]
    pub x: String,
    #[serde(default)]
    pub discord: String,
}

impl Contact {
    pub fn is_empty(&self) -> bool {
        self.telegram.is_empty() && self.x.is_empty() && self.discord.is_empty()
    }

    /// Tidied: bare usernames, whether typed with an @ or as a link. Or why
    /// one cannot be used.
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
        let discord = self.discord.trim().trim_start_matches('@').to_lowercase();
        let word = |h: &str| h.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_');
        if !telegram.is_empty() && (!(5..=32).contains(&telegram.len()) || !word(&telegram)) {
            bail!("a Telegram username is 5 to 32 letters, digits and underscores");
        }
        if !x.is_empty() && (x.len() > 15 || !word(&x)) {
            bail!("an X username is up to 15 letters, digits and underscores");
        }
        if !discord.is_empty()
            && (!(2..=32).contains(&discord.len())
                || !discord.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'.')
                || discord.contains(".."))
        {
            bail!("a Discord username is 2 to 32 lowercase letters, digits, underscores and full stops");
        }
        Ok(Contact { telegram, x, discord })
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
        // Filled in from the seller's verified wallet when posted.
        if !draft.pay_to.is_empty() {
            orien_address(&draft.pay_to)?;
        }
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
                 -- A random id for the account, which says nothing about it.
                 handle TEXT NOT NULL UNIQUE,
                 -- How the account signs in, lowercase.
                 email TEXT UNIQUE,
                 bio TEXT NOT NULL DEFAULT '',
                 picture TEXT,
                 -- No longer used: wallets were once proven by a payment and
                 -- belonged to one account. They are in payout now.
                 wallet TEXT UNIQUE,
                 contact_email TEXT NOT NULL DEFAULT '',
                 telegram TEXT NOT NULL DEFAULT '',
                 x TEXT NOT NULL DEFAULT '',
                 created INTEGER NOT NULL
             );
             CREATE TABLE IF NOT EXISTS codes (
                 email TEXT PRIMARY KEY,
                 code TEXT NOT NULL,
                 sent INTEGER NOT NULL,
                 tries INTEGER NOT NULL
             );
             -- Passkey sign-in was never used and is gone.
             DROP TABLE IF EXISTS passkeys;
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
             CREATE TABLE IF NOT EXISTS listing_pictures (
                 listing INTEGER NOT NULL REFERENCES listings(id) ON DELETE CASCADE,
                 file TEXT NOT NULL,
                 position INTEGER NOT NULL,
                 PRIMARY KEY (listing, file)
             );
             CREATE TABLE IF NOT EXISTS conversations (
                 id INTEGER PRIMARY KEY,
                 -- NULL in a direct conversation, whose sides are stored
                 -- lowest id first.
                 listing INTEGER REFERENCES listings(id) ON DELETE CASCADE,
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
                 -- Who started it, for the daily limit.
                 starter INTEGER NOT NULL,
                 -- The last message each side had when they deleted the
                 -- conversation: they see only what came after.
                 buyer_cleared INTEGER NOT NULL DEFAULT 0,
                 seller_cleared INTEGER NOT NULL DEFAULT 0,
                 UNIQUE (listing, buyer)
             );
             CREATE TABLE IF NOT EXISTS messages (
                 id INTEGER PRIMARY KEY,
                 conversation INTEGER NOT NULL REFERENCES conversations(id) ON DELETE CASCADE,
                 sender INTEGER NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
                 body TEXT NOT NULL,
                 -- text, request (the seller asks for payment) or paid (the buyer says they paid).
                 kind TEXT NOT NULL DEFAULT 'text',
                 -- In ORN's smallest units, for requests and payments.
                 amount INTEGER,
                 created INTEGER NOT NULL
             );
             CREATE INDEX IF NOT EXISTS messages_by_conversation ON messages(conversation, id);
             CREATE INDEX IF NOT EXISTS messages_by_sender ON messages(sender, created);
             CREATE TABLE IF NOT EXISTS blocks (
                 blocker INTEGER NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
                 blocked INTEGER NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
                 -- When, for the list of people you've blocked (0: before it was kept).
                 at INTEGER NOT NULL DEFAULT 0,
                 PRIMARY KEY (blocker, blocked)
             );",
        )?;
        let store = Store { db };
        store.add_payout()?;
        store.create_swap_tables()?;
        store.create_vault_table()?;
        store.create_moderation_tables()?;
        store.create_push_tables()?;
        store.create_account_wallet_table()?;
        store.create_proof_table()?;
        store.create_oidc_tables()?;
        store.create_community_tables()?;
        store.create_notice_tables()?;
        store.create_room_tables()?;
        store.allow_direct_conversations()?;
        // Names are unique whatever their case.
        store.db.execute("CREATE UNIQUE INDEX IF NOT EXISTS accounts_name ON accounts (lower(name))", [])?;
        Ok(store)
    }

    /// Databases from before direct messages: conversations had to have a
    /// listing and did not record who started them. SQLite cannot drop NOT
    /// NULL, so the table is rebuilt once, keeping every id, with foreign keys off so its
    /// messages stay.
    fn allow_direct_conversations(&self) -> Result<()> {
        let has_starter: bool = self.db.query_row(
            "SELECT COUNT(*) > 0 FROM pragma_table_info('conversations') WHERE name = 'starter'",
            [],
            |row| row.get(0),
        )?;
        if !has_starter {
            self.db.execute_batch(
                "PRAGMA foreign_keys = OFF;
                 BEGIN;
                 CREATE TABLE conversations_new (
                     id INTEGER PRIMARY KEY,
                     -- NULL in a direct conversation, whose sides are stored
                     -- lowest id first.
                     listing INTEGER REFERENCES listings(id) ON DELETE CASCADE,
                     buyer INTEGER NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
                     seller INTEGER NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
                     created INTEGER NOT NULL,
                     updated INTEGER NOT NULL,
                     buyer_read INTEGER NOT NULL DEFAULT 0,
                     seller_read INTEGER NOT NULL DEFAULT 0,
                     buyer_told INTEGER NOT NULL DEFAULT 0,
                     seller_told INTEGER NOT NULL DEFAULT 0,
                     -- Who started it, for the daily limit.
                     starter INTEGER NOT NULL,
                     UNIQUE (listing, buyer)
                 );
                 INSERT INTO conversations_new
                     (id, listing, buyer, seller, created, updated, buyer_read, seller_read, buyer_told, seller_told, starter)
                     SELECT id, listing, buyer, seller, created, updated, buyer_read, seller_read, buyer_told, seller_told, buyer
                     FROM conversations;
                 DROP TABLE conversations;
                 ALTER TABLE conversations_new RENAME TO conversations;
                 COMMIT;
                 PRAGMA foreign_keys = ON;",
            )?;
        }
        let has_block_time: bool = self.db.query_row(
            "SELECT COUNT(*) > 0 FROM pragma_table_info('blocks') WHERE name = 'at'",
            [],
            |row| row.get(0),
        )?;
        if !has_block_time {
            self.db.execute("ALTER TABLE blocks ADD COLUMN at INTEGER NOT NULL DEFAULT 0", [])?;
        }
        let has_cleared: bool = self.db.query_row(
            "SELECT COUNT(*) > 0 FROM pragma_table_info('conversations') WHERE name = 'buyer_cleared'",
            [],
            |row| row.get(0),
        )?;
        if !has_cleared {
            self.db.execute_batch(
                "ALTER TABLE conversations ADD COLUMN buyer_cleared INTEGER NOT NULL DEFAULT 0;
                 ALTER TABLE conversations ADD COLUMN seller_cleared INTEGER NOT NULL DEFAULT 0;",
            )?;
        }
        self.db.execute_batch(
            "CREATE UNIQUE INDEX IF NOT EXISTS conversations_direct ON conversations (buyer, seller) WHERE listing IS NULL;
             CREATE INDEX IF NOT EXISTS conversations_by_starter ON conversations (starter, created);",
        )?;
        Ok(())
    }

    /// The wallet buyers pay, which any number of accounts may name. Copied
    /// once from the old `wallet` column, which allowed one account each.
    fn add_payout(&self) -> Result<()> {
        let has: bool = self.db.query_row(
            "SELECT COUNT(*) > 0 FROM pragma_table_info('accounts') WHERE name = 'payout'",
            [],
            |row| row.get(0),
        )?;
        if !has {
            self.db.execute_batch(
                "ALTER TABLE accounts ADD COLUMN payout TEXT;
                 UPDATE accounts SET payout = wallet;
                 DROP TABLE IF EXISTS verifications;",
            )?;
        }
        let has_banner: bool = self.db.query_row(
            "SELECT COUNT(*) > 0 FROM pragma_table_info('accounts') WHERE name = 'banner'",
            [],
            |row| row.get(0),
        )?;
        if !has_banner {
            self.db.execute("ALTER TABLE accounts ADD COLUMN banner TEXT", [])?;
        }
        // Profiles show a Discord username instead of a public email, which
        // messages made needless: the emails shown before are forgotten.
        let has_discord: bool = self.db.query_row(
            "SELECT COUNT(*) > 0 FROM pragma_table_info('accounts') WHERE name = 'discord'",
            [],
            |row| row.get(0),
        )?;
        if !has_discord {
            self.db.execute_batch(
                "ALTER TABLE accounts ADD COLUMN discord TEXT NOT NULL DEFAULT '';
                 UPDATE accounts SET contact_email = '';",
            )?;
        }
        let has_messages_from: bool = self.db.query_row(
            "SELECT COUNT(*) > 0 FROM pragma_table_info('accounts') WHERE name = 'messages_from'",
            [],
            |row| row.get(0),
        )?;
        if !has_messages_from {
            self.db.execute("ALTER TABLE accounts ADD COLUMN messages_from TEXT NOT NULL DEFAULT 'everyone'", [])?;
        }
        Ok(())
    }

    /// A new account. `handle` is a random id that says nothing about it.
    pub fn create_account(&self, name: &str, handle: &str) -> Result<Account> {
        self.insert_account(name, handle, None)
    }

    fn insert_account(&self, name: &str, handle: &str, email: Option<&str>) -> Result<Account> {
        let name = self.new_name(name, None)?;
        let created = now();
        self.db.execute(
            "INSERT INTO accounts (name, handle, email, created) VALUES (?1, ?2, ?3, ?4)",
            params![name, handle, email, created],
        )?;
        Ok(Account { id: self.db.last_insert_rowid(), name, bio: String::new(), picture: None, banner: None, wallet: None, wallet_verified: false, contact: Contact::default(), created, messages_from: MESSAGES_FROM[0].into() })
    }

    /// A new account that signs in by email. The email must be one a code
    /// has just proven.
    pub fn create_account_with_email(&self, name: &str, email: &str) -> Result<Account> {
        self.insert_account(name, &hex::encode(random::<16>()?), Some(email))
    }

    /// The email an account signs in with, if it has one.
    pub fn email_of(&self, account: i64) -> Result<Option<String>> {
        Ok(self.db.query_row("SELECT email FROM accounts WHERE id = ?1", [account], |row| row.get(0)).optional()?.flatten())
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

    /// `name` as a username nobody else has, `except` the account itself.
    pub fn new_name(&self, name: &str, except: Option<i64>) -> Result<String> {
        let name = username(name)?;
        // Names that could pass for Orien itself are kept back, unless it's
        // the name the account already has.
        let current: Option<String> = match except {
            Some(id) => self.db.query_row("SELECT name FROM accounts WHERE id = ?1", [id], |row| row.get(0)).optional()?,
            None => None,
        };
        if reserved(&name) && current.is_none_or(|c| !c.eq_ignore_ascii_case(&name)) {
            bail!("that name is kept for Orien itself; pick another");
        }
        let taken: bool = self.db.query_row(
            "SELECT COUNT(*) > 0 FROM accounts WHERE lower(name) = lower(?1) AND id IS NOT ?2",
            params![name, except],
            |row| row.get(0),
        )?;
        if taken {
            bail!("the name {name} is taken; try another");
        }
        Ok(name)
    }

    /// The account's random handle, which says nothing about it.
    pub fn handle_of(&self, account: i64) -> Result<String> {
        Ok(self.db.query_row("SELECT handle FROM accounts WHERE id = ?1", [account], |row| row.get(0))?)
    }

    /// The account with this name, whatever its case.
    pub fn account_by_name(&self, name: &str) -> Result<Option<i64>> {
        Ok(self.db.query_row("SELECT id FROM accounts WHERE lower(name) = lower(?1)", [name.trim()], |row| row.get(0)).optional()?)
    }

    /// Changes an account's name and bio, tidied, or says why not.
    pub fn set_profile(&self, id: i64, name: &str, bio: &str, contact: &Contact) -> Result<Account> {
        let (name, bio) = profile_text(name, bio)?;
        let name = self.new_name(&name, Some(id))?;
        let contact = contact.checked()?;
        self.db.execute(
            "UPDATE accounts SET name = ?2, bio = ?3, discord = ?4, telegram = ?5, x = ?6 WHERE id = ?1",
            params![id, name, bio, contact.discord, contact.telegram, contact.x],
        )?;
        self.account(id)?.ok_or_else(|| anyhow::anyhow!("no such account"))
    }

    /// Who may start a direct conversation with the account.
    pub fn set_messages_from(&self, id: i64, choice: &str) -> Result<()> {
        if !MESSAGES_FROM.contains(&choice) {
            bail!("who can message you is everyone, talked or nobody");
        }
        self.db.execute("UPDATE accounts SET messages_from = ?2 WHERE id = ?1", params![id, choice])?;
        Ok(())
    }

    /// Sets an account's picture, and returns the one it replaces, whose
    /// file can go.
    pub fn set_picture(&self, id: i64, picture: &str) -> Result<Option<String>> {
        let old: Option<String> =
            self.db.query_row("SELECT picture FROM accounts WHERE id = ?1", [id], |row| row.get(0))?;
        self.db.execute("UPDATE accounts SET picture = ?2 WHERE id = ?1", params![id, picture])?;
        Ok(old)
    }

    /// Takes an account's picture away (a moderator's call): it shows the
    /// robot face again. Returns the picture, whose file can go.
    pub fn clear_picture(&self, id: i64) -> Result<Option<String>> {
        let old: Option<String> =
            self.db.query_row("SELECT picture FROM accounts WHERE id = ?1", [id], |row| row.get(0)).optional()?.flatten();
        self.db.execute("UPDATE accounts SET picture = NULL WHERE id = ?1", [id])?;
        // The group chat redraws their messages with the new face.
        self.bump_room_author(id)?;
        Ok(old)
    }

    /// Sets or clears an account's banner, and returns the one it replaces,
    /// whose file can go.
    pub fn set_banner(&self, id: i64, banner: Option<&str>) -> Result<Option<String>> {
        let old: Option<String> =
            self.db.query_row("SELECT banner FROM accounts WHERE id = ?1", [id], |row| row.get(0))?;
        self.db.execute("UPDATE accounts SET banner = ?2 WHERE id = ?1", params![id, banner])?;
        Ok(old)
    }

    /// A seller's open listings, newest first, for their profile.
    pub fn open_listings_of(&self, seller: i64) -> Result<Vec<Listing>> {
        let sql = format!("{} AND l.seller = ?1 AND l.open = 1 ORDER BY l.updated DESC", Store::LISTING);
        let mut statement = self.db.prepare(&sql)?;
        let rows = statement.query_map([seller], Store::row)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// Links `address` to `account`: buyers of its listings pay there from
    /// now on. Nothing has to be proven. Someone who links another person's
    /// wallet only sends that person their buyers' payments, so several
    /// accounts may name the same wallet, and nobody can block its owner.
    pub fn set_wallet(&self, account: i64, address: &str) -> Result<()> {
        let address = orien_address(address)?;
        self.db.execute("UPDATE accounts SET payout = ?2 WHERE id = ?1", params![account, address])?;
        self.db.execute("UPDATE listings SET pay_to = ?2 WHERE seller = ?1", params![account, address])?;
        Ok(())
    }

    /// The verified wallet `account` is paid at: selling needs one, so
    /// buyers always pay a wallet proven to be the seller's.
    fn wallet_of(&self, account: i64) -> Result<String> {
        let wallet: Option<String> =
            self.db.query_row("SELECT payout FROM accounts WHERE id = ?1", [account], |row| row.get(0))?;
        match wallet {
            Some(wallet) if self.wallet_verified(account)? => Ok(wallet),
            _ => bail!("verify a wallet to be paid at first, under Your wallets on your account page"),
        }
    }

    /// Forgets a code that could not be sent.
    pub fn forget_code(&self, email: &str) -> Result<()> {
        self.db.execute("DELETE FROM codes WHERE email = ?1", [email])?;
        Ok(())
    }

    #[doc(hidden)]
    pub fn forget_codes_for_tests(&self) {
        self.db.execute("DELETE FROM codes", []).unwrap();
    }

    pub fn account(&self, id: i64) -> Result<Option<Account>> {
        Ok(self
            .db
            .query_row(
                "SELECT id, name, bio, picture, payout, created, discord, telegram, x, banner,
                        EXISTS (SELECT 1 FROM verified_wallets p WHERE p.account = accounts.id AND p.proven IS NOT NULL AND p.address = accounts.payout),
                        messages_from
                 FROM accounts WHERE id = ?1",
                [id],
                |row| {
                    Ok(Account {
                        id: row.get(0)?,
                        name: row.get(1)?,
                        bio: row.get(2)?,
                        picture: row.get(3)?,
                        banner: row.get(9)?,
                        wallet: row.get(4)?,
                        wallet_verified: row.get(10)?,
                        contact: Contact { discord: row.get(6)?, telegram: row.get(7)?, x: row.get(8)? },
                        created: row.get(5)?,
                        messages_from: row.get(11)?,
                    })
                },
            )
            .optional()?)
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
        let hash = token_hash(token);
        let found: Option<(i64, u64)> = self
            .db
            .query_row(
                "SELECT account, expires FROM sessions
                 WHERE token = ?1 AND expires >= ?2 AND account NOT IN (SELECT account FROM suspended)",
                params![hash, now()],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        let Some((account, expires)) = found else { return Ok(None) };
        // Renewed at most once a day, to keep writes rare.
        if expires < now() + SESSION_SECONDS - 24 * 3600 {
            self.db.execute("UPDATE sessions SET expires = ?2 WHERE token = ?1", params![hash, now() + SESSION_SECONDS])?;
        }
        Ok(Some(account))
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
        if self.is_removed(id)? {
            bail!("this listing was taken down, and cannot be changed");
        }
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

    /// Adds a picture to a listing of `seller`'s, last; at most four.
    pub fn add_listing_picture(&self, seller: i64, listing: i64, file: &str) -> Result<()> {
        self.own_listing(seller, listing)?;
        let count: i64 = self.db.query_row("SELECT COUNT(*) FROM listing_pictures WHERE listing = ?1", [listing], |r| r.get(0))?;
        if count >= MAX_LISTING_PICTURES {
            bail!("a listing has at most {MAX_LISTING_PICTURES} pictures");
        }
        let next: i64 = self.db.query_row(
            "SELECT COALESCE(MAX(position), -1) + 1 FROM listing_pictures WHERE listing = ?1",
            [listing],
            |r| r.get(0),
        )?;
        self.db.execute("INSERT INTO listing_pictures (listing, file, position) VALUES (?1, ?2, ?3)", params![listing, file, next])?;
        Ok(())
    }

    /// Takes a picture off a listing; its file can go.
    pub fn remove_listing_picture(&self, seller: i64, listing: i64, file: &str) -> Result<()> {
        self.own_listing(seller, listing)?;
        let gone = self.db.execute("DELETE FROM listing_pictures WHERE listing = ?1 AND file = ?2", params![listing, file])?;
        if gone == 0 {
            bail!("that picture is not on this listing");
        }
        Ok(())
    }

    /// Makes a picture the listing's cover, the one shown first.
    pub fn make_cover(&self, seller: i64, listing: i64, file: &str) -> Result<()> {
        self.own_listing(seller, listing)?;
        let first: i64 = self.db.query_row(
            "SELECT COALESCE(MIN(position), 0) - 1 FROM listing_pictures WHERE listing = ?1",
            [listing],
            |r| r.get(0),
        )?;
        let moved =
            self.db.execute("UPDATE listing_pictures SET position = ?3 WHERE listing = ?1 AND file = ?2", params![listing, file, first])?;
        if moved == 0 {
            bail!("that picture is not on this listing");
        }
        Ok(())
    }

    fn own_listing(&self, seller: i64, listing: i64) -> Result<()> {
        let owner: Option<i64> =
            self.db.query_row("SELECT seller FROM listings WHERE id = ?1", [listing], |r| r.get(0)).optional()?;
        if owner != Some(seller) {
            bail!("no such listing of yours");
        }
        Ok(())
    }

    /// Opens or closes a listing; only its seller may. A closed listing
    /// stays, for its reviews, but is not shown in the marketplace.
    pub fn set_open(&self, seller: i64, id: i64, open: bool) -> Result<()> {
        if self.is_removed(id)? {
            bail!("this listing was taken down, and cannot be opened again");
        }
        if open {
            self.wallet_of(seller)?;
        }
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
             l.pay_to, l.created, l.updated, l.open,
             (SELECT GROUP_CONCAT(file, ',') FROM
                 (SELECT file FROM listing_pictures WHERE listing = l.id ORDER BY position))
         FROM listings l JOIN accounts a ON a.id = l.seller
         WHERE l.id NOT IN (SELECT listing FROM removed_listings)
             AND l.seller NOT IN (SELECT account FROM suspended)";

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
            pictures: row
                .get::<_, Option<String>>(13)?
                .map(|files| files.split(',').map(String::from).collect())
                .unwrap_or_default(),
        })
    }

    pub fn listing(&self, id: i64) -> Result<Option<Listing>> {
        let sql = format!("{} AND l.id = ?1", Store::LISTING);
        Ok(self.db.query_row(&sql, [id], Store::row).optional()?)
    }

    /// Open listings, newest first: in one category or all, matching a
    /// search in the title or description if one is given.
    pub fn browse(&self, category: Option<&str>, search: Option<&str>, limit: u32, offset: u32) -> Result<Vec<Listing>> {
        self.browse_for(None, category, search, limit, offset)
    }

    /// Open listings as `viewer` sees them: without those of people they
    /// blocked, or who blocked them.
    pub fn browse_for(&self, viewer: Option<i64>, category: Option<&str>, search: Option<&str>, limit: u32, offset: u32) -> Result<Vec<Listing>> {
        let sql = format!(
            "{} AND l.open = 1 AND (?1 IS NULL OR l.category = ?1)
                 AND (?2 IS NULL OR l.title LIKE ?2 ESCAPE '\\' OR l.description LIKE ?2 ESCAPE '\\')
                 AND (?5 IS NULL OR NOT EXISTS (SELECT 1 FROM blocks b
                     WHERE (b.blocker = ?5 AND b.blocked = l.seller) OR (b.blocker = l.seller AND b.blocked = ?5)))
             ORDER BY l.updated DESC, l.id DESC LIMIT ?3 OFFSET ?4",
            Store::LISTING
        );
        let pattern = search.map(str::trim).filter(|s| !s.is_empty()).map(|s| {
            let escaped = s.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_");
            format!("%{escaped}%")
        });
        let mut statement = self.db.prepare(&sql)?;
        let rows = statement.query_map(params![category, pattern, limit.min(100), offset, viewer], Store::row)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// Every listing of one seller, open ones first.
    pub fn listings_of(&self, seller: i64) -> Result<Vec<Listing>> {
        let sql = format!("{} AND l.seller = ?1 ORDER BY l.open DESC, l.updated DESC", Store::LISTING);
        let mut statement = self.db.prepare(&sql)?;
        let rows = statement.query_map([seller], Store::row)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }
}

pub(crate) fn random<const N: usize>() -> Result<[u8; N]> {
    let mut bytes = [0u8; N];
    getrandom::fill(&mut bytes).map_err(|e| anyhow::anyhow!("no randomness: {e}"))?;
    Ok(bytes)
}

fn code_hash(email: &str, code: &str) -> String {
    blake3::hash(format!("{email}\n{code}").as_bytes()).to_hex().to_string()
}

/// An amount of ORN as people write it: 200, or 1.5.
pub fn orn(units: u64) -> String {
    let (whole, fraction) = (units / 1_000_000_000, units % 1_000_000_000);
    if fraction == 0 {
        whole.to_string()
    } else {
        format!("{whole}.{}", format!("{fraction:09}").trim_end_matches('0'))
    }
}

/// An Orien address tidied, or why it is not one.
pub fn orien_address(text: &str) -> Result<String> {
    let address = text.trim().to_lowercase();
    if address.len() != 64 || !address.bytes().all(|b| b.is_ascii_hexdigit()) {
        bail!("an Orien address is 64 letters and digits, 0-9 and a-f");
    }
    Ok(address)
}

/// A name as a username, or why it cannot be one.
pub fn username(name: &str) -> Result<String> {
    let name = name.trim();
    let length = name.chars().count();
    if !(MIN_NAME..=MAX_NAME).contains(&length) || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        bail!("a name is {MIN_NAME} to {MAX_NAME} letters, numbers or underscores, with no spaces");
    }
    Ok(name.to_string())
}

/// Whether a name could pass for Orien or its people: any name holding
/// "orien", "official", "support", "admin", "moderator" or "verif", once
/// look-alike digits are read as letters and underscores dropped; or one
/// whose whole name, or a word of it between underscores, is "mod",
/// "mods", "staff", "team", "helpdesk", "security", "founder" or "system".
pub fn reserved(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    let folded: String = lower
        .chars()
        .filter(|c| *c != '_')
        .map(|c| match c {
            '0' => 'o',
            '1' => 'i',
            '3' => 'e',
            '4' => 'a',
            '5' => 's',
            '7' => 't',
            c => c,
        })
        .collect();
    const WITHIN: [&str; 6] = ["orien", "official", "support", "admin", "moderator", "verif"];
    const WORDS: [&str; 8] = ["mod", "mods", "staff", "team", "helpdesk", "security", "founder", "system"];
    // "1" also stands for "l": "0ffic1al" and "supp0rt" are caught by the
    // first reading, "a1min" by neither, which is fine.
    WITHIN.iter().any(|w| folded.contains(w) || folded.replace('i', "l").contains(w))
        || lower.split('_').any(|part| WORDS.contains(&part))
        || WORDS.contains(&folded.as_str())
}

/// A name and bio tidied, or why they cannot be used.
pub fn profile_text(name: &str, bio: &str) -> Result<(String, String)> {
    let (name, bio) = (username(name)?, clean(bio));
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

pub(crate) fn token_hash(token: &str) -> String {
    blake3::hash(token.as_bytes()).to_hex().to_string()
}

#[cfg(test)]
mod tests {
    #[test]
    fn names_that_could_pass_for_orien_are_kept_back() {
        for name in ["Orien", "orien_support", "TheOrienTeam", "0r1en", "official_help", "Supp0rt", "site_admin", "verify_bot", "mod", "orn_mods", "Staff", "founder"] {
            assert!(super::reserved(name), "{name}");
        }
        for name in ["Maya", "niko_dev", "modest", "teamwork", "steamer", "orn_fan", "SpaceFace"] {
            assert!(!super::reserved(name), "{name}");
        }
        let store = Store::in_memory().unwrap();
        assert!(store.create_account("OrienSupport", "a").is_err());
        let maya = store.create_account("Maya", "b").unwrap().id;
        assert!(store.set_profile(maya, "orien_maya", "", &Contact::default()).is_err());
        // An account that has such a name already keeps it.
        store.db.execute("UPDATE accounts SET name = 'Orien' WHERE id = ?1", [maya]).unwrap();
        assert!(store.set_profile(maya, "Orien", "Hello", &Contact::default()).is_ok());
    }

    #[test]
    fn an_old_database_keeps_its_conversations_when_direct_messages_come() {
        let path = std::env::temp_dir().join(format!("orien-market-upgrade-{}.db", std::process::id()));
        let _ = std::fs::remove_file(&path);
        {
            let db = rusqlite::Connection::open(&path).unwrap();
            db.execute_batch(
                "PRAGMA foreign_keys = ON;
                 CREATE TABLE accounts (id INTEGER PRIMARY KEY, name TEXT NOT NULL, handle TEXT NOT NULL UNIQUE, email TEXT UNIQUE,
                     bio TEXT NOT NULL DEFAULT '', picture TEXT, wallet TEXT UNIQUE, contact_email TEXT NOT NULL DEFAULT '',
                     telegram TEXT NOT NULL DEFAULT '', x TEXT NOT NULL DEFAULT '', created INTEGER NOT NULL);
                 CREATE TABLE listings (id INTEGER PRIMARY KEY, seller INTEGER NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
                     title TEXT NOT NULL, description TEXT NOT NULL, category TEXT NOT NULL, price INTEGER NOT NULL, unit TEXT NOT NULL,
                     pay_to TEXT NOT NULL, created INTEGER NOT NULL, updated INTEGER NOT NULL, open INTEGER NOT NULL);
                 CREATE TABLE conversations (id INTEGER PRIMARY KEY, listing INTEGER NOT NULL REFERENCES listings(id) ON DELETE CASCADE,
                     buyer INTEGER NOT NULL REFERENCES accounts(id) ON DELETE CASCADE, seller INTEGER NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
                     created INTEGER NOT NULL, updated INTEGER NOT NULL, buyer_read INTEGER NOT NULL DEFAULT 0, seller_read INTEGER NOT NULL DEFAULT 0,
                     buyer_told INTEGER NOT NULL DEFAULT 0, seller_told INTEGER NOT NULL DEFAULT 0, UNIQUE (listing, buyer));
                 CREATE TABLE messages (id INTEGER PRIMARY KEY, conversation INTEGER NOT NULL REFERENCES conversations(id) ON DELETE CASCADE,
                     sender INTEGER NOT NULL REFERENCES accounts(id) ON DELETE CASCADE, body TEXT NOT NULL, kind TEXT NOT NULL DEFAULT 'text',
                     amount INTEGER, created INTEGER NOT NULL);
                 INSERT INTO accounts (id, name, handle, created) VALUES (1, 'Maya', 'a', 1), (2, 'Sam', 'b', 1);
                 INSERT INTO listings VALUES (1, 1, 'Logo', '', 'design', 1, '', '', 1, 1, 1);
                 INSERT INTO conversations (id, listing, buyer, seller, created, updated) VALUES (7, 1, 2, 1, 1, 1);
                 INSERT INTO messages (conversation, sender, body, created) VALUES (7, 2, 'Hi', 1);",
            )
            .unwrap();
        }
        let store = Store::open(&path).unwrap();
        let mine = store.conversations(2).unwrap();
        assert_eq!((mine.len(), mine[0].id, mine[0].listing, mine[0].last.as_ref().unwrap().body.as_str()), (1, 7, Some(1), "Hi"));
        let direct = store.direct_conversation(2, "Maya").unwrap();
        store.send(direct, 2, "Hello").unwrap();
        assert_eq!(store.conversations(1).unwrap().len(), 2);
        drop(store);
        Store::open(&path).unwrap();
        let _ = std::fs::remove_file(&path);
    }

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
        store.verified_wallet_for_tests(maya.id, &"a".repeat(64)).unwrap();
        store.verified_wallet_for_tests(sam.id, &"b".repeat(64)).unwrap();

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

        store.add_listing_picture(maya.id, posted.id, "one.jpg").unwrap();
        store.add_listing_picture(maya.id, posted.id, "two.jpg").unwrap();
        assert!(store.add_listing_picture(sam.id, posted.id, "three.jpg").is_err(), "only the seller adds pictures");
        store.make_cover(maya.id, posted.id, "two.jpg").unwrap();
        assert_eq!(store.listing(posted.id).unwrap().unwrap().pictures, vec!["two.jpg", "one.jpg"]);
        store.remove_listing_picture(maya.id, posted.id, "two.jpg").unwrap();
        for n in 0..3 {
            store.add_listing_picture(maya.id, posted.id, &format!("{n}.jpg")).unwrap();
        }
        assert!(store.add_listing_picture(maya.id, posted.id, "five.jpg").is_err(), "four at most");

        store.set_open(maya.id, posted.id, false).unwrap();
        assert!(store.browse(None, None, 10, 0).unwrap().is_empty());
        assert_eq!(store.listings_of(maya.id).unwrap().len(), 1, "a closed listing stays for its seller");
    }

    #[test]
    fn listings_are_checked() {
        let store = Store::in_memory().unwrap();
        let maya = store.create_account("Maya", "maya").unwrap();
        store.verified_wallet_for_tests(maya.id, &"a".repeat(64)).unwrap();
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
    fn any_account_may_name_a_wallet() {
        let store = Store::in_memory().unwrap();
        let maya = store.create_account("Maya", "maya").unwrap();
        let sam = store.create_account("Sam", "sam").unwrap();
        let wallet = "c".repeat(64);
        assert!(store.set_wallet(maya.id, "nonsense").is_err());
        store.set_wallet(maya.id, &wallet.to_uppercase()).unwrap();
        assert_eq!(store.account(maya.id).unwrap().unwrap().wallet, Some(wallet.clone()));
        store.set_wallet(sam.id, &wallet).unwrap();
        assert_eq!(store.account(sam.id).unwrap().unwrap().wallet, Some(wallet), "nobody can block a wallet's owner");
    }

    #[test]
    fn contact_details_are_tidied_or_refused() {
        let typed = Contact { telegram: "https://t.me/maya_designs".into(), x: "@MayaLin".into(), discord: " @Maya.Designs ".into() };
        let tidy = typed.checked().unwrap();
        assert_eq!((tidy.telegram.as_str(), tidy.x.as_str(), tidy.discord.as_str()), ("maya_designs", "MayaLin", "maya.designs"));
        assert!(Contact { discord: "m".into(), ..Contact::default() }.checked().is_err());
        assert!(Contact { discord: "two..dots".into(), ..Contact::default() }.checked().is_err());
        assert!(Contact { discord: "has space".into(), ..Contact::default() }.checked().is_err());
        assert_eq!(Contact { x: "https://x.com/maya/".into(), ..Contact::default() }.checked().unwrap().x, "maya");
        assert!(Contact { telegram: "abc".into(), ..Contact::default() }.checked().is_err());
        assert!(Contact { x: "way_too_long_for_x_names".into(), ..Contact::default() }.checked().is_err());
        assert!(Contact { x: "no spaces".into(), ..Contact::default() }.checked().is_err());
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
        // Used, a sign-in renews itself; unused for a month, it lapses.
        store.db.execute("UPDATE sessions SET expires = ?1", [now() + 3600]).unwrap();
        assert_eq!(store.session(&token).unwrap(), Some(maya.id));
        let renewed: u64 = store.db.query_row("SELECT expires FROM sessions", [], |row| row.get(0)).unwrap();
        assert!(renewed >= now() + SESSION_SECONDS - 5);
        store.db.execute("UPDATE sessions SET expires = ?1", [now() - 1]).unwrap();
        assert_eq!(store.session(&token).unwrap(), None);
        store.end_session(&token).unwrap();
        assert_eq!(store.session(&token).unwrap(), None);
    }
}
