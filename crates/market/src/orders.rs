//! Funded buy orders backed by `programs/orien-order` on Solana.
//!
//! Flow (neither side needs to be online once their step is sent):
//! 1. Buyer creates secret S and hash H = SHA256(S), locks SOL in `orien-order`,
//!    and registers `{ order_pubkey, secret_hex }` with this server over TLS.
//!    S is kept only in SQLite and is NEVER returned by any endpoint or logged.
//! 2. Seller locks ORN on Orien for `buyer_orien` with hash H, calls `fill` on
//!    Solana naming their `seller_sol` payout, and reports `{ lock_from, lock_seq }`.
//! 3. This server independently verifies BOTH the Solana order AND the Orien
//!    lock directly against Orien validators (never trusting seller claims).
//!    Only after verifying the ORN lock does the server:
//!    (a) claim the ORN on Orien with S (can only pay `buyer_orien`), and
//!    (b) relay `claim` on Solana with S (can only pay the recorded `seller_sol`).
//!    Seller is paid in ~1 minute.

use crate::store::{Store, now};
use anyhow::{Context, Result, anyhow, bail};
use orien_ledger::AccountId;
use orien_swap::order::{self, OrderAccount, STATE_FILLED, STATE_OPEN};
use orien_swap::sol::MIN_LAMPORTS;
use orien_swap::solana::{self, Keypair, Pubkey, Solana};
use rusqlite::{OptionalExtension, params};
use serde::Serialize;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// Default per-order SOL cap (10 SOL) to bound server-secret exposure.
pub const DEFAULT_MAX_ORDER_LAMPORTS: u64 = 10 * solana::LAMPORTS_PER_SOL;
/// Maximum active funded buy orders one account may hold at once.
pub const MAX_FUNDED_ORDERS: i64 = 5;
/// Minimum remaining lifetime (seconds) for a newly registered funded order.
pub const MIN_ORDER_LIFE_SECONDS: i64 = 300;

/// Public view of a funded buy order (NEVER includes the secret S).
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct FundedOrder {
    pub id: i64,
    pub buyer: i64,
    pub buyer_name: String,
    pub buyer_picture: Option<String>,
    pub buyer_done: i64,
    pub order_pubkey: String,
    pub hash: String,
    pub buyer_orien: String,
    pub buyer_sol: String,
    pub authority_sol: String,
    pub orn: u64,
    pub lamports: u64,
    pub expiry: i64,
    pub min_bond: u64,
    pub max_fill_window: i64,
    /// open, filled, done, cancelled, expired
    pub state: String,
    pub seller: Option<i64>,
    pub seller_name: Option<String>,
    pub seller_sol: Option<String>,
    pub lock_from: Option<String>,
    pub lock_seq: Option<u64>,
    pub fill_deadline: Option<i64>,
    pub orn_claimed: bool,
    pub sol_claimed: bool,
    pub sol_claim_sig: Option<String>,
    pub created: u64,
    pub updated: u64,
}

/// Internal record including the secret S, used ONLY inside settlement.
pub(crate) struct OrderWithSecret {
    pub order: FundedOrder,
    pub secret: [u8; 32],
}

/// How the server reads and relays transactions to the Solana `orien-order`
/// program. Abstracted as a trait so tests run without touching real Solana.
pub trait OrderChain: Send + Sync {
    /// Base58 address of the deployed `orien-order` program.
    fn program_id(&self) -> String;
    /// Current Solana finalized Unix time.
    fn now(&self) -> Result<i64>;
    /// Reads the finalized `orien-order` account at `order_pubkey`, if present.
    fn read_order(&self, order_pubkey: &Pubkey) -> Result<Option<OrderAccount>>;
    /// Relays `fill(order, seller, fill_window)` when `min_bond == 0` so a seller with 0 SOL can reserve.
    fn relay_fill(&self, order_pubkey: &Pubkey, seller: &Pubkey, fill_window: i64) -> Result<String>;
    /// Relays `claim(order, seller, buyer, secret)` and returns the tx signature.
    fn relay_claim(&self, order_pubkey: &Pubkey, seller: &Pubkey, buyer: &Pubkey, secret: &[u8; 32]) -> Result<String>;
    /// Relays `reopen(order, buyer)` after a fill window lapses.
    fn relay_reopen(&self, order_pubkey: &Pubkey, buyer: &Pubkey) -> Result<String>;
    /// Relays `expire(order, buyer)` after order expiry.
    fn relay_expire(&self, order_pubkey: &Pubkey, buyer: &Pubkey) -> Result<String>;
}

