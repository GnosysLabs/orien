# Locked payments, and swapping ORN for Bitcoin

Built: the ledger rules (`crates/ledger`), the network side (`crates/net`),
`orien escrow`, `orien claim` and `orien escrow-status`, the Bitcoin side
(`crates/swap`), and swaps in Orien Wallet. Two details changed from the
first design: a locked payment is named by its sender and order number
(`ADDRESS:NUMBER`) rather than its digest, and the Bitcoin script also checks
that the secret is 32 bytes (`OP_SIZE 32 OP_EQUALVERIFY`). The swap's timings
are 48 hours for the ORN lock and 72 blocks for the Bitcoin lock. Three
reviews before release added:

- The buyer pays only for ORN proven locked by validators' signatures: the
  Lock order's certificate with confirmations, or a Merkle path from the
  escrow list to the last checkpoint's state root (the `Locks` request). It
  counts the ORN claimed only on the same kind of proof. Its sense of time
  takes the clock into account, so hidden checkpoints cannot make a deadline
  look further off.
- Payments are checked against their own transactions, and spends against
  their signatures. The seller reveals the secret only for payments every
  explorer reports 3 blocks deep, claims only as many coins as the price
  needs (largest first), and only at the network's fee with room to triple
  it, so dust paid into the lock cannot make the claim unmineable. The buyer
  reads every explorer that answers and remembers every payment it sees.
  Deadlines are set and called from a height every explorer agrees on.
- Claims and refunds use replace-by-fee and are sent again until mined, at
  the highest fee rate any explorer reports, raised by a quarter every 3
  blocks unmined. From 6 hours before the ORN deadline the buyer's refund
  doubles its fee every block, up to nine tenths of the coins.
- The buyer refunds each payment on its own once it is mined, so a
  stranger's unconfirmed payment cannot pin the refund; keeps refunding
  until the ORN are claimed, even after the secret appears; and takes back
  any Bitcoin left in the lock after claiming them.
- The seller locks under the order number saved before anything is signed,
  and under no other, so the ORN are never locked twice. After a crash each
  side recognises its own signed claim or refund. The app saves after every
  swap's step.

Known limit: the `Locks` answer carries the whole escrow list, so past about
130,000 waiting locked payments it outgrows a 16 MiB message and the buyer
falls back to certificates, which work only within the lock's own epoch.
Paging it is future work.

## In plain words

A **locked payment** sends ORN to someone, but they only receive it if they
reveal a secret before a deadline. If they don't, the ORN go back to the
sender on their own. That one feature lets two strangers swap ORN for Bitcoin
(or for ETH or USDC) with nobody in the middle and nobody able to cheat:

1. Alice wants Bitcoin for her ORN. She picks a secret, and locks 500 ORN for
   Bob behind it, for two days.
2. Bob sees the locked ORN (final in under a second) and locks 0.01 BTC for
   Alice behind the same secret, for one day. Bitcoin has had this kind of
   lock for years.
3. Alice takes the Bitcoin. To do it she has to reveal the secret, and the
   Bitcoin network shows it to everyone.
4. Bob reads the secret and takes the ORN.

If Bob never locks his Bitcoin, Alice's ORN come back after two days. If
Alice never takes the Bitcoin, Bob's come back after one day, and Alice's
after two. Either both sides get paid, or both get their coins back. Bob's
lock is shorter so that Alice can never take the Bitcoin too late for Bob to
take the ORN.

Orien Wallet will do all of this for its user, including the Bitcoin side:
it makes a one-time Bitcoin address for the swap, the user pays into it from
any Bitcoin wallet, and the app claims or refunds automatically.

## Three new kinds of order

New variants go at the end of `Kind`, which leaves every existing order's
encoding alone.

```rust
enum Kind {
    Payment,
    Stake { validator, proof },
    Unstake,
    /// Locks `amount` for `to`, claimable with the secret behind `hash`
    /// until epoch `until` begins.
    Lock { hash: [u8; 32], until: u64 },
    /// Claims the locked payment `lock` (the digest of its Lock order) by
    /// revealing its secret. Signed by the payee; `amount` is the locked
    /// amount and `to` the payee itself.
    Claim { lock: Hash32, secret: [u8; 32] },
}
```

There is no refund order. Refunds happen at checkpoints (below), which is
what keeps locked payments free of races.

### Lock

- Checked like a payment, with the same fee (0.1%, at least 0.000001 ORN,
  at most 1 ORN). The sender's balance must cover the amount and the fee.
- `until` must be at least 2 epochs and at most 720 epochs (30 days) after
  the epoch the order is certified in, so a lock always lasts at least one
  full hour and the state never holds locks for long.
