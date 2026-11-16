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
/// How long a device's claim to run a taken offer lasts without its offer code.
pub const RUN_SECONDS: u64 = 5 * 60;
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
    /// For the seller: the Bitcoin address to be paid at, when the offer
    /// says so (offers posted from the website, and newer wallets).
    pub payout: Option<String>,
    /// For the seller: the Orien address the ORN come from.
    pub wallet: Option<String>,
    /// For the seller: the device running the swap, once one claimed it.
    pub runner: Option<String>,
    pub taker: Option<i64>,
    pub taker_name: Option<String>,
    /// The buyer's Orien address, which the seller locks the ORN for.
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

/// A Bitcoin address, checked, to be paid at.
fn bitcoin_address(text: &str) -> Result<String> {
    use std::str::FromStr;
    let text = text.trim();
    orien_swap::bitcoin::Address::from_str(text)
        .ok()
        .and_then(|address| address.require_network(orien_swap::bitcoin::Network::Bitcoin).ok())
        .map(|_| text.to_string())
        .ok_or_else(|| anyhow::anyhow!("that is not a Bitcoin address"))
}

impl Store {
    pub(crate) fn create_swap_tables(&self) -> Result<()> {
        self.create_swap_tables_first()?;
        // Added after the board was first built: where the buyer wants the ORN.
        let has: bool = self
            .db
            .prepare("SELECT 1 FROM pragma_table_info('swap_offers') WHERE name = 'taker_address'")?
            .exists([])?;
        if !has {
            self.db.execute("ALTER TABLE swap_offers ADD COLUMN taker_address TEXT", [])?;
        }
        // Added later still: what any of the seller's wallets needs to run
        // the swap (the Bitcoin address to be paid at, and the wallet the
        // ORN come from), and which of the seller's devices is running it.
        let has_payout: bool = self
            .db
            .prepare("SELECT 1 FROM pragma_table_info('swap_offers') WHERE name = 'payout'")?
            .exists([])?;
        if !has_payout {
            self.db.execute_batch(
                "ALTER TABLE swap_offers ADD COLUMN payout TEXT;
                 ALTER TABLE swap_offers ADD COLUMN wallet TEXT;
                 ALTER TABLE swap_offers ADD COLUMN runner TEXT;
                 ALTER TABLE swap_offers ADD COLUMN runner_at INTEGER;",
            )?;
        }
        Ok(())
    }