/// Live Solana `orien-order` relayer using `orien_swap::solana::Solana`.
pub struct LiveOrderChain {
    solana: Solana,
    program: Pubkey,
    relayer: Option<Keypair>,
}

impl LiveOrderChain {
    pub fn new(rpc_urls: &[&str], program_id: &str, relayer_secret: Option<[u8; 32]>) -> Result<LiveOrderChain> {
        let program = solana::address(program_id).context("invalid --order-program Solana address")?;
        let solana = Solana::with(rpc_urls, None);
        let relayer = relayer_secret.map(|s| Keypair::from_secret(&s));
        Ok(LiveOrderChain { solana, program, relayer })
    }

    fn send_ixs(&self, ixs: &[solana::Instruction]) -> Result<String> {
        let payer = self.relayer.as_ref().ok_or_else(|| anyhow!("no Solana relayer key configured"))?;
        let blockhash = self.solana.blockhash()?;
        let tx = solana::transaction(payer, &[], ixs, blockhash)?;
        self.solana.send(&tx)
    }

    fn send_ix(&self, ix: solana::Instruction) -> Result<String> {
        self.send_ixs(&[ix])
    }
}

impl OrderChain for LiveOrderChain {
    fn program_id(&self) -> String {
        solana::text(&self.program)
    }

    fn now(&self) -> Result<i64> {
        self.solana.time()
    }

    fn read_order(&self, order_pubkey: &Pubkey) -> Result<Option<OrderAccount>> {
        self.solana.order(&self.program, order_pubkey)
    }

    fn relay_fill(&self, order_pubkey: &Pubkey, seller: &Pubkey, fill_window: i64) -> Result<String> {
        let payer = self.relayer.as_ref().ok_or_else(|| anyhow!("no Solana relayer key configured"))?;
        let ixs = order::fill(&self.program, &payer.pubkey(), order_pubkey, seller, fill_window, 0);
        self.send_ixs(&ixs)
    }

    fn relay_claim(&self, order_pubkey: &Pubkey, seller: &Pubkey, buyer: &Pubkey, secret: &[u8; 32]) -> Result<String> {
        self.send_ix(order::claim(&self.program, order_pubkey, seller, buyer, secret))
    }

    fn relay_reopen(&self, order_pubkey: &Pubkey, buyer: &Pubkey) -> Result<String> {
        self.send_ix(order::reopen(&self.program, order_pubkey, buyer))
    }

    fn relay_expire(&self, order_pubkey: &Pubkey, buyer: &Pubkey) -> Result<String> {
        self.send_ix(order::expire(&self.program, order_pubkey, buyer))
    }
}

/// Loads a 64-byte Solana JSON keypair file (as created by `solana-keygen`)
/// and extracts its 32-byte ed25519 secret seed.
pub fn load_relayer_keypair(path: &std::path::Path) -> Result<[u8; 32]> {
    let text = std::fs::read_to_string(path).with_context(|| format!("cannot read {}", path.display()))?;
    let bytes: Vec<u8> = serde_json::from_str(&text).context("Solana keypair file must be a JSON byte array")?;
    if bytes.len() < 32 {
        bail!("Solana keypair file is too short");
    }
    let mut seed = [0u8; 32];
    seed.copy_from_slice(&bytes[..32]);
    Ok(seed)
}

/// In-memory fake Solana order chain for tests.
#[derive(Default)]
pub struct FakeOrderChain {
    pub program: String,
    pub clock: Mutex<i64>,
    pub orders: Mutex<HashMap<Pubkey, OrderAccount>>,
    pub claims: Mutex<Vec<(Pubkey, Pubkey, Pubkey, [u8; 32])>>,
    pub reopens: Mutex<Vec<Pubkey>>,
    pub expires: Mutex<Vec<Pubkey>>,
}

