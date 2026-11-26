//! Live devnet test for orien-order:
//!   1. Open order -> wrong secret / double fill / wrong seller refused -> fill -> claim.
//!   2. Open order -> cancel (refunds buyer).
//!   3. Open order -> fill -> wait for fill window lapse -> reopen -> fill by seller 2 -> claim.
//!   4. Open order -> wait for expiry -> expire (refunds buyer).
//!
//! Run: `ORIEN_ORDER_PROGRAM=<devnet-id> cargo run --example devnet`
//! Pays with worthless test SOL from ~/.config/orien/solana-devnet.json.

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
const DEFAULT_PROGRAM: &str = "7eToRgZD6HhgdT1BVawNtndeVs4eovXJiKrh2zB51pwp";
const LEN: u64 = 217;
const SOL: u64 = 1_000_000_000;
const ORN_AMOUNT: u64 = 50_000_000_000; // 50 ORN
const BUYER_ORIEN: [u8; 32] = [0xab; 32];

fn rpc(method: &str, params: Value) -> Result<Value, String> {
    for attempt in 0..8 {
        let resp = ureq::post(RPC).send_json(json!({ "jsonrpc": "2.0", "id": 1, "method": method, "params": params }));
        match resp {
            Ok(mut r) => {
                let answer: Value = r.body_mut().read_json().map_err(|e| e.to_string())?;
                return match answer.get("error") {
                    Some(error) => Err(error.to_string()),
                    None => Ok(answer["result"].clone()),
                };
            }
            Err(e) if e.to_string().contains("429") && attempt < 7 => {
                std::thread::sleep(Duration::from_secs(2 + attempt));
            }
            Err(e) => return Err(e.to_string()),
        }
    }
    Err("rate limited".into())
}

fn blockhash() -> Hash {
    let found = rpc("getLatestBlockhash", json!([{ "commitment": "confirmed" }])).unwrap();
    Hash::from_str(found["value"]["blockhash"].as_str().unwrap()).unwrap()
}

fn balance(who: &Address) -> u64 {
    rpc("getBalance", json!([who.to_string(), { "commitment": "confirmed" }])).unwrap()["value"].as_u64().unwrap()
}

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
    seller2: Keypair,
    secret: [u8; 32],
    deposit: u64,
}

impl Run {
    fn open(&self, amount: u64, expiry: i64, min_bond: u64, max_fill: i64) -> Keypair {
        let order = Keypair::new();
        let hash = solana_sha256_hasher::hash(&self.secret).to_bytes();
        let mut data = vec![0u8];
        data.extend_from_slice(&hash);
        data.extend_from_slice(self.payer.pubkey().as_ref());
        data.extend_from_slice(self.payer.pubkey().as_ref());
        data.extend_from_slice(&BUYER_ORIEN);
        data.extend_from_slice(&ORN_AMOUNT.to_le_bytes());
        data.extend_from_slice(&amount.to_le_bytes());
        data.extend_from_slice(&expiry.to_le_bytes());
        data.extend_from_slice(&min_bond.to_le_bytes());
        data.extend_from_slice(&max_fill.to_le_bytes());
        let create = create_account(&self.payer.pubkey(), &order.pubkey(), amount + self.deposit, LEN, &self.program);
        let ix = Instruction::new_with_bytes(self.program, &data, vec![AccountMeta::new(order.pubkey(), true)]);
        let sig = send(&[create, ix], &self.payer, &[&self.payer, &order]).expect("open_order");
        println!("  opened order {} ({sig})", order.pubkey());
        order
    }

    fn fill(&self, order: &Address, seller: &Address, window: i64, bond: u64) -> Result<String, String> {
        let mut data = vec![2u8];
        data.extend_from_slice(seller.as_ref());
        data.extend_from_slice(&window.to_le_bytes());
        let mut ixs = Vec::new();
        if bond > 0 {
            ixs.push(transfer(&self.payer.pubkey(), order, bond));
        }
        ixs.push(Instruction::new_with_bytes(self.program, &data, vec![AccountMeta::new(*order, false)]));
        send(&ixs, &self.payer, &[&self.payer])
    }

    fn claim(&self, order: &Address, seller: &Address, secret: [u8; 32]) -> Result<String, String> {
        let mut data = vec![3u8];
        data.extend_from_slice(&secret);
        let ix = Instruction::new_with_bytes(
            self.program,
            &data,
            vec![
                AccountMeta::new(*order, false),
                AccountMeta::new(*seller, false),
                AccountMeta::new(self.payer.pubkey(), false),
            ],
        );
        send(&[ix], &self.payer, &[&self.payer])
    }

    fn cancel(&self, order: &Address) -> Result<String, String> {
        let ix = Instruction::new_with_bytes(
            self.program,
            &[1],
            vec![
                AccountMeta::new(*order, false),
                AccountMeta::new(self.payer.pubkey(), true),
                AccountMeta::new(self.payer.pubkey(), false),
            ],
        );
        send(&[ix], &self.payer, &[&self.payer])
    }

