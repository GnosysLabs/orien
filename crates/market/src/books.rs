//! Buy orders that fill in parts, on Solana's `orien-book` program.
//!
//! The buyer's wallet locks SOL in an order that commits to 64 secret
//! hashes, and gives the 64 secrets to this server over TLS: kept only in
//! SQLite, never returned by any endpoint or logged. A seller asks to sell
//! part of it: this server (the order's named filler) reserves that part on
//! Solana under a secret not used before, and tells the seller its hash.
//! The seller locks that many ORN on Orien for the buyer under that hash and
//! reports the lock. Only after checking the lock directly on Orien does
//! this server claim the ORN for the buyer with the part's secret, and then
//! the part's SOL for the seller with the same secret. A part whose seller
//! never locks goes back into the order after its deadline.
//!
//! Orders on the older `orien-order` program (whole fills only) carry on in
//! `orders.rs` until they're filled, cancelled or expire.

use crate::orders::verify_orien_lock;
use crate::store::{Store, now};
use anyhow::{Context, Result, anyhow, bail};
use orien_ledger::AccountId;
use orien_swap::book::{self, BookAccount, SECRETS, Secrets};
use orien_swap::solana::{self, Keypair, Pubkey, Solana};
use rusqlite::{OptionalExtension, params};
use serde::Serialize;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// Most open part-filling buy orders one account may have at once.
pub const MAX_BOOK_ORDERS: i64 = 20;
/// How long a part whose fill never reached Solana is kept before it's given up.
const FILL_LANDS_SECONDS: u64 = 300;
/// ORN are claimed for the buyer only with at least this long left to pay the
/// seller on Solana: once the buyer has the ORN, the seller's SOL must follow.
const CLAIM_MARGIN_SECONDS: i64 = 600;

/// How this server reads `orien-book` orders and sends its transactions.
pub trait BookChain: Send + Sync {
    fn program(&self) -> Pubkey;
    /// The key that signs fills: the filler every order must name.
    fn filler(&self) -> Option<Pubkey>;
    fn now(&self) -> Result<i64>;
    fn read(&self, order: &Pubkey) -> Result<Option<BookAccount>>;
    #[allow(clippy::too_many_arguments)]
    fn fill(&self, order: &Pubkey, index: usize, hash: &[u8; 32], proof: &[[u8; 32]; book::DEPTH], seller: &Pubkey, orn: u64, window: i64) -> Result<String>;
    fn claim(&self, order: &Pubkey, slot: u8, seller: &Pubkey, buyer: &Pubkey, secret: &[u8; 32]) -> Result<String>;
    fn release(&self, order: &Pubkey, slot: u8) -> Result<String>;
    fn expire(&self, order: &Pubkey, buyer: &Pubkey) -> Result<String>;
}

pub struct LiveBookChain {
    solana: Solana,
    program: Pubkey,
    relayer: Option<Keypair>,
}

impl LiveBookChain {
    pub fn new(rpc_urls: &[&str], program_id: &str, relayer_secret: Option<[u8; 32]>) -> Result<LiveBookChain> {
        let program = solana::address(program_id).context("invalid --book-program Solana address")?;
        Ok(LiveBookChain { solana: Solana::with(rpc_urls, None), program, relayer: relayer_secret.map(|s| Keypair::from_secret(&s)) })
    }

    fn send(&self, ixs: &[solana::Instruction]) -> Result<String> {
        let payer = self.relayer.as_ref().ok_or_else(|| anyhow!("no Solana relayer key configured"))?;
        let tx = solana::transaction(payer, &[], ixs, self.solana.blockhash()?)?;
        self.solana.send(&tx)
    }
}

impl BookChain for LiveBookChain {
    fn program(&self) -> Pubkey {
        self.program
    }
    fn filler(&self) -> Option<Pubkey> {
        self.relayer.as_ref().map(Keypair::pubkey)
    }
    fn now(&self) -> Result<i64> {
        self.solana.time()
    }
    fn read(&self, order: &Pubkey) -> Result<Option<BookAccount>> {
        self.solana.book(&self.program, order)
    }
    fn fill(&self, order: &Pubkey, index: usize, hash: &[u8; 32], proof: &[[u8; 32]; book::DEPTH], seller: &Pubkey, orn: u64, window: i64) -> Result<String> {
        let filler = self.filler().ok_or_else(|| anyhow!("no Solana relayer key configured"))?;
        self.send(&[book::fill(&self.program, order, &filler, index, hash, proof, seller, orn, window)])
    }
    fn claim(&self, order: &Pubkey, slot: u8, seller: &Pubkey, buyer: &Pubkey, secret: &[u8; 32]) -> Result<String> {
        self.send(&[book::claim(&self.program, order, slot, seller, buyer, secret)])
    }
    fn release(&self, order: &Pubkey, slot: u8) -> Result<String> {
        self.send(&[book::release(&self.program, order, slot)])
    }
    fn expire(&self, order: &Pubkey, buyer: &Pubkey) -> Result<String> {
        self.send(&[book::expire(&self.program, order, buyer)])
    }
}

