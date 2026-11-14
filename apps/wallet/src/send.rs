//! Sending a payment, with the same safety rules as `orien send`.
//!
//! These rules live in the `orien` command's own code today, not in the
//! shared library, so this is a copy of them. They should move into
//! `orien-net` one day, so both places use one copy.
//!
//! The rule that matters most: a wallet never signs two different payments
//! under one number, because two signatures from one one-time key leak
//! enough to forge a third.

use orien_net::client::{AccountStatus, Client, Done, TransferError, Unfinished};
use orien_net::unlock::Signing;
use orien_ledger::{AccountId, FEES, FORFEITED, Fees, Kind, MINT, REWARDS, Signature, SignedOrder, Signer};
use serde::Serialize;
use std::time::{Duration, Instant};

/// What the window shows once a payment is final.
#[derive(Serialize)]
pub struct Sent {
    pub amount: u64,
    pub fee: u64,
    /// The amount and fee as people read them.
    pub amount_text: String,
    pub fee_text: String,
    pub to: String,
    pub seconds: f64,
    /// Set when the app finished an earlier payment instead of sending the
    /// one asked for, which then still has to be sent.
    pub earlier: bool,
}

/// True when a checkpoint cancelled the number `signed` was made under and
/// retired the key that signed it.
fn retired(status: &AccountStatus, signed: &SignedOrder) -> bool {
    match &status.signer {
        Some(Signer::Spare(leaf)) if signed.order.seq == status.seq => {
            !matches!(&signed.signature, Signature::Spare(signature) if signature.leaf == *leaf)
        }
        _ => false,
    }
}

/// Payments under the account's current number that this wallet signed
/// and that are not final yet.
fn unfinished(client: &Client, wallet: &Signing, status: &AccountStatus) -> Vec<Unfinished> {
    let committee = client.committee();
    let network = committee.network();
    let mut found: Vec<Unfinished> = client
        .unfinished(&wallet.address(), status)
        .into_iter()
        .filter(|payment| wallet.signed_by_this_wallet(&payment.signed, network))
        .collect();
    let live = |own: &&SignedOrder| own.order.seq == status.seq && !retired(status, own);
    if let Some(own) = wallet.pending().filter(live) {
        let digest = own.order.digest(network);
        if !found.iter().any(|payment| payment.digest == digest) {
            found.push(Unfinished { signed: own.clone(), digest, stake: 0 });
        }
    }
    found
}

fn explain(error: TransferError) -> String {
    match error {
        TransferError::NotConfirmed { .. } => format!(
            "{error}. Nothing is lost: the payment keeps its place and nothing else can take it. \
             Press Send again to finish it."
        ),
        other if other.closing() => "The network was pausing for its hourly checkpoint and did not \
             finish in time. Nothing is lost. Press Send again in a minute to finish it."
            .to_string(),
        other => other.to_string(),
    }
}

/// Collects votes and confirmations until the payment is final.
fn finish(client: &Client, wallet: &mut Signing, signed: &SignedOrder, earlier: bool) -> Result<Sent, String> {
    let started = Instant::now();
    // A checkpoint pauses payments for a few seconds. Wait it out.
    match client.pay(signed, Duration::from_secs(90)).map_err(explain)? {
        Done::Receipt(_) | Done::InCheckpoint(_) => {}
    }
    wallet.confirm(signed.order.seq).map_err(|error| error.to_string())?;
    Ok(Sent {
        amount: signed.order.amount,
        fee: signed.order.max_fee,
        amount_text: crate::money::show(signed.order.amount as i128),
        fee_text: crate::money::show(signed.order.max_fee as i128),
        to: signed.order.to.to_string(),
        seconds: started.elapsed().as_secs_f64(),
        earlier,
    })
}

pub fn send(client: &Client, wallet: &mut Signing, fees: Fees, to: AccountId, amount: u64) -> Result<Sent, String> {
    let network = *client.committee().network();
    // The network's own accounts cannot be paid. Refuse before signing, so
    // no one-time key is spent on a payment the network would refuse.
    if [MINT, REWARDS, FEES, FORFEITED].contains(&to) {
        return Err("That address belongs to the network itself, and nobody can pay it.".into());
    }
    // Every payment carries the network's fee; without it validators refuse it.
    wallet.accept_fee(fees);
    let fee = fees.on(&Kind::Payment, amount);
    let status = client.account(&wallet.address(), wallet.next_seq());
    if status.views.iter().all(Option::is_none) {
        return Err("no validator could be reached".into());
    }
    let seq = status.seq;

    // A payment a checkpoint cancelled: let go of it.
    if let Some(pending) = wallet.pending().filter(|pending| retired(&status, pending)).cloned() {
        wallet.forget(pending.order.seq).map_err(|error| error.to_string())?;
    }
    // A payment signed earlier that has become final since.
    if let Some(earlier) = wallet.pending().filter(|earlier| earlier.order.seq < seq).cloned() {
        wallet.confirm(earlier.order.seq).map_err(|error| error.to_string())?;
    }

    let unfinished = unfinished(client, wallet, &status);
    // Votes for a payment this wallet cannot account for mean its key for
    // this number may be in use elsewhere. Signing now could reuse it.
    if status.locks.iter().any(|lock| !unfinished.iter().any(|payment| payment.digest == *lock)) {
        return Err("Validators are holding a payment from this account that this wallet did not sign \
                    here. To keep your key safe, nothing was signed. Check again in a minute."
            .into());
    }

    match unfinished.as_slice() {
        [] => match status.spendable {
            Some(balance) if balance < amount as i128 + fee as i128 => {
                return Err(format!(
                    "Not enough funds: this wallet holds {}, and the payment with its fee is {}.",
                    crate::money::show(balance),
                    crate::money::show(amount as i128 + fee as i128)
                ));
            }
            None => return Err("Too few validators are caught up on this account. Try again shortly.".into()),
            _ => {}
        },
        // The same payment is already signed: finish it rather than sign again.
        [one] if one.signed.order.kind == Kind::Payment && one.signed.order.to == to && one.signed.order.amount == amount => {
            if wallet.pending().is_none_or(|own| own.order != one.signed.order) {
                wallet.adopt(&one.signed, &network).map_err(|error| error.to_string())?;
            }
            return finish(client, wallet, &one.signed.clone(), false);
        }
        // A different payment is signed and waiting. It has to finish before
        // anything new can be signed, so finish it first, if it still can.
        [one] if client.can_still_finish(&unfinished, one) => {
            if wallet.pending().is_none_or(|own| own.order != one.signed.order) {
                wallet.adopt(&one.signed, &network).map_err(|error| error.to_string())?;
            }
            return finish(client, wallet, &one.signed.clone(), true);
        }
        _ => {
            return Err("This account has two different payments signed under one number, so neither \
                        can finish. The network's next hourly checkpoint cancels both and frees the \
                        account. Nothing has moved."
                .into());
        }
    }

    let signed = wallet
        .sign(Kind::Payment, seq, to, amount, status.signer.as_ref(), &network)
        .map_err(|error| error.to_string())?;
    finish(client, wallet, &signed, false)
}
