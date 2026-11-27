//! Solana, for ORN–SOL swaps and Orien Wallet's SOL.
//!
//! Reading: several RPC services, asked the same way the Bitcoin explorers
//! are (`chain.rs`): answers reused while fresh, a service that says it's busy
//! (429) or failing (5xx) left alone for a while, and decisions that could
//! cost someone made only when most of them answer, taking the least
//! favourable answer.
//!
//! Writing: legacy transactions built and signed here, byte by byte, so the
//! wallet (and the iPhone app) need none of Solana's own large libraries.
//! The format is small and fixed; a live test on devnet checks it.

pub mod history;
pub mod devnet_pool;
pub mod profile;

use crate::chain::{MAX_REST, REST, remember, remembered, rest, resting};
use anyhow::{Context, Result, anyhow, bail};
use ed25519_dalek::{Signer, SigningKey};
use serde_json::{Value, json};
use std::time::Duration;

/// orien-lock on Solana's main network: deployed 2026-11-16 from the
/// reviewed 4,016-byte build, then made final, so nobody can change it.
pub const MAINNET_LOCK_PROGRAM: &str = "DAeXj31NAoSgno7SiFX7t4tWoMDBtpN9rn8c2X7WNtYE";

pub const LAMPORTS_PER_SOL: u64 = 1_000_000_000;
/// The orien-lock program's lock account, in bytes.
pub const LOCK_LEN: u64 = 113;
/// orien-lock on devnet, for testing. (Mainnet's is set once it's deployed.)
pub const DEVNET_LOCK_PROGRAM: &str = "99zcMPAgZwXxoLDwor424zro9BGMATTsMk8djqov2qKf";
/// Solana's System Program: all zeros.
pub const SYSTEM_PROGRAM: Pubkey = [0; 32];

/// How long a read is reused: a Solana slot is 400 ms, but nothing a wallet
/// shows needs to be fresher than this.
const FRESH: Duration = Duration::from_secs(10);

pub type Pubkey = [u8; 32];

/// A Solana address from its usual base58 text.
pub fn address(text: &str) -> Result<Pubkey> {
    let bytes = bs58::decode(text.trim()).into_vec().map_err(|_| anyhow!("that is not a Solana address"))?;
    bytes.try_into().map_err(|_| anyhow!("that is not a Solana address"))
}

/// An address, or a signature, as base58 text.
pub fn text(bytes: &[u8]) -> String {
    bs58::encode(bytes).into_string()
}

/// Whether an address can have a regular Ed25519 wallet signing key.
pub fn is_wallet_key(key: &Pubkey) -> bool {
    ed25519_dalek::VerifyingKey::from_bytes(key).is_ok_and(|key| !key.is_weak())
}

/// A Solana key pair.
pub struct Keypair(SigningKey);

impl Keypair {
    /// From a 32-byte ed25519 secret (Orien Wallet derives one per account).
    pub fn from_secret(secret: &[u8; 32]) -> Keypair {
        Keypair(SigningKey::from_bytes(secret))
    }

    /// A fresh one, for a lock account.
    pub fn random() -> Keypair {
        let mut secret = [0u8; 32];
        bitcoin::secp256k1::rand::RngCore::fill_bytes(&mut bitcoin::secp256k1::rand::thread_rng(), &mut secret);
        Keypair::from_secret(&secret)
    }

    pub fn pubkey(&self) -> Pubkey {
        self.0.verifying_key().to_bytes()
    }

    fn sign(&self, message: &[u8]) -> [u8; 64] {
        self.0.sign(message).to_bytes()
    }
}

// ---- Reading -------------------------------------------------------------

/// One RPC service.
pub struct Rpc {
    base: String,
    agent: ureq::Agent,
}

impl Rpc {
    pub fn at(base: &str) -> Rpc {
        let agent = ureq::AgentBuilder::new().timeout(Duration::from_secs(20)).user_agent("orien-swap").build();
        Rpc { base: base.trim_end_matches('/').to_string(), agent }
    }

    /// A JSON-RPC call. With `fresh`, an answer is reused for that long.
    fn call(&self, method: &str, params: Value, fresh: Option<Duration>) -> Result<Value> {
        let body = json!({ "jsonrpc": "2.0", "id": 1, "method": method, "params": params }).to_string();
        let key = format!("{} {body}", self.base);
        if let Some(fresh) = fresh
            && let Some(answer) = remembered(&key, fresh)
        {
            return Ok(serde_json::from_str(&answer)?);
        }
        if let Some(until) = resting(&self.base) {
            bail!("{} is busy; asking again in {} seconds", self.base, until.saturating_duration_since(std::time::Instant::now()).as_secs());
        }
        let answer = match self.agent.post(&self.base).set("Content-Type", "application/json").send_string(&body) {
            Ok(response) => response.into_string()?,
            Err(ureq::Error::Status(code, response)) if code == 429 || code >= 500 => {
                let asked = response.header("retry-after").and_then(|after| after.trim().parse::<u64>().ok());
                rest(&self.base, Duration::from_secs(asked.unwrap_or(REST.as_secs()).clamp(10, MAX_REST.as_secs())));
                bail!("{} is busy ({code}); asking the other services", self.base)
            }
            Err(error) => return Err(error).with_context(|| format!("cannot reach {}", self.base)),
        };
        let parsed: Value = serde_json::from_str(&answer).with_context(|| format!("{} gave no JSON", self.base))?;
        if let Some(error) = parsed.get("error") {
            // Some services say they're busy in the answer itself.
            if error["code"].as_i64() == Some(429) || error["code"].as_i64() == Some(-32429) {
                rest(&self.base, REST);
            }
            bail!("{}: {}", self.base, error["message"].as_str().unwrap_or("an error"));
        }
        let result = parsed["result"].clone();
        if fresh.is_some() {
            remember(&key, &result.to_string());
        }
        Ok(result)
    }
}

