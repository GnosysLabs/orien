//! What the founder does when something slips past the screening model:
//! reports people make, taking a listing down, and suspending an account.
//! Done with `orien-market admin` on the server.

use crate::store::{Store, clean_text, now};
use anyhow::{Result, bail};
use rusqlite::{OptionalExtension, params};
use serde::Serialize;

/// Reports one account may make in a day.
pub const REPORTS_A_DAY: i64 = 20;
pub const MAX_REASON: usize = 1000;

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Report {
    pub id: i64,
    pub reporter: Option<i64>,
    pub reporter_name: Option<String>,
    /// listing, account or message.
    pub kind: String,
    pub target: i64,
    /// What it is about, in a few words: a listing's title, an account's
    /// name, a message's text.
    pub about: String,
    pub reason: String,
    pub created: u64,
}

impl Store {
    pub(crate) fn create_moderation_tables(&self) -> Result<()> {
        self.db.execute_batch(
            "CREATE TABLE IF NOT EXISTS reports (
                 id INTEGER PRIMARY KEY,
                 reporter INTEGER REFERENCES accounts(id) ON DELETE SET NULL,
                 kind TEXT NOT NULL,
                 target INTEGER NOT NULL,
                 reason TEXT NOT NULL,
                 created INTEGER NOT NULL,
                 handled INTEGER NOT NULL DEFAULT 0
             );
             CREATE TABLE IF NOT EXISTS removed_listings (
                 listing INTEGER PRIMARY KEY REFERENCES listings(id) ON DELETE CASCADE,
                 reason TEXT NOT NULL,
                 at INTEGER NOT NULL
             );
             CREATE TABLE IF NOT EXISTS suspended (
                 account INTEGER PRIMARY KEY REFERENCES accounts(id) ON DELETE CASCADE,
                 reason TEXT NOT NULL,
                 at INTEGER NOT NULL
             );",
        )?;
        Ok(())
    }

    /// Someone flags a listing, an account or a message for the founder.
    pub fn report(&self, reporter: i64, kind: &str, target: i64, reason: &str) -> Result<()> {
        let reason = clean_text(reason);
        if reason.chars().count() > MAX_REASON {
            bail!("say what is wrong in at most {MAX_REASON} characters");
        }
        let exists: bool = match kind {
            "listing" => self.listing(target)?.is_some(),
            "account" => self.account(target)?.is_some(),
            // Only a message in one of the reporter's own conversations.
            "message" => self
                .db
                .query_row(
                    "SELECT 1 FROM messages m JOIN conversations c ON c.id = m.conversation
                     WHERE m.id = ?1 AND (c.buyer = ?2 OR c.seller = ?2)",
                    params![target, reporter],
                    |_| Ok(()),
                )
                .optional()?
                .is_some(),
            _ => bail!("a report is about a listing, an account or a message"),
        };
        if !exists {
            bail!("there is nothing like that to report");
        }
        let today: i64 = self.db.query_row(
            "SELECT COUNT(*) FROM reports WHERE reporter = ?1 AND created > ?2",
            params![reporter, now().saturating_sub(24 * 3600)],
            |r| r.get(0),
        )?;
        if today >= REPORTS_A_DAY {
            bail!("you have sent {REPORTS_A_DAY} reports today; thank you, and send more tomorrow");
        }
        self.db.execute(
            "INSERT INTO reports (reporter, kind, target, reason, created) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![reporter, kind, target, reason, now()],
        )?;
        Ok(())
    }

    /// Reports not yet dealt with, oldest first.
    pub fn open_reports(&self) -> Result<Vec<Report>> {
        let mut statement = self.db.prepare(
            "SELECT r.id, r.reporter, a.name, r.kind, r.target, r.reason, r.created,
                    CASE r.kind
                        WHEN 'listing' THEN (SELECT title FROM listings WHERE id = r.target)
                        WHEN 'account' THEN (SELECT name FROM accounts WHERE id = r.target)
                        WHEN 'message' THEN (SELECT body FROM messages WHERE id = r.target)
                    END
             FROM reports r LEFT JOIN accounts a ON a.id = r.reporter
             WHERE r.handled = 0 ORDER BY r.id",
        )?;
        let rows = statement.query_map([], |row| {
            Ok(Report {
                id: row.get(0)?,
                reporter: row.get(1)?,
                reporter_name: row.get(2)?,
                kind: row.get(3)?,
                target: row.get(4)?,
                reason: row.get(5)?,
                created: row.get(6)?,
                about: row.get::<_, Option<String>>(7)?.unwrap_or_else(|| "(gone)".into()),
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    pub fn mark_handled(&self, report: i64) -> Result<()> {
        if self.db.execute("UPDATE reports SET handled = 1 WHERE id = ?1", [report])? == 0 {
            bail!("there is no report {report}");
        }
        Ok(())
    }

    /// Takes a listing down for good: nobody sees it, and its seller cannot
    /// open it again.
    pub fn remove_listing(&self, listing: i64, reason: &str) -> Result<()> {
        if self.db.query_row("SELECT 1 FROM listings WHERE id = ?1", [listing], |_| Ok(())).optional()?.is_none() {
            bail!("there is no listing {listing}");
        }
        self.db.execute("UPDATE listings SET open = 0 WHERE id = ?1", [listing])?;
        self.db.execute(
            "INSERT OR REPLACE INTO removed_listings (listing, reason, at) VALUES (?1, ?2, ?3)",
            params![listing, reason, now()],
        )?;
        self.db.execute("UPDATE reports SET handled = 1 WHERE kind = 'listing' AND target = ?1", [listing])?;
        Ok(())
    }

    /// Suspends an account: it is signed out everywhere and cannot sign in,
    /// its listings and swap offers are taken down, and its profile is hidden.
    pub fn suspend(&self, account: i64, reason: &str) -> Result<()> {
        if self.account(account)?.is_none() {
            bail!("there is no account {account}");
        }
        self.db.execute("INSERT OR REPLACE INTO suspended (account, reason, at) VALUES (?1, ?2, ?3)", params![account, reason, now()])?;
        self.db.execute("DELETE FROM sessions WHERE account = ?1", [account])?;
        self.db.execute("UPDATE listings SET open = 0 WHERE seller = ?1", [account])?;
        self.db.execute("UPDATE swap_offers SET state = 'closed' WHERE seller = ?1 AND state = 'open'", [account])?;
        self.db.execute("UPDATE reports SET handled = 1 WHERE kind = 'account' AND target = ?1", [account])?;
        Ok(())
    }

    /// Lifts a suspension. Its listings stay closed until it opens them again.
    pub fn unsuspend(&self, account: i64) -> Result<()> {
        if self.db.execute("DELETE FROM suspended WHERE account = ?1", [account])? == 0 {
            bail!("account {account} is not suspended");
        }
        Ok(())
    }

    /// Deletes an account and everything it made: its listings and their
    /// pictures, its conversations and messages, its swap offers. Returns the
    /// picture files to remove. A suspended account cannot erase itself.
    pub fn delete_account(&self, account: i64) -> Result<Vec<String>> {
        if self.is_suspended(account)? {
            bail!("a suspended account cannot be deleted");
        }
        let mut files: Vec<String> = {
            let mut statement = self.db.prepare(
                "SELECT file FROM listing_pictures WHERE listing IN (SELECT id FROM listings WHERE seller = ?1)",
            )?;
            statement.query_map([account], |row| row.get(0))?.collect::<rusqlite::Result<_>>()?
        };
        if let Some(found) = self.account(account)? {
            files.extend(found.picture);
            files.extend(found.banner);
        }
        // Everything that names the account goes with it.
        self.db.execute("DELETE FROM accounts WHERE id = ?1", [account])?;
        Ok(files)
    }

    pub fn is_suspended(&self, account: i64) -> Result<bool> {
        Ok(self.db.query_row("SELECT 1 FROM suspended WHERE account = ?1", [account], |_| Ok(())).optional()?.is_some())
    }

    pub fn is_removed(&self, listing: i64) -> Result<bool> {
        Ok(self.db.query_row("SELECT 1 FROM removed_listings WHERE listing = ?1", [listing], |_| Ok(())).optional()?.is_some())
    }
}

#[cfg(test)]
mod tests {
    use crate::store::{Draft, Store};

    fn draft() -> Draft {
        Draft {
            title: "Logo design".into(),
            description: String::new(),
            category: "design".into(),
            price: 1,
            unit: String::new(),
            pay_to: String::new(),
        }
    }

    #[test]
    fn a_reported_listing_is_taken_down_for_good() {
        let store = Store::in_memory().unwrap();
        let maya = store.create_account("Maya", "maya").unwrap().id;
        let sam = store.create_account("Sam", "sam").unwrap().id;
        store.verified_wallet_for_tests(maya, &"a".repeat(64)).unwrap();
        let listing = store.post(maya, &draft()).unwrap().id;
        assert!(store.report(sam, "listing", 999, "spam").is_err(), "nothing to report");
        assert!(store.report(sam, "weather", listing, "spam").is_err());
        store.report(sam, "listing", listing, "Looks like a scam").unwrap();
        let reports = store.open_reports().unwrap();
        assert_eq!((reports[0].about.as_str(), reports[0].reporter_name.as_deref()), ("Logo design", Some("Sam")));

        store.remove_listing(listing, "a scam").unwrap();
        assert!(store.open_reports().unwrap().is_empty(), "dealt with");
        assert!(store.listing(listing).unwrap().is_none(), "gone for everyone");
        assert!(store.listings_of(maya).unwrap().is_empty(), "the seller too");
        assert!(store.set_open(maya, listing, true).is_err(), "and it cannot be opened again");
    }

    #[test]
    fn an_account_deletes_itself_and_everything_it_made() {
        let store = Store::in_memory().unwrap();
        let maya = store.create_account("Maya", "maya").unwrap().id;
        let sam = store.create_account("Sam", "sam").unwrap().id;
        store.verified_wallet_for_tests(maya, &"a".repeat(64)).unwrap();
        let listing = store.post(maya, &draft()).unwrap().id;
        store.add_listing_picture(maya, listing, "pic.jpg").unwrap();
        let c = store.conversation_about(sam, listing).unwrap();
        store.send(c, sam, "Hello").unwrap();
        let token = store.start_session(maya).unwrap();
        assert_eq!(store.delete_account(maya).unwrap(), vec!["pic.jpg".to_string()]);
        assert!(store.account(maya).unwrap().is_none());
        assert_eq!(store.session(&token).unwrap(), None);
        assert!(store.browse(None, None, 10, 0).unwrap().is_empty());
        assert!(store.conversations(sam).unwrap().is_empty(), "its conversations go too");
    }

    #[test]
    fn a_suspended_account_is_signed_out_and_its_listings_go() {
        let store = Store::in_memory().unwrap();
        let maya = store.create_account("Maya", "maya").unwrap().id;
        store.verified_wallet_for_tests(maya, &"a".repeat(64)).unwrap();
        store.post(maya, &draft()).unwrap();
        let token = store.start_session(maya).unwrap();
        store.suspend(maya, "scams").unwrap();
        assert_eq!(store.session(&token).unwrap(), None);
        assert!(store.browse(None, None, 10, 0).unwrap().is_empty());
        assert!(store.is_suspended(maya).unwrap());
        store.unsuspend(maya).unwrap();
        assert!(!store.is_suspended(maya).unwrap());
    }
}
