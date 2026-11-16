//! A real run of orien-lock on Solana's test network: a lock claimed with
//! its secret, and a lock refunded after its deadline, with the refusals in
//! between. `cargo run --example devnet` (it pays with the test-network key
//! in ~/.config/orien/solana-devnet.json, worthless test SOL).

use base64::Engine;
use serde_json::{Value, json};
use solana_address::Address;
use solana_hash::Hash;
use solana_instruction::{AccountMeta, Instruction};
use solana_keypair::Keypair;
use solana_signer::Signer;
use solana_system_interface::instruction::{create_account, transfer};
use solana_transaction::Transaction;
use std::str::FromStr;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const RPC: &str = "https://api.devnet.solana.com";
const PROGRAM: &str = "99zcMPAgZwXxoLDwor424zro9BGMATTsMk8djqov2qKf";
const LEN: u64 = 113;
const SOL: u64 = 1_000_000_000;

fn rpc(method: &str, params: Value) -> Result<Value, String> {
    let answer: Value = ureq::post(RPC)
        .send_json(json!({ "jsonrpc": "2.0", "id": 1, "method": method, "params": params }))
        .map_err(|e| e.to_string())?
        .body_mut()
        .read_json()
        .map_err(|e| e.to_string())?;
    match answer.get("error") {
        Some(error) => Err(error.to_string()),
        None => Ok(answer["result"].clone()),
    }
}

fn blockhash() -> Hash {
    let found = rpc("getLatestBlockhash", json!([{ "commitment": "confirmed" }])).unwrap();
    Hash::from_str(found["value"]["blockhash"].as_str().unwrap()).unwrap()
}

fn balance(who: &Address) -> u64 {
    rpc("getBalance", json!([who.to_string(), { "commitment": "confirmed" }])).unwrap()["value"].as_u64().unwrap()
}

/// Sends and waits until confirmed; an error if the network refuses it.
fn send(ixs: &[Instruction], payer: &Keypair, signers: &[&Keypair]) -> Result<String, String> {
    let tx = Transaction::new_signed_with_payer(ixs, Some(&payer.pubkey()), signers, blockhash());
    let wire = base64::engine::general_purpose::STANDARD.encode(bincode::serialize(&tx).unwrap());
    let signature = rpc("sendTransaction", json!([wire, { "encoding": "base64", "preflightCommitment": "confirmed" }]))?;
    let signature = signature.as_str().unwrap().to_string();
    for _ in 0..60 {
        std::thread::sleep(Duration::from_millis(800));
        let status = rpc("getSignatureStatuses", json!([[signature]]))?;
        let status = &status["value"][0];
        if !status.is_null() {
            if !status["err"].is_null() {
                return Err(status["err"].to_string());
            }
            if matches!(status["confirmationStatus"].as_str(), Some("confirmed" | "finalized")) {
                return Ok(signature);
            }
        }
    }
    Err("not confirmed in time".into())
}

fn now() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs() as i64
}

struct Run {
    program: Address,
    payer: Keypair,
    seller: Keypair,
    secret: [u8; 32],
}

impl Run {
    fn lock(&self, amount: u64, deadline: i64) -> Keypair {
        let lock = Keypair::new();
        let deposit = rpc("getMinimumBalanceForRentExemption", json!([LEN])).unwrap().as_u64().unwrap();
        let hash = solana_sha256_hasher::hash(&self.secret).to_bytes();
        let mut data = vec![0u8];
        data.extend_from_slice(&hash);
        data.extend_from_slice(self.seller.pubkey().as_ref());
        data.extend_from_slice(self.payer.pubkey().as_ref());
        data.extend_from_slice(&deadline.to_le_bytes());
        data.extend_from_slice(&amount.to_le_bytes());
        let create = create_account(&self.payer.pubkey(), &lock.pubkey(), amount + deposit, LEN, &self.program);
        let ix = Instruction::new_with_bytes(self.program, &data, vec![AccountMeta::new(lock.pubkey(), true)]);
        let signature = send(&[create, ix], &self.payer, &[&self.payer, &lock]).expect("lock");
        println!("  locked {} SOL at {} ({signature})", amount as f64 / SOL as f64, lock.pubkey());
        lock
    }

    fn claim(&self, lock: &Keypair, secret: [u8; 32]) -> Result<String, String> {
        let mut data = vec![1u8];
        data.extend_from_slice(&secret);
        let accounts = vec![
            AccountMeta::new(lock.pubkey(), false),
            AccountMeta::new(self.seller.pubkey(), false),
            AccountMeta::new(self.payer.pubkey(), false),
        ];
        send(&[Instruction::new_with_bytes(self.program, &data, accounts)], &self.payer, &[&self.payer])
    }

    fn refund(&self, lock: &Keypair) -> Result<String, String> {
        let accounts = vec![AccountMeta::new(lock.pubkey(), false), AccountMeta::new(self.payer.pubkey(), false)];
        send(&[Instruction::new_with_bytes(self.program, &[2], accounts)], &self.payer, &[&self.payer])
    }
}

fn main() {
    let key = std::fs::read_to_string(std::env::var("HOME").unwrap() + "/.config/orien/solana-devnet.json").expect("the test-network key");
    let bytes: Vec<u8> = serde_json::from_str(&key).unwrap();
    let payer = Keypair::try_from(bytes.as_slice()).unwrap();
    let mut secret = [0u8; 32];
    for (i, b) in secret.iter_mut().enumerate() {
        *b = (now() as u8).wrapping_mul(31).wrapping_add(i as u8);
    }
    let run = Run { program: Address::from_str(PROGRAM).unwrap(), payer, seller: Keypair::new(), secret };
    println!("program {PROGRAM}, buyer {}, seller {}", run.payer.pubkey(), run.seller.pubkey());

    // The seller's address exists, as a wallet's would.
    send(&[transfer(&run.payer.pubkey(), &run.seller.pubkey(), SOL / 100)], &run.payer, &[&run.payer]).expect("fund seller");

    println!("1. A lock, claimed with the secret");
    let lock = run.lock(SOL / 20, now() + 600);
    let (seller_before, lock_held) = (balance(&run.seller.pubkey()), balance(&lock.pubkey()));
    println!("  wrong secret: {}", if run.claim(&lock, [9; 32]).is_err() { "refused, as it should be" } else { "ACCEPTED (wrong!)" });
    println!("  refund before the deadline: {}", if run.refund(&lock).is_err() { "refused, as it should be" } else { "ACCEPTED (wrong!)" });
    let signature = run.claim(&lock, run.secret).expect("claim");
    let paid = balance(&run.seller.pubkey()) - seller_before;
    println!("  claimed ({signature}): the seller got {} SOL, the lock holds {} lamports (it held {lock_held})", paid as f64 / SOL as f64, balance(&lock.pubkey()));
    assert_eq!(paid, SOL / 20);

    println!("2. A lock refunded after its deadline");
    let deadline = now() + 25;
    let lock = run.lock(SOL / 50, deadline);
    while now() < deadline + 20 {
        std::thread::sleep(Duration::from_secs(5));
    }
    println!("  claim after the deadline: {}", if run.claim(&lock, run.secret).is_err() { "refused, as it should be" } else { "ACCEPTED (wrong!)" });
    let signature = run.refund(&lock).expect("refund");
    println!("  refunded ({signature}): the lock holds {} lamports", balance(&lock.pubkey()));
    println!("all done");
}
