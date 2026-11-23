//! Messages between two accounts. A conversation is either about a listing
//! (one buyer and one listing) or direct (two people, no listing; one per
//! pair, stored with the lower account id as `buyer`).

use crate::store::{Store, clean_text, now};
use anyhow::{Result, bail};
use rusqlite::{OptionalExtension, params};
use serde::Serialize;

pub const MAX_MESSAGE: usize = 2000;
/// New conversations one account may start in a day.
pub const NEW_CONVERSATIONS_A_DAY: i64 = 10;
/// Messages one account may send in a day.
pub const MESSAGES_A_DAY: i64 = 300;

const DAY: u64 = 24 * 3600;

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Message {
    pub id: i64,
    pub sender: i64,
    pub body: String,
    /// "text", "request" (the seller asks to be paid `amount`) or "paid" (the
    /// buyer says they paid `amount`).
    pub kind: String,
    pub amount: Option<u64>,
    pub created: u64,
}

/// What kind of message, with its amount.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Text,
    Request(u64),
    Paid(u64),
}

impl Kind {
    fn parts(self) -> (&'static str, Option<u64>) {
        match self {
            Kind::Text => ("text", None),
            Kind::Request(amount) => ("request", Some(amount)),
            Kind::Paid(amount) => ("paid", Some(amount)),
        }
    }
}

/// A conversation as one side sees it. The listing fields are null in a
/// direct conversation.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Conversation {
    pub id: i64,
    pub listing: Option<i64>,
    pub listing_title: Option<String>,
    pub price: Option<u64>,
    pub unit: Option<String>,
    /// The seller's verified wallet, where payments go.
    pub pay_to: Option<String>,
    /// The other side: who this account is talking to.
    pub with: i64,
    pub with_name: String,
    pub with_picture: Option<String>,
    /// Whether this account is the buyer (always false in a direct conversation).
    pub buying: bool,
    pub last: Option<Message>,
    pub unread: bool,
    pub updated: u64,
    /// Whether this account has blocked the other side.
    pub blocked: bool,
}

/// What a sender's message needs to reach the other side by email.
pub struct Notice {
    pub to_email: String,
    pub from_name: String,
    /// None in a direct conversation.
    pub listing_title: Option<String>,
    pub conversation: i64,
}

impl Store {
    /// The other side of a conversation from `sender`, and `sender`'s name.
    pub fn other_side(&self, conversation: i64, sender: i64) -> Result<Option<(i64, String)>> {
        Ok(self
            .db
            .query_row(
                "SELECT CASE WHEN c.buyer = ?2 THEN c.seller ELSE c.buyer END, s.name
                 FROM conversations c JOIN accounts s ON s.id = ?2 WHERE c.id = ?1",
                params![conversation, sender],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?)
    }

    /// A message text tidied, or why it cannot be sent.
    pub fn message_text(text: &str) -> Result<String> {
        let body = clean_text(text);
        if body.is_empty() {
            bail!("write a message first");
        }
        if body.chars().count() > MAX_MESSAGE {
            bail!("a message is at most {MAX_MESSAGE} characters");
        }
        Ok(body)
    }

    fn sent_today(&self, account: i64) -> Result<i64> {
        Ok(self.db.query_row(
            "SELECT COUNT(*) FROM messages WHERE sender = ?1 AND created > ?2",
            params![account, now().saturating_sub(DAY)],
            |row| row.get(0),
        )?)
    }

    /// Blocks or unblocks a person for `blocker`, everywhere: no new
    /// conversations or messages either way, and their listings hidden.
    pub fn block_account(&self, blocker: i64, blocked: i64, block: bool) -> Result<()> {
        if blocker == blocked {
            bail!("you cannot block yourself");
        }
        if self.account(blocked)?.is_none() {
            bail!("there is no such person");
        }
        if block {
            self.db.execute("INSERT OR IGNORE INTO blocks (blocker, blocked) VALUES (?1, ?2)", params![blocker, blocked])?;
        } else {
            self.db.execute("DELETE FROM blocks WHERE blocker = ?1 AND blocked = ?2", params![blocker, blocked])?;
        }
        Ok(())
    }

    /// Why `from` may not write to `to`, if they may not.
    pub fn block_between(&self, from: i64, to: i64) -> Result<Option<&'static str>> {
        Ok(if self.blocked_by(to, from)? {
            Some("this person is not taking messages from you")
        } else if self.blocked_by(from, to)? {
            Some("you have blocked this person; unblock them to write")
        } else {
            None
        })
    }

