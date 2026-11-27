//! Exact-input swaps in the Raydium CPMM pool for Circle USDC on Solana devnet.
use super::*;
use base64::{Engine, engine::general_purpose::STANDARD};
use bitcoin::hashes::{Hash, sha256};

pub const USDC: &str = "4zMMC9srt5Ri5X14GAgXhaHii3GnPAEERYPJgZJDncDU";
pub const POOL: &str = "FeRts7d5DfXKXq1hGMkeiGEHayDdjsmSyJ41rHVcKo8t";
pub(super) const WRAPPED_SOL: &str = "So11111111111111111111111111111111111111112";
pub(super) const TOKEN: &str = "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA";
const ASSOCIATED: &str = "ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL";
pub(super) const PROGRAM: &str = "DRaycpLY18LhpbydsBWbVJtxpNv9oXPgjRSfpF2bWpYb";
const AUTHORITY: &str = "CXniRufdq5xL8t8jZAPxsPZDpuudwuJSPWnbcD5Y5Nxq";
const CONFIG: &str = "5Gt9qrPJ6FVe9VHtwF2W2JrFR6p9jmx4DxBkgfPdaApk";
const OBSERVATION: &str = "349AquiFdZDhnMNUNHepximG5JDsyskb5TKkifmMajYe";
const SOL_VAULT: &str = "4qkXenqkyjWo5MozQvQSBWMa7xoSQtojcy5Z7FkJfKzm";
const USDC_VAULT: &str = "yuy43wAJF4LKQqS5G28ckHAXbeNYBgaaBqDdhrZgd3i";

fn digest(bytes: &[u8]) -> Pubkey {
    sha256::Hash::hash(bytes).to_byte_array()
}

fn associated(owner: Pubkey, mint: Pubkey, token: Pubkey, program: Pubkey) -> Result<Pubkey> {
    for bump in (0..=255_u8).rev() {
        let pda = digest(
            &[
                &owner[..],
                &token,
                &mint,
                &[bump],
                &program,
                b"ProgramDerivedAddress",
            ]
            .concat(),
        );
        if ed25519_dalek::VerifyingKey::from_bytes(&pda).is_err() {
            return Ok(pda);
        }
    }
    bail!("Could not derive the USDC token account.")
}

pub struct Swap {
    pub payer: Pubkey,
    pub blockhash: Pubkey,
    pub last_valid_height: u64,
    pub output: u64,
    pub minimum: u64,
    pub fee: u64,
    pub account_deposit: u64,
    temporary: Keypair,
    instructions: Vec<Instruction>,
}

impl Swap {
    pub fn signed(&self, signer: &Keypair) -> Result<Vec<u8>> {
        anyhow::ensure!(
            signer.pubkey() == self.payer,
            "The active Solana wallet changed."
        );
        transaction(
            signer,
            &[&self.temporary],
            &self.instructions,
            self.blockhash,
        )
    }
}

fn source_account(network: &Solana, payer: Pubkey, amount: u64) -> Result<Pubkey> {
    let response = network.answers(|rpc| rpc.call("getTokenAccountsByOwner",
        json!([text(&payer), {"mint":USDC}, {"commitment":"confirmed","encoding":"jsonParsed"}]), None))?;
    let accounts = response[0]["value"]
        .as_array()
        .context("Could not read this wallet’s USDC accounts.")?;
    for account in accounts {
        let info = &account["account"]["data"]["parsed"]["info"];
        let balance = info["tokenAmount"]["amount"]
            .as_str()
            .and_then(|s| s.parse::<u64>().ok());
        if info["state"] == "initialized" && balance.is_some_and(|b| b >= amount) {
            return address(
                account["pubkey"]
                    .as_str()
                    .context("Missing USDC account address.")?,
            );
        }
    }
    bail!("You need that much spendable devnet USDC in one token account.")
}

