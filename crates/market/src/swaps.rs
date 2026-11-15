//! The swap board: offers of ORN for Bitcoin, and the codes the two
//! wallets pass each other through it. The swap itself runs in the wallets;
//! the board only carries messages and cannot touch anyone's coins.
//!
//! An offer is open; a buyer takes it and it is held for them; the seller's
//! wallet locks the ORN and posts its offer code; the buyer's wallet pays the
//! Bitcoin and posts its reply code; the swap runs and either side marks it
//! done or cancelled. A hold whose next step does not come in time lapses,
//! the offer opens again, and the buyer is counted as having let it lapse.

use crate::store::{Store, now};
use anyhow::{Result, bail};
use orien_swap::swap::{Offer, Reply};
use rusqlite::{OptionalExtension, params};
use serde::Serialize;

/// How long each step may take before the hold lapses.
pub const HOLD_SECONDS: u64 = 30 * 60;
/// Offers one account may have on the board at once.
pub const MAX_OFFERS: i64 = 5;
/// Lapses in a week after which an account may not take offers for a week.
pub const LAPSES_ALLOWED: i64 = 2;
const WEEK: u64 = 7 * 24 * 3600;
/// The smallest swap, as the wallet allows on a quiet Bitcoin day. On a
/// busy one the wallet asks for more before anyone pays.
pub const MIN_SATS: u64 = orien_swap::swap::MIN_SATS;

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct SwapOffer {
    pub id: i64,
    pub seller: i64,
    pub seller_name: String,
    pub seller_picture: Option<String>,
    /// Swaps the seller has finished, as buyer or seller.
    pub seller_done: i64,
    pub orn: u64,
    pub sats: u64,
    /// open, held, swapping, done, cancelled or closed.
    pub state: String,
    pub created: u64,
}

/// An offer as one of its two sides sees it, with what their wallet needs.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct MySwap {
    pub offer: SwapOffer,
    pub selling: bool,
    pub taker: Option<i64>,
    pub taker_name: Option<String>,
    /// The buyer's verified wallet, which the seller locks the ORN for.
    pub taker_wallet: Option<String>,
    pub offer_code: Option<String>,
    pub reply_code: Option<String>,
    /// When the current step lapses, while the offer is held or swapping.
    pub due: Option<u64>,
    /// Whether this side's wallet has reported how the swap ended.
    pub finished: bool,
}

const OFFER: &str = "SELECT o.id, o.seller, a.name, a.picture,
        (SELECT COUNT(*) FROM swap_offers d WHERE d.state = 'done' AND (d.seller = o.seller OR d.taker = o.seller)),
        o.orn, o.sats, o.state, o.created
     FROM swap_offers o JOIN accounts a ON a.id = o.seller";

fn offer_row(row: &rusqlite::Row) -> rusqlite::Result<SwapOffer> {
    Ok(SwapOffer {
        id: row.get(0)?,
        seller: row.get(1)?,
        seller_name: row.get(2)?,
        seller_picture: row.get(3)?,
        seller_done: row.get(4)?,
        orn: row.get::<_, i64>(5)? as u64,
        sats: row.get::<_, i64>(6)? as u64,
        state: row.get(7)?,
        created: row.get(8)?,
    })
}