/// An account as Solana holds it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Account {
    pub lamports: u64,
    pub owner: Pubkey,
    pub data: Vec<u8>,
    /// A program: it can never be paid.
    pub executable: bool,
}

/// An orien-lock lock, as it stands.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Lock {
    pub hash: [u8; 32],
    /// Paid the amount with the secret.
    pub seller: Pubkey,
    /// Paid everything back after the deadline.
    pub buyer: Pubkey,
    /// A Unix time, in seconds.
    pub deadline: i64,
    pub amount: u64,
    /// What the lock account holds: the amount and its deposit.
    pub lamports: u64,
}

impl Lock {
    /// The lock in an account, if the account is one: owned by `program`,
    /// the right length, and locked.
    pub fn read(account: &Account, program: &Pubkey) -> Option<Lock> {
        let d = &account.data;
        if &account.owner != program || d.len() != LOCK_LEN as usize || d[0] != 1 {
            return None;
        }
        let at = |range: std::ops::Range<usize>| -> [u8; 32] { d[range].try_into().expect("32 bytes") };
        Some(Lock {
            hash: at(1..33),
            seller: at(33..65),
            buyer: at(65..97),
            deadline: i64::from_le_bytes(d[97..105].try_into().expect("8 bytes")),
            amount: u64::from_le_bytes(d[105..113].try_into().expect("8 bytes")),
            lamports: account.lamports,
        })
    }
}

/// How far along a sent transaction is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Landed {
    Processed,
    Confirmed,
    Finalized,
}

pub struct TransactionStatus {
    pub level: Landed,
    pub error: Option<Value>,
}

/// Solana, through several services, and where orien-lock is on it.
pub struct Solana {
    rpcs: Vec<Rpc>,
    /// orien-lock's address on this network, once it's there.
    pub lock_program: Option<Pubkey>,
}

impl Solana {
    /// Solana's main network, through the public services that answer with
    /// no account. orien-lock isn't on it yet.
    pub fn mainnet() -> Solana {
        Solana::with(
            &["https://api.mainnet-beta.solana.com", "https://solana-rpc.publicnode.com"],
            Some(address(MAINNET_LOCK_PROGRAM).expect("a valid address")),
        )
    }

    /// Solana's test network, with orien-lock.
    pub fn devnet() -> Solana {
        Solana::with(&["https://api.devnet.solana.com"], Some(address(DEVNET_LOCK_PROGRAM).expect("a valid address")))
    }

    pub fn with(bases: &[&str], lock_program: Option<Pubkey>) -> Solana {
        Solana { rpcs: bases.iter().map(|base| Rpc::at(base)).collect(), lock_program }
    }

    fn quorum(&self) -> usize {
        self.rpcs.len() / 2 + 1
    }

    /// What the services that answer say; an error only if none does.
    fn answers<T>(&self, ask: impl Fn(&Rpc) -> Result<T>) -> Result<Vec<T>> {
        let (mut answers, mut last) = (Vec::new(), None);
        for rpc in &self.rpcs {
            match ask(rpc) {
                Ok(answer) => answers.push(answer),
                Err(error) => last = Some(error),
            }
        }
        if answers.is_empty() {
            return Err(last.unwrap_or_else(|| anyhow!("no Solana service is set")));
        }
        Ok(answers)
    }

    /// What most services say; an error if too few answer.
    fn most<T>(&self, ask: impl Fn(&Rpc) -> Result<T>) -> Result<Vec<T>> {
        let (mut answers, mut last) = (Vec::new(), None);
        for rpc in &self.rpcs {
            match ask(rpc) {
                Ok(answer) => answers.push(answer),
                Err(error) => last = Some(error),
            }
        }
        if answers.len() < self.quorum() {
            return Err(last
                .unwrap_or_else(|| anyhow!("no Solana service is set"))
                .context("too few Solana services answered; trying again shortly"));
        }
        Ok(answers)
    }

    /// An address's SOL, in lamports: the least most services report,
    /// confirmed. For showing a balance and checking one can pay.
    pub fn balance(&self, who: &Pubkey) -> Result<u64> {
        let balances = self.most(|rpc| {
            let found = rpc.call("getBalance", json!([text(who), { "commitment": "confirmed" }]), Some(FRESH))?;
            found["value"].as_u64().ok_or_else(|| anyhow!("{} gave no balance", rpc.base))
        })?;
        Ok(balances.into_iter().min().expect("at least one"))
    }