impl FakeOrderChain {
    pub fn new(now_unix: i64) -> FakeOrderChain {
        FakeOrderChain {
            program: order::DEVNET_ORDER_PROGRAM.into(),
            clock: Mutex::new(now_unix),
            ..FakeOrderChain::default()
        }
    }
}

impl OrderChain for FakeOrderChain {
    fn program_id(&self) -> String {
        self.program.clone()
    }
    fn now(&self) -> Result<i64> {
        Ok(*self.clock.lock().unwrap())
    }
    fn read_order(&self, order_pubkey: &Pubkey) -> Result<Option<OrderAccount>> {
        Ok(self.orders.lock().unwrap().get(order_pubkey).cloned())
    }
    fn relay_fill(&self, order_pubkey: &Pubkey, seller: &Pubkey, fill_window: i64) -> Result<String> {
        let mut map = self.orders.lock().unwrap();
        let acct = map.get_mut(order_pubkey).ok_or_else(|| anyhow!("order not found"))?;
        let t = *self.clock.lock().unwrap();
        if acct.state != STATE_OPEN || t >= acct.terms.expiry || seller == &[0u8; 32] || seller == &acct.terms.buyer {
            bail!("cannot fill order");
        }
        if fill_window <= 0 || fill_window > acct.terms.max_fill_window || acct.terms.min_bond > 0 {
            bail!("bad fill window or bond");
        }
        acct.state = STATE_FILLED;
        acct.seller = *seller;
        acct.fill_deadline = t.saturating_add(fill_window).min(acct.terms.expiry);
        Ok(format!("fake-fill-{}", solana::text(order_pubkey)))
    }
    fn relay_claim(&self, order_pubkey: &Pubkey, seller: &Pubkey, buyer: &Pubkey, secret: &[u8; 32]) -> Result<String> {
        let mut map = self.orders.lock().unwrap();
        let acct = map.get(order_pubkey).ok_or_else(|| anyhow!("order not found"))?;
        if acct.state != STATE_FILLED {
            bail!("order is not filled");
        }
        if &acct.seller != seller || &acct.terms.buyer != buyer {
            bail!("payee mismatch");
        }
        if orien_ledger::types::lock_hash(secret) != acct.terms.hash {
            bail!("wrong secret");
        }
        if *self.clock.lock().unwrap() >= acct.fill_deadline {
            bail!("fill deadline passed");
        }
        map.remove(order_pubkey);
        self.claims.lock().unwrap().push((*order_pubkey, *seller, *buyer, *secret));
        Ok(format!("fake-claim-{}", solana::text(order_pubkey)))
    }
    fn relay_reopen(&self, order_pubkey: &Pubkey, _buyer: &Pubkey) -> Result<String> {
        let mut map = self.orders.lock().unwrap();
        let acct = map.get_mut(order_pubkey).ok_or_else(|| anyhow!("order not found"))?;
        acct.state = STATE_OPEN;
        acct.seller = [0u8; 32];
        acct.fill_deadline = 0;
        self.reopens.lock().unwrap().push(*order_pubkey);
        Ok(format!("fake-reopen-{}", solana::text(order_pubkey)))
    }
    fn relay_expire(&self, order_pubkey: &Pubkey, _buyer: &Pubkey) -> Result<String> {
        self.orders.lock().unwrap().remove(order_pubkey);
        self.expires.lock().unwrap().push(*order_pubkey);
        Ok(format!("fake-expire-{}", solana::text(order_pubkey)))
    }
}

const ORDER_SELECT: &str = "SELECT o.id, o.buyer, b.name, b.picture,
        (SELECT COUNT(*) FROM swap_offers d WHERE d.state = 'done' AND (d.seller = o.buyer OR d.taker = o.buyer))
        + (SELECT COUNT(*) FROM funded_orders f WHERE f.state = 'done' AND (f.buyer = o.buyer OR f.seller = o.buyer)),
        o.order_pubkey, o.hash, o.buyer_orien, o.buyer_sol, o.authority_sol,
        o.orn, o.lamports, o.expiry, o.min_bond, o.max_fill_window,
        o.state, o.seller, s.name, o.seller_sol, o.lock_from, o.lock_seq,
        o.fill_deadline, o.orn_claimed, o.sol_claimed, o.sol_claim_sig,
        o.created, o.updated
    FROM funded_orders o
    JOIN accounts b ON b.id = o.buyer
    LEFT JOIN accounts s ON s.id = o.seller";

