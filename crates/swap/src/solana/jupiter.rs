//! SOL and USDC swaps on Solana's main network, routed by Jupiter. Jupiter
//! builds the transaction; before the wallet signs it, Solana simulates it
//! and the wallet checks that only the expected SOL and USDC would move.
use super::*;
use base64::{Engine, engine::general_purpose::STANDARD};

const QUOTE: &str = "https://lite-api.jup.ag/swap/v1/quote";
const BUILD: &str = "https://lite-api.jup.ag/swap/v1/swap";
const WRAPPED_SOL: &str = "So11111111111111111111111111111111111111112";
/// Half a percent: the least the swap may give before it fails instead.
const SLIPPAGE_BPS: u64 = 50;
/// Network and priority fees allowed on top of the amount paid, in lamports.
const MAX_FEES: i128 = 3_000_000;

pub struct Swap {
    pub payer: Pubkey,
    pub blockhash: Pubkey,
    pub last_valid_height: u64,
    pub output: u64,
    pub minimum: u64,
    pub fee: u64,
    pub account_deposit: u64,
    wire: Vec<u8>,
    message: std::ops::Range<usize>,
}

impl Swap {
    /// The transaction, signed by the wallet's Solana key.
    pub fn signed(&self, signer: &Keypair) -> Result<Vec<u8>> {
        anyhow::ensure!(signer.pubkey() == self.payer, "The active Solana wallet changed.");
        let mut wire = self.wire.clone();
        let signature = signer.sign(&wire[self.message.clone()]);
        // One signer, the payer: its signature is the first.
        let (_, at) = read_compact(&wire, 0)?;
        wire[at..at + 64].copy_from_slice(&signature);
        Ok(wire)
    }
}

fn read_compact(bytes: &[u8], mut at: usize) -> Result<(usize, usize)> {
    let mut value = 0usize;
    for shift in [0, 7, 14] {
        let byte = *bytes.get(at).context("The swap transaction is cut short.")?;
        at += 1;
        value |= usize::from(byte & 0x7f) << shift;
        if byte & 0x80 == 0 {
            return Ok((value, at));
        }
    }
    bail!("The swap transaction is malformed.")
}

/// Where the message starts, its one signer and its recent blockhash, after
/// checking the wallet is the only one that signs and pays.
fn inspect(wire: &[u8], payer: &Pubkey) -> Result<(std::ops::Range<usize>, Pubkey)> {
    let (signatures, at) = read_compact(wire, 0)?;
    anyhow::ensure!(signatures == 1, "The swap asks for more than this wallet's signature.");
    let start = at + 64;
    let mut at = start;
    let version = *wire.get(at).context("The swap transaction is cut short.")?;
    if version & 0x80 != 0 {
        anyhow::ensure!(version == 0x80, "Unknown Solana transaction version.");
        at += 1;
    }
    let header = wire.get(at..at + 3).context("The swap transaction is cut short.")?;
    anyhow::ensure!(header[0] == 1, "The swap asks for more than this wallet's signature.");
    at += 3;
    let (keys, after) = read_compact(wire, at)?;
    anyhow::ensure!(keys >= 1, "The swap transaction is malformed.");
    let first = wire.get(after..after + 32).context("The swap transaction is cut short.")?;
    anyhow::ensure!(first == payer, "The swap isn't paid by this wallet.");
    let at = after + 32 * keys;
    let blockhash: Pubkey = wire.get(at..at + 32).context("The swap transaction is cut short.")?.try_into()?;
    Ok((start..wire.len(), blockhash))
}

fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new().timeout(Duration::from_secs(20)).user_agent("orien-wallet").build()
}