/// In-memory `orien-book` for tests, following the program's rules.
pub struct FakeBookChain {
    pub program: Pubkey,
    pub filler: Pubkey,
    pub clock: Mutex<i64>,
    pub orders: Mutex<HashMap<Pubkey, BookAccount>>,
    /// Paid parts: (order, seller, lamports).
    pub paid: Mutex<Vec<(Pubkey, Pubkey, u64)>>,
    pub released: Mutex<Vec<(Pubkey, u8)>>,
    pub expired: Mutex<Vec<Pubkey>>,
}

impl FakeBookChain {
    pub fn new(now_unix: i64, filler: Pubkey) -> FakeBookChain {
        FakeBookChain {
            program: solana::address(book::DEVNET_BOOK_PROGRAM).expect("devnet program"),
            filler,
            clock: Mutex::new(now_unix),
            orders: Mutex::default(),
            paid: Mutex::default(),
            released: Mutex::default(),
            expired: Mutex::default(),
        }
    }
}

impl BookChain for FakeBookChain {
    fn program(&self) -> Pubkey {
        self.program
    }
    fn filler(&self) -> Option<Pubkey> {
        Some(self.filler)
    }
    fn now(&self) -> Result<i64> {
        Ok(*self.clock.lock().unwrap())
    }
    fn read(&self, order: &Pubkey) -> Result<Option<BookAccount>> {
        Ok(self.orders.lock().unwrap().get(order).cloned())
    }
    fn fill(&self, order: &Pubkey, index: usize, hash: &[u8; 32], proof: &[[u8; 32]; book::DEPTH], seller: &Pubkey, orn: u64, window: i64) -> Result<String> {
        let t = *self.clock.lock().unwrap();
        let mut map = self.orders.lock().unwrap();
        let acct = map.get_mut(order).ok_or_else(|| anyhow!("no order"))?;
        if t >= acct.terms.expiry || index >= SECRETS || acct.used & (1u64 << index) != 0 || book::root_of(hash, index, proof) != acct.terms.root {
            bail!("refused");
        }
        if window <= 0 || window > acct.terms.max_fill_window || acct.parts.len() >= book::SLOTS {
            bail!("refused");
        }
        let lamports = acct.share(orn).ok_or_else(|| anyhow!("bad size"))?;
        let slot = (0..book::SLOTS as u8).find(|s| acct.parts.iter().all(|p| p.slot != *s)).expect("a free slot");
        acct.used |= 1u64 << index;
        acct.orn_left -= orn;
        acct.lamports_left -= lamports;
        let deadline = (t + window).min(acct.terms.expiry);
        acct.parts.push(book::Part { slot, index: index as u8, hash: *hash, seller: *seller, orn, lamports, deadline });
        Ok(format!("fake-fill-{index}"))
    }
    fn claim(&self, order: &Pubkey, slot: u8, seller: &Pubkey, buyer: &Pubkey, secret: &[u8; 32]) -> Result<String> {
        let t = *self.clock.lock().unwrap();
        let mut map = self.orders.lock().unwrap();
        let acct = map.get_mut(order).ok_or_else(|| anyhow!("no order"))?;
        let at = acct.parts.iter().position(|p| p.slot == slot).ok_or_else(|| anyhow!("no part"))?;
        let part = acct.parts[at].clone();
        if &part.seller != seller || &acct.terms.buyer != buyer || book::secret_hash(secret) != part.hash || t >= part.deadline {
            bail!("refused");
        }
        acct.parts.remove(at);
        self.paid.lock().unwrap().push((*order, *seller, part.lamports));
        if acct.orn_left == 0 && acct.parts.is_empty() {
            map.remove(order);
        }
        Ok(format!("fake-claim-{slot}"))
    }
    fn release(&self, order: &Pubkey, slot: u8) -> Result<String> {
        let t = *self.clock.lock().unwrap();
        let mut map = self.orders.lock().unwrap();
        let acct = map.get_mut(order).ok_or_else(|| anyhow!("no order"))?;
        let at = acct.parts.iter().position(|p| p.slot == slot && t >= p.deadline).ok_or_else(|| anyhow!("refused"))?;
        let part = acct.parts.remove(at);
        acct.orn_left += part.orn;
        acct.lamports_left += part.lamports;
        self.released.lock().unwrap().push((*order, slot));
        Ok(format!("fake-release-{slot}"))
    }
    fn expire(&self, order: &Pubkey, _buyer: &Pubkey) -> Result<String> {
        self.orders.lock().unwrap().remove(order);
        self.expired.lock().unwrap().push(*order);
        Ok("fake-expire".into())
    }
}

