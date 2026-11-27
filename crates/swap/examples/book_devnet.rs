//! A live run of orien-book on Solana devnet, through the same client code
//! Orien Wallet and the Market use: an order filled in parts, a part that
//! lapses and goes back, the last part closing the order, and a cancel.
//!
//!   cargo run -p orien-swap --example book_devnet -- ~/.config/orien/solana-devnet.json

use anyhow::{Result, anyhow, bail};
use orien_swap::book::{self, BookTerms, MIN_PART, Secrets};
use orien_swap::solana::{self, Keypair, Landed, Solana};
use std::{thread::sleep, time::Duration};

fn send(sol: &Solana, payer: &Keypair, others: &[&Keypair], ixs: &[solana::Instruction]) -> Result<()> {
    let tx = solana::transaction(payer, others, ixs, sol.blockhash()?)?;
    let signature = sol.send(&tx)?;
    for _ in 0..90 {
        if let Some(Landed::Finalized) = sol.landed(&signature)? {
            return Ok(());
        }
        sleep(Duration::from_secs(1));
    }
    bail!("{signature} did not finalize")
}

/// Whether `at` is gone (closed), waiting out the services' short caching.
fn gone(sol: &Solana, at: &solana::Pubkey) -> Result<bool> {
    for _ in 0..30 {
        if !sol.account(at)?.is_some_and(|a| a.lamports > 0) {
            return Ok(true);
        }
        sleep(Duration::from_secs(1));
    }
    Ok(false)
}

fn main() -> Result<()> {
    let path = std::env::args().nth(1).ok_or_else(|| anyhow!("give the devnet key file"))?;
    let bytes: Vec<u8> = serde_json::from_str(&std::fs::read_to_string(path)?)?;
    let payer = Keypair::from_secret(bytes[..32].try_into()?);
    let sol = Solana::devnet();
    let program = solana::address(book::DEVNET_BOOK_PROGRAM)?;
    let me = payer.pubkey();
    let (seller_a, seller_b) = (Keypair::random(), Keypair::random());
    let secrets: Vec<[u8; 32]> = (0..64u8).map(|i| { let mut s = [0x5a; 32]; s[0] = i; s }).collect();
    let tree = Secrets::from_secrets(&secrets).unwrap();
    let now = sol.time()?;

    // 1,000 ORN for 0.05 SOL.
    let order = Keypair::random();
    let terms = BookTerms {
        root: tree.root(),
        buyer: me,
        authority: me,
        filler: me,
        buyer_orien: [0xab; 32],
        orn_total: 1_000_000_000_000,
        lamports_total: 50_000_000,
        expiry: now + 3600,
        max_fill_window: 600,
        min_part: MIN_PART,
    };
    let deposit = sol.rent(book::BOOK_LEN)?;
    let before = sol.balance(&me)?;
    send(&sol, &payer, &[&order], &book::open(&program, &me, &order.pubkey(), &terms, deposit))?;
    let o = order.pubkey();
    println!("opened {}", solana::text(&o));

    // Part one: 400 ORN to seller A, claimed with secret 0.
    let first = 400_000_000_000;
    let state = sol.book(&program, &o)?.ok_or_else(|| anyhow!("no order"))?;
    let share = state.share(first).ok_or_else(|| anyhow!("no share"))?;
    send(&sol, &payer, &[], &[book::fill(&program, &o, &me, 0, &tree.hash(0), &tree.proof(0), &seller_a.pubkey(), first, 600)])?;
    send(&sol, &payer, &[], &[book::claim(&program, &o, 0, &seller_a.pubkey(), &me, &secrets[0])])?;
    let paid_a = sol.balance(&seller_a.pubkey())?;
    println!("part 1: seller A got {paid_a} lamports (share {share})");
    if paid_a != share {
        bail!("seller A was paid {paid_a}, not {share}");
    }

    // Part two: seller B, left to lapse, then released.
    send(&sol, &payer, &[], &[book::fill(&program, &o, &me, 1, &tree.hash(1), &tree.proof(1), &seller_b.pubkey(), 200_000_000_000, 5)])?;
    sleep(Duration::from_secs(20));
    send(&sol, &payer, &[], &[book::release(&program, &o, 0)])?;
    let state = sol.book(&program, &o)?.ok_or_else(|| anyhow!("no order"))?;
    println!("part 2 lapsed and went back: {} ORN, {} lamports left", state.orn_left, state.lamports_left);
    if state.orn_left != 1_000_000_000_000 - first || !state.parts.is_empty() {
        bail!("the lapsed part did not go back");
    }

    // A used secret is refused.
    if send(&sol, &payer, &[], &[book::fill(&program, &o, &me, 1, &tree.hash(1), &tree.proof(1), &seller_b.pubkey(), 200_000_000_000, 600)]).is_ok() {
        bail!("a used secret was accepted");
    }
    println!("a used secret was refused");

    // The rest to seller B with secret 2: the order closes.
    let rest = state.orn_left;
    send(&sol, &payer, &[], &[book::fill(&program, &o, &me, 2, &tree.hash(2), &tree.proof(2), &seller_b.pubkey(), rest, 600)])?;
    send(&sol, &payer, &[], &[book::claim(&program, &o, 0, &seller_b.pubkey(), &me, &secrets[2])])?;
    let paid_b = sol.balance(&seller_b.pubkey())?;
    println!("part 3: seller B got {paid_b} lamports; together {}", paid_a + paid_b);
    if paid_a + paid_b != 50_000_000 {
        bail!("the parts don't add up");
    }
    if !gone(&sol, &o)? {
        bail!("the order did not close");
    }
    println!("closed; the buyer spent {} lamports in all (50,000,000 to the sellers, the rest fees)", before - sol.balance(&me)?);

    // A second order, cancelled.
    let order = Keypair::random();
    let terms = BookTerms { root: tree.root(), expiry: sol.time()? + 3600, ..terms };
    send(&sol, &payer, &[&order], &book::open(&program, &me, &order.pubkey(), &terms, deposit))?;
    send(&sol, &payer, &[], &[book::cancel(&program, &order.pubkey(), &me, &me)])?;
    if !gone(&sol, &order.pubkey())? {
        bail!("the cancelled order did not close");
    }
    println!("a second order opened and cancelled");
    println!("all good");
    Ok(())
}