    /// The total owned across token accounts for a mint, in its smallest units.
    pub fn token_balance(&self, owner: &Pubkey, mint: &Pubkey) -> Result<u64> {
        let balances = self.most(|rpc| {
            let found = rpc.call(
                "getTokenAccountsByOwner",
                json!([text(owner), { "mint": text(mint) }, { "commitment": "confirmed", "encoding": "jsonParsed" }]),
                Some(FRESH),
            )?;
            let accounts = found["value"].as_array().ok_or_else(|| anyhow!("{} gave no token accounts", rpc.base))?;
            accounts.iter().try_fold(0_u64, |total, account| {
                let amount = account.pointer("/account/data/parsed/info/tokenAmount/amount")
                    .and_then(Value::as_str)
                    .and_then(|amount| amount.parse::<u64>().ok())
                    .ok_or_else(|| anyhow!("{} gave an invalid token balance", rpc.base))?;
                total.checked_add(amount).ok_or_else(|| anyhow!("token balance is too large"))
            })
        })?;
        Ok(balances.into_iter().min().expect("at least one"))
    }

    /// An account, finalized, if most services agree on it exactly; none if
    /// any of them has none.
    pub fn account(&self, who: &Pubkey) -> Result<Option<Account>> {
        let accounts = self.most(|rpc| {
            let found = rpc.call("getAccountInfo", json!([text(who), { "commitment": "finalized", "encoding": "base64" }]), Some(FRESH))?;
            let value = &found["value"];
            if value.is_null() {
                return Ok(None);
            }
            use base64::Engine;
            let data = value["data"][0].as_str().ok_or_else(|| anyhow!("{} gave no account data", rpc.base))?;
            Ok(Some(Account {
                lamports: value["lamports"].as_u64().ok_or_else(|| anyhow!("no lamports"))?,
                owner: address(value["owner"].as_str().ok_or_else(|| anyhow!("no owner"))?)?,
                data: base64::engine::general_purpose::STANDARD.decode(data)?,
                executable: value["executable"].as_bool().ok_or_else(|| anyhow!("no executable flag"))?,
            }))
        })?;
        if accounts.iter().any(Option::is_none) {
            return Ok(None);
        }
        if accounts.windows(2).any(|pair| pair[0] != pair[1]) {
            bail!("the Solana services disagree about {}; trying again shortly", text(who));
        }
        Ok(accounts.into_iter().next().expect("at least one"))
    }

    /// The orien-lock lock at `at`, finalized, if there is one.
    pub fn lock(&self, at: &Pubkey) -> Result<Option<Lock>> {
        let program = self.lock_program.ok_or_else(|| anyhow!("SOL swaps are not open on this network yet"))?;
        Ok(self.account(at)?.and_then(|account| Lock::read(&account, &program)))
    }

    /// The orien-order account at `at` owned by `program`, finalized, if open or filled.
    pub fn order(&self, program: &Pubkey, at: &Pubkey) -> Result<Option<crate::order::OrderAccount>> {
        Ok(self.account(at)?.and_then(|account| crate::order::OrderAccount::read(&account, program)))
    }

    /// Solana's clock, finalized: the latest any service reports, so a
    /// deadline is never thought further off than it is.
    pub fn time(&self) -> Result<i64> {
        let times = self.answers(|rpc| {
            let slot = rpc.call("getSlot", json!([{ "commitment": "finalized" }]), Some(Duration::from_secs(2)))?;
            let slot = slot.as_u64().ok_or_else(|| anyhow!("{} gave no slot", rpc.base))?;
            rpc.call("getBlockTime", json!([slot]), Some(Duration::from_secs(2)))?.as_i64().ok_or_else(|| anyhow!("{} gave no time", rpc.base))
        })?;
        Ok(times.into_iter().max().expect("at least one"))
    }

    /// The deposit an account of `len` bytes needs: the most any service says.
    pub fn rent(&self, len: u64) -> Result<u64> {
        let rents = self.answers(|rpc| {
            rpc.call("getMinimumBalanceForRentExemption", json!([len]), Some(Duration::from_secs(3600)))?
                .as_u64()
                .ok_or_else(|| anyhow!("{} gave no rent", rpc.base))
        })?;
        Ok(rents.into_iter().max().expect("at least one"))
    }

    /// A recent blockhash, for a new transaction.
    pub fn blockhash(&self) -> Result<[u8; 32]> {
        let hashes = self.answers(|rpc| {
            let found = rpc.call("getLatestBlockhash", json!([{ "commitment": "confirmed" }]), None)?;
            address(found["value"]["blockhash"].as_str().ok_or_else(|| anyhow!("{} gave no blockhash", rpc.base))?)
        })?;
        Ok(hashes[0])
    }