/// A part-filling buy order, as anyone may see it (never its secrets).
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct BookOrder {
    pub id: i64,
    pub buyer: i64,
    pub buyer_name: String,
    pub buyer_picture: Option<String>,
    pub buyer_done: i64,
    pub order_pubkey: String,
    pub buyer_orien: String,
    pub buyer_sol: String,
    pub orn_total: u64,
    pub lamports_total: u64,
    /// What's still to fill, as Solana last said.
    pub orn_left: u64,
    pub lamports_left: u64,
    pub expiry: i64,
    /// open, done, cancelled or expired.
    pub state: String,
    pub created: u64,
}

/// A part of an order a seller is filling.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct BookPart {
    pub id: i64,
    pub order: i64,
    pub index: u8,
    /// The hash the seller's ORN lock must use.
    pub hash: String,
    pub seller: i64,
    pub seller_sol: String,
    pub orn: u64,
    pub lamports: u64,
    /// reserved, filled (lock reported), orn_claimed, done, released or failed.
    pub state: String,
    pub lock_from: Option<String>,
    pub lock_seq: Option<u64>,
    pub created: u64,
}

const ORDER: &str = "SELECT o.id, o.buyer, a.name, a.picture,
        (SELECT COUNT(*) FROM swap_offers d WHERE d.state = 'done' AND (d.seller = o.buyer OR d.taker = o.buyer)),
        o.order_pubkey, o.buyer_orien, o.buyer_sol, o.orn_total, o.lamports_total, o.orn_left, o.lamports_left,
        o.expiry, o.state, o.created
     FROM book_orders o JOIN accounts a ON a.id = o.buyer";

fn order_row(r: &rusqlite::Row) -> rusqlite::Result<BookOrder> {
    Ok(BookOrder {
        id: r.get(0)?,
        buyer: r.get(1)?,
        buyer_name: r.get(2)?,
        buyer_picture: r.get(3)?,
        buyer_done: r.get(4)?,
        order_pubkey: r.get(5)?,
        buyer_orien: r.get(6)?,
        buyer_sol: r.get(7)?,
        orn_total: r.get::<_, i64>(8)? as u64,
        lamports_total: r.get::<_, i64>(9)? as u64,
        orn_left: r.get::<_, i64>(10)? as u64,
        lamports_left: r.get::<_, i64>(11)? as u64,
        expiry: r.get(12)?,
        state: r.get(13)?,
        created: r.get::<_, i64>(14)? as u64,
    })
}

const PART: &str = "SELECT id, order_id, idx, hash, seller, seller_sol, orn, lamports, state, lock_from, lock_seq, created FROM book_parts";

fn part_row(r: &rusqlite::Row) -> rusqlite::Result<BookPart> {
    Ok(BookPart {
        id: r.get(0)?,
        order: r.get(1)?,
        index: r.get::<_, i64>(2)? as u8,
        hash: r.get(3)?,
        seller: r.get(4)?,
        seller_sol: r.get(5)?,
        orn: r.get::<_, i64>(6)? as u64,
        lamports: r.get::<_, i64>(7)? as u64,
        state: r.get(8)?,
        lock_from: r.get(9)?,
        lock_seq: r.get::<_, Option<i64>>(10)?.map(|n| n as u64),
        created: r.get::<_, i64>(11)? as u64,
    })
}