    pub fn blocked_by(&self, blocker: i64, blocked: i64) -> Result<bool> {
        Ok(self
            .db
            .query_row("SELECT 1 FROM blocks WHERE blocker = ?1 AND blocked = ?2", params![blocker, blocked], |_| Ok(()))
            .optional()?
            .is_some())
    }

    /// The conversation `buyer` has about `listing`, started if need be.
    pub fn conversation_about(&self, buyer: i64, listing: i64) -> Result<i64> {
        let found: Option<(i64, bool)> = self
            .db
            .query_row("SELECT seller, open FROM listings WHERE id = ?1", [listing], |row| Ok((row.get(0)?, row.get(1)?)))
            .optional()?;
        let Some((seller, open)) = found else { bail!("there is no such listing") };
        if seller == buyer {
            bail!("this is your own listing");
        }
        let existing: Option<i64> = self
            .db
            .query_row("SELECT id FROM conversations WHERE listing = ?1 AND buyer = ?2", params![listing, buyer], |row| {
                row.get(0)
            })
            .optional()?;
        if let Some(id) = existing {
            return Ok(id);
        }
        if !open {
            bail!("this listing is closed");
        }
        self.may_start(buyer)?;
        let at = now();
        self.db.execute(
            "INSERT INTO conversations (listing, buyer, seller, created, updated, starter) VALUES (?1, ?2, ?3, ?4, ?4, ?2)",
            params![listing, buyer, seller, at],
        )?;
        Ok(self.db.last_insert_rowid())
    }

    /// The direct conversation between `from` and the account named `to`,
    /// started if need be. Anyone signed in may write to anyone, as about a
    /// listing; blocks, suspensions and the daily limits still apply.
    pub fn direct_conversation(&self, from: i64, to: &str) -> Result<i64> {
        let Some(to) = self.account_by_name(to)? else { bail!("there is no such person") };
        if to == from {
            bail!("you cannot message yourself");
        }
        if self.is_suspended(to)? {
            bail!("this account is suspended");
        }
        if let Some(why) = self.block_between(from, to)? {
            bail!(why);
        }
        if let Some(id) = self.direct_between(from, to)? {
            return Ok(id);
        }
        if let Some(why) = self.new_messages_refused(from, to)? {
            bail!(why);
        }
        self.may_start(from)?;
        let (a, b) = (from.min(to), from.max(to));
        let at = now();
        self.db.execute(
            "INSERT INTO conversations (listing, buyer, seller, created, updated, starter) VALUES (NULL, ?1, ?2, ?3, ?3, ?4)",
            params![a, b, at, from],
        )?;
        Ok(self.db.last_insert_rowid())
    }

    /// The direct conversation two accounts have, if any.
    fn direct_between(&self, one: i64, other: i64) -> Result<Option<i64>> {
        Ok(self
            .db
            .query_row(
                "SELECT id FROM conversations WHERE listing IS NULL AND buyer = ?1 AND seller = ?2",
                params![one.min(other), one.max(other)],
                |row| row.get(0),
            )
            .optional()?)
    }