fn simulate(
    network: &Solana,
    payer: Pubkey,
    instructions: &[Instruction],
    blockhash: Pubkey,
    input: u64,
) -> Result<u64> {
    let (message, signers) = legacy_message(&payer, instructions, blockhash);
    let mut wire = Vec::new();
    compact(signers.len(), &mut wire);
    wire.resize(wire.len() + signers.len() * 64, 0);
    wire.extend(message);
    let response = network.answers(|rpc| {
        rpc.call(
            "simulateTransaction",
            json!([STANDARD.encode(&wire),
        {"sigVerify":false,"encoding":"base64","commitment":"confirmed"}]),
            None,
        )
    })?;
    let result = &response[0]["value"];
    anyhow::ensure!(
        result["err"].is_null(),
        "The devnet pool could not execute this amount. Try a different amount or refresh the quote. ({})",
        result["err"]
    );
    let pool = address(POOL)?;
    let event = digest(b"event:SwapEvent");
    for log in result["logs"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
    {
        let Some(encoded) = log.strip_prefix("Program data: ") else {
            continue;
        };
        let Ok(data) = STANDARD.decode(encoded) else {
            continue;
        };
        if data.len() >= 72
            && data[..8] == event[..8]
            && data[8..40] == pool
            && data[56..64] == input.to_le_bytes()
        {
            return Ok(u64::from_le_bytes(data[64..72].try_into()?));
        }
    }
    bail!("The devnet pool did not return a verifiable quote.")
}

pub fn quote(network: &Solana, payer: Pubkey, sol_in: bool, amount: u64) -> Result<Swap> {
    anyhow::ensure!(amount > 0, "Enter an amount above zero.");
    let token = address(TOKEN)?;
    let ata_program = address(ASSOCIATED)?;
    let usd = address(USDC)?;
    let native = address(WRAPPED_SOL)?;
    let temporary = Keypair::random();
    let temp = temporary.pubkey();
    let rent = network.rent(165)?;
    let r = |key| (key, false, false);
    let w = |key| (key, false, true);
    let signer = (payer, true, true);
    let mut instructions = Vec::new();
    let mut account_deposit = 0;
    let usd_account = if sol_in {
        let ata = associated(payer, usd, token, ata_program)?;
        if network.account(&ata)?.is_none() {
            account_deposit = rent;
        }
        instructions.push(Instruction {
            program: ata_program,
            accounts: vec![
                signer,
                w(ata),
                r(payer),
                r(usd),
                r(SYSTEM_PROGRAM),
                r(token),
            ],
            data: vec![1],
        });
        ata
    } else {
        source_account(network, payer, amount)?
    };
    let funding = rent
        .checked_add(if sol_in { amount } else { 0 })
        .context("Amount too large.")?;
    instructions.push(create_account(&payer, &temp, funding, 165, &token));
    instructions.push(Instruction {
        program: token,
        accounts: vec![w(temp), r(native)],
        data: [&[18][..], &payer].concat(),
    });
    let sol_side = (temp, address(SOL_VAULT)?, native);
    let usd_side = (usd_account, address(USDC_VAULT)?, usd);
    let (from, to) = if sol_in {
        (sol_side, usd_side)
    } else {
        (usd_side, sol_side)
    };
    let swap_ix = instructions.len();
    instructions.push(Instruction {
        program: address(PROGRAM)?,
        accounts: vec![
            signer,
            r(address(AUTHORITY)?),
            r(address(CONFIG)?),
            w(address(POOL)?),
            w(from.0),
            w(to.0),
            w(from.1),
            w(to.1),
            r(token),
            r(token),
            r(from.2),
            r(to.2),
            w(address(OBSERVATION)?),
        ],
        data: [
            &digest(b"global:swap_base_input")[..8],
            &amount.to_le_bytes(),
            &1_u64.to_le_bytes(),
        ]
        .concat(),
    });
    instructions.push(Instruction {
        program: token,
        accounts: vec![w(temp), w(payer), signer],
        data: vec![9],
    });
    let (blockhash, last_valid_height) = network.blockhash_lifetime()?;
    let fee = network.fee_for_message(&payer, &instructions, blockhash)?;
    let needed = funding
        .checked_add(account_deposit)
        .and_then(|v| v.checked_add(fee))
        .context("Amount too large.")?;
    anyhow::ensure!(
        network.balance(&payer)? >= needed,
        "Insufficient devnet SOL for the swap, account deposits and network fee."
    );
    let output = simulate(network, payer, &instructions, blockhash, amount)?;
    anyhow::ensure!(output > 0, "This amount is too small for the pool.");
    let minimum = output - output / 200;
    instructions[swap_ix].data[16..24].copy_from_slice(&minimum.to_le_bytes());
    Ok(Swap {
        payer,
        blockhash,
        last_valid_height,
        output,
        minimum,
        fee,
        account_deposit,
        temporary,
        instructions,
    })
}
