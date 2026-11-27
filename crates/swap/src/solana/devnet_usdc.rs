//! Transfers of Circle USDC, on whichever Solana network is given.
use super::*;
use bitcoin::hashes::{Hash, sha256};

pub const USDC: &str = "4zMMC9srt5Ri5X14GAgXhaHii3GnPAEERYPJgZJDncDU";
pub(super) const TOKEN: &str = "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA";
pub(super) const ASSOCIATED: &str = "ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL";

pub(super) fn associated(
    owner: Pubkey,
    mint: Pubkey,
    token: Pubkey,
    program: Pubkey,
) -> Result<Pubkey> {
    for bump in (0..=255_u8).rev() {
        let pda = sha256::Hash::hash(
            &[
                &owner[..],
                &token,
                &mint,
                &[bump],
                &program,
                b"ProgramDerivedAddress",
            ]
            .concat(),
        )
        .to_byte_array();
        if ed25519_dalek::VerifyingKey::from_bytes(&pda).is_err() {
            return Ok(pda);
        }
    }
    bail!("Could not derive the USDC token account.")
}

pub(super) fn create_associated(
    payer: Pubkey,
    owner: Pubkey,
    ata: Pubkey,
    mint: Pubkey,
    token: Pubkey,
    program: Pubkey,
) -> Instruction {
    Instruction {
        program,
        accounts: vec![
            (payer, true, true),
            (ata, false, true),
            (owner, false, false),
            (mint, false, false),
            (SYSTEM_PROGRAM, false, false),
            (token, false, false),
        ],
        data: vec![1],
    }
}

pub(super) fn accounts(network: &Solana, owner: Pubkey) -> Result<Vec<(Pubkey, u64, bool)>> {
    let owner = text(&owner);
    let response = network.answers(|rpc| {
        rpc.call(
            "getTokenAccountsByOwner",
            json!([owner, {"mint":network.usdc}, {"commitment":"confirmed","encoding":"jsonParsed"}]),
            None,
        )
    })?;
    let rows = response[0]["value"]
        .as_array()
        .context("Could not read the wallet’s USDC accounts.")?;
    rows.iter()
        .map(|row| {
            let info = &row["account"]["data"]["parsed"]["info"];
            anyhow::ensure!(
                row["account"]["owner"] == TOKEN && info["owner"] == owner && info["mint"] == network.usdc,
                "Unexpected USDC account."
            );
            let key = address(
                row["pubkey"]
                    .as_str()
                    .context("Missing token account address.")?,
            )?;
            let balance = info["tokenAmount"]["amount"]
                .as_str()
                .context("Missing USDC balance.")?
                .parse()?;
            Ok((key, balance, info["state"] == "initialized"))
        })
        .collect()
}

pub struct Transfer {
    pub instructions: Vec<Instruction>,
    pub amount: u64,
    pub balance_after: u64,
    pub deposit: u64,
}

pub fn transfer(
    network: &Solana,
    payer: Pubkey,
    recipient: Pubkey,
    requested: Option<u64>,
) -> Result<Transfer> {
    let mut sources = accounts(network, payer)?;
    let mut balance = 0_u64;
    let mut spendable = 0_u64;
    for &(_, units, available) in &sources {
        balance = balance
            .checked_add(units)
            .context("Invalid USDC balance.")?;
        if available {
            spendable += units;
        }
    }
    let amount = requested.unwrap_or(spendable);
    anyhow::ensure!(
        amount > 0 && amount <= spendable,
        "You don’t have enough spendable USDC."
    );
    let mint = address(network.usdc)?;
    let token = address(TOKEN)?;
    let program = address(ASSOCIATED)?;
    let destination = associated(recipient, mint, token, program)?;
    let mut instructions = Vec::new();
    let mut deposit = 0;
    match network.account(&destination)? {
        Some(account) if account.owner == token => {
            anyhow::ensure!(
                account.data.get(..32) == Some(&mint[..])
                    && account.data.get(32..64) == Some(&recipient[..])
                    && account.data.get(108) == Some(&1),
                "The recipient’s USDC account cannot receive this transfer."
            );
        }
        // A prefunded system account can still be initialized as the ATA.
        existing
            if existing
                .as_ref()
                .is_none_or(|a| a.owner == SYSTEM_PROGRAM && a.data.is_empty()) =>
        {
            deposit = network
                .rent(165)?
                .saturating_sub(existing.map_or(0, |a| a.lamports));
            instructions.push(create_associated(
                payer,
                recipient,
                destination,
                mint,
                token,
                program,
            ));
        }
        _ => bail!("The recipient’s USDC account cannot receive this transfer."),
    }
    sources.sort_by_key(|(_, units, _)| std::cmp::Reverse(*units));
    let mut remaining = amount;
    for (source, units, available) in sources {
        if !available || units == 0 {
            continue;
        }
        let part = remaining.min(units);
        // SPL Token TransferChecked, using Circle USDC’s six decimal places.
        instructions.push(Instruction {
            program: token,
            accounts: vec![
                (source, false, true),
                (mint, false, false),
                (destination, false, true),
                (payer, true, false),
            ],
            data: [&[12][..], &part.to_le_bytes(), &[6]].concat(),
        });
        remaining -= part;
        if remaining == 0 {
            break;
        }
        anyhow::ensure!(
            instructions.len() < 24,
            "Too many token accounts for one transaction. Try a smaller amount."
        );
    }
    Ok(Transfer {
        instructions,
        amount,
        balance_after: balance - amount,
        deposit,
    })
}