fn order_row(row: &rusqlite::Row) -> rusqlite::Result<FundedOrder> {
    Ok(FundedOrder {
        id: row.get(0)?,
        buyer: row.get(1)?,
        buyer_name: row.get(2)?,
        buyer_picture: row.get(3)?,
        buyer_done: row.get(4)?,
        order_pubkey: row.get(5)?,
        hash: row.get(6)?,
        buyer_orien: row.get(7)?,
        buyer_sol: row.get(8)?,
        authority_sol: row.get(9)?,
        orn: row.get::<_, i64>(10)? as u64,
        lamports: row.get::<_, i64>(11)? as u64,
        expiry: row.get(12)?,
        min_bond: row.get::<_, i64>(13)? as u64,
        max_fill_window: row.get(14)?,
        state: row.get(15)?,
        seller: row.get(16)?,
        seller_name: row.get(17)?,
        seller_sol: row.get(18)?,
        lock_from: row.get(19)?,
        lock_seq: row.get::<_, Option<i64>>(20)?.map(|v| v as u64),
        fill_deadline: row.get(21)?,
        orn_claimed: row.get::<_, i64>(22)? != 0,
        sol_claimed: row.get::<_, i64>(23)? != 0,
        sol_claim_sig: row.get(24)?,
        created: row.get::<_, i64>(25)? as u64,
        updated: row.get::<_, i64>(26)? as u64,
    })
}