impl Store {
    pub(crate) fn create_swap_tables(&self) -> Result<()> {
        self.db.execute_batch(
            "CREATE TABLE IF NOT EXISTS swap_offers (
                 id INTEGER PRIMARY KEY,
                 seller INTEGER NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
                 orn INTEGER NOT NULL,
                 sats INTEGER NOT NULL,
                 state TEXT NOT NULL,
                 taker INTEGER REFERENCES accounts(id) ON DELETE SET NULL,
                 offer_code TEXT,
                 reply_code TEXT,
                 due INTEGER,
                 created INTEGER NOT NULL,
                 updated INTEGER NOT NULL
             );
             CREATE INDEX IF NOT EXISTS swap_offers_state ON swap_offers(state, created);
             CREATE TABLE IF NOT EXISTS swap_finishes (
                 offer INTEGER NOT NULL REFERENCES swap_offers(id) ON DELETE CASCADE,
                 account INTEGER NOT NULL,
                 done INTEGER NOT NULL,
                 at INTEGER NOT NULL,
                 PRIMARY KEY (offer, account)
             );
             CREATE TABLE IF NOT EXISTS swap_lapses (
                 account INTEGER NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
                 at INTEGER NOT NULL
             );",
        )?;
        Ok(())
    }

    /// Lets holds whose next step did not come in time lapse: the offer
    /// opens again and the buyer is counted.
    pub fn lapse_swaps(&self) -> Result<()> {
        let at = now();
        // A swap only one side ever reported on is finished three days later.
        let lonely: Vec<i64> = {
            let mut statement = self.db.prepare(
                "SELECT o.id FROM swap_offers o WHERE o.state = 'swapping'
                 AND (SELECT COUNT(*) FROM swap_finishes f WHERE f.offer = o.id) = 1
                 AND (SELECT MIN(at) FROM swap_finishes f WHERE f.offer = o.id) < ?1",
            )?;
            statement.query_map([at.saturating_sub(3 * 24 * 3600)], |row| row.get(0))?.collect::<rusqlite::Result<_>>()?
        };
        for id in lonely {
            self.settle_swap(id, true)?;
        }
        let lapsed: Vec<(i64, i64)> = {
            let mut statement = self
                .db
                .prepare("SELECT id, taker FROM swap_offers WHERE state IN ('held', 'swapping') AND due < ?1 AND taker IS NOT NULL")?;
            statement.query_map([at], |row| Ok((row.get(0)?, row.get(1)?)))?.collect::<rusqlite::Result<_>>()?
        };
        for (id, taker) in lapsed {
            // Once the buyer has paid and replied, nothing lapses: the swap
            // finishes in the wallets, and either side marks it.
            let replied: bool = self.db.query_row("SELECT reply_code IS NOT NULL FROM swap_offers WHERE id = ?1", [id], |r| r.get(0))?;
            if replied {
                continue;
            }
            self.db.execute(
                "UPDATE swap_offers SET state = 'open', taker = NULL, offer_code = NULL, due = NULL, updated = ?2 WHERE id = ?1",
                params![id, at],
            )?;
            self.db.execute("INSERT INTO swap_lapses (account, at) VALUES (?1, ?2)", params![taker, at])?;
        }
        Ok(())
    }

    /// Puts an offer of `orn` for `sats` on the board.
    pub fn post_swap(&self, seller: i64, orn: u64, sats: u64) -> Result<SwapOffer> {
        if self.account(seller)?.and_then(|a| a.wallet).is_none() {
            bail!("verify your Orien wallet first, under Your profile");
        }
        if orn == 0 {
            bail!("offer more than nothing");
        }
        if sats < MIN_SATS {
            bail!("a swap is at least {MIN_SATS} satoshis (0.00005 BTC), so that Bitcoin's fees leave enough");
        }
        let open: i64 = self.db.query_row(
            "SELECT COUNT(*) FROM swap_offers WHERE seller = ?1 AND state IN ('open', 'held', 'swapping')",
            [seller],
            |r| r.get(0),
        )?;
        if open >= MAX_OFFERS {
            bail!("you can have {MAX_OFFERS} swap offers on the board at once");
        }
        let at = now();
        self.db.execute(
            "INSERT INTO swap_offers (seller, orn, sats, state, created, updated) VALUES (?1, ?2, ?3, 'open', ?4, ?4)",
            params![seller, orn as i64, sats as i64, at],
        )?;
        Ok(self.swap_offer(self.db.last_insert_rowid())?.expect("just written"))
    }

    pub fn swap_offer(&self, id: i64) -> Result<Option<SwapOffer>> {
        let sql = format!("{OFFER} WHERE o.id = ?1");
        Ok(self.db.query_row(&sql, [id], offer_row).optional()?)
    }

    /// Open offers, cheapest Bitcoin per ORN first.
    pub fn open_swaps(&self) -> Result<Vec<SwapOffer>> {
        self.lapse_swaps()?;
        let sql = format!("{OFFER} WHERE o.state = 'open' ORDER BY CAST(o.sats AS REAL) / o.orn, o.created LIMIT 200");
        let mut statement = self.db.prepare(&sql)?;
        Ok(statement.query_map([], offer_row)?.collect::<rusqlite::Result<_>>()?)
    }

    /// Takes an offer off the board. The offer is held for `buyer` while the
    /// seller's wallet locks the ORN.
    pub fn take_swap(&self, buyer: i64, id: i64) -> Result<MySwap> {
        self.lapse_swaps()?;
        let wallet = self.account(buyer)?.and_then(|a| a.wallet);
        if wallet.is_none() {
            bail!("verify your Orien wallet first, under Your profile, so the seller knows where to lock the ORN");
        }
        let lapses: i64 = self.db.query_row(
            "SELECT COUNT(*) FROM swap_lapses WHERE account = ?1 AND at > ?2",
            params![buyer, now().saturating_sub(WEEK)],
            |r| r.get(0),
        )?;
        if lapses >= LAPSES_ALLOWED {
            bail!("swaps you took lapsed {lapses} times this week, so you can take offers again in a few days");
        }
        let busy: i64 = self.db.query_row(
            "SELECT COUNT(*) FROM swap_offers WHERE taker = ?1 AND state IN ('held', 'swapping')",
            [buyer],
            |r| r.get(0),
        )?;
        if busy > 0 {
            bail!("finish the swap you have under way before taking another");
        }
        let Some(offer) = self.swap_offer(id)? else { bail!("there is no such offer") };
        if offer.seller == buyer {
            bail!("this is your own offer");
        }
        let at = now();
        let taken = self.db.execute(
            "UPDATE swap_offers SET state = 'held', taker = ?2, due = ?3, updated = ?4 WHERE id = ?1 AND state = 'open'",
            params![id, buyer, at + HOLD_SECONDS, at],
        )?;
        if taken == 0 {
            bail!("someone else took this offer a moment ago");
        }
        self.my_swap(id, buyer)
    }

    /// The seller withdraws an offer nobody holds.
    pub fn close_swap(&self, seller: i64, id: i64) -> Result<()> {
        let closed = self.db.execute(
            "UPDATE swap_offers SET state = 'closed', updated = ?3 WHERE id = ?1 AND seller = ?2 AND state = 'open'",
            params![id, seller, now()],
        )?;
        if closed == 0 {
            bail!("only an offer of yours that nobody has taken can be withdrawn");
        }
        Ok(())
    }

    /// The seller's wallet has locked the ORN: its offer code, for the buyer.
    pub fn post_offer_code(&self, seller: i64, id: i64, code: &str) -> Result<MySwap> {
        self.lapse_swaps()?;
        let swap = self.my_swap(id, seller)?;
        if !swap.selling || swap.offer.state != "held" {
            bail!("this offer is not waiting for your wallet");
        }
        let offer = Offer::read(code.trim())?;
        let expected = swap.taker_wallet.as_deref().unwrap_or_default();
        if offer.orn != swap.offer.orn || offer.sats != swap.offer.sats || offer.buyer.to_string() != expected {
            bail!("that offer code is not for this swap");
        }
        self.db.execute(
            "UPDATE swap_offers SET state = 'swapping', offer_code = ?2, due = ?3, updated = ?4 WHERE id = ?1",
            params![id, code.trim(), now() + HOLD_SECONDS, now()],
        )?;
        self.my_swap(id, seller)
    }

    /// The buyer's wallet has paid the Bitcoin: its reply code, for the seller.
    pub fn post_reply_code(&self, buyer: i64, id: i64, code: &str) -> Result<MySwap> {
        self.lapse_swaps()?;
        let swap = self.my_swap(id, buyer)?;
        if swap.selling || swap.offer.state != "swapping" || swap.reply_code.is_some() {
            bail!("this swap is not waiting for your wallet");
        }
        let reply = Reply::read(code.trim())?;
        let offer = Offer::read(swap.offer_code.as_deref().unwrap_or_default())?;
        if reply.hash != offer.hash {
            bail!("that reply code is not for this swap");
        }
        self.db.execute(
            "UPDATE swap_offers SET reply_code = ?2, due = NULL, updated = ?3 WHERE id = ?1",
            params![id, code.trim(), now()],
        )?;
        self.my_swap(id, buyer)
    }

    /// One side's wallet reports how a swap under way ended for it: done, or
    /// cancelled (its coins came back). The swap is finished once both have
    /// said so, done only if both say done; or, if only one side ever
    /// reports, three days after it did.
    pub fn finish_swap(&self, account: i64, id: i64, done: bool) -> Result<()> {
        let swap = self.my_swap(id, account)?;
        if swap.offer.state != "swapping" {
            bail!("this swap is not under way");
        }
        self.db.execute(
            "INSERT OR REPLACE INTO swap_finishes (offer, account, done, at) VALUES (?1, ?2, ?3, ?4)",
            params![id, account, done, now()],
        )?;
        self.settle_swap(id, false)
    }

    /// Finishes a swap both sides have reported on, or, with `waited`, one
    /// that one side reported on long enough ago.
    fn settle_swap(&self, id: i64, waited: bool) -> Result<()> {
        let (reports, all_done): (i64, bool) = self.db.query_row(
            "SELECT COUNT(*), COALESCE(MIN(done), 0) FROM swap_finishes WHERE offer = ?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        if reports >= 2 || (waited && reports == 1) {
            let state = if all_done { "done" } else { "cancelled" };
            self.db.execute("UPDATE swap_offers SET state = ?2, due = NULL, updated = ?3 WHERE id = ?1", params![id, state, now()])?;
        }
        Ok(())
    }

    /// One offer as `account`, its seller or its buyer, sees it.
    pub fn my_swap(&self, id: i64, account: i64) -> Result<MySwap> {
        type Row = (i64, Option<i64>, Option<String>, Option<String>, Option<u64>);
        let found: Option<Row> = self
            .db
            .query_row(
                "SELECT seller, taker, offer_code, reply_code, due FROM swap_offers WHERE id = ?1",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
            )
            .optional()?;
        let Some((seller, taker, offer_code, reply_code, due)) = found else { bail!("there is no such offer") };
        if account != seller && Some(account) != taker {
            bail!("there is no such swap of yours");
        }
        let taker_account = match taker {
            Some(t) => self.account(t)?,
            None => None,
        };
        Ok(MySwap {
            offer: self.swap_offer(id)?.expect("just read"),
            selling: account == seller,
            taker,
            taker_name: taker_account.as_ref().map(|a| a.name.clone()),
            taker_wallet: taker_account.and_then(|a| a.wallet),
            offer_code,
            reply_code,
            due,
            finished: self
                .db
                .query_row("SELECT 1 FROM swap_finishes WHERE offer = ?1 AND account = ?2", params![id, account], |_| Ok(()))
                .optional()?
                .is_some(),
        })
    }

    /// Every offer `account` posted or took, newest first: what its wallet
    /// polls to know what to do next.
    pub fn my_swaps(&self, account: i64) -> Result<Vec<MySwap>> {
        self.lapse_swaps()?;
        let ids: Vec<i64> = {
            let mut statement = self.db.prepare(
                "SELECT id FROM swap_offers WHERE (seller = ?1 OR taker = ?1) AND state != 'closed' ORDER BY updated DESC LIMIT 50",
            )?;
            statement.query_map([account], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?
        };
        ids.into_iter().map(|id| self.my_swap(id, account)).collect()
    }

    /// Swaps an account has finished, as buyer or seller.
    pub fn swaps_done(&self, account: i64) -> Result<i64> {
        Ok(self.db.query_row(
            "SELECT COUNT(*) FROM swap_offers WHERE state = 'done' AND (seller = ?1 OR taker = ?1)",
            [account],
            |r| r.get(0),
        )?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::Store;
    use orien_swap::swap::LockRef;

    const MAYA_WALLET: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const SAM_WALLET: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

    fn board() -> (Store, i64, i64) {
        let store = Store::in_memory().unwrap();
        let maya = store.create_account("Maya", "maya").unwrap().id;
        let sam = store.create_account("Sam", "sam").unwrap().id;
        store.set_wallet(maya, MAYA_WALLET).unwrap();
        store.set_wallet(sam, SAM_WALLET).unwrap();
        (store, maya, sam)
    }

    fn offer_code(orn: u64, sats: u64, buyer: &str) -> (String, [u8; 32]) {
        let offer = Offer {
            network: "orien".into(),
            orn,
            sats,
            hash: [7; 32],
            lock: LockRef { from: MAYA_WALLET.parse().unwrap(), seq: 1 },
            buyer: buyer.parse().unwrap(),
            key: "02".to_string() + &"11".repeat(32),
        };
        (offer.code(), offer.hash)
    }

    #[test]
    fn a_swap_goes_from_offer_to_done_through_the_board() {
        let (store, maya, sam) = board();
        let posted = store.post_swap(maya, 500_000_000_000, 1_000_000).unwrap();
        assert_eq!(store.open_swaps().unwrap(), vec![posted.clone()]);
        assert!(store.take_swap(maya, posted.id).is_err(), "not one's own");

        let held = store.take_swap(sam, posted.id).unwrap();
        assert_eq!((held.offer.state.as_str(), held.selling), ("held", false));
        assert!(store.open_swaps().unwrap().is_empty(), "held for one buyer only");
        let mine = store.my_swaps(maya).unwrap();
        assert_eq!(mine[0].taker_wallet.as_deref(), Some(SAM_WALLET), "the seller's wallet learns where to lock");

        let (wrong, _) = offer_code(500_000_000_000, 1_000_000, MAYA_WALLET);
        assert!(store.post_offer_code(maya, posted.id, &wrong).is_err(), "an offer for someone else");
        assert!(store.post_offer_code(maya, posted.id, "orien-offer:nonsense").is_err());
        let (code, hash) = offer_code(500_000_000_000, 1_000_000, SAM_WALLET);
        assert!(store.post_offer_code(sam, posted.id, &code).is_err(), "only the seller's wallet");
        let swapping = store.post_offer_code(maya, posted.id, &code).unwrap();
        assert_eq!(swapping.offer.state, "swapping");
        assert_eq!(store.my_swaps(sam).unwrap()[0].offer_code.as_deref(), Some(code.as_str()));

        let other = Reply { hash: [9; 32], key: "02".to_string() + &"22".repeat(32), height: 900_000 }.code();
        assert!(store.post_reply_code(sam, posted.id, &other).is_err(), "a reply for another swap");
        let reply = Reply { hash, key: "02".to_string() + &"22".repeat(32), height: 900_000 }.code();
        store.post_reply_code(sam, posted.id, &reply).unwrap();
        assert_eq!(store.my_swaps(maya).unwrap()[0].reply_code.as_deref(), Some(reply.as_str()));

        // Done only once both wallets say so.
        store.finish_swap(sam, posted.id, true).unwrap();
        assert_eq!(store.swap_offer(posted.id).unwrap().unwrap().state, "swapping", "the seller is still claiming");
        assert!(store.my_swap(posted.id, sam).unwrap().finished);
        assert!(!store.my_swap(posted.id, maya).unwrap().finished);
        store.finish_swap(maya, posted.id, true).unwrap();
        assert_eq!(store.swap_offer(posted.id).unwrap().unwrap().state, "done");
        assert_eq!((store.swaps_done(maya).unwrap(), store.swaps_done(sam).unwrap()), (1, 1));
    }

    #[test]
    fn a_swap_one_side_reports_on_finishes_after_three_days_and_any_cancel_counts() {
        let (store, maya, sam) = board();
        let id = store.post_swap(maya, 1, MIN_SATS).unwrap().id;
        store.take_swap(sam, id).unwrap();
        store.db.execute("UPDATE swap_offers SET state = 'swapping', reply_code = 'r' WHERE id = ?1", [id]).unwrap();
        store.finish_swap(maya, id, false).unwrap();
        store.lapse_swaps().unwrap();
        assert_eq!(store.swap_offer(id).unwrap().unwrap().state, "swapping");
        store.db.execute("UPDATE swap_finishes SET at = 1", []).unwrap();
        store.lapse_swaps().unwrap();
        assert_eq!(store.swap_offer(id).unwrap().unwrap().state, "cancelled");
        assert_eq!(store.swaps_done(maya).unwrap(), 0);
    }

    #[test]
    fn holds_lapse_and_repeat_lapsers_are_paused() {
        let (store, maya, sam) = board();
        let first = store.post_swap(maya, 1, MIN_SATS).unwrap().id;
        store.take_swap(sam, first).unwrap();
        assert!(store.take_swap(sam, store.post_swap(maya, 2, MIN_SATS).unwrap().id).is_err(), "one at a time");
        for _ in 0..LAPSES_ALLOWED {
            store.db.execute("UPDATE swap_offers SET due = 1 WHERE taker = ?1", [sam]).unwrap();
            store.lapse_swaps().unwrap();
            assert_eq!(store.swap_offer(first).unwrap().unwrap().state, "open", "a lapsed hold reopens");
            let _ = store.take_swap(sam, first);
        }
        let err = store.take_swap(sam, first).unwrap_err().to_string();
        assert!(err.contains("lapsed"), "{err}");
    }

    #[test]
    fn offers_are_checked() {
        let (store, maya, _) = board();
        assert!(store.post_swap(maya, 0, MIN_SATS).is_err());
        assert!(store.post_swap(maya, 1, MIN_SATS - 1).is_err());
        let stranger = store.create_account("Eve", "eve").unwrap().id;
        assert!(store.post_swap(stranger, 1, MIN_SATS).is_err(), "a verified wallet first");
        for _ in 0..MAX_OFFERS {
            store.post_swap(maya, 1, MIN_SATS).unwrap();
        }
        assert!(store.post_swap(maya, 1, MIN_SATS).is_err());
        let open = store.open_swaps().unwrap();
        store.close_swap(maya, open[0].id).unwrap();
        assert_eq!(store.open_swaps().unwrap().len() as i64, MAX_OFFERS - 1);
    }
}
