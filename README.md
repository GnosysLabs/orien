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
   the same rule, and validators enforce it. An address also commits to
   sixteen spare keys, for the day a key does get used twice.
3. **Validators sign in batches.** A validator signs one Merkle root over
   every vote and confirmation it was asked for in the last few
   milliseconds. One signature covers them all.
4. **A checkpoint every so often.** About once an hour the validators pause
   for a moment and agree on a snapshot of every balance. This is the only
   time they need to agree on anything, and no payment waits in line for
   it. The checkpoint frees an account whose owner signed twice, lets
   validators change their keys, chooses the next hour's validators from
   the coins staked behind them, takes the stake of any validator caught
   cheating, pays the hour's fees to the staked validators, and pays the
   hour's newly released coins to whoever did proof-of-work. The design is
   in `docs/epochs.md`.
5. **Coins from nothing.** No coins exist at launch. Of a 100 million cap,
   half of what remains is released every four years, about 1,977 coins in
   the first hour, shared out each hour in proportion to BLAKE3
   proof-of-work. Work only hands out coins: it never decides payments. The
   first seed is a Bitcoin block announced in advance, so nobody can mine
   before launch. Every payment pays 0.1% of its amount, at least 0.000001
   coin and at most 1 coin; amounts have nine decimal places.

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
no money moves, and only the cheater's account is stuck. The next checkpoint
cancels both payments and frees the account, which from then on signs with
one of its spare keys. A local network takes a checkpoint every minute, so
wait a minute and send again:

```bash
./target/release/orien devnet double-spend --wallet bob --split
```

```bash
./target/release/orien balance devnet/wallets/bob.wallet
```

```bash
./target/release/orien send -w devnet/wallets/bob.wallet --to devnet/wallets/carol.wallet --amount 10
```

`devnet init --epoch-seconds` sets how often checkpoints happen. A payment
sent while one is being made simply waits for it.

### Mining

`orien mine` does proof-of-work for an account on every processor core. At
each checkpoint the coins released since the last one are shared among
the work done in that hour:

```bash
./target/release/orien mine devnet/wallets/carol.wallet
```

`orien status` shows how many coins have been released so far.
`devnet init --from-nothing` starts a network with no coins at all, as the
real one will; `--bitcoin HEIGHT:HASH` anchors it to a Bitcoin block.

### Staking a new validator

Anyone can become a validator by staking at least 10,000 coins behind a
validator key. The launch validators of the genesis file hold seats that
share 3 million coins of voting weight, which fades as real stake arrives.
Start a validator with a fresh key, then stake for it; it joins at the next
checkpoint:

```bash
./target/release/orien keygen newcomer.key
```

```bash
./target/release/orien node -k newcomer.key -d newcomer-data --listen 127.0.0.1:7005 -g devnet/genesis.json
```

```bash
./target/release/orien stake -w devnet/wallets/alice.wallet --amount 10000 --validator-key newcomer.key --address 127.0.0.1:7005 -g devnet/genesis.json
```

`orien status` lists each epoch's validators with their share of the vote.
`orien unstake` releases staked coins after a wait of 168 epochs, one week.
A validator that votes for two conflicting payments, or signs twice with
one key, loses its stake at the next checkpoint.

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
was down catches up from the others when it returns, checkpoints included.
If one machine holds two of the four, make it the one that is always on.
With fewer than three of four running, the network cannot take a checkpoint
and pauses until a third is back.

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
| Receipt (3 of 4 votes, 3 of 4 confirmations) | 12,086 bytes |

Finality in two steps costs throughput on this setup. Each payment waits
for two disk syncs, one after the other, and here four validators share one
disk. Before confirmations were added the same test ran at 1,400 to 1,650
payments a second.

With the four validators on three machines instead (a laptop, a desktop on
the same home network, and a server 57 ms away holding two):

| | |
|---|---|
| Time to finality, median of 40 payments | 162 ms |
| Fastest and slowest of the 40 | 138 ms and 240 ms |

A payment needs two round trips to enough validators: one for the votes and
one for the confirmations. With the server needed for a quorum, about 115 ms
of the 162 is the network's own travel time.

A checkpoint pauses payments while the validators agree on it:

| | |
|---|---|
| A payment that runs into a checkpoint, one laptop | about 0.2 s |
| The same on the three machines | about 1.1 s |
| Signatures a validator spends on an idle checkpoint | 3 |

Reproduce the throughput figure with:

```bash
cargo test --release -p orien-net --test devnet throughput -- --ignored --nocapture
```

## Layout

| Crate | What it holds |
|---|---|
| `crates/crypto` | One-time signatures, validator key trees, batch Merkle proofs |
| `crates/ledger` | The rules: orders, votes, certificates, when to vote, how balances change, what a checkpoint is and how validators agree on one |
| `crates/net` | The validator node, the closing of epochs, storage, sync between validators, client, wallet files |
| `crates/cli` | The `orien` command |

## Not done yet

- **No delegation.** Staking backs your own validator; backing someone
  else's is designed for later.
- **Work shares are kept in memory.** A validator restarted mid-hour
  forgets the shares it had taken; those that reached other validators
  still count.
- **Sixteen spare keys per account.** Each time an owner signs two different
  payments under one number, a checkpoint cancels both and the account moves
  to its next spare key. After sixteen such accidents the account stays
  locked.
- **Signing twice with one key leaks it.** Two signatures from one one-time
  key reveal enough to forge further payments under that number. A wallet
  asks the validators before it signs and refuses if the key is already in
  use, but two copies of a wallet signing at the same instant can still do
  it. Until validators holding a third of the stake have voted on either
  payment, someone who sees both signatures could get a forged third one
  accepted. The spare keys are fixed in the address beforehand, so whoever
  can forge the burned key has no hold on the account after the checkpoint.
- **No receipt for a payment a checkpoint settled.** A payment that had its
  votes when a checkpoint began, and that the checkpoint took in, is final
  without confirmations. The wallet says so, but cannot yet hand over a
  proof of it.
- **A receipt from a later epoch needs the checkpoints.** A receipt from the
  first epoch checks against the genesis file alone. One from a later epoch
  checks against that epoch's validators, which a computer learns by
  following the checkpoints, one small download per epoch.
- **Checkpoints are kept forever.** A validator deletes each epoch's
  payment log and closing messages once it has moved on and keeps only its
  last three snapshots of the balances; one that has been away catches up
  by the checkpoints. The checkpoints themselves, about 40 KB an hour with
  33 validators, are kept so that anyone can follow the network from its
  genesis file.
- **A wallet cannot replace a signed payment.** Once signed, a payment can
  only be finished as it is, by any copy of the wallet. It cannot be
  cancelled.
- **A validator that uses up its whole key tree is stuck.** Validators build
  a fresh tree when the old one is half used and switch to it at a
  checkpoint. One that still runs out completely, after a long outage at
  the wrong moment, has no key left to announce the new tree with.
- **Receipts grow with the validator set**, at 1 to 3 KB per vote.
- **Networking is basic**: one thread per connection, no encryption, no
  limits. Raise `ulimit -n` before putting real load on a validator.
- **Keys sit unencrypted on disk**, readable only by their owner.
- **No outside review.** The building blocks are standard published
  constructions, but this composition is new. It must be reviewed by
  cryptographers before it holds anything of value.