    /// A recent blockhash and the height after which it can no longer land.
    pub fn blockhash_lifetime(&self) -> Result<(Pubkey, u64)> {
        let values = self.answers(|rpc| {
            let found = rpc.call("getLatestBlockhash", json!([{ "commitment": "confirmed" }]), None)?;
            let value = &found["value"];
            let hash = address(value["blockhash"].as_str().ok_or_else(|| anyhow!("Solana gave no blockhash"))?)?;
            let last = value["lastValidBlockHeight"].as_u64().ok_or_else(|| anyhow!("Solana gave no transaction expiry"))?;
            Ok((hash, last))
        })?;
        Ok(values[0])
    }

    pub fn finalized_block_height(&self) -> Result<u64> {
        let heights = self.most(|rpc| {
            rpc.call("getBlockHeight", json!([{ "commitment": "finalized" }]), None)?
                .as_u64()
                .ok_or_else(|| anyhow!("Solana gave no block height"))
        })?;
        Ok(heights.into_iter().min().expect("at least one"))
    }

    pub fn blockhash_valid(&self, blockhash: &Pubkey) -> Result<bool> {
        let answers = self.most(|rpc| {
            let found = rpc.call("isBlockhashValid", json!([text(blockhash), { "commitment": "confirmed" }]), None)?;
            found["value"].as_bool().ok_or_else(|| anyhow!("Solana did not check the blockhash"))
        })?;
        Ok(answers.into_iter().all(|valid| valid))
    }

    /// Quotes the fee for the same message the wallet will sign.
    pub fn fee_for_message(&self, payer: &Pubkey, instructions: &[Instruction], blockhash: Pubkey) -> Result<u64> {
        use base64::Engine;
        let (message, _) = legacy_message(payer, instructions, blockhash);
        let wire = base64::engine::general_purpose::STANDARD.encode(message);
        let fees = self.most(|rpc| {
            let found = rpc.call("getFeeForMessage", json!([wire, { "commitment": "confirmed" }]), None)?;
            found["value"].as_u64().ok_or_else(|| anyhow!("Solana could not quote the fee. Review the transfer again."))
        })?;
        Ok(fees.into_iter().max().expect("at least one"))
    }

    /// Sends a signed transaction through every service; its signature, if
    /// any of them took it.
    pub fn send(&self, transaction: &[u8]) -> Result<String> {
        use base64::Engine;
        let wire = base64::engine::general_purpose::STANDARD.encode(transaction);
        self.answers(|rpc| {
            rpc.call("sendTransaction", json!([wire, { "encoding": "base64", "preflightCommitment": "confirmed" }]), None)?
                .as_str()
                .map(str::to_string)
                .ok_or_else(|| anyhow!("{} gave no signature", rpc.base))
        })
        .map(|signatures| signatures[0].clone())
    }

    /// How far a sent transaction has landed; an error if it failed.
    pub fn landed(&self, signature: &str) -> Result<Option<Landed>> {
        let Some(status) = self.transaction_status(signature)? else { return Ok(None) };
        if let Some(error) = status.error {
            bail!("Solana refused the transaction: {error}");
        }
        Ok(Some(status.level))
    }

    pub fn transaction_status(&self, signature: &str) -> Result<Option<TransactionStatus>> {
        let statuses = self.answers(|rpc| {
            let found = rpc.call("getSignatureStatuses", json!([[signature], { "searchTransactionHistory": true }]), None)?;
            Ok(found["value"][0].clone())
        })?;
        let mut best = None;
        for status in statuses.iter().filter(|s| !s.is_null()) {
            let level = match status["confirmationStatus"].as_str() {
                Some("finalized") => Landed::Finalized,
                Some("confirmed") => Landed::Confirmed,
                _ => Landed::Processed,
            };
            if !status["err"].is_null() {
                return Ok(Some(TransactionStatus { level, error: Some(status["err"].clone()) }));
            }
            best = best.max(Some(level));
        }
        Ok(best.map(|level| TransactionStatus { level, error: None }))
    }

    /// The secret that claimed the lock at `lock`, read from the claim
    /// transaction, if one has. Checked against `hash`, so any one service
    /// that has it is enough.
    pub fn secret(&self, lock: &Pubkey, hash: &[u8; 32]) -> Result<Option<[u8; 32]>> {
        let program = self.lock_program.ok_or_else(|| anyhow!("SOL swaps are not open on this network yet"))?;
        let found = self.answers(|rpc| {
            let signatures = rpc.call("getSignaturesForAddress", json!([text(lock), { "limit": 20, "commitment": "confirmed" }]), Some(FRESH))?;
            for entry in signatures.as_array().into_iter().flatten().filter(|e| e["err"].is_null()) {
                let Some(signature) = entry["signature"].as_str() else { continue };
                let tx = rpc.call(
                    "getTransaction",
                    json!([signature, { "encoding": "json", "commitment": "confirmed", "maxSupportedTransactionVersion": 0 }]),
                    Some(Duration::from_secs(3600)),
                )?;
                let keys = &tx["transaction"]["message"]["accountKeys"];
                for ix in tx["transaction"]["message"]["instructions"].as_array().into_iter().flatten() {
                    let index = ix["programIdIndex"].as_u64().unwrap_or(u64::MAX) as usize;
                    if keys[index].as_str().and_then(|k| address(k).ok()) != Some(program) {
                        continue;
                    }
                    let data = bs58::decode(ix["data"].as_str().unwrap_or("")).into_vec().unwrap_or_default();
                    if data.len() == 33 && data[0] == 1 {
                        let secret: [u8; 32] = data[1..].try_into().expect("32 bytes");
                        if &orien_ledger::types::lock_hash(&secret) == hash {
                            return Ok(Some(secret));
                        }
                    }
                }
            }
            Ok(None)
        })?;
        Ok(found.into_iter().flatten().next())
    }
}