impl Store {
    pub(crate) fn create_funded_order_tables(&self) -> Result<()> {
        self.db.execute_batch(
            "CREATE TABLE IF NOT EXISTS funded_orders (
                 id INTEGER PRIMARY KEY,
                 buyer INTEGER NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
                 order_pubkey TEXT NOT NULL UNIQUE,
                 hash TEXT NOT NULL,
                 secret BLOB NOT NULL,
                 buyer_orien TEXT NOT NULL,
                 buyer_sol TEXT NOT NULL,
                 authority_sol TEXT NOT NULL,
                 orn INTEGER NOT NULL,
                 lamports INTEGER NOT NULL,
                 expiry INTEGER NOT NULL,
                 min_bond INTEGER NOT NULL DEFAULT 0,
                 max_fill_window INTEGER NOT NULL,
                 state TEXT NOT NULL,
                 seller INTEGER REFERENCES accounts(id) ON DELETE SET NULL,
                 seller_sol TEXT,
                 lock_from TEXT,
                 lock_seq INTEGER,
                 fill_deadline INTEGER,
                 orn_claimed INTEGER NOT NULL DEFAULT 0,
                 sol_claimed INTEGER NOT NULL DEFAULT 0,
                 sol_claim_sig TEXT,
                 created INTEGER NOT NULL,
                 updated INTEGER NOT NULL,
                 finished INTEGER,
                 usd REAL
             );
             CREATE INDEX IF NOT EXISTS funded_orders_state ON funded_orders(state, created);",
        )?;
        Ok(())
    }

    /// Registers a buyer's on-chain funded Solana order and stores secret S
    /// strictly inside SQLite (never returned or logged).
    pub fn post_funded_order(
        &self,
        buyer: i64,
        order_pubkey: &str,
        on_chain: &OrderAccount,
        secret: &[u8; 32],
        max_lamports: u64,
    ) -> Result<FundedOrder> {
        if on_chain.state != STATE_OPEN {
            bail!("that Solana order is not open");
        }
        if on_chain.terms.amount < MIN_LAMPORTS {
            bail!("a SOL buy order is at least {} SOL", MIN_LAMPORTS as f64 / 1e9);
        }
        if on_chain.terms.amount > max_lamports {
            bail!("buy orders are limited to {} SOL per order for now", max_lamports as f64 / 1e9);
        }
        if on_chain.terms.orn_amount == 0 {
            bail!("order more than nothing");
        }
        if orien_ledger::types::lock_hash(secret) != on_chain.terms.hash {
            bail!("the secret does not match the on-chain order's hash");
        }
        let open: i64 = self.db.query_row(
            "SELECT COUNT(*) FROM funded_orders WHERE buyer = ?1 AND state IN ('open', 'filled')",
            [buyer],
            |r| r.get(0),
        )?;
        if open >= MAX_FUNDED_ORDERS {
            bail!("you can have at most {MAX_FUNDED_ORDERS} active buy orders at once");
        }
        let at = now() as i64;
        let hash_hex = hex::encode(on_chain.terms.hash);
        let buyer_orien_hex = hex::encode(on_chain.terms.buyer_orien);
        let buyer_sol = solana::text(&on_chain.terms.buyer);
        let authority_sol = solana::text(&on_chain.terms.authority);
        self.db
            .execute(
                "INSERT INTO funded_orders (
                     buyer, order_pubkey, hash, secret, buyer_orien, buyer_sol, authority_sol,
                     orn, lamports, expiry, min_bond, max_fill_window, state, created, updated
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, 'open', ?13, ?13)",
                params![
                    buyer,
                    order_pubkey,
                    hash_hex,
                    secret.as_slice(),
                    buyer_orien_hex,
                    buyer_sol,
                    authority_sol,
                    on_chain.terms.orn_amount as i64,
                    on_chain.terms.amount as i64,
                    on_chain.terms.expiry,
                    on_chain.terms.min_bond as i64,
                    on_chain.terms.max_fill_window,
                    at,
                ],
            )
            .map_err(|e| {
                if e.to_string().contains("UNIQUE") {
                    anyhow!("that Solana order is already listed")
                } else {
                    e.into()
                }
            })?;
        Ok(self.funded_order(self.db.last_insert_rowid())?.expect("just written"))
    }

    pub fn funded_order(&self, id: i64) -> Result<Option<FundedOrder>> {
        let sql = format!("{ORDER_SELECT} WHERE o.id = ?1");
        Ok(self.db.query_row(&sql, [id], order_row).optional()?)
    }

    pub(crate) fn funded_order_with_secret(&self, id: i64) -> Result<Option<OrderWithSecret>> {
        let Some(order) = self.funded_order(id)? else { return Ok(None) };
        let blob: Vec<u8> = self.db.query_row("SELECT secret FROM funded_orders WHERE id = ?1", [id], |r| r.get(0))?;
        let secret: [u8; 32] = blob.try_into().map_err(|_| anyhow!("corrupt secret length"))?;
        Ok(Some(OrderWithSecret { order, secret }))
    }

    /// Open funded buy orders, highest SOL per ORN first (best bid at top).
    pub fn open_funded_orders(&self) -> Result<Vec<FundedOrder>> {
        let sql = format!(
            "{ORDER_SELECT} WHERE o.state = 'open' AND o.expiry > ?1
             ORDER BY -(CAST(o.lamports AS REAL) / o.orn), o.created LIMIT 200"
        );
        let mut stmt = self.db.prepare(&sql)?;
        let rows = stmt.query_map([now() as i64], order_row)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// Funded orders where `account` is buyer or seller, newest first.
    pub fn my_funded_orders(&self, account: i64) -> Result<Vec<FundedOrder>> {
        let sql = format!("{ORDER_SELECT} WHERE o.buyer = ?1 OR o.seller = ?1 ORDER BY o.updated DESC LIMIT 50");
        let mut stmt = self.db.prepare(&sql)?;
        let rows = stmt.query_map([account], order_row)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// All funded orders that need background monitoring/settlement.
    pub(crate) fn active_funded_order_ids(&self) -> Result<Vec<i64>> {
        let mut stmt = self.db.prepare("SELECT id FROM funded_orders WHERE state IN ('open', 'filled') ORDER BY updated ASC LIMIT 200")?;
        let rows = stmt.query_map([], |r| r.get(0))?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// Marks an open funded order as filled once BOTH the Solana fill AND
    /// the Orien ORN lock have been verified directly on-chain.
    pub fn mark_funded_filled(
        &self,
        id: i64,
        seller: i64,
        seller_sol: &str,
        lock_from: &str,
        lock_seq: u64,
        fill_deadline: i64,
    ) -> Result<FundedOrder> {
        let updated = self.db.execute(
            "UPDATE funded_orders
             SET state = 'filled', seller = ?2, seller_sol = ?3, lock_from = ?4, lock_seq = ?5, fill_deadline = ?6, updated = ?7
             WHERE id = ?1 AND state = 'open'",
            params![id, seller, seller_sol, lock_from, lock_seq as i64, fill_deadline, now() as i64],
        )?;
        if updated == 0 {
            bail!("that order is no longer open");
        }
        Ok(self.funded_order(id)?.expect("just updated"))
    }

    pub fn mark_funded_orn_claimed(&self, id: i64) -> Result<()> {
        self.db.execute("UPDATE funded_orders SET orn_claimed = 1, updated = ?2 WHERE id = ?1", params![id, now() as i64])?;
        Ok(())
    }

    pub fn mark_funded_done(&self, id: i64, sol_claim_sig: Option<&str>) -> Result<bool> {
        let at = now() as i64;
        let changed = self.db.execute(
            "UPDATE funded_orders
             SET state = 'done', orn_claimed = 1, sol_claimed = 1, sol_claim_sig = COALESCE(?2, sol_claim_sig),
                 updated = ?3, finished = COALESCE(finished, ?3)
             WHERE id = ?1 AND state != 'done'",
            params![id, sol_claim_sig, at],
        )?;
        Ok(changed > 0)
    }

    pub fn reopen_funded_order(&self, id: i64) -> Result<()> {
        self.db.execute(
            "UPDATE funded_orders
             SET state = 'open', seller = NULL, seller_sol = NULL, lock_from = NULL, lock_seq = NULL,
                 fill_deadline = NULL, orn_claimed = 0, updated = ?2
             WHERE id = ?1 AND state = 'filled' AND orn_claimed = 0",
            params![id, now() as i64],
        )?;
        Ok(())
    }

    pub fn close_funded_order(&self, id: i64, state: &str) -> Result<()> {
        self.db.execute(
            "UPDATE funded_orders SET state = ?2, updated = ?3 WHERE id = ?1 AND state IN ('open', 'filled')",
            params![id, state, now() as i64],
        )?;
        Ok(())
    }

    pub fn unpriced_funded_orders(&self) -> Result<Vec<(i64, u64)>> {
        let mut stmt = self.db.prepare(
            "SELECT id, lamports FROM funded_orders WHERE state = 'done' AND usd IS NULL AND finished > ?1",
        )?;
        let rows = stmt.query_map([now().saturating_sub(600) as i64], |r| Ok((r.get(0)?, r.get::<_, i64>(1)? as u64)))?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    pub fn price_funded_order(&self, id: i64, usd: f64) -> Result<()> {
        if self.db.execute("UPDATE funded_orders SET usd = ?2 WHERE id = ?1 AND usd IS NULL", params![id, usd])? == 0 {
            return Ok(());
        }
        let (orn, lamports, finished): (i64, i64, Option<i64>) = self.db.query_row(
            "SELECT orn, lamports, finished FROM funded_orders WHERE id = ?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )?;
        let data = serde_json::json!({ "orn": orn as u64, "coin": "sol", "lamports": lamports as u64, "usd": usd });
        self.add_happening("swap", None, &data, finished.map_or_else(now, |f| f as u64))
    }

    /// Finished funded buy orders with dollar prices, newest first before `before`.
    pub fn funded_order_history(&self, before: Option<u64>, limit: u32) -> Result<Vec<serde_json::Value>> {
        let mut stmt = self.db.prepare(
            "SELECT orn, lamports, usd, finished FROM funded_orders
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
}

/// Verifies that the Orien lock `(lock_from, lock_seq)` is proven on Orien
/// with the exact `buyer_orien`, `orn`, `hash`, and at least `min_epochs_left`.
/// Returns `Ok(true)` if the lock is already claimed with `expected_secret`,
/// `Ok(false)` if it is waiting and valid to claim now, or `Err` otherwise.
pub fn verify_orien_lock(
    payments: &dyn crate::wallets::Payments,
    lock_from: &AccountId,
    lock_seq: u64,
    buyer_orien: &AccountId,
    orn: u64,
    expected_hash: &[u8; 32],
    expected_secret: Option<&[u8; 32]>,
    min_epochs_left: u64,
) -> Result<bool> {
    let Some(proven) = payments.proven_lock(lock_from, lock_seq)? else {
        bail!("the seller's ORN lock is not proven on Orien yet");
    };
    if let (Some(revealed), Some(want)) = (proven.secret, expected_secret)
        && &revealed == want
    {
        return Ok(true);
    }
    let Some(escrow) = proven.made else {
        bail!("no active ORN lock found at that address and sequence");
    };
    if &escrow.to != buyer_orien {
        bail!("that ORN lock pays a different Orien address");
    }
    if escrow.amount != orn {
        bail!("that ORN lock holds the wrong amount of ORN");
    }
    if &escrow.hash != expected_hash {
        bail!("that ORN lock uses a different secret hash");
    }
    if escrow.until < proven.epoch.saturating_add(min_epochs_left) {
        bail!("that ORN lock expires too soon to settle safely");
    }
    Ok(false)
}

/// Runs one settlement & housekeeping pass across all active funded buy orders.
///
/// SECURITY INVARIANT: the server NEVER uses or relays secret S until it has
/// verified the seller's ORN lock directly against Orien validators.
pub fn settle_once(app: &Arc<crate::api::App>) -> Result<usize> {
    let Some(chain) = app.order_chain() else {
        return Ok(0);
    };
    let ids = app.store_lock().active_funded_order_ids()?;
    let mut settled = 0usize;
    let chain_now = chain.now()?;
    for id in ids {
        let Some(rec) = app.store_lock().funded_order_with_secret(id)? else { continue };
        let order = rec.order;
        let Ok(order_pk) = solana::address(&order.order_pubkey) else { continue };
        let Ok(buyer_pk) = solana::address(&order.buyer_sol) else { continue };
        let on_chain = match chain.read_order(&order_pk) {
            Ok(v) => v,
            Err(e) => {
                eprintln!("funded order #{id}: cannot read Solana account: {e}");
                continue;
            }
        };

        // 1. Account already closed on Solana.
        let Some(acct) = on_chain else {
            if order.orn_claimed {
                // Both ORN and SOL are settled.
                if app.store_lock().mark_funded_done(id, None)? {
                    settled += 1;
                    notify_done(app, &order);
                }
            } else {
                let status = if chain_now >= order.expiry { "expired" } else { "cancelled" };
                app.store_lock().close_funded_order(id, status)?;
            }
            continue;
        };

        // 2. Order expired on Solana -> relay expire to refund buyer.
        if chain_now >= acct.terms.expiry && !order.orn_claimed {
            if let Err(e) = chain.relay_expire(&order_pk, &buyer_pk) {
                eprintln!("funded order #{id}: cannot relay expire: {e}");
            } else {
                eprintln!("funded order #{id}: expired and refunded buyer");
                app.store_lock().close_funded_order(id, "expired")?;
            }
            continue;
        }

        // 3. Any on-chain lapsed fill (whether or not the seller reported an ORN lock) -> relay reopen.
        if !order.orn_claimed && acct.state == STATE_FILLED && chain_now >= acct.fill_deadline {
            if let Err(e) = chain.relay_reopen(&order_pk, &buyer_pk) {
                eprintln!("funded order #{id}: cannot relay reopen: {e}");
            } else {
                eprintln!("funded order #{id}: fill lapsed, reopened order");
                app.store_lock().reopen_funded_order(id)?;
            }
            continue;
        }

        // 4. Open order — nothing to settle yet.
        if order.state == "open" {
            continue;
        }

        // 5. Filled order within window: verify ORN lock directly on Orien FIRST.
        let (Some(lock_from_hex), Some(lock_seq)) = (order.lock_from.as_deref(), order.lock_seq) else {
            continue;
        };
        let Ok(lock_from) = lock_from_hex.parse::<AccountId>() else { continue };
        let Ok(buyer_orien) = order.buyer_orien.parse::<AccountId>() else { continue };
        let Ok(hash_bytes) = hex::decode(&order.hash) else { continue };
        let Ok(expected_hash): Result<[u8; 32], _> = hash_bytes.try_into() else { continue };

        let mut orn_claimed = order.orn_claimed;
        if !orn_claimed {
            match verify_orien_lock(
                app.payments().as_ref(),
                &lock_from,
                lock_seq,
                &buyer_orien,
                order.orn,
                &expected_hash,
                Some(&rec.secret),
                1,
            ) {
                Ok(true) => {
                    // Already claimed on Orien (e.g. buyer's own open wallet claimed it).
                    app.store_lock().mark_funded_orn_claimed(id)?;
                    orn_claimed = true;
                }
                Ok(false) => {
                    // Verified directly on Orien! Claim ORN for buyer_orien with S.
                    match app.payments().claim_lock(&lock_from, lock_seq, &buyer_orien, order.orn, rec.secret) {
                        Ok(()) => {
                            // Never log rec.secret!
                            eprintln!("funded order #{id}: claimed {} ORN on Orien for buyer", crate::store::orn(order.orn));
                            app.store_lock().mark_funded_orn_claimed(id)?;
                            orn_claimed = true;
                        }
                        Err(e) => {
                            eprintln!("funded order #{id}: ORN claim failed: {e}");
                            continue;
                        }
                    }
                }
                Err(e) => {
                    eprintln!("funded order #{id}: waiting for valid ORN lock: {e}");
                    continue;
                }
            }
        }

        // 6. ONLY after ORN is claimed on Orien: relay `claim` on Solana to pay seller.
        if orn_claimed && !order.sol_claimed && acct.state == STATE_FILLED && chain_now < acct.fill_deadline {
            match chain.relay_claim(&order_pk, &acct.seller, &acct.terms.buyer, &rec.secret) {
                Ok(sig) => {
                    // Never log rec.secret!
                    eprintln!(
                        "funded order #{id}: relayed Solana claim sig={sig} to {}",
                        solana::text(&acct.seller)
                    );
                    if app.store_lock().mark_funded_done(id, Some(&sig))? {
                        settled += 1;
                        notify_done(app, &order);
                    }
                }
                Err(e) => {
                    eprintln!("funded order #{id}: Solana claim relay failed: {e}");
                }
            }
        }
    }
    Ok(settled)
}

fn notify_done(app: &Arc<crate::api::App>, order: &FundedOrder) {
    let orn = crate::store::orn(order.orn);
    let sol = format!("{} SOL", format!("{:.4}", order.lamports as f64 / 1e9).trim_end_matches('0').trim_end_matches('.'));
    app.tell(crate::push::Event {
        account: order.buyer,
        address: Some(order.buyer_orien.clone()),
        body: format!("Buy order filled: {orn} ORN are in your wallet"),
        kind: "swap",
        open: serde_json::json!({ "order": order.id }),
    });
    if let Some(seller) = order.seller {
        app.tell(crate::push::Event {
            account: seller,
            address: order.lock_from.clone(),
            body: format!("Order settled: {sol} sent to your SOL wallet"),
            kind: "swap",
            open: serde_json::json!({ "order": order.id }),
        });
    }
}

/// Background worker that settles funded buy orders every few seconds so
/// sellers are paid in ~1 minute without either side staying online.
pub fn watch(app: Arc<crate::api::App>) {
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(std::time::Duration::from_secs(8));
        loop {
            tick.tick().await;
            let app_clone = app.clone();
            let _ = tokio::task::spawn_blocking(move || {
                if let Err(e) = settle_once(&app_clone) {
                    eprintln!("funded orders watcher: {e}");
                }
            })
            .await;
            crate::api::price_finished_orders(&app).await;
        }
    });
}