impl Store {
    pub(crate) fn create_book_tables(&self) -> Result<()> {
        self.db.execute_batch(
            "CREATE TABLE IF NOT EXISTS book_orders (
                 id INTEGER PRIMARY KEY,
                 buyer INTEGER NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
                 order_pubkey TEXT NOT NULL UNIQUE,
                 secrets BLOB NOT NULL,
                 buyer_orien TEXT NOT NULL,
                 buyer_sol TEXT NOT NULL,
                 orn_total INTEGER NOT NULL,
                 lamports_total INTEGER NOT NULL,
                 orn_left INTEGER NOT NULL,
                 lamports_left INTEGER NOT NULL,
                 expiry INTEGER NOT NULL,
                 state TEXT NOT NULL,
                 created INTEGER NOT NULL,
                 updated INTEGER NOT NULL
             );
             CREATE INDEX IF NOT EXISTS book_orders_state ON book_orders(state, created);
             CREATE TABLE IF NOT EXISTS book_parts (
                 id INTEGER PRIMARY KEY,
                 order_id INTEGER NOT NULL REFERENCES book_orders(id) ON DELETE CASCADE,
                 idx INTEGER NOT NULL,
                 hash TEXT NOT NULL,
                 seller INTEGER NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
                 seller_sol TEXT NOT NULL,
                 orn INTEGER NOT NULL,
                 lamports INTEGER NOT NULL,
                 state TEXT NOT NULL,
                 lock_from TEXT,
                 lock_seq INTEGER,
                 claim_sig TEXT,
                 created INTEGER NOT NULL,
                 updated INTEGER NOT NULL,
                 finished INTEGER,
                 usd REAL,
                 UNIQUE (order_id, idx)
             );
             CREATE INDEX IF NOT EXISTS book_parts_seller ON book_parts(seller, updated);",
        )?;
        Ok(())
    }

    /// Registers a buyer's part-filling order, found on Solana as `acct`,
    /// with its 64 `secrets` (kept here only). The secrets must be the
    /// order's, and the order must name `filler` as the one who fills.
    pub fn post_book_order(&self, buyer: i64, order_pubkey: &str, acct: &BookAccount, secrets: &[[u8; 32]], filler: &Pubkey, max_lamports: u64) -> Result<BookOrder> {
        let tree = Secrets::from_secrets(secrets).ok_or_else(|| anyhow!("an order has 64 secrets"))?;
        if tree.root() != acct.terms.root {
            bail!("those secrets aren't this order's");
        }
        if &acct.terms.filler != filler {
            bail!("this order names another server to fill it");
        }
        if acct.terms.lamports_total < book::MIN_PART || acct.terms.lamports_total > max_lamports {
            bail!("a buy order is between {} and {} SOL", book::MIN_PART as f64 / 1e9, max_lamports as f64 / 1e9);
        }
        if acct.orn_left != acct.terms.orn_total || !acct.parts.is_empty() || acct.used != 0 {
            bail!("that order has been filled from already");
        }
        let open: i64 = self.db.query_row("SELECT COUNT(*) FROM book_orders WHERE buyer = ?1 AND state = 'open'", [buyer], |r| r.get(0))?;
        if open >= MAX_BOOK_ORDERS {
            bail!("you can have {MAX_BOOK_ORDERS} buy orders open at once");
        }
        let blob: Vec<u8> = secrets.iter().flatten().copied().collect();
        let at = now();
        self.db.execute(
            "INSERT INTO book_orders (buyer, order_pubkey, secrets, buyer_orien, buyer_sol, orn_total, lamports_total, orn_left, lamports_left, expiry, state, created, updated)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?6, ?7, ?8, 'open', ?9, ?9)",
            params![
                buyer,
                order_pubkey,
                blob,
                hex::encode(acct.terms.buyer_orien),
                solana::text(&acct.terms.buyer),
                acct.terms.orn_total as i64,
                acct.terms.lamports_total as i64,
                acct.terms.expiry,
                at
            ],
        )?;
        self.book_order(self.db.last_insert_rowid())?.ok_or_else(|| anyhow!("just written"))
    }

    pub fn book_order(&self, id: i64) -> Result<Option<BookOrder>> {
        Ok(self.db.query_row(&format!("{ORDER} WHERE o.id = ?1"), [id], order_row).optional()?)
    }

    /// Open orders with something left to fill, for the board.
    pub fn open_book_orders(&self) -> Result<Vec<BookOrder>> {
        let mut stmt = self.db.prepare(&format!("{ORDER} WHERE o.state = 'open' AND o.orn_left > 0 ORDER BY o.created DESC LIMIT 200"))?;
        let rows = stmt.query_map([], order_row)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// An account's orders as buyer, and the parts it's filling as seller.
    pub fn my_books(&self, account: i64) -> Result<(Vec<BookOrder>, Vec<BookPart>)> {
        let mut stmt = self.db.prepare(&format!("{ORDER} WHERE o.buyer = ?1 ORDER BY o.updated DESC LIMIT 50"))?;
        let orders = stmt.query_map([account], order_row)?.collect::<rusqlite::Result<_>>()?;
        let mut stmt = self.db.prepare(&format!("{PART} WHERE seller = ?1 ORDER BY updated DESC LIMIT 50"))?;
        let parts = stmt.query_map([account], part_row)?.collect::<rusqlite::Result<_>>()?;
        Ok((orders, parts))
    }

    pub fn book_part(&self, id: i64) -> Result<Option<BookPart>> {
        Ok(self.db.query_row(&format!("{PART} WHERE id = ?1"), [id], part_row).optional()?)
    }

    fn book_parts_of(&self, order: i64) -> Result<Vec<BookPart>> {
        let mut stmt = self.db.prepare(&format!("{PART} WHERE order_id = ?1"))?;
        let rows = stmt.query_map([order], part_row)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    pub(crate) fn book_secret(&self, order: i64, index: u8) -> Result<[u8; 32]> {
        let blob: Vec<u8> = self.db.query_row("SELECT secrets FROM book_orders WHERE id = ?1", [order], |r| r.get(0))?;
        let at = index as usize * 32;
        blob.get(at..at + 32).and_then(|s| s.try_into().ok()).ok_or_else(|| anyhow!("no such secret"))
    }

    fn book_secrets(&self, order: i64) -> Result<Secrets> {
        let blob: Vec<u8> = self.db.query_row("SELECT secrets FROM book_orders WHERE id = ?1", [order], |r| r.get(0))?;
        let secrets: Vec<[u8; 32]> = blob.chunks(32).filter_map(|c| c.try_into().ok()).collect();
        Secrets::from_secrets(&secrets).ok_or_else(|| anyhow!("an order's secrets are damaged"))
    }

    /// A secret number neither used on Solana (`used`) nor given to a part here.
    fn book_free_index(&self, order: i64, used: u64) -> Result<Option<usize>> {
        let taken: Vec<i64> = {
            let mut stmt = self.db.prepare("SELECT idx FROM book_parts WHERE order_id = ?1")?;
            stmt.query_map([order], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?
        };
        Ok((0..SECRETS).find(|i| used & (1u64 << i) == 0 && !taken.contains(&(*i as i64))))
    }

    #[allow(clippy::too_many_arguments)]
    fn insert_book_part(&self, order: i64, index: usize, hash: &[u8; 32], seller: i64, seller_sol: &str, orn: u64, lamports: u64) -> Result<BookPart> {
        let at = now();
        self.db.execute(
            "INSERT INTO book_parts (order_id, idx, hash, seller, seller_sol, orn, lamports, state, created, updated)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'reserved', ?8, ?8)",
            params![order, index as i64, hex::encode(hash), seller, seller_sol, orn as i64, lamports as i64, at],
        )?;
        self.book_part(self.db.last_insert_rowid())?.ok_or_else(|| anyhow!("just written"))
    }

    fn set_book_part(&self, id: i64, state: &str) -> Result<()> {
        let finished = matches!(state, "done" | "released" | "failed").then(now);
        self.db.execute(
            "UPDATE book_parts SET state = ?2, updated = ?3, finished = COALESCE(finished, ?4) WHERE id = ?1",
            params![id, state, now(), finished],
        )?;
        Ok(())
    }

    fn set_book_left(&self, id: i64, orn: u64, lamports: u64) -> Result<()> {
        self.db.execute(
            "UPDATE book_orders SET orn_left = ?2, lamports_left = ?3, updated = ?4 WHERE id = ?1 AND (orn_left != ?2 OR lamports_left != ?3)",
            params![id, orn as i64, lamports as i64, now()],
        )?;
        Ok(())
    }

    fn close_book_order(&self, id: i64, state: &str) -> Result<()> {
        self.db.execute("UPDATE book_orders SET state = ?2, updated = ?3 WHERE id = ?1 AND state = 'open'", params![id, state, now()])?;
        Ok(())
    }

    fn active_book_ids(&self) -> Result<Vec<i64>> {
        let mut stmt = self.db.prepare(
            "SELECT id FROM book_orders WHERE state = 'open'
             UNION SELECT DISTINCT order_id FROM book_parts WHERE state IN ('reserved', 'filled', 'orn_claimed')",
        )?;
        let rows = stmt.query_map([], |r| r.get(0))?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// Finished parts not yet given a dollar value.
    pub fn unpriced_book_parts(&self) -> Result<Vec<(i64, u64)>> {
        let mut stmt = self.db.prepare("SELECT id, lamports FROM book_parts WHERE state = 'done' AND usd IS NULL")?;
        let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get::<_, i64>(1)? as u64)))?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    pub fn price_book_part(&self, id: i64, usd: f64) -> Result<()> {
        self.db.execute("UPDATE book_parts SET usd = ?2 WHERE id = ?1 AND usd IS NULL", params![id, usd])?;
        Ok(())
    }

    /// Finished parts as trades, newest first, with no names or addresses.
    pub fn book_history(&self, before: Option<u64>, limit: u32) -> Result<Vec<serde_json::Value>> {
        let mut stmt = self.db.prepare(
            "SELECT orn, lamports, usd, finished FROM book_parts
             WHERE state = 'done' AND finished IS NOT NULL AND usd IS NOT NULL AND finished < ?1
             ORDER BY finished DESC, id DESC LIMIT ?2",
        )?;
        let rows = stmt.query_map(params![before.unwrap_or(i64::MAX as u64) as i64, limit], |r| {
            Ok(serde_json::json!({
                "orn": r.get::<_, i64>(0)? as u64,
                "coin": "sol",
                "lamports": r.get::<_, i64>(1)? as u64,
                "usd": r.get::<_, Option<f64>>(2)?,
                "at": r.get::<_, i64>(3)? as u64,
            }))
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// Records the seller's reported ORN lock for `part`.
    pub fn book_part_locked(&self, part: i64, seller: i64, lock_from: &str, lock_seq: u64) -> Result<BookPart> {
        let changed = self.db.execute(
            "UPDATE book_parts SET state = 'filled', lock_from = ?3, lock_seq = ?4, updated = ?5 WHERE id = ?1 AND seller = ?2 AND state = 'reserved'",
            params![part, seller, lock_from, lock_seq as i64, now()],
        )?;
        let found = self.book_part(part)?.ok_or_else(|| anyhow!("no such part"))?;
        if changed == 0 && found.state != "filled" {
            bail!("that part is no longer waiting for your ORN lock");
        }
        Ok(found)
    }
}

/// Serializes reserving parts, so two sellers never get the same secret.
static RESERVING: Mutex<()> = Mutex::new(());

/// Reserves `orn` of order `id` for `seller`, paid at `seller_sol`: a part
/// on Solana under a secret not used before. Returns the part, whose hash
/// the seller's ORN lock must use. Blocking: call from a blocking task.
pub fn reserve(app: &crate::api::App, chain: &dyn BookChain, id: i64, seller: i64, seller_sol: &Pubkey, orn: u64) -> Result<BookPart> {
    let _one = RESERVING.lock().unwrap_or_else(|p| p.into_inner());
    let order = app.store_lock().book_order(id)?.ok_or_else(|| anyhow!("there is no such order"))?;
    if order.state != "open" {
        bail!("that order is no longer open");
    }
    if order.buyer == seller {
        bail!("this is your own order");
    }
    let order_pk = solana::address(&order.order_pubkey)?;
    let acct = chain.read(&order_pk)?.ok_or_else(|| anyhow!("that order is no longer on Solana"))?;
    if seller_sol == &acct.terms.buyer {
        bail!("that SOL address is the buyer's own: give your own SOL wallet");
    }
    let t = chain.now()?;
    if t + 1800 >= acct.terms.expiry {
        bail!("that order expires too soon to fill");
    }
    if acct.parts.len() >= book::SLOTS {
        bail!("that order is busy with other sellers; try again in a few minutes");
    }
    let lamports = acct.share(orn).ok_or_else(|| {
        anyhow!("sell at least 0.01 SOL worth, and leave at least 0.01 SOL of the order (or fill all of it)")
    })?;
    let secrets = app.store_lock().book_secrets(id)?;
    let index = app.store_lock().book_free_index(id, acct.used)?.ok_or_else(|| anyhow!("that order has no parts left to give"))?;
    let hash = secrets.hash(index);
    let part = app.store_lock().insert_book_part(id, index, &hash, seller, &solana::text(seller_sol), orn, lamports)?;
    if let Err(e) = chain.fill(&order_pk, index, &hash, &secrets.proof(index), seller_sol, orn, acct.terms.max_fill_window) {
        app.store_lock().set_book_part(part.id, "failed")?;
        return Err(e.context("Solana didn't take the part"));
    }
    Ok(part)
}

/// Checks a seller's report that they locked part `part`'s ORN: the part is
/// on Solana for them, and the lock on Orien pays the buyer that many ORN
/// under the part's hash. Blocking.
pub fn check_lock(app: &crate::api::App, chain: &dyn BookChain, part: &BookPart, lock_from: &AccountId, lock_seq: u64) -> Result<()> {
    let order = app.store_lock().book_order(part.order)?.ok_or_else(|| anyhow!("no such order"))?;
    let acct = chain.read(&solana::address(&order.order_pubkey)?)?.ok_or_else(|| anyhow!("that order is no longer on Solana"))?;
    let on_chain = acct.parts.iter().find(|p| p.index == part.index).ok_or_else(|| anyhow!("Solana hasn't finalized your part yet; try again in a few seconds"))?;
    if solana::text(&on_chain.seller) != part.seller_sol || hex::encode(on_chain.hash) != part.hash || on_chain.orn != part.orn {
        bail!("that part on Solana isn't yours");
    }
    if chain.now()? >= on_chain.deadline {
        bail!("that part's time has run out");
    }
    let buyer_orien: AccountId = order.buyer_orien.parse().map_err(|_| anyhow!("bad buyer address"))?;
    let hash: [u8; 32] = hex::decode(&part.hash).ok().and_then(|b| b.try_into().ok()).ok_or_else(|| anyhow!("bad hash"))?;
    verify_orien_lock(app.payments().as_ref(), lock_from, lock_seq, &buyer_orien, part.orn, &hash, None, 1)?;
    Ok(())
}

/// One pass over every order with something under way: claims ORN for the
/// buyer, then SOL for the seller, part by part, once each seller's ORN lock
/// is checked on Orien; gives back parts whose time ran out; returns expired
/// orders' SOL; keeps what's left current for the board.
///
/// SECURITY INVARIANT: a part's secret is used only after its seller's ORN
/// lock has been checked directly on Orien.
pub fn settle_once(app: &Arc<crate::api::App>) -> Result<usize> {
    let Some(chain) = app.book_chain() else { return Ok(0) };
    static SETTLING: Mutex<()> = Mutex::new(());
    let _one = SETTLING.lock().unwrap_or_else(|p| p.into_inner());
    let ids = app.store_lock().active_book_ids()?;
    let t = chain.now()?;
    let mut settled = 0;
    for id in ids {
        let Some(order) = app.store_lock().book_order(id)? else { continue };
        let Ok(order_pk) = solana::address(&order.order_pubkey) else { continue };
        let Ok(buyer_pk) = solana::address(&order.buyer_sol) else { continue };
        let parts = app.store_lock().book_parts_of(id)?;
        let acct = match chain.read(&order_pk) {
            Ok(acct) => acct,
            Err(e) => {
                eprintln!("book order #{id}: cannot read Solana: {e}");
                continue;
            }
        };
        let Some(acct) = acct else {
            // Closed: filled to the end, cancelled or expired.
            for part in &parts {
                match part.state.as_str() {
                    "orn_claimed" => app.store_lock().set_book_part(part.id, "done")?,
                    "reserved" | "filled" => app.store_lock().set_book_part(part.id, "released")?,
                    _ => {}
                }
            }
            let filled = parts.iter().any(|p| p.state == "orn_claimed" || p.state == "done");
            let state = if t >= order.expiry { "expired" } else if filled && order.orn_left == 0 { "done" } else { "cancelled" };
            app.store_lock().close_book_order(id, state)?;
            continue;
        };
        app.store_lock().set_book_left(id, acct.orn_left, acct.lamports_left)?;
        if t >= acct.terms.expiry {
            if let Err(e) = chain.expire(&order_pk, &buyer_pk) {
                eprintln!("book order #{id}: cannot expire: {e}");
            }
            continue;
        }
        for part in &parts {
            let on_chain = acct.parts.iter().find(|p| p.index == part.index);
            match (part.state.as_str(), on_chain) {
                // Never reached Solana: given up after a while.
                ("reserved", None) if now() > part.created + FILL_LANDS_SECONDS && acct.used & (1u64 << part.index) == 0 => {
                    app.store_lock().set_book_part(part.id, "failed")?;
                }
                // Reached Solana and then gone without being claimed: released.
                ("reserved" | "filled", None) if acct.used & (1u64 << part.index) != 0 => {
                    app.store_lock().set_book_part(part.id, "released")?;
                }
                ("filled", Some(p)) if t + CLAIM_MARGIN_SECONDS < p.deadline => {
                    let (Some(from), Some(seq)) = (part.lock_from.as_deref(), part.lock_seq) else { continue };
                    let Ok(from) = from.parse::<AccountId>() else { continue };
                    let Ok(buyer_orien) = order.buyer_orien.parse::<AccountId>() else { continue };
                    let secret = app.store_lock().book_secret(id, part.index)?;
                    let hash: [u8; 32] = p.hash;
                    match verify_orien_lock(app.payments().as_ref(), &from, seq, &buyer_orien, part.orn, &hash, Some(&secret), 1) {
                        Ok(true) => app.store_lock().set_book_part(part.id, "orn_claimed")?,
                        Ok(false) => match app.payments().claim_lock(&from, seq, &buyer_orien, part.orn, secret) {
                            Ok(()) => {
                                eprintln!("book order #{id}: claimed {} ORN on Orien for the buyer", crate::store::orn(part.orn));
                                app.store_lock().set_book_part(part.id, "orn_claimed")?;
                            }
                            Err(e) => {
                                eprintln!("book order #{id}: ORN claim failed: {e}");
                                continue;
                            }
                        },
                        Err(e) => {
                            eprintln!("book order #{id}: waiting for a valid ORN lock: {e}");
                            continue;
                        }
                    }
                    // The ORN are the buyer's: the seller's SOL at once.
                    match chain.claim(&order_pk, p.slot, &p.seller, &acct.terms.buyer, &secret) {
                        Ok(sig) => {
                            eprintln!("book order #{id}: paid part {} to {} ({sig})", part.id, solana::text(&p.seller));
                            app.store_lock().set_book_part(part.id, "done")?;
                            settled += 1;
                            notify(app, &order, part);
                        }
                        Err(e) => eprintln!("book order #{id}: SOL claim failed, trying again: {e}"),
                    }
                }
                // ORN claimed, SOL not yet: try the SOL again.
                ("orn_claimed", Some(p)) if t < p.deadline => {
                    let secret = app.store_lock().book_secret(id, part.index)?;
                    if let Ok(sig) = chain.claim(&order_pk, p.slot, &p.seller, &acct.terms.buyer, &secret) {
                        eprintln!("book order #{id}: paid part {} ({sig})", part.id);
                        app.store_lock().set_book_part(part.id, "done")?;
                        settled += 1;
                        notify(app, &order, part);
                    }
                }
                ("orn_claimed", None) => app.store_lock().set_book_part(part.id, "done")?,
                _ => {}
            }
        }
        // Parts whose time ran out go back into the order.
        let claimed = |index: u8| parts.iter().any(|q| q.index == index && q.state == "orn_claimed");
        for p in acct.parts.iter().filter(|p| t >= p.deadline && !claimed(p.index)) {
            if let Err(e) = chain.release(&order_pk, p.slot) {
                eprintln!("book order #{id}: cannot release slot {}: {e}", p.slot);
            }
        }
    }
    Ok(settled)
}

fn notify(app: &Arc<crate::api::App>, order: &BookOrder, part: &BookPart) {
    let sol = format!("{} SOL", format!("{:.4}", part.lamports as f64 / 1e9).trim_end_matches('0').trim_end_matches('.'));
    app.tell(crate::push::Event {
        account: order.buyer,
        address: Some(order.buyer_orien.clone()),
        body: format!("Buy order: {} ORN are in your wallet", crate::store::orn(part.orn)),
        kind: "swap",
        open: serde_json::json!({ "book": order.id }),
    });
    app.tell(crate::push::Event {
        account: part.seller,
        address: part.lock_from.clone(),
        body: format!("Sold {} ORN: {sol} sent to your SOL wallet", crate::store::orn(part.orn)),
        kind: "swap",
        open: serde_json::json!({ "book": order.id }),
    });
}

/// Settles part-filling orders every few seconds.
pub fn watch(app: Arc<crate::api::App>) {
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(std::time::Duration::from_secs(8));
        loop {
            tick.tick().await;
            let app = app.clone();
            let _ = tokio::task::spawn_blocking(move || {
                if let Err(e) = settle_once(&app) {
                    eprintln!("book orders: {e:#}");
                }
            })
            .await;
        }
    });
}