/// A quote for paying `amount` (lamports, or USDC's millionths), signed into
/// a transaction that has been simulated and checked.
pub fn quote(network: &Solana, payer: Pubkey, sol_in: bool, amount: u64) -> Result<Swap> {
    anyhow::ensure!(amount > 0, "Enter an amount to swap.");
    let usdc = network.usdc;
    let (input, output) = if sol_in { (WRAPPED_SOL, usdc) } else { (usdc, WRAPPED_SOL) };
    let found: Value = agent()
        .get(QUOTE)
        .query("inputMint", input)
        .query("outputMint", output)
        .query("amount", &amount.to_string())
        .query("slippageBps", &SLIPPAGE_BPS.to_string())
        // One pool, straight from one coin to the other: a route through a
        // third coin leaves an empty account behind, holding some SOL.
        .query("onlyDirectRoutes", "true")
        .call()
        .map_err(|error| match error {
            ureq::Error::Status(400, _) => anyhow!("No swap route for that amount. Try a different amount."),
            other => anyhow!("Couldn't reach the swap service. Try again in a moment. ({other})"),
        })?
        .into_json()?;
    let number = |key: &str| found[key].as_str().and_then(|n| n.parse::<u64>().ok()).with_context(|| format!("The swap quote has no {key}."));
    let (out, minimum) = (number("outAmount")?, number("otherAmountThreshold")?);
    anyhow::ensure!(number("inAmount")? == amount && found["swapMode"] == "ExactIn", "The swap quote doesn't match the amount.");
    anyhow::ensure!(minimum > 0 && minimum <= out, "The swap quote is invalid.");

    let built: Value = agent()
        .post(BUILD)
        .send_json(json!({
            "quoteResponse": found,
            "userPublicKey": text(&payer),
            "wrapAndUnwrapSol": true,
            "dynamicComputeUnitLimit": true,
            "prioritizationFeeLamports": { "priorityLevelWithMaxLamports": { "maxLamports": 1_000_000, "priorityLevel": "medium" } },
        }))
        .map_err(|error| anyhow!("Couldn't prepare the swap. Try again in a moment. ({error})"))?
        .into_json()?;
    let wire = STANDARD.decode(built["swapTransaction"].as_str().context("The swap service sent no transaction.")?)?;
    let last_valid_height = built["lastValidBlockHeight"].as_u64().context("The swap service sent no expiry.")?;
    let (message, blockhash) = inspect(&wire, &payer)?;

    // What the wallet holds now, and what Solana says it would hold after.
    let mint = address(usdc)?;
    let token = address(devnet_usdc::TOKEN)?;
    let usdc_account = devnet_usdc::associated(payer, mint, token, address(devnet_usdc::ASSOCIATED)?)?;
    let sol_before = i128::from(network.shown_balance(&payer)?);
    let usdc_existing = network.account(&usdc_account)?;
    let usdc_before = i128::from(usdc_existing.as_ref().filter(|a| a.owner == token).map_or(Ok(0), |a| {
        a.data.get(64..72).map(|b| u64::from_le_bytes(b.try_into().expect("8 bytes"))).context("Unexpected USDC account.")
    })?);
    let simulated = network.first(|rpc| {
        rpc.call(
            "simulateTransaction",
            json!([STANDARD.encode(&wire), {
                "sigVerify": false, "replaceRecentBlockhash": false, "commitment": "confirmed", "encoding": "base64",
                "accounts": { "encoding": "jsonParsed", "addresses": [text(&payer), text(&usdc_account)] },
            }]),
            None,
        )
    })?;
    let value = &simulated["value"];
    anyhow::ensure!(value["err"].is_null(), "Solana says this swap would fail right now. Try again in a moment.");
    let after = value["accounts"].as_array().context("Solana didn't show the swap's result.")?;
    let wallet = after.first().context("Solana didn't show the swap's result.")?;
    anyhow::ensure!(wallet["owner"] == text(&SYSTEM_PROGRAM), "The swap would hand this wallet to a program. Refused.");
    let sol_after = i128::from(wallet["lamports"].as_u64().context("Solana didn't show the swap's result.")?);
    let usdc_after = match after.get(1) {
        Some(account) if !account.is_null() => {
            let info = &account["data"]["parsed"]["info"];
            anyhow::ensure!(info["owner"] == text(&payer) && info["mint"] == usdc, "Unexpected USDC account.");
            anyhow::ensure!(info.get("delegate").is_none_or(Value::is_null), "The swap would let someone else spend this wallet's USDC. Refused.");
            i128::from(info["tokenAmount"]["amount"].as_str().and_then(|n| n.parse::<u64>().ok()).context("Unexpected USDC account.")?)
        }
        _ => 0,
    };
    let (sol_change, usdc_change) = (sol_after - sol_before, usdc_after - usdc_before);
    // The network fee, as Solana's own simulation counts it; the deposit is
    // a new USDC account's, kept in this wallet's own account.
    let fee = i128::from(value["fee"].as_u64().unwrap_or(5_000));
    let deposit = if sol_in && usdc_existing.is_none() { i128::from(network.rent(165)?) } else { 0 };
    let amount = i128::from(amount);
    if sol_in {
        anyhow::ensure!(usdc_change >= i128::from(minimum), "The swap would give less USDC than quoted. Refused.");
        anyhow::ensure!(-sol_change >= amount && -sol_change <= amount + deposit + fee.max(MAX_FEES), "The swap would take more SOL than shown. Refused.");
    } else {
        anyhow::ensure!(-usdc_change == amount, "The swap would take a different amount of USDC. Refused.");
        anyhow::ensure!(sol_change + fee >= i128::from(minimum), "The swap would give less SOL than quoted. Refused.");
    }
    Ok(Swap {
        payer,
        blockhash,
        last_valid_height,
        output: out,
        minimum,
        fee: u64::try_from(fee.max(0)).unwrap_or(0),
        account_deposit: u64::try_from(deposit).unwrap_or(0),
        wire,
        message,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signs_only_a_transaction_this_wallet_alone_pays_for() {
        let key = Keypair::from_secret(&[7; 32]);
        let mut wire = vec![1];
        wire.extend([0; 64]);
        wire.extend([0x80, 1, 0, 1, 2]);
        wire.extend(key.pubkey());
        wire.extend([9; 32]);
        wire.extend([5; 32]);
        wire.extend([0, 0]);
        let (message, blockhash) = inspect(&wire, &key.pubkey()).unwrap();
        assert_eq!(blockhash, [5; 32]);
        let swap = Swap { payer: key.pubkey(), blockhash, last_valid_height: 0, output: 0, minimum: 0, fee: 0, account_deposit: 0, wire: wire.clone(), message: message.clone() };
        let signed = swap.signed(&key).unwrap();
        assert_eq!(&signed[1..65], &key.sign(&wire[message])[..]);
        assert!(inspect(&wire, &[1; 32]).is_err());
        let mut two = wire.clone();
        two[0] = 2;
        assert!(inspect(&two, &key.pubkey()).is_err());
    }

    /// Quotes and simulates a real swap of 0.01 SOL for a wallet named in
    /// ORIEN_SOL_WALLET; signs and sends nothing.
    #[test]
    #[ignore]
    fn simulates_a_real_swap() {
        let Ok(owner) = std::env::var("ORIEN_SOL_WALLET") else { return };
        let network = Solana::mainnet();
        let swap = quote(&network, address(&owner).unwrap(), true, 10_000_000).unwrap();
        eprintln!("0.01 SOL -> {} USDC units (at least {}), fee {} lamports, deposit {}", swap.output, swap.minimum, swap.fee, swap.account_deposit);
    }
}