    /// Why `to`'s choice of who may message them keeps `from` from starting
    /// a direct conversation, if it does. "talked" lets in anyone they have
    /// exchanged a message with before, about a listing or not.
    fn new_messages_refused(&self, from: i64, to: i64) -> Result<Option<String>> {
        let (name, choice): (String, String) =
            self.db.query_row("SELECT name, messages_from FROM accounts WHERE id = ?1", [to], |row| Ok((row.get(0)?, row.get(1)?)))?;
        let open = match choice.as_str() {
            "nobody" => false,
            "talked" => self.db.query_row(
                "SELECT EXISTS (SELECT 1 FROM conversations c
                     WHERE ((c.buyer = ?1 AND c.seller = ?2) OR (c.buyer = ?2 AND c.seller = ?1))
                     AND EXISTS (SELECT 1 FROM messages m WHERE m.conversation = c.id))",
                params![from, to],
                |row| row.get(0),
            )?,
            _ => true,
        };
        Ok((!open).then(|| format!("{name} isn't taking new messages")))
    }

    /// Whether `viewer` can write to `to` directly now: in the conversation
    /// they already have, or by starting one. For hiding a Message button.
    pub fn takes_messages(&self, viewer: i64, to: i64) -> Result<bool> {
        if viewer == to || self.is_suspended(to)? || self.block_between(viewer, to)?.is_some() {
            return Ok(false);
        }
        Ok(self.direct_between(viewer, to)?.is_some() || self.new_messages_refused(viewer, to)?.is_none())
    }

    /// Checks `account` may start another conversation today.
    fn may_start(&self, account: i64) -> Result<()> {
        let started: i64 = self.db.query_row(
            "SELECT COUNT(*) FROM conversations WHERE starter = ?1 AND created > ?2",
            params![account, now().saturating_sub(DAY)],
            |row| row.get(0),
        )?;
        if started >= NEW_CONVERSATIONS_A_DAY {
            bail!("you have started {NEW_CONVERSATIONS_A_DAY} conversations today; you can start more tomorrow");
        }
        Ok(())
    }

    /// Whether a conversation is about a listing.
    fn has_listing(&self, conversation: i64) -> Result<bool> {
        Ok(self.db.query_row("SELECT listing IS NOT NULL FROM conversations WHERE id = ?1", [conversation], |row| row.get(0))?)
    }

    /// The two sides of a conversation, if `account` is one of them.
    fn sides(&self, conversation: i64, account: i64) -> Result<(i64, i64)> {
        let sides: Option<(i64, i64)> = self
            .db
            .query_row("SELECT buyer, seller FROM conversations WHERE id = ?1", [conversation], |row| {
                Ok((row.get(0)?, row.get(1)?))
            })
            .optional()?;
        match sides {
            Some((buyer, seller)) if account == buyer || account == seller => Ok((buyer, seller)),
            _ => bail!("there is no such conversation of yours"),
        }
    }

    /// Checks `sender` may send in this conversation now.
    pub fn may_send(&self, conversation: i64, sender: i64) -> Result<()> {
        let (buyer, seller) = self.sides(conversation, sender)?;
        let other = if sender == buyer { seller } else { buyer };
        if self.blocked_by(other, sender)? {
            bail!("this person is not taking messages from you");
        }
        if self.blocked_by(sender, other)? {
            bail!("you have blocked this person; unblock them to write");
        }
        if self.sent_today(sender)? >= MESSAGES_A_DAY {
            bail!("you have sent {MESSAGES_A_DAY} messages today; you can send more tomorrow");
        }
        Ok(())
    }

    /// Adds a message, already checked by `may_send` and screened. Returns
    /// the email to send the other side, if they should be told.
    pub fn send(&self, conversation: i64, sender: i64, body: &str) -> Result<(Message, Option<Notice>)> {
        self.post_message(conversation, sender, &Store::message_text(body)?, Kind::Text)
    }

    /// The seller asks the buyer to pay `amount`.
    pub fn request_payment(&self, conversation: i64, seller: i64, amount: u64) -> Result<(Message, Option<Notice>)> {
        let (_, s) = self.sides(conversation, seller)?;
        if !self.has_listing(conversation)? {
            bail!("payment requests are for conversations about a listing");
        }
        if s != seller {
            bail!("only the seller asks for payment");
        }
        if amount == 0 {
            bail!("ask for more than nothing");
        }
        let body = format!("Payment request: {} ORN", crate::store::orn(amount));
        self.post_message(conversation, seller, &body, Kind::Request(amount))
    }

    /// The buyer says they have paid what the seller asked.
    pub fn mark_paid(&self, conversation: i64, buyer: i64, request: i64) -> Result<(Message, Option<Notice>)> {
        let (b, _) = self.sides(conversation, buyer)?;
        if !self.has_listing(conversation)? {
            bail!("payment requests are for conversations about a listing");
        }
        if b != buyer {
            bail!("only the buyer pays");
        }
        let amount: Option<Option<i64>> = self
            .db
            .query_row(
                "SELECT amount FROM messages WHERE id = ?1 AND conversation = ?2 AND kind = 'request'",
                params![request, conversation],
                |row| row.get(0),
            )
            .optional()?;
        let Some(Some(amount)) = amount else { bail!("there is no such payment request") };
        let body = format!("Paid {} ORN", crate::store::orn(amount as u64));
        self.post_message(conversation, buyer, &body, Kind::Paid(amount as u64))
    }

    fn post_message(&self, conversation: i64, sender: i64, body: &str, kind: Kind) -> Result<(Message, Option<Notice>)> {
        self.may_send(conversation, sender)?;
        let (buyer, _) = self.sides(conversation, sender)?;
        let at = now();
        let (kind_name, amount) = kind.parts();
        self.db.execute(
            "INSERT INTO messages (conversation, sender, body, kind, amount, created) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![conversation, sender, body, kind_name, amount.map(|a| a as i64), at],
        )?;
        let id = self.db.last_insert_rowid();
        // The sender has read their own message.
        let (read, other_told, other) = if sender == buyer {
            ("buyer_read", "seller_told", "seller")
        } else {
            ("seller_read", "buyer_told", "buyer")
        };
        self.db.execute(&format!("UPDATE conversations SET updated = ?2, {read} = ?3 WHERE id = ?1"), params![conversation, at, id])?;
        // The other side is emailed once, until they read the conversation.
        let notice: Option<(String, String, Option<String>, bool)> = self
            .db
            .query_row(
                &format!(
                    "SELECT o.email, s.name, l.title, c.{other_told}
                     FROM conversations c
                     JOIN accounts o ON o.id = c.{other}
                     JOIN accounts s ON s.id = ?2
                     LEFT JOIN listings l ON l.id = c.listing
                     WHERE c.id = ?1"
                ),
                params![conversation, sender],
                |row| Ok((row.get::<_, Option<String>>(0)?.unwrap_or_default(), row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .optional()?;
        let notice = match notice {
            Some((to_email, from_name, listing_title, false)) if !to_email.is_empty() => {
                self.db.execute(&format!("UPDATE conversations SET {other_told} = 1 WHERE id = ?1"), [conversation])?;
                Some(Notice { to_email, from_name, listing_title, conversation })
            }
            _ => None,
        };
        let message = Message { id, sender, body: body.to_string(), kind: kind_name.into(), amount, created: at };
        Ok((message, notice))
    }

    /// A conversation's messages, oldest first, marked read for `account`.
    pub fn read(&self, conversation: i64, account: i64) -> Result<(Conversation, Vec<Message>)> {
        let (buyer, _) = self.sides(conversation, account)?;
        let cleared = self.cleared(conversation, account == buyer)?;
        let mut statement = self
            .db
            .prepare("SELECT id, sender, body, kind, amount, created FROM messages WHERE conversation = ?1 AND id > ?2 ORDER BY id")?;
        let messages: Vec<Message> = statement
            .query_map(params![conversation, cleared], |row| {
                Ok(Message {
                    id: row.get(0)?,
                    sender: row.get(1)?,
                    body: row.get(2)?,
                    kind: row.get(3)?,
                    amount: row.get::<_, Option<i64>>(4)?.map(|a| a as u64),
                    created: row.get(5)?,
                })
            })?
            .collect::<rusqlite::Result<_>>()?;
        let last = messages.last().map_or(0, |m| m.id);
        let (read, told) = if account == buyer { ("buyer_read", "buyer_told") } else { ("seller_read", "seller_told") };
        self.db.execute(&format!("UPDATE conversations SET {read} = MAX({read}, ?2), {told} = 0 WHERE id = ?1"), params![conversation, last])?;
        // Deleted, with nothing new since: gone for them.
        let Some(conversation) = self.conversations(account)?.into_iter().find(|c| c.id == conversation) else {
            bail!("there is no such conversation of yours");
        };
        Ok((conversation, messages))
    }

    /// The last message one side had when they deleted the conversation.
    fn cleared(&self, conversation: i64, buying: bool) -> Result<i64> {
        let column = if buying { "buyer_cleared" } else { "seller_cleared" };
        Ok(self.db.query_row(&format!("SELECT {column} FROM conversations WHERE id = ?1"), [conversation], |row| row.get(0))?)
    }

    /// Deletes a conversation for `account` only: they no longer see its
    /// messages, payment requests included, and it leaves their list until
    /// a new message comes, which starts it afresh. The other side keeps
    /// theirs.
    pub fn delete_conversation(&self, conversation: i64, account: i64) -> Result<()> {
        let (buyer, _) = self.sides(conversation, account)?;
        let last: i64 =
            self.db.query_row("SELECT COALESCE(MAX(id), 0) FROM messages WHERE conversation = ?1", [conversation], |row| row.get(0))?;
        let (cleared, read, told) =
            if account == buyer { ("buyer_cleared", "buyer_read", "buyer_told") } else { ("seller_cleared", "seller_read", "seller_told") };
        // Read up to the mark too, so nothing before it counts as unread.
        self.db.execute(
            &format!("UPDATE conversations SET {cleared} = ?2, {read} = MAX({read}, ?2), {told} = 0 WHERE id = ?1"),
            params![conversation, last],
        )?;
        Ok(())
    }

    /// Every conversation of `account`, most recent first.
    pub fn conversations(&self, account: i64) -> Result<Vec<Conversation>> {
        let mut statement = self.db.prepare(
            "SELECT c.id, c.listing, l.title, c.buyer, c.seller, c.updated,
                    CASE WHEN c.buyer = ?1 THEN c.buyer_read ELSE c.seller_read END,
                    o.id, o.name, o.picture,
                    m.id, m.sender, m.body, m.created,
                    EXISTS (SELECT 1 FROM blocks WHERE blocker = ?1 AND blocked = o.id),
                    l.price, l.unit, l.pay_to, m.kind, m.amount,
                    CASE WHEN c.buyer = ?1 THEN c.buyer_cleared ELSE c.seller_cleared END
             FROM conversations c
             LEFT JOIN listings l ON l.id = c.listing
             JOIN accounts o ON o.id = CASE WHEN c.buyer = ?1 THEN c.seller ELSE c.buyer END
             LEFT JOIN messages m ON m.id = (SELECT MAX(id) FROM messages WHERE conversation = c.id)
             WHERE c.buyer = ?1 OR c.seller = ?1
             ORDER BY c.updated DESC, c.id DESC",
        )?;
        let rows = statement.query_map([account], |row| {
            let read: i64 = row.get(6)?;
            let last = match row.get::<_, Option<i64>>(10)? {
                Some(id) => Some(Message {
                    id,
                    sender: row.get(11)?,
                    body: row.get(12)?,
                    kind: row.get(18)?,
                    amount: row.get::<_, Option<i64>>(19)?.map(|a| a as u64),
                    created: row.get(13)?,
                }),
                None => None,
            };
            // Deleted by this account, with nothing new since: left out below.
            let cleared: i64 = row.get(20)?;
            let last = last.filter(|m| m.id > cleared);
            let unread = last.as_ref().is_some_and(|m| m.id > read && m.sender != account);
            let listing: Option<i64> = row.get(1)?;
            Ok(Conversation {
                id: row.get(0)?,
                listing,
                listing_title: row.get(2)?,
                price: row.get::<_, Option<i64>>(15)?.map(|p| p as u64),
                unit: row.get(16)?,
                pay_to: row.get(17)?,
                buying: listing.is_some() && row.get::<_, i64>(3)? == account,
                updated: row.get(5)?,
                with: row.get(7)?,
                with_name: row.get(8)?,
                with_picture: row.get(9)?,
                last,
                unread,
                blocked: row.get(14)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?.into_iter().filter(|c| c.last.is_some()).collect())
    }

    /// How many conversations have messages `account` has not read.
    pub fn unread(&self, account: i64) -> Result<usize> {
        Ok(self.conversations(account)?.iter().filter(|c| c.unread).count())
    }

    /// Blocks or unblocks the other side of a conversation.
    pub fn block(&self, conversation: i64, account: i64, block: bool) -> Result<()> {
        let (buyer, seller) = self.sides(conversation, account)?;
        let other = if account == buyer { seller } else { buyer };
        if block {
            self.db.execute("INSERT OR IGNORE INTO blocks (blocker, blocked) VALUES (?1, ?2)", params![account, other])?;
        } else {
            self.db.execute("DELETE FROM blocks WHERE blocker = ?1 AND blocked = ?2", params![account, other])?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::store::{Contact, Draft, Store};

    fn store_with_listing() -> (Store, i64, i64, i64) {
        let store = Store::in_memory().unwrap();
        let maya = store.create_account_with_email("Maya", "maya@example.com").unwrap().id;
        let sam = store.create_account_with_email("Sam", "sam@example.com").unwrap().id;
        store.verified_wallet_for_tests(maya, &"a".repeat(64)).unwrap();
        store.set_profile(maya, "Maya", "", &Contact::default()).unwrap();
        let draft = Draft {
            title: "Logo design".into(),
            description: String::new(),
            category: "design".into(),
            price: 1,
            unit: String::new(),
            pay_to: String::new(),
        };
        let listing = store.post(maya, &draft).unwrap().id;
        (store, maya, sam, listing)
    }

    #[test]
    fn a_buyer_and_a_seller_talk_and_each_is_emailed_once_until_they_read() {
        let (store, maya, sam, listing) = store_with_listing();
        assert!(store.conversation_about(maya, listing).is_err(), "not with oneself");
        let c = store.conversation_about(sam, listing).unwrap();
        assert_eq!(store.conversation_about(sam, listing).unwrap(), c, "one conversation per buyer and listing");
        assert!(store.conversations(sam).unwrap().is_empty(), "shown once it has a message");

        let (_, notice) = store.send(c, sam, "Hi! Could you do it by Friday?").unwrap();
        let notice = notice.expect("maya is told");
        assert_eq!((notice.to_email.as_str(), notice.from_name.as_str()), ("maya@example.com", "Sam"));
        let (_, again) = store.send(c, sam, "And in blue?").unwrap();
        assert!(again.is_none(), "one email until she reads");
        assert_eq!(store.unread(maya).unwrap(), 1);
        assert_eq!(store.unread(sam).unwrap(), 0, "one's own messages are read");

        let (seen, messages) = store.read(c, maya).unwrap();
        assert_eq!((seen.with_name.as_str(), seen.buying, messages.len()), ("Sam", false, 2));
        assert_eq!(store.unread(maya).unwrap(), 0);
        let (_, reply) = store.send(c, maya, "Friday works.").unwrap();
        assert_eq!(reply.unwrap().to_email, "sam@example.com");
        let (_, later) = store.send(c, sam, "Great").unwrap();
        assert!(later.is_some(), "told again after reading");

        // Asking for payment, and saying it is paid.
        assert!(store.request_payment(c, sam, 5).is_err(), "only the seller asks");
        let (request, _) = store.request_payment(c, maya, 200_000_000_000).unwrap();
        assert_eq!((request.kind.as_str(), request.amount, request.body.as_str()), ("request", Some(200_000_000_000), "Payment request: 200 ORN"));
        assert!(store.mark_paid(c, maya, request.id).is_err(), "only the buyer pays");
        let (paid, _) = store.mark_paid(c, sam, request.id).unwrap();
        assert_eq!((paid.kind.as_str(), paid.amount), ("paid", Some(200_000_000_000)));
        assert!(store.mark_paid(c, sam, paid.id).is_err(), "only a request is paid");

        let stranger = store.create_account("Eve", "eve").unwrap().id;
        assert!(store.read(c, stranger).is_err(), "only the two sides read it");
        assert!(store.send(c, stranger, "hi").is_err());
        assert!(store.send(c, sam, "   ").is_err());
    }

    #[test]
    fn a_block_stops_messages_until_lifted() {
        let (store, maya, sam, listing) = store_with_listing();
        let c = store.conversation_about(sam, listing).unwrap();
        store.send(c, sam, "Hello").unwrap();
        store.block(c, maya, true).unwrap();
        assert!(store.send(c, sam, "Hello?").is_err());
        assert!(store.send(c, maya, "Wait").is_err(), "a blocker unblocks to write");
        assert!(store.conversations(maya).unwrap()[0].blocked);
        store.block(c, maya, false).unwrap();
        store.send(c, sam, "Hello again").unwrap();
    }

    #[test]
    fn anyone_can_write_to_anyone_in_one_direct_conversation() {
        let (store, maya, sam, listing) = store_with_listing();
        assert!(store.direct_conversation(sam, "Sam").is_err(), "not to oneself");
        assert!(store.direct_conversation(sam, "nobody").is_err());
        let c = store.direct_conversation(sam, "maya").unwrap();
        assert_eq!(store.direct_conversation(maya, "Sam").unwrap(), c, "one per pair, whoever starts");
        assert_ne!(store.conversation_about(sam, listing).unwrap(), c, "apart from the listing's");

        let (_, notice) = store.send(c, sam, "Hi Maya").unwrap();
        let notice = notice.expect("maya is told");
        assert_eq!((notice.from_name.as_str(), notice.listing_title), ("Sam", None));
        let (seen, messages) = store.read(c, maya).unwrap();
        assert_eq!((seen.listing, seen.listing_title, seen.price, seen.buying), (None, None, None, false));
        assert_eq!((seen.with_name.as_str(), messages.len()), ("Sam", 1));
        assert!(store.request_payment(c, maya, 5).is_err(), "payments need a listing");
        assert!(store.request_payment(c, sam, 5).is_err());

        store.block(c, maya, true).unwrap();
        assert!(store.send(c, sam, "Hello?").is_err());
        assert!(store.direct_conversation(sam, "Maya").is_err(), "a block stops it before it starts");
        store.block(c, maya, false).unwrap();
        store.suspend(maya, "test").unwrap();
        assert!(store.direct_conversation(sam, "Maya").is_err(), "not to a suspended account");
    }

    #[test]
    fn people_choose_who_may_start_a_conversation_with_them() {
        let (store, maya, sam, listing) = store_with_listing();
        let lena = store.create_account("Lena", "lena").unwrap().id;
        assert!(store.takes_messages(sam, maya).unwrap(), "everyone, by default");

        store.set_messages_from(maya, "nobody").unwrap();
        let refused = store.direct_conversation(sam, "Maya").unwrap_err().to_string();
        assert_eq!(refused, "Maya isn't taking new messages");
        assert!(!store.takes_messages(sam, maya).unwrap());
        // A listing's messages always come through.
        let about = store.conversation_about(sam, listing).unwrap();
        store.send(about, sam, "Is it still available?").unwrap();

        store.set_messages_from(maya, "talked").unwrap();
        assert!(store.direct_conversation(lena, "Maya").is_err(), "Lena never talked with her");
        assert!(!store.takes_messages(lena, maya).unwrap());
        let direct = store.direct_conversation(sam, "Maya").unwrap();
        assert!(store.takes_messages(sam, maya).unwrap(), "Sam wrote about her listing");
        store.send(direct, sam, "Hi").unwrap();

        store.set_messages_from(maya, "nobody").unwrap();
        assert_eq!(store.direct_conversation(sam, "Maya").unwrap(), direct, "an existing conversation goes on");
        store.send(direct, sam, "Still here").unwrap();
        assert!(store.takes_messages(sam, maya).unwrap());
        assert!(store.set_messages_from(maya, "friends").is_err());
        assert_eq!(store.account(maya).unwrap().unwrap().messages_from, "nobody");
    }

    #[test]
    fn deleting_a_conversation_clears_it_for_one_side_until_a_new_message() {
        let (store, maya, sam, listing) = store_with_listing();
        let c = store.conversation_about(sam, listing).unwrap();
        store.send(c, sam, "Hi").unwrap();
        let (request, _) = store.request_payment(c, maya, 5).unwrap();
        store.send(c, sam, "Paying soon").unwrap();
        assert_eq!(store.unread(maya).unwrap(), 1);

        store.delete_conversation(c, maya).unwrap();
        assert!(store.conversations(maya).unwrap().is_empty(), "gone from her list");
        assert_eq!(store.unread(maya).unwrap(), 0);
        assert!(store.read(c, maya).is_err(), "nothing to open");
        let (_, his) = store.read(c, sam).unwrap();
        assert_eq!(his.len(), 3, "Sam keeps his");
        assert!(store.delete_conversation(c, store.create_account("Eve", "eve").unwrap().id).is_err(), "only the two sides");

        // A new message brings it back, starting from that message.
        store.mark_paid(c, sam, request.id).unwrap();
        assert_eq!(store.unread(maya).unwrap(), 1);
        let (seen, hers) = store.read(c, maya).unwrap();
        assert_eq!((hers.len(), hers[0].kind.as_str()), (1, "paid"));
        assert_eq!(seen.last.unwrap().kind, "paid");
        let (_, his) = store.read(c, sam).unwrap();
        assert_eq!(his.len(), 4);
    }

    #[test]
    fn direct_conversations_count_toward_the_daily_limit() {
        let (store, _, sam, _) = store_with_listing();
        for n in 0..super::NEW_CONVERSATIONS_A_DAY {
            let name = format!("Person_{n}");
            store.create_account(&name, &format!("h{n}")).unwrap();
            store.direct_conversation(sam, &name).unwrap();
        }
        store.create_account("One_more", "h99").unwrap();
        assert!(store.direct_conversation(sam, "One_more").is_err());
    }

    #[test]
    fn new_conversations_are_limited_each_day() {
        let (store, maya, sam, _) = store_with_listing();
        let draft = |n: i64| Draft {
            title: format!("Listing {n}"),
            description: String::new(),
            category: "other".into(),
            price: 1,
            unit: String::new(),
            pay_to: String::new(),
        };
        for n in 0..super::NEW_CONVERSATIONS_A_DAY {
            let listing = store.post(maya, &draft(n)).unwrap().id;
            store.conversation_about(sam, listing).unwrap();
        }
        let one_more = store.post(maya, &draft(99)).unwrap().id;
        assert!(store.conversation_about(sam, one_more).is_err());
    }
}
