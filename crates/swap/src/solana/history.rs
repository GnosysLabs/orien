//! Recent transactions mentioning the owner or one of their accounts for the selected mint.
use super::{FRESH, Pubkey, Rpc, SYSTEM_PROGRAM, Solana, devnet_pool, text};
use anyhow::{Result, anyhow};
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::HashSet;
use std::time::Duration;

const RECENT: usize = 8;
const MAX_TOKEN_ACCOUNTS: usize = 8;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Signature {
    signature: String,
    slot: u64,
    block_time: Option<u64>,
    err: Option<Value>,
    confirmation_status: Option<String>,
}

pub struct HistoryEntry {
    pub signature: String,
    pub time: Option<u64>,
    pub failed: bool,
    pub finalized: bool,
    pub movement: Option<Movement>,
}

pub enum Movement {
    Sol { units: i128, peer: String },
    Token { units: i128 },
    PoolSwap { sol: i128, token: i128 },
}

pub struct History {
    pub entries: Vec<HistoryEntry>,
    pub incomplete: bool,
}

impl Solana {
    /// Read a submitted wallet transaction directly while the address history is being indexed.
    pub fn confirmed_transaction(&self, owner: &Pubkey, signature: &str) -> Result<Option<HistoryEntry>> {
        anyhow::ensure!(
            signature.len() <= 88 && bs58::decode(signature).into_vec()?.len() == 64,
            "Invalid transaction signature"
        );
        let response =
            self.answers(|rpc| rpc.call("getTransaction", transaction_params(signature), None))?;
        let tx = &response[0];
        if !tx["meta"].is_object() {
            return Ok(None);
        }
        let owner = text(owner);
        let payer = &tx["transaction"]["message"]["accountKeys"][0]["pubkey"];
        anyhow::ensure!(
            payer.as_str() == Some(&owner),
            "The transaction belongs to another wallet."
        );
        let failed = !tx["meta"]["err"].is_null();
        let movement = if failed {
            None
        } else {
            transfer(tx, &owner, devnet_pool::USDC, &[])
        };
        Ok(Some(HistoryEntry {
            signature: signature.to_owned(),
            time: tx["blockTime"].as_u64(),
            finalized: false,
            failed,
            movement,
        }))
    }

    pub fn recent_history(&self, owner: &Pubkey, mint: &Pubkey) -> Result<History> {
        let (owner, mint) = (text(owner), text(mint));
        self.answers(|rpc| recent(rpc, &owner, &mint))?
            .into_iter()
            .next()
            .ok_or_else(|| anyhow!("no history response"))
    }
}

fn transaction_params(signature: &str) -> Value {
    json!([signature, {"encoding":"jsonParsed","commitment":"confirmed","maxSupportedTransactionVersion":1}])
}

fn recent(rpc: &Rpc, owner: &str, mint: &str) -> Result<History> {
    let mut addresses = vec![owner.to_owned()];
    let mut incomplete;
    // A token deposit may mention only the receiving token account, never its owner.
    match rpc.call(
        "getTokenAccountsByOwner",
        json!([owner, {"mint": mint}, {"commitment":"confirmed","encoding":"jsonParsed"}]),
        Some(FRESH),
    ) {
        Ok(result) => {
            let accounts = result["value"]
                .as_array()
                .ok_or_else(|| anyhow!("missing token accounts"))?;
            incomplete = accounts.len() > MAX_TOKEN_ACCOUNTS;
            for account in accounts.iter().take(MAX_TOKEN_ACCOUNTS) {
                addresses.push(
                    account["pubkey"]
                        .as_str()
                        .ok_or_else(|| anyhow!("missing token address"))?
                        .to_owned(),
                );
            }
        }
        Err(_) => incomplete = true,
    }
    let mut signatures = Vec::new();
    let mut responded = false;
    for address in &addresses {
        let page = rpc
            .call(
                "getSignaturesForAddress",
                json!([address, {"limit": RECENT, "commitment":"confirmed"}]),
                Some(FRESH),
            )
            .and_then(|value| Ok(serde_json::from_value::<Vec<Signature>>(value)?));
        match page {
            Ok(rows) => {
                responded = true;
                signatures.extend(rows);
            }
            Err(_) => incomplete = true,
        }
    }
    anyhow::ensure!(responded, "Solana activity is unavailable");
    // Preserve the RPC's newest-first ordering within each slot.
    signatures.sort_by_key(|entry| std::cmp::Reverse(entry.slot));
    let mut seen = HashSet::new();
    let mut entries = Vec::new();
    for sig in signatures
        .into_iter()
        .filter(|s| seen.insert(s.signature.clone()))
        .take(RECENT)
    {
        let failed = sig.err.is_some();
        let finalized = sig.confirmation_status.as_deref() == Some("finalized");
        let mut movement = None;
        if !failed {
            let tx = rpc.call(
                "getTransaction",
                transaction_params(&sig.signature),
                Some(if finalized {
                    Duration::from_secs(300)
                } else {
                    FRESH
                }),
            );
            match tx {
                Ok(tx) if tx["meta"].is_object() => {
                    movement = transfer(&tx, owner, mint, &addresses[1..])
                }
                _ => incomplete = true,
            }
        }
        entries.push(HistoryEntry {
            signature: sig.signature,
            time: sig.block_time,
            failed,
            finalized,
            movement,
        });
    }
    Ok(History {
        entries,
        incomplete,
    })
}