    fn reopen(&self, order: &Address) -> Result<String, String> {
        let ix = Instruction::new_with_bytes(
            self.program,
            &[4],
            vec![AccountMeta::new(*order, false), AccountMeta::new(self.payer.pubkey(), false)],
        );
        send(&[ix], &self.payer, &[&self.payer])
    }

    fn expire(&self, order: &Address) -> Result<String, String> {
        let ix = Instruction::new_with_bytes(
            self.program,
            &[5],
            vec![AccountMeta::new(*order, false), AccountMeta::new(self.payer.pubkey(), false)],
        );
        send(&[ix], &self.payer, &[&self.payer])
    }
}

fn main() {
    let program_str = std::env::var("ORIEN_ORDER_PROGRAM").unwrap_or_else(|_| DEFAULT_PROGRAM.into());
    let key = std::fs::read_to_string(std::env::var("HOME").unwrap() + "/.config/orien/solana-devnet.json").expect("devnet key");
    let bytes: Vec<u8> = serde_json::from_str(&key).unwrap();
    let payer = Keypair::try_from(bytes.as_slice()).unwrap();
    let deposit = rpc("getMinimumBalanceForRentExemption", json!([LEN])).unwrap().as_u64().unwrap();
    let mut secret = [0u8; 32];
    for (i, b) in secret.iter_mut().enumerate() {
        *b = (now() as u8).wrapping_mul(37).wrapping_add(i as u8);
    }
    let run = Run {
        program: Address::from_str(&program_str).unwrap(),
        payer,
        seller: Keypair::new(),
        seller2: Keypair::new(),
        secret,
        deposit,
    };
    println!("program {program_str}, buyer {}, deposit {deposit} lamports", run.payer.pubkey());

    // Fund seller wallets so they exist on-chain.
    send(
        &[
            transfer(&run.payer.pubkey(), &run.seller.pubkey(), SOL / 200),
            transfer(&run.payer.pubkey(), &run.seller2.pubkey(), SOL / 200),
        ],
        &run.payer,
        &[&run.payer],
    )
    .expect("fund sellers");

    println!("1. Open -> Fill -> Claim (with bond)");
    let bond = SOL / 500;
    let o1 = run.open(SOL / 50, now() + 600, bond, 300);
    run.fill(&o1.pubkey(), &run.seller.pubkey(), 120, bond).expect("fill");
    assert!(run.fill(&o1.pubkey(), &run.seller2.pubkey(), 120, bond).is_err(), "double fill refused");
    assert!(run.claim(&o1.pubkey(), &run.seller.pubkey(), [9; 32]).is_err(), "wrong secret refused");
    assert!(run.claim(&o1.pubkey(), &run.seller2.pubkey(), run.secret).is_err(), "wrong seller refused");
    let before = balance(&run.seller.pubkey());
    let sig = run.claim(&o1.pubkey(), &run.seller.pubkey(), run.secret).expect("claim");
    assert_eq!(balance(&run.seller.pubkey()) - before, SOL / 50 + bond);
    assert_eq!(balance(&o1.pubkey()), 0);
    println!("  claimed OK ({sig})");

    println!("2. Open -> Cancel");
    let o2 = run.open(SOL / 100, now() + 600, 0, 300);
    let sig = run.cancel(&o2.pubkey()).expect("cancel");
    assert_eq!(balance(&o2.pubkey()), 0);
    println!("  cancelled OK ({sig})");

    println!("3. Open -> Fill -> Lapse -> Reopen -> Fill2 -> Claim");
    let o3 = run.open(SOL / 100, now() + 600, bond, 300);
    run.fill(&o3.pubkey(), &run.seller.pubkey(), 15, bond).expect("fill1");
    std::thread::sleep(Duration::from_secs(22));
    let sig = run.reopen(&o3.pubkey()).expect("reopen");
    println!("  reopened OK ({sig})");
    run.fill(&o3.pubkey(), &run.seller2.pubkey(), 120, bond).expect("fill2");
    let before2 = balance(&run.seller2.pubkey());
    let sig = run.claim(&o3.pubkey(), &run.seller2.pubkey(), run.secret).expect("claim2");
    assert_eq!(balance(&run.seller2.pubkey()) - before2, SOL / 100 + bond);
    assert_eq!(balance(&o3.pubkey()), 0);
    println!("  second fill claimed OK ({sig})");

    println!("4. Open -> Expire");
    let o4 = run.open(SOL / 100, now() + 18, 0, 300);
    std::thread::sleep(Duration::from_secs(24));
    let sig = run.expire(&o4.pubkey()).expect("expire");
    assert_eq!(balance(&o4.pubkey()), 0);
    println!("  expired OK ({sig})");

    println!("ALL DEVNET CHECKS PASSED");
}
