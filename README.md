# Orien

A quantum-proof payment network with no blocks, no miners and no global
ordering. A payment is final in a fraction of a second and never waits in a
line behind other payments. The name is from the Latin *oriens*, rising.

## The idea

1. **Accounts order themselves.** A payment says "account A, payment #7,
   send 5 to B". Each validator votes for it if it has never seen a different
   #7 from A. Votes from validators holding more than two thirds of the stake
   make a certificate, and two conflicting payments can never both get one,
   because any two such groups share an honest validator. The payment is
   final once as many validators have confirmed that they recorded the
   certificate. The receipt holds both sets of signatures.
2. **One-time signatures, with the ledger keeping count.** Every payment is
   signed with a hash-based one-time key and names the key for the next
   payment. "Never sign two different #7s" and "never use a key twice" are
   the same rule, and validators enforce it.
3. **Validators sign in batches.** A validator signs one Merkle root over
   every vote and confirmation it was asked for in the last few
   milliseconds. One signature covers them all.

Everything rests on one assumption: the BLAKE3 hash function.

## Try it

```bash
cargo build --release
```

```bash
./target/release/orien devnet init
```

```bash
./target/release/orien devnet run
```

In another terminal:

```bash
export ORIEN_GENESIS=devnet/genesis.json
```

```bash
./target/release/orien balance devnet/wallets/alice.wallet
```

```bash
./target/release/orien send -w devnet/wallets/alice.wallet --to devnet/wallets/carol.wallet --amount 250 --receipt payment.receipt
```

```bash
./target/release/orien verify payment.receipt
```

```bash
./target/release/orien devnet double-spend --wallet bob
```

```bash
./target/release/orien status
```

`status` ends each line with a fingerprint of every balance as that
validator holds them. Validators that show the same fingerprint hold exactly
the same ledger.

`devnet double-spend --split` shows the other way to cheat: it sends each
conflicting payment to half the validators at once. Neither becomes final,
no money moves, and only the cheater's account is stuck.

### An interrupted payment

A wallet can lose its connection, or the whole device, after signing a
payment and before the payment is final. Validators keep every signed
payment they vote for until it is final, and hand it to anyone who asks.
Any other copy of the wallet can therefore pick the payment up and finish
it. `devnet interrupt` plays the lost device:

```bash
./target/release/orien devnet interrupt
```

```bash
./target/release/orien balance devnet/wallets/alice.wallet
```

```bash
./target/release/orien resume -w devnet/wallets/alice.wallet
```

Until the payment is finished, `send` refuses to sign anything else from
that account: the one-time key for that payment number is already used.

## Run it across machines

`devnet run` keeps all validators in one process. To spread them out:

1. Create the network once, on any machine, giving each validator the
   address the other machines reach it on:

   ```bash
   orien devnet init --dir net --name my-network --address 10.0.0.1:7101 --address 10.0.0.2:7101 --address 10.0.0.3:7101 --address 10.0.0.3:7102
   ```

2. Copy `net/genesis.json` to every machine, and each `net/validators/vN.key`
   to the one machine that will run that validator.
3. On each machine:

   ```bash
   orien node -g genesis.json -k vN.key -d data --listen 10.0.0.1:7101
   ```

Addresses are not part of the network's identity. Changing one in the
genesis file does not change the network, as long as every copy of the file
gets the change.

Four validators tolerate one that is down, hacked or lying. A validator that
was down catches up from the others when it returns. If one machine holds
two of the four, make it the one that is always on.

Traffic is not encrypted or rate-limited yet. Forged messages are rejected
because every vote and receipt is signed, but for now give validators
private addresses only and listen on those, never on a public one.

`deploy/rehearsal.sh` does all of this for one particular set of machines
and is meant to be copied: it builds for Linux, copies one folder to each
machine over SSH, and starts, stops and removes the validators. It installs
nothing on them.

## Measured

On one Apple M5 laptop running all four validators and every client, with
each validator syncing its votes to disk:

| | |
|---|---|
| Time to finality, one payment | 25 to 55 ms |
| Throughput, 50 clients at once | 500 to 600 payments/s |
| Throughput, 100 to 250 clients at once | 950 to 1,500 payments/s |
| Throughput, 500 clients at once | 750 to 1,000 payments/s |
| Payments covered by one validator signature under load | 10 to 28 |
| One-time signature | 1,248 bytes |
| Receipt (3 of 4 votes, 3 of 4 confirmations) | 12,044 bytes |

Finality in two steps costs throughput on this setup. Each payment waits
for two disk syncs, one after the other, and here four validators share one
disk. Before confirmations were added the same test ran at 1,400 to 1,650
payments a second.

With the four validators on three machines instead (a laptop, a desktop on
the same home network, and a server 57 ms away holding two):

| | |
|---|---|
| Time to finality, median of 40 payments | 156 ms |
| Fastest and slowest of the 40 | 137 ms and 241 ms |

A payment needs two round trips to enough validators: one for the votes and
one for the confirmations. With the server needed for a quorum, about 115 ms
of the 156 is the network's own travel time.

Reproduce the throughput figure with:

```bash
cargo test --release -p orien-net --test devnet throughput -- --ignored --nocapture
```

## Layout

| Crate | What it holds |
|---|---|
| `crates/crypto` | One-time signatures, validator key trees, batch Merkle proofs |
| `crates/ledger` | The rules: orders, votes, certificates, when to vote, how balances change |
| `crates/net` | The validator node, storage, sync between validators, client, wallet files |
| `crates/cli` | The `orien` command |

## Not done yet

- **Fixed validator set.** Validators come from the genesis file. Adding and
  removing them, staking and rewards need an agreement step at epoch
  boundaries.
- **No fees.** Nothing yet stops someone flooding the network with tiny
  payments or dust accounts.
- **A double-signed account can stay locked.** If two different payments are
  signed under one number and the validators' votes split, neither can become
  final. `orien resume` can still finish one of them while enough validators
  have not voted. After that the account is stuck. The fix is to release such
  locks at epoch boundaries.
- **Signing twice with one key leaks it.** Two signatures from one one-time
  key reveal enough to forge further payments under that number. A wallet
  asks the validators before it signs and refuses if the key is already in
  use, but two copies of a wallet signing at the same instant can still do
  it. Until validators holding a third of the stake have voted on either
  payment, someone who sees both signatures could get a forged third one
  accepted. Releasing a locked account will also have to move it to a spare
  key named in advance.
- **A wallet cannot replace a signed payment.** Once signed, a payment can
  only be finished as it is, by any copy of the wallet. It cannot be
  cancelled.
- **Validator keys run out.** A key tree signs 2^height batches (65,536 by
  default). Rolling over to a fresh tree belongs with epochs.
- **Receipts grow with the validator set**, at 1 to 3 KB per vote. Checkpoints
  that replace old receipts are the planned answer.
- **Networking is basic**: one thread per connection, no encryption, no
  limits. Raise `ulimit -n` before putting real load on a validator.
- **Keys sit unencrypted on disk**, readable only by their owner.
- **No outside review.** The building blocks are standard published
  constructions, but this composition is new. It must be reviewed by
  cryptographers before it holds anything of value.