// ---- Writing -------------------------------------------------------------

/// One instruction: a program, its accounts as (address, signer, writable),
/// and its data.
#[derive(Clone, Debug)]
pub struct Instruction {
    pub program: Pubkey,
    pub accounts: Vec<(Pubkey, bool, bool)>,
    pub data: Vec<u8>,
}

/// Solana's compact length: 7 bits a byte, high bit for "more".
fn compact(mut n: usize, out: &mut Vec<u8>) {
    loop {
        let mut byte = (n & 0x7f) as u8;
        n >>= 7;
        if n != 0 {
            byte |= 0x80;
        }
        out.push(byte);
        if n == 0 {
            break;
        }
    }
}

fn legacy_message(payer: &Pubkey, instructions: &[Instruction], blockhash: Pubkey) -> (Vec<u8>, Vec<Pubkey>) {
    // Every account once, with the strongest rights any use gives it.
    let mut keys: Vec<(Pubkey, bool, bool)> = vec![(*payer, true, true)];
    let mut note = |key: Pubkey, signer: bool, writable: bool| match keys.iter_mut().find(|(k, _, _)| *k == key) {
        Some(entry) => {
            entry.1 |= signer;
            entry.2 |= writable;
        }
        None => keys.push((key, signer, writable)),
    };
    for ix in instructions {
        for &(key, signer, writable) in &ix.accounts {
            note(key, signer, writable);
        }
        note(ix.program, false, false);
    }
    // The payer first, then signers that write, signers that don't, others
    // that write, others that don't.
    let rank = |&(_, signer, writable): &(Pubkey, bool, bool)| match (signer, writable) {
        (true, true) => 0,
        (true, false) => 1,
        (false, true) => 2,
        (false, false) => 3,
    };
    keys[1..].sort_by_key(rank);
    let signers: Vec<Pubkey> = keys.iter().filter(|k| k.1).map(|k| k.0).collect();
    let readonly_signed = keys.iter().filter(|k| k.1 && !k.2).count();
    let readonly_unsigned = keys.iter().filter(|k| !k.1 && !k.2).count();

    let mut message = vec![signers.len() as u8, readonly_signed as u8, readonly_unsigned as u8];
    compact(keys.len(), &mut message);
    for (key, _, _) in &keys {
        message.extend_from_slice(key);
    }
    message.extend_from_slice(&blockhash);
    compact(instructions.len(), &mut message);
    let index = |key: &Pubkey| keys.iter().position(|(k, _, _)| k == key).expect("noted above") as u8;
    for ix in instructions {
        message.push(index(&ix.program));
        compact(ix.accounts.len(), &mut message);
        for (key, _, _) in &ix.accounts {
            message.push(index(key));
        }
        compact(ix.data.len(), &mut message);
        message.extend_from_slice(&ix.data);
    }
    (message, signers)
}

/// A signed legacy transaction, ready to send. `payer` pays the fee and
/// signs first; `others` are the other signers the instructions need.
pub fn transaction(payer: &Keypair, others: &[&Keypair], instructions: &[Instruction], blockhash: Pubkey) -> Result<Vec<u8>> {
    let payer_key = payer.pubkey();
    let (message, signers) = legacy_message(&payer_key, instructions, blockhash);
    let mut out = Vec::new();
    compact(signers.len(), &mut out);
    for key in &signers {
        let signer = if *key == payer_key { payer } else { others.iter().find(|o| o.pubkey() == *key).ok_or_else(|| anyhow!("a signer is missing"))? };
        out.extend_from_slice(&signer.sign(&message));
    }
    out.extend_from_slice(&message);
    if out.len() > 1232 {
        bail!("the transaction is too big for Solana");
    }
    Ok(out)
}

/// The transaction's id: its first signature, as base58.
pub fn signature_of(transaction: &[u8]) -> String {
    text(&transaction[1..65])
}

/// System Program: send `lamports` from `from` to `to`.
pub fn transfer(from: &Pubkey, to: &Pubkey, lamports: u64) -> Instruction {
    let mut data = 2u32.to_le_bytes().to_vec();
    data.extend_from_slice(&lamports.to_le_bytes());
    Instruction { program: SYSTEM_PROGRAM, accounts: vec![(*from, true, true), (*to, false, true)], data }
}

