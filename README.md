# Orien

A quantum-proof payment network with no blocks, no miners and no global
ordering. Payments are final in one round trip. The name is from the Latin
*oriens*, rising.

## The idea

1. **Accounts order themselves.** A payment says "account A, payment #7,
   send 5 to B". Each validator votes for it if it has never seen a different
   #7 from A. Once validators holding more than two thirds of the stake have
   voted, the payment is final. Two conflicting payments can never both get
   there, because any two such groups share an honest validator.
2. **One-time signatures, with the ledger keeping count.** Every payment is
   signed with a hash-based one-time key and names the key for the next
   payment. "Never sign two different #7s" and "never use a key twice" are
   the same rule, and validators enforce it.
3. **Validators sign in batches.** A validator signs one Merkle root over
   every order it accepted in the last few milliseconds. One signature
   covers them all.

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

1. Run `orien devnet init --dir net` once, on any machine.
2. Edit the `address` of each validator in `net/genesis.json` to an address
   the other machines can reach (a hostname works). Addresses are not part of
   the network's identity, so this does not change the network.
3. Copy `genesis.json` to every machine, and each `validators/vN.key` to the
   one machine that will run that validator.
4. On each machine:

   ```bash
   orien node -g genesis.json -k vN.key -d data --listen 0.0.0.0:7001
   ```

Four validators tolerate one that is down, hacked or lying. A validator that
was down catches up from the others when it returns.

Traffic is not encrypted or rate-limited yet. Forged messages are rejected
because every vote and receipt is signed, but for now run validators on a
private network.

## Measured

On one Apple M5 laptop running all four validators and every client, with
each validator syncing its votes to disk:

| | |
|---|---|
| Time to finality, one payment | 20 to 35 ms |
| Throughput, 50 to 500 clients at once | 1,400 to 1,650 payments/s |
| Payments covered by one validator signature under load | 33 to 50 |
| One-time signature | 1,248 bytes |
| Receipt (3 of 4 votes) | 5,450 bytes |

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