- Applying it moves the amount out of the sender's balance into a new
  **escrow** in the state, keyed by the Lock order's digest:
  `Escrow { from, to, amount, hash, until }`.

### Claim

- An order from the payee's own account. The payee often holds no ORN yet,
  so a Claim is allowed from an account the state does not know (its first
  key proves the address, as for any first order), and its fee, the minimum,
  comes out of the escrow rather than the balance.
- A validator votes for it only if the escrow exists, names this account as
  `to`, the order's epoch is before `until`, and
  **SHA-256(secret) = hash**.
- Applying it pays `amount − fee` to the payee, removes the escrow, and
  records the secret in the state for the counterparty to read.

The escrow can only ever be claimed by its payee, and the payee's orders
are numbered like any account's, so two claims of one escrow can never both
be certified: the second finds the escrow gone. Nothing about a locked
payment needs validators to agree on an order between different accounts.

### Refunds at checkpoints

At the checkpoint that closes epoch `until − 1`, every escrow still in the
state goes back to its sender's balance. A Claim certified in an epoch
before `until` is applied before that, because the checkpoint applies the
epoch's payments first. A Claim cannot straddle: votes belong to one epoch,
and validators stop voting when an epoch closes.

So the deadline is exact, and there is never a moment when both a claim and
a refund could happen.

The one thing to avoid is claiming at the last minute: a claim that is not
yet final when the deadline's checkpoint comes is refused after it, and the
escrow goes back. Wallets therefore claim as soon as they learn the secret,
and the deadlines in a swap leave a day between them.

## Why SHA-256 for the secret

Bitcoin's locks can check SHA-256 but not BLAKE3, and Ethereum checks
SHA-256 cheaply too. For the same secret to unlock both sides, the lock must
use SHA-256. This is the only place Orien uses anything but BLAKE3, and it
protects only the swap itself: a weakness in SHA-256 could let someone take
a locked payment early, but could not touch anything else on the network.
Against quantum computers, SHA-256 keeps about 128 bits of security.

## What the state gains

- `escrows: BTreeMap<Hash32, Escrow>`, in Lock order digest order.
- `revealed: BTreeMap<Hash32, ([u8; 32], u64)>`: each claimed escrow's
  secret and the epoch it was revealed in, kept for 168 epochs (a week),
  then dropped.

Both are part of the state root, as leaves after the accounts like the
barred keys, and of snapshots. New requests let anyone read an escrow and a
revealed secret, with a proof against the last checkpoint.

## Upgrading the live network

The network holds real coins, so it is upgraded, never reset:

1. **Data format 7** reads format 6 snapshots and adds the empty escrow and
   secret lists. No existing data changes; its state root is unchanged while
   the new lists are empty.
2. **An activation epoch.** The new orders count only from a set epoch, a
   few hours after the release: epoch 15 (2026-11-15 12:11 UTC), opened by
   orien 1.3.0. Until then, every validator, old or new,
   follows the same rules. That gives time to update all four validators,
   one at a time, and to publish the new `orien` and Orien Wallet, before
   anyone can make a locked payment.
3. **Older programs** cannot read the new kinds of order. `orien update` and
   Orien Wallet's auto-update cover people who keep them current. A wallet
   that is never updated keeps working for everything except reading locked
   payments.

## How the Bitcoin side works

Orien Wallet makes a Bitcoin script (P2WSH) for each swap:

```
OP_IF
    OP_SHA256 <hash> OP_EQUALVERIFY <payee key> OP_CHECKSIG
OP_ELSE
    <deadline> OP_CHECKLOCKTIMEVERIFY OP_DROP <payer key> OP_CHECKSIG
OP_ENDIF
```

The keys are one-time keys the app makes for this swap only. The user funds
the script's address from any Bitcoin wallet; the app watches the Bitcoin
network through a public explorer, and claims or refunds with its own swap
key, paying out to the Bitcoin address the user gives. The Bitcoin side
waits for a confirmation or two, so a swap takes from about twenty minutes
to an hour, though the ORN side is instant.

## Built in pieces

1. The ledger: the two kinds of order, escrows, refunds at checkpoints, the
   state root and snapshots, with tests.
2. The network: data format 7, the activation epoch, the new requests, and a
   full swap on a test network between two wallets.
3. `orien lock`, `orien claim` and `orien swap` in the command-line program.
4. Swaps in Orien Wallet, with the Bitcoin side.
5. The paper, How it works and the skill.
6. Upgrading the live network, with your yes, at a checkpoint.

Swap offers in a marketplace, and swaps with Ethereum, build on this later
and need no further change to the network.