/// System Program: a new account at `new`, funded from `from`.
pub fn create_account(from: &Pubkey, new: &Pubkey, lamports: u64, space: u64, owner: &Pubkey) -> Instruction {
    let mut data = 0u32.to_le_bytes().to_vec();
    data.extend_from_slice(&lamports.to_le_bytes());
    data.extend_from_slice(&space.to_le_bytes());
    data.extend_from_slice(owner);
    Instruction { program: SYSTEM_PROGRAM, accounts: vec![(*from, true, true), (*new, true, true)], data }
}

/// orien-lock: the instructions that create and fill a lock at `lock`,
/// funded by `buyer` with the amount and the deposit.
pub fn lock(program: &Pubkey, buyer: &Pubkey, lock: &Pubkey, terms: &Lock, deposit: u64) -> Vec<Instruction> {
    let mut data = vec![0u8];
    data.extend_from_slice(&terms.hash);
    data.extend_from_slice(&terms.seller);
    data.extend_from_slice(&terms.buyer);
    data.extend_from_slice(&terms.deadline.to_le_bytes());
    data.extend_from_slice(&terms.amount.to_le_bytes());
    // Not create_account, which fails on an address that already holds
    // SOL: anyone who reads the lock address in the reply code could send
    // it a little to block the swap. A transfer, then making the account
    // 113 bytes and the program's (both need the lock key), works either way.
    vec![
        transfer(buyer, lock, terms.amount + deposit),
        allocate(lock, LOCK_LEN),
        assign(lock, program),
        Instruction { program: *program, accounts: vec![(*lock, true, true)], data },
    ]
}

/// System Program: gives an account `space` bytes; it signs.
pub fn allocate(who: &Pubkey, space: u64) -> Instruction {
    let mut data = 8u32.to_le_bytes().to_vec();
    data.extend_from_slice(&space.to_le_bytes());
    Instruction { program: SYSTEM_PROGRAM, accounts: vec![(*who, true, true)], data }
}

/// System Program: hands an account to `owner`; it signs.
pub fn assign(who: &Pubkey, owner: &Pubkey) -> Instruction {
    let mut data = 1u32.to_le_bytes().to_vec();
    data.extend_from_slice(owner);
    Instruction { program: SYSTEM_PROGRAM, accounts: vec![(*who, true, true)], data }
}

/// orien-lock: pay the seller with the secret.
pub fn claim(program: &Pubkey, lock: &Pubkey, terms: &Lock, secret: &[u8; 32]) -> Instruction {
    let mut data = vec![1u8];
    data.extend_from_slice(secret);
    Instruction { program: *program, accounts: vec![(*lock, false, true), (terms.seller, false, true), (terms.buyer, false, true)], data }
}