fn transfer(tx: &Value, owner: &str, mint: &str, accounts: &[String]) -> Option<Movement> {
    let message = &tx["transaction"]["message"];
    let keys = message["accountKeys"].as_array()?;
    let token_total = |balances: &Value| -> Option<i128> {
        let mut total = 0_i128;
        for balance in balances.as_array()? {
            if balance["mint"].as_str() != Some(mint) {
                continue;
            }
            let owned = match balance["owner"].as_str() {
                Some(address) => address == owner,
                None => {
                    let index = usize::try_from(balance["accountIndex"].as_u64()?).ok()?;
                    let address = keys.get(index)?["pubkey"].as_str()?;
                    accounts.iter().any(|a| a == address)
                }
            };
            if owned {
                total += i128::from(
                    balance["uiTokenAmount"]["amount"]
                        .as_str()?
                        .parse::<u64>()
                        .ok()?,
                );
            }
        }
        Some(total)
    };
    if let (Some(pre), Some(post)) = (
        token_total(&tx["meta"]["preTokenBalances"]),
        token_total(&tx["meta"]["postTokenBalances"]),
    ) && pre != post
    {
        let token = post - pre;
        if mint == devnet_pool::USDC
            && let Some(sol) = swapped_sol(tx, owner, token)
        {
            return Some(Movement::PoolSwap { sol, token });
        }
        return Some(Movement::Token { units: token });
    }
    // Only label a simple System transfer as sent/received. Fees, rent changes
    // and arbitrary program calls should never appear as payments.
    let mut instructions = message["instructions"].as_array()?.iter().filter(|ix| {
        ix["programId"].as_str() != Some("ComputeBudget111111111111111111111111111111")
    });
    let ix = instructions.next()?;
    if instructions.next().is_some()
        || ix["programId"].as_str()? != text(&SYSTEM_PROGRAM)
        || ix["parsed"]["type"].as_str()? != "transfer"
    {
        return None;
    }
    let info = &ix["parsed"]["info"];
    let (source, destination) = (info["source"].as_str()?, info["destination"].as_str()?);
    let amount = i128::from(info["lamports"].as_u64()?);
    if destination == source || amount == 0 {
        return None;
    }
    let (units, peer) = if source == owner {
        (-amount, destination)
    } else if destination == owner {
        (amount, source)
    } else {
        return None;
    };
    Some(Movement::Sol {
        units,
        peer: peer.to_owned(),
    })
}