    fn create_swap_tables_first(&self) -> Result<()> {
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
            // Whose step was it? Before the offer code, the seller's wallet
            // was to lock the ORN: the buyer did nothing wrong.
            let buyers_turn: bool =
                self.db.query_row("SELECT offer_code IS NOT NULL FROM swap_offers WHERE id = ?1", [id], |r| r.get(0))?;
            self.db.execute(
                "UPDATE swap_offers SET state = 'open', taker = NULL, taker_address = NULL, offer_code = NULL, due = NULL,
                     runner = NULL, runner_at = NULL, updated = ?2
                 WHERE id = ?1",
                params![id, at],
            )?;
            if buyers_turn {
                self.db.execute("INSERT INTO swap_lapses (account, at) VALUES (?1, ?2)", params![taker, at])?;
            }
        }
        Ok(())
    }

    /// Puts an offer of `orn` for `sats` on the board.
    /// No verified wallet is needed: the ORN locked on the network are the
    /// proof, and the buyer pays only once their wallet has seen them.
    pub fn post_swap(&self, seller: i64, orn: u64, sats: u64) -> Result<SwapOffer> {
        self.post_swap_for(seller, orn, sats, None, None)
    }

    /// `post_swap`, saying where the Bitcoin go and which wallet sells, so
    /// that any of the seller's wallets can run it.
    pub fn post_swap_for(&self, seller: i64, orn: u64, sats: u64, payout: Option<&str>, wallet: Option<&str>) -> Result<SwapOffer> {
        let payout = payout.map(bitcoin_address).transpose()?;
        let wallet = wallet.map(crate::store::orien_address).transpose()?;
        if orn == 0 {
            bail!("offer more than nothing");
        }
        if sats < MIN_SATS {
            bail!(
                "a swap is at least {MIN_SATS} satoshis ({} BTC), so that any Bitcoin wallet can send it and its fees leave enough",
                format!("{:.8}", MIN_SATS as f64 / 1e8).trim_end_matches('0')
            );
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
            "INSERT INTO swap_offers (seller, orn, sats, state, created, updated, payout, wallet) VALUES (?1, ?2, ?3, 'open', ?4, ?4, ?5, ?6)",
            params![seller, orn as i64, sats as i64, at, payout, wallet],
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
    /// seller's wallet locks the ORN for `address`: the buyer's wallet sends
    /// its own. No verified wallet and no ORN are needed, so someone with
    /// none can buy their first. A wrong address only hurts its giver, whose
    /// wallet would never see ORN locked for it, and so never pay.
    pub fn take_swap(&self, buyer: i64, id: i64, address: &str) -> Result<MySwap> {
        self.lapse_swaps()?;
        let address = crate::store::orien_address(address)?;
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
            "UPDATE swap_offers SET state = 'held', taker = ?2, taker_address = ?5, due = ?3, updated = ?4
             WHERE id = ?1 AND state = 'open'",
            params![id, buyer, at + HOLD_SECONDS, at, address],
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
        self.post_offer_code_from(seller, id, code, None)
    }

    /// `post_offer_code`, from `device`: refused when another of the
    /// seller's devices is running this swap.
    pub fn post_offer_code_from(&self, seller: i64, id: i64, code: &str, device: Option<&str>) -> Result<MySwap> {
        self.lapse_swaps()?;
        let swap = self.my_swap(id, seller)?;
        if !swap.selling || swap.offer.state != "held" {
            bail!("this offer is not waiting for your wallet");
        }
        if let Some(runner) = &swap.runner
            && device != Some(runner.as_str())
        {
            bail!("another of your devices is running this swap");
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

    /// One of the seller's devices asks to run this taken offer. The first
    /// to ask runs it; a claim lapses if no offer code follows within
    /// RUN_SECONDS, since nothing is locked yet. Once the offer code is in,
    /// the runner is settled: only it knows the secret.
    pub fn run_swap(&self, seller: i64, id: i64, device: &str) -> Result<bool> {
        let device = device.trim();
        if device.is_empty() || device.len() > 100 {
            bail!("name this device with 1 to 100 characters");
        }
        self.lapse_swaps()?;
        let swap = self.my_swap(id, seller)?;
        if !swap.selling {
            bail!("only the seller's wallets run an offer");
        }
        let (runner, runner_at): (Option<String>, Option<u64>) =
            self.db.query_row("SELECT runner, runner_at FROM swap_offers WHERE id = ?1", [id], |r| Ok((r.get(0)?, r.get(1)?)))?;
        if runner.as_deref() == Some(device) {
            return Ok(true);
        }
        if swap.offer.state != "held" || swap.offer_code.is_some() {
            return Ok(false);
        }
        let lapsed = runner_at.is_none_or(|at| now() >= at + RUN_SECONDS);
        if runner.is_some() && !lapsed {
            return Ok(false);
        }
        self.db.execute("UPDATE swap_offers SET runner = ?2, runner_at = ?3 WHERE id = ?1", params![id, device, now()])?;
        Ok(true)
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
        type Row = (i64, Option<i64>, Option<String>, Option<String>, Option<u64>, Option<String>, [Option<String>; 3]);
        let found: Option<Row> = self
            .db
            .query_row(
                "SELECT seller, taker, offer_code, reply_code, due, taker_address, payout, wallet, runner FROM swap_offers WHERE id = ?1",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, [r.get(6)?, r.get(7)?, r.get(8)?])),
            )
            .optional()?;
        let Some((seller, taker, offer_code, reply_code, due, taker_address, [payout, wallet, runner])) = found else {
            bail!("there is no such offer")
        };
        if account != seller && Some(account) != taker {
            bail!("there is no such swap of yours");
        }
        let taker_account = match taker {
            Some(t) => self.account(t)?,
            None => None,
        };
        // Only the seller's own wallets need these.
        let selling = account == seller;
        let (payout, wallet, runner) = if selling { (payout, wallet, runner) } else { (None, None, None) };
        Ok(MySwap {
            offer: self.swap_offer(id)?.expect("just read"),
            selling,
            payout,
            wallet,
            runner,
            taker,
            taker_name: taker_account.as_ref().map(|a| a.name.clone()),
            taker_wallet: taker_address,
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
        assert!(store.take_swap(maya, posted.id, MAYA_WALLET).is_err(), "not one's own");

        let newcomer = store.create_account("Nina", "nina").unwrap().id;
        assert!(store.take_swap(newcomer, posted.id, "not an address").is_err());
        let held = store.take_swap(sam, posted.id, SAM_WALLET).unwrap();
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
        store.take_swap(sam, id, SAM_WALLET).unwrap();
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
        store.take_swap(sam, first, SAM_WALLET).unwrap();
        assert!(store.take_swap(sam, store.post_swap(maya, 2, MIN_SATS).unwrap().id, SAM_WALLET).is_err(), "one at a time");
        // The seller's wallet never came: the hold lapses, the buyer is not blamed.
        store.db.execute("UPDATE swap_offers SET due = 1 WHERE taker = ?1", [sam]).unwrap();
        store.lapse_swaps().unwrap();
        assert_eq!(store.swap_offer(first).unwrap().unwrap().state, "open", "a lapsed hold reopens");
        let blamed: i64 = store.db.query_row("SELECT COUNT(*) FROM swap_lapses", [], |r| r.get(0)).unwrap();
        assert_eq!(blamed, 0, "the seller's step was missed, not the buyer's");
        // The buyer never paid: that counts.
        for _ in 0..LAPSES_ALLOWED {
            store.take_swap(sam, first, SAM_WALLET).unwrap();
            store.post_offer_code(maya, first, &offer_code(1, MIN_SATS, SAM_WALLET).0).unwrap();
            store.db.execute("UPDATE swap_offers SET due = 1 WHERE taker = ?1", [sam]).unwrap();
            store.lapse_swaps().unwrap();
            assert_eq!(store.swap_offer(first).unwrap().unwrap().state, "open");
        }
        let err = store.take_swap(sam, first, SAM_WALLET).unwrap_err().to_string();
        assert!(err.contains("lapsed"), "{err}");
    }

    #[test]
    fn any_of_the_sellers_devices_runs_a_taken_offer_but_only_one() {
        let (store, maya, sam) = board();
        let payout = "bc1qar0srrr7xfkvy5l643lydnw9re59gtzzwf5mdq";
        assert!(store.post_swap_for(maya, 5, MIN_SATS, Some("not bitcoin"), Some(MAYA_WALLET)).is_err());
        assert!(store.post_swap_for(maya, 5, MIN_SATS, Some(payout), Some("123")).is_err());
        let id = store.post_swap_for(maya, 5, MIN_SATS, Some(payout), Some(MAYA_WALLET)).unwrap().id;
        assert!(!store.run_swap(maya, id, "mac").unwrap(), "nothing to run until it is taken");
        store.take_swap(sam, id, SAM_WALLET).unwrap();
        let mine = store.my_swap(id, maya).unwrap();
        assert_eq!((mine.payout.as_deref(), mine.wallet.as_deref()), (Some(payout), Some(MAYA_WALLET)));
        assert_eq!(store.my_swap(id, sam).unwrap().payout, None, "the buyer is not shown it");
        assert!(store.run_swap(sam, id, "phone").is_err(), "only the seller's devices");

        assert!(store.run_swap(maya, id, "mac").unwrap());
        assert!(store.run_swap(maya, id, "mac").unwrap(), "the same device again");
        assert!(!store.run_swap(maya, id, "linux").unwrap(), "another device waits");
        assert_eq!(store.my_swap(id, maya).unwrap().runner.as_deref(), Some("mac"));

        // The Mac went to sleep before locking anything: after five minutes
        // another device may take over, and the Mac's late code is refused.
        store.db.execute("UPDATE swap_offers SET runner_at = runner_at - ?2 WHERE id = ?1", params![id, RUN_SECONDS as i64]).unwrap();
        assert!(store.run_swap(maya, id, "linux").unwrap());
        let code = offer_code(5, MIN_SATS, SAM_WALLET).0;
        assert!(store.post_offer_code_from(maya, id, &code, Some("mac")).is_err());
        assert!(store.post_offer_code_from(maya, id, &code, None).is_err(), "an old wallet cannot cut in either");
        store.post_offer_code_from(maya, id, &code, Some("linux")).unwrap();

        // With the code in, the runner is settled for good.
        store.db.execute("UPDATE swap_offers SET runner_at = 0 WHERE id = ?1", [id]).unwrap();
        assert!(!store.run_swap(maya, id, "mac").unwrap());
        assert!(store.run_swap(maya, id, "linux").unwrap());
    }

    #[test]
    fn offers_are_checked() {
        let (store, maya, _) = board();
        assert!(store.post_swap(maya, 0, MIN_SATS).is_err());
        assert!(store.post_swap(maya, 1, MIN_SATS - 1).is_err());
        let stranger = store.create_account("Eve", "eve").unwrap().id;
        assert!(store.post_swap(stranger, 1, MIN_SATS).is_ok(), "no verified wallet needed: the lock is the proof");
        for _ in 0..MAX_OFFERS {
            store.post_swap(maya, 1, MIN_SATS).unwrap();
        }
        assert!(store.post_swap(maya, 1, MIN_SATS).is_err());
        let mine = store.open_swaps().unwrap().into_iter().find(|o| o.seller == maya).unwrap();
        store.close_swap(maya, mine.id).unwrap();
        let left = store.open_swaps().unwrap().into_iter().filter(|o| o.seller == maya).count() as i64;
        assert_eq!(left, MAX_OFFERS - 1);
    }
}