/// orien-lock: everything back to the buyer, after the deadline.
pub fn refund(program: &Pubkey, lock: &Pubkey, terms: &Lock) -> Instruction {
    Instruction { program: *program, accounts: vec![(*lock, false, true), (terms.buyer, false, true)], data: vec![2] }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compact_lengths_match_solanas() {
        let enc = |n| {
            let mut out = Vec::new();
            compact(n, &mut out);
            out
        };
        assert_eq!(enc(0), [0]);
        assert_eq!(enc(127), [0x7f]);
        assert_eq!(enc(128), [0x80, 0x01]);
        assert_eq!(enc(16383), [0xff, 0x7f]);
        assert_eq!(enc(16384), [0x80, 0x80, 0x01]);
    }

    #[test]
    fn addresses_round_trip_and_bad_ones_are_refused() {
        let key = address(DEVNET_LOCK_PROGRAM).unwrap();
        assert_eq!(text(&key), DEVNET_LOCK_PROGRAM);
        assert_eq!(text(&SYSTEM_PROGRAM), "11111111111111111111111111111111");
        assert!(address("not an address").is_err());
        assert!(address("111").is_err());
    }

    #[test]
    fn a_transfer_is_laid_out_as_solana_expects() {
        let (payer, to) = (Keypair::from_secret(&[1; 32]), [9u8; 32]);
        let tx = transaction(&payer, &[], &[transfer(&payer.pubkey(), &to, 5)], [3; 32]).unwrap();
        // 1 signature, then the header: 1 signer, 0 read-only signed, 1 read-only (the System Program).
        assert_eq!(tx[0], 1);
        let message = &tx[65..];
        assert_eq!(&message[..3], &[1, 0, 1]);
        // Keys: payer, recipient, System Program.
        assert_eq!(message[3], 3);
        assert_eq!(&message[4..36], &payer.pubkey());
        assert_eq!(&message[36..68], &to);
        assert_eq!(&message[68..100], &SYSTEM_PROGRAM);
        assert_eq!(&message[100..132], &[3; 32]);
        // One instruction: program 2, accounts [0, 1], 12 bytes of data.
        assert_eq!(&message[132..137], &[1, 2, 2, 0, 1]);
        assert_eq!(message[137], 12);
        assert_eq!(&message[138..150], &[2, 0, 0, 0, 5, 0, 0, 0, 0, 0, 0, 0]);
        // The signature is the payer's, over the message.
        let signature = ed25519_dalek::Signature::from_bytes(tx[1..65].try_into().unwrap());
        assert!(ed25519_dalek::VerifyingKey::from_bytes(&payer.pubkey()).unwrap().verify_strict(message, &signature).is_ok());
    }

    #[test]
    fn a_missing_signer_is_caught() {
        let (payer, lock) = (Keypair::from_secret(&[1; 32]), Keypair::from_secret(&[2; 32]));
        let ix = create_account(&payer.pubkey(), &lock.pubkey(), 1, LOCK_LEN, &[5; 32]);
        assert!(transaction(&payer, &[], std::slice::from_ref(&ix), [0; 32]).is_err());
        assert!(transaction(&payer, &[&lock], &[ix], [0; 32]).is_ok());
    }

    #[test]
    fn a_lock_is_read_only_from_the_right_program() {
        let program = [7u8; 32];
        let mut data = vec![1u8];
        data.extend_from_slice(&[1; 32]);
        data.extend_from_slice(&[2; 32]);
        data.extend_from_slice(&[3; 32]);
        data.extend_from_slice(&1_800_000_000i64.to_le_bytes());
        data.extend_from_slice(&5u64.to_le_bytes());
        let account = Account { lamports: 99, owner: program, data, executable: false };
        let lock = Lock::read(&account, &program).unwrap();
        assert_eq!((lock.seller, lock.buyer, lock.deadline, lock.amount), ([2; 32], [3; 32], 1_800_000_000, 5));
        assert!(Lock::read(&account, &[8; 32]).is_none(), "another program's account");
        let mut unlocked = account.clone();
        unlocked.data[0] = 0;
        assert!(Lock::read(&unlocked, &program).is_none());
    }

    /// The real thing, on devnet: a lock, a wrong secret refused, the claim,
    /// and the secret read back from Solana. Pays with the test-network key.
    /// `cargo test -p orien-swap solana_devnet -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn solana_devnet() {
        let key = std::fs::read_to_string(std::env::var("HOME").unwrap() + "/.config/orien/solana-devnet.json").unwrap();
        let bytes: Vec<u8> = serde_json::from_str(&key).unwrap();
        let buyer = Keypair::from_secret(bytes[..32].try_into().unwrap());
        assert_eq!(buyer.pubkey().as_slice(), &bytes[32..64], "the key file's public half matches");
        let sol = Solana::devnet();
        let program = sol.lock_program.unwrap();
        let seller = Keypair::random();
        let wait = |signature: &str| {
            for _ in 0..60 {
                std::thread::sleep(Duration::from_millis(800));
                if sol.landed(signature).unwrap() >= Some(Landed::Confirmed) {
                    return;
                }
            }
            panic!("{signature} did not land");
        };

        let fund = transaction(&buyer, &[], &[transfer(&buyer.pubkey(), &seller.pubkey(), LAMPORTS_PER_SOL / 100)], sol.blockhash().unwrap()).unwrap();
        wait(&sol.send(&fund).unwrap());
        println!("funded the seller: {}", signature_of(&fund));

        let secret = [42u8; 32];
        let terms = Lock {
            hash: orien_ledger::types::lock_hash(&secret),
            seller: seller.pubkey(),
            buyer: buyer.pubkey(),
            deadline: sol.time().unwrap() + 600,
            amount: LAMPORTS_PER_SOL / 50,
            lamports: 0,
        };
        let at = Keypair::random();
        // A stranger sends the lock address a little first: it still locks.
        let early = transaction(&buyer, &[], &[transfer(&buyer.pubkey(), &at.pubkey(), sol.rent(0).unwrap())], sol.blockhash().unwrap()).unwrap();
        wait(&sol.send(&early).unwrap());
        let deposit = sol.rent(LOCK_LEN).unwrap();
        let locking = transaction(&buyer, &[&at], &lock(&program, &buyer.pubkey(), &at.pubkey(), &terms, deposit), sol.blockhash().unwrap()).unwrap();
        wait(&sol.send(&locking).unwrap());
        println!("locked: {}", signature_of(&locking));

        // Finalized before the seller trusts it.
        let mut found = None;
        for _ in 0..60 {
            std::thread::sleep(Duration::from_secs(1));
            if let Ok(Some(lock)) = sol.lock(&at.pubkey()) {
                found = Some(lock);
                break;
            }
        }
        let found = found.expect("the lock, finalized");
        assert_eq!((found.seller, found.buyer, found.amount, found.hash), (terms.seller, terms.buyer, terms.amount, terms.hash));

        let wrong = transaction(&buyer, &[], &[claim(&program, &at.pubkey(), &terms, &[1; 32])], sol.blockhash().unwrap()).unwrap();
        assert!(sol.send(&wrong).is_err(), "a wrong secret is refused before it lands");

        let before = sol.balance(&seller.pubkey()).unwrap();
        let claiming = transaction(&buyer, &[], &[claim(&program, &at.pubkey(), &terms, &secret)], sol.blockhash().unwrap()).unwrap();
        wait(&sol.send(&claiming).unwrap());
        println!("claimed: {}", signature_of(&claiming));
        std::thread::sleep(Duration::from_secs(11));
        assert_eq!(sol.balance(&seller.pubkey()).unwrap(), before + terms.amount);

        let mut read = None;
        for _ in 0..30 {
            if let Ok(Some(secret)) = sol.secret(&at.pubkey(), &terms.hash) {
                read = Some(secret);
                break;
            }
            std::thread::sleep(Duration::from_secs(2));
        }
        assert_eq!(read, Some(secret), "the buyer can read the secret back from Solana");
    }

    /// One small lock claimed and one refunded on the main network, paid
    /// by the deploy key: `cargo test -p orien-swap solana_mainnet -- --ignored`.
    #[test]
    #[ignore]
    fn solana_mainnet() {
        let key = std::fs::read_to_string(std::env::var("HOME").unwrap() + "/.config/orien/solana-mainnet-deployer.json").unwrap();
        let bytes: Vec<u8> = serde_json::from_str(&key).unwrap();
        let buyer = Keypair::from_secret(bytes[..32].try_into().unwrap());
        assert_eq!(buyer.pubkey().as_slice(), &bytes[32..64]);
        let sol = Solana::mainnet();
        let program = sol.lock_program.unwrap();
        let wait = |signature: &str| {
            for _ in 0..90 {
                std::thread::sleep(Duration::from_secs(1));
                if sol.landed(signature).unwrap_or(None) >= Some(Landed::Confirmed) {
                    return;
                }
            }
            panic!("{signature} did not land");
        };
        let deposit = sol.rent(LOCK_LEN).unwrap();
        let amount = 1_000_000; // 0.001 SOL, above the 0.00089 a new address needs
        let make = |seller: &Keypair, secret: &[u8; 32], seconds: i64| -> (Keypair, Lock) {
            let terms = Lock {
                hash: orien_ledger::types::lock_hash(secret),
                seller: seller.pubkey(),
                buyer: buyer.pubkey(),
                deadline: sol.time().unwrap() + seconds,
                amount,
                lamports: 0,
            };
            let at = Keypair::random();
            let tx = transaction(&buyer, &[&at], &lock(&program, &buyer.pubkey(), &at.pubkey(), &terms, deposit), sol.blockhash().unwrap()).unwrap();
            wait(&sol.send(&tx).unwrap());
            println!("locked: {}", signature_of(&tx));
            (at, terms)
        };
        let finalized = |at: &Keypair| {
            for _ in 0..60 {
                std::thread::sleep(Duration::from_secs(1));
                if let Ok(Some(found)) = sol.lock(&at.pubkey()) {
                    return found;
                }
            }
            panic!("the lock never finalized");
        };

        // Claimed with the secret: the seller is paid, the deposit comes back.
        let (seller, secret) = (Keypair::random(), [7u8; 32]);
        let (at, terms) = make(&seller, &secret, 600);
        let found = finalized(&at);
        assert_eq!((found.seller, found.buyer, found.amount, found.hash), (terms.seller, terms.buyer, terms.amount, terms.hash));
        let wrong = transaction(&buyer, &[], &[claim(&program, &at.pubkey(), &terms, &[1; 32])], sol.blockhash().unwrap()).unwrap();
        assert!(sol.send(&wrong).is_err(), "a wrong secret is refused");
        let early = transaction(&buyer, &[], &[refund(&program, &at.pubkey(), &terms)], sol.blockhash().unwrap()).unwrap();
        assert!(sol.send(&early).is_err(), "no refund before the deadline");
        let claiming = transaction(&buyer, &[], &[claim(&program, &at.pubkey(), &terms, &secret)], sol.blockhash().unwrap()).unwrap();
        wait(&sol.send(&claiming).unwrap());
        println!("claimed: {}", signature_of(&claiming));
        let mut read = None;
        for _ in 0..45 {
            std::thread::sleep(Duration::from_secs(2));
            if let Ok(Some(found)) = sol.secret(&at.pubkey(), &terms.hash) {
                read = Some(found);
                break;
            }
        }
        assert_eq!(read, Some(secret), "the secret can be read back");
        assert_eq!(sol.balance(&seller.pubkey()).unwrap(), amount, "the seller has exactly the amount");
        // The seller's 0.001 SOL back to the deploy key, leaving nothing.
        let home = transaction(&seller, &[], &[transfer(&seller.pubkey(), &buyer.pubkey(), amount - 5_000)], sol.blockhash().unwrap()).unwrap();
        wait(&sol.send(&home).unwrap());
        println!("seller's SOL home: {}", signature_of(&home));

        // Not claimed: refunded in full once the deadline passes.
        let (at, terms) = make(&Keypair::random(), &[8u8; 32], 30);
        finalized(&at);
        while sol.time().unwrap() < terms.deadline {
            std::thread::sleep(Duration::from_secs(3));
        }
        let back = transaction(&buyer, &[], &[refund(&program, &at.pubkey(), &terms)], sol.blockhash().unwrap()).unwrap();
        wait(&sol.send(&back).unwrap());
        println!("refunded: {}", signature_of(&back));
    }
}