// Native SOL passes through a temporary wrapped-SOL account. Its transfer
// inside the pool instruction excludes the fee and any token-account deposit.
fn swapped_sol(tx: &Value, owner: &str, token: i128) -> Option<i128> {
    let instructions = tx["transaction"]["message"]["instructions"].as_array()?;
    let (index, swap) = instructions.iter().enumerate().find(|(_, ix)| {
        ix["programId"] == devnet_pool::PROGRAM
            && ix["accounts"][0] == owner
            && ix["accounts"][3] == devnet_pool::POOL
    })?;
    let accounts = &swap["accounts"];
    let paying_sol = token > 0;
    let (native, vault, native_mint, usd_mint) = if paying_sol {
        (4, 6, 10, 11)
    } else {
        (5, 7, 11, 10)
    };
    if accounts[native_mint] != devnet_pool::WRAPPED_SOL || accounts[usd_mint] != devnet_pool::USDC
    {
        return None;
    }
    let temporary = accounts[native].as_str()?;
    // A swap of persistent wrapped tokens is distinct from a native SOL payment.
    if !instructions[..index].iter().any(|ix| {
        ix["programId"] == devnet_pool::TOKEN
            && ix["parsed"]["type"] == "initializeAccount3"
            && ix["parsed"]["info"]["account"] == temporary
            && ix["parsed"]["info"]["mint"] == devnet_pool::WRAPPED_SOL
            && ix["parsed"]["info"]["owner"] == owner
    }) || !instructions[index + 1..].iter().any(|ix| {
        ix["programId"] == devnet_pool::TOKEN
            && ix["parsed"]["type"] == "closeAccount"
            && ix["parsed"]["info"]["account"] == temporary
            && ix["parsed"]["info"]["destination"] == owner
    }) {
        return None;
    }
    let vault = accounts[vault].as_str()?;
    let (source, destination) = if paying_sol {
        (temporary, vault)
    } else {
        (vault, temporary)
    };
    let group = tx["meta"]["innerInstructions"]
        .as_array()?
        .iter()
        .find(|g| g["index"].as_u64() == Some(index as u64))?;
    let transfer = group["instructions"].as_array()?.iter().find(|ix| {
        let info = &ix["parsed"]["info"];
        ix["programId"] == devnet_pool::TOKEN
            && ix["parsed"]["type"] == "transferChecked"
            && info["source"] == source
            && info["destination"] == destination
            && info["mint"] == devnet_pool::WRAPPED_SOL
    })?;
    let units = i128::from(
        transfer["parsed"]["info"]["tokenAmount"]["amount"]
            .as_str()?
            .parse::<u64>()
            .ok()?,
    );
    Some(if paying_sol { -units } else { units })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reports_the_transfer_amount_without_fees_and_does_not_invent_payments() {
        let mut tx = json!({"transaction":{"message":{"accountKeys":[{"pubkey":"alice"},{"pubkey":"bob"}],
            "instructions":[{"programId":text(&SYSTEM_PROGRAM),"parsed":{"type":"transfer",
            "info":{"source":"alice","destination":"bob","lamports":123456789}}}]}},
            "meta":{"fee":5000, "preTokenBalances":[], "postTokenBalances":[]}});
        assert!(
            matches!(transfer(&tx, "alice", "mint", &[]), Some(Movement::Sol { units: -123456789, peer }) if peer == "bob")
        );
        assert!(matches!(
            transfer(&tx, "bob", "mint", &[]),
            Some(Movement::Sol {
                units: 123456789,
                ..
            })
        ));
        tx["transaction"]["message"]["instructions"][0]["parsed"]["type"] = json!("createAccount");
        assert!(transfer(&tx, "alice", "mint", &[]).is_none());
    }

    #[test]
    fn token_changes_follow_the_owner_across_opened_and_closed_token_accounts() {
        let tx = json!({"transaction":{"message":{"accountKeys":[{"pubkey":"token-a"},{"pubkey":"token-b"}]}},
            "meta":{"preTokenBalances":[{"owner":"alice","mint":"mint","accountIndex":0,"uiTokenAmount":{"amount":"9200000"}}],
            "postTokenBalances":[{"owner":"bob","mint":"mint","accountIndex":1,"uiTokenAmount":{"amount":"9200000"}}]}});
        assert!(matches!(
            transfer(&tx, "alice", "mint", &[]),
            Some(Movement::Token { units: -9200000 })
        ));
        assert!(matches!(
            transfer(&tx, "bob", "mint", &[]),
            Some(Movement::Token { units: 9200000 })
        ));
        assert!(transfer(&tx, "alice", "other", &[]).is_none());
    }
}
