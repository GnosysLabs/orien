# Orien

A quantum-proof cryptocurrency, ORN, with no blocks and no global ordering.
A payment is final in a fraction of a second and never waits in a line
behind other payments. The name is from the Latin *oriens*, rising.

The network has been live since 14 November 2026.

- Website and downloads: [orien.network](https://orien.network)
- Technical paper, which states every rule exactly:
  [orien.network/paper](https://orien.network/paper/)
- Explorer, for every checkpoint, address and reward:
  [explorer.orien.network](https://explorer.orien.network)
- What changed, release by release:
  [orien.network/changelog](https://orien.network/changelog/)
- Community: [Discord](https://discord.gg/5FFRZAw3u9) and
  [X](https://x.com/OrienNetwork)

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
   the first hour, shared out each hour in proportion to proof-of-work.
   Mining has used RandomX, which suits ordinary computers' processors,
   since epoch 21 (15 November 2026); the first 21 hours used BLAKE3.
   Work only hands out coins: it never decides payments. The
   first seed comes from a Bitcoin block, so nobody can mine before that
   block exists; the live network starts from block 975,847. Every payment
   pays 0.1% of its amount, at least 0.000001 ORN and at most 1 ORN;
   amounts have nine decimal places.

The safety of payments rests on one assumption: the BLAKE3 hash function.
RandomX only decides who receives new coins.

## Use it

The easiest way is **Orien Wallet**, from [orien.network](https://orien.network):
for Mac, Windows and Linux, and for iPhone as a free public beta on
[TestFlight](https://testflight.apple.com/join/butuQ4Ms). It makes a wallet,
sends and receives, mines, and swaps ORN for SOL or Bitcoin. The Windows app
is not code-signed yet, so Windows shows a warning the first time it opens.

For the command line, on a Mac or Linux:

```bash
curl -fsSL https://orien.network/install.sh | sh
```

On Windows, in PowerShell:

```powershell
irm https://orien.network/install.ps1 | iex
```

The installer checks the download against its published checksum and puts
`orien` and the live network's genesis file in `~/.orien`. Then:

```bash
orien wallet new
```

```bash
orien mine
```

```bash
orien balance
```

```bash
orien send --to ADDRESS --amount 5
```

- **Wallets.** `orien wallet new` shows a 24-word recovery phrase; write it
  down. `orien wallet restore` rebuilds the same wallet, with the same
  address and coins, on another computer. `orien wallet password` seals
  the wallet file with a password (Argon2id and XChaCha20-Poly1305).
- **Mining.** `orien mine` asks how many processor cores to use;
  `--threads 4` answers in advance. `orien benchmark` finds the number that
  earns the most without wasting heat, and `orien mine` suggests it from
  then on. Each hour's new ORN are shared in
  proportion to the shares each miner found. The explorer shows, for every
  checkpoint, which wallets were paid and how much.
- **Swaps.** Orien Wallet swaps ORN for SOL or Bitcoin with anyone, with no
  one in the middle: each side's coins are locked so that either both
  payments happen or both come back. On Solana the lock is a small program,
  `programs/orien-lock`; on Bitcoin it is a script. Underneath, on Orien, are
  **locked payments**: `orien escrow --to ADDRESS
  --amount 500 --hours 48` locks ORN that the receiver takes with
  `orien claim` by revealing a secret, and that come back on their own
  after the deadline; `orien escrow-status` shows one. The design is in
  `docs/locked-payments.md`.
- **Updates.** `orien update` installs a newer version only if the Orien
  release key signed it.
- **Orien Wallet.** `apps/wallet` is the app (Tauri) for the same wallet, on
  desktop and iPhone: balance, history, sending, an address book, mining,
  swaps, the Market, and making or restoring a wallet.

## The live network

Anyone can stake 10,000 ORN to run a validator (see *Staking a new
validator* below). A network that starts with no coins has no stake, so it
started with four **launch validators**, named in the genesis file and run by
Gnosys Labs LLC, today on one server. From epoch 117 (18:11 UTC on 19
November 2026) they are reached by name, `v1.orien.network` to
`v4.orien.network`, so each can move to a server of its own without anyone
updating. Launch seats hold no coins and never earn fees. Their voting
weight shrinks as real stake arrives, and once 3,000,000 ORN is staked they
leave for good. Until more than 2,000,000 ORN is staked, the network depends
on them being online.

The rest of this file runs a local test network, with no real coins.

## Try it on a local test network

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
no coins move, and only the cheater's account is stuck. The next checkpoint
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

`orien mine` does proof-of-work for an account, on as many processor cores
as you choose. At each checkpoint the coins released since the last one are
shared among the work done in that hour:

```bash
./target/release/orien mine devnet/wallets/carol.wallet --threads 2
```

`orien status` shows how many coins have been released so far.
`devnet init --from-nothing` starts a network with no coins at all, as the
live one did; `--bitcoin HEIGHT:HASH` anchors it to a Bitcoin block.

### Writing a launch genesis file

Each launch validator makes a key on its own machine and shares only the
public line `orien keygen` prints. The genesis file is then written from
those lines and a Bitcoin block that has just been mined:

```bash
./target/release/orien genesis new genesis.json --validator "mac,ROOT,16,100.76.239.128:7101" --validator "vps,ROOT,16,100.99.59.98:7101" --bitcoin HEIGHT:HASH
```

It hands out no coins, takes a checkpoint every hour, and gives the launch
validators equal seats. `orien genesis show genesis.json` prints what a
genesis file says and its network id, so that everyone can check they hold
the same file.

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
one key, loses its stake at the next checkpoint, and its key is barred from
the validators for good.

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

Addresses are not part of the network's identity, but the first checkpoint
records each launch validator's address and every later one carries it
forward, so a launch validator's address is fixed once the network has
taken its first checkpoint. A staked validator names its address in its
stake order and can move by sending another.

Four validators tolerate one that is down, hacked or lying. A validator that
was down catches up from the others when it returns, checkpoints included.
If one machine holds two of the four, make it the one that is always on.
With fewer than three of four running, the network cannot take a checkpoint
and pauses until a third is back.

Validators can listen on public addresses. Every message that decides
anything is signed with a quantum-proof signature and checked on arrival,
and wallets and miners take balances and the work target only from
checkpoints they check themselves, so nobody between two computers can
forge or alter what matters. Traffic is not encrypted: payments are public
anyway, but whoever is on the path can see who talks to whom. A validator
takes at most 256 connections from one internet address and 20,000 in
all (`--max-per-address` and `--max-connections` change that), and closes connections that stay silent for 30 seconds or send junk; a
flood from thousands of machines at once needs protection in front of the
server, as for any service on the internet.

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
| Checking one signature, one core | about 25 µs (40,000 a second) |
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
| `crates/swap` | The Bitcoin and Solana sides of swaps: the locks, claim and refund, and each side's steps |
| `crates/market` | The server behind accounts, the Market, swap offers and the explorer |
| `programs/orien-lock` | The Solana program that locks SOL for swaps |
| `apps/wallet` | Orien Wallet, for Mac, Windows, Linux and iPhone |
| `web` | The website, orien.network, with the installers, the technical paper and the explorer |
| `discord-bot` | The community Discord's bot, and the website's AI assistant |
| `deploy` | Publishing the website and releases, and the live validators' service |
| `docs` | The design notes |

## Security

Orien is built only from standard, published constructions: the BLAKE3
hash, one-time signatures in Merkle key trees, RandomX for mining, and
SHA-256 for the locks swaps use. The way they are put together is new, and it has not yet had an
independent review. Cryptographers and security researchers are welcome to
look closely. Please report anything you find to hello@orien.network before
publishing it.

## Known limitations

What someone holding or sending ORN should know:

- **Signing twice with one key leaks it.** Two signatures from one one-time
  key reveal enough to forge further payments under that number. A wallet
  records what each of its keys signed, never signs anything different with
  it, and asks the validators before it signs, but two copies of one wallet
  signing at the same instant can still do it. Until validators holding a
  third of the stake have voted on either payment, someone who sees both
  signatures could get a forged third one accepted. The account's spare keys
  are fixed in its address beforehand, so whoever can forge the burned key
  has no hold on the account after the checkpoint.
- **Sixteen spare keys per account.** Each time an owner signs two different
  payments under one number, a checkpoint cancels both and the account moves
  to its next spare key. After sixteen such accidents the account stays
  locked.
- **A signed payment can't be cancelled.** Once signed, a payment can only be
  finished as it is, by any copy of the wallet.

### How it works, by design

- **Checkpoints and history are kept forever.** A validator deletes each
  epoch's payment log and closing messages once it has moved on, and keeps
  only its last three snapshots of the balances; one that has been away
  catches up by the checkpoints. The checkpoints themselves, about 40 KB an
  hour with 33 validators, are kept so that anyone can follow the network
  from its genesis file, and so is each account's incoming history.
- **Receipts grow with the validator set**, by 1 to 3 KB per vote.
- **A receipt from a later epoch needs the checkpoints.** A receipt from the
  first epoch checks against the genesis file alone; one from a later epoch
  checks against that epoch's validators, which a computer learns by
  following the checkpoints, one small download per epoch. `orien verify`
  downloads the ones it needs by itself.

### Still to come

- **Delegation.** Staking backs your own validator; backing someone else's
  is designed for later.
- **Receipts for payments a checkpoint settled.** A payment that had its
  votes when a checkpoint began, and that the checkpoint took in, is final
  without confirmations. The wallet says so, but cannot yet hand over a
  proof of it.
- **Encrypted connections.** Validators, wallets and miners talk without
  encryption, with one thread per connection.
- **Work shares on disk.** A validator restarted mid-hour forgets the shares
  it had taken; those that reached other validators still count.
- **A way back for a validator that uses up its key tree.** Validators build
  a fresh tree when the old one is half used and switch to it at a
  checkpoint. One that still runs out completely, after a long outage at the
  wrong moment, has no key left to announce the new tree with.

### Running a validator

- **Connections.** A validator takes at most 20,000 connections, 256 from
  one address (the epoch's validators are exempt). Each uses a thread and
  two open files, so allow about 45,000 open files (`ulimit -n`, or
  `LimitNOFILE` in a systemd service) before putting real load on one.
- **Validator keys sit unencrypted on disk**, readable only by their owner.
  Wallets, by contrast, can be sealed with a password, and Orien Wallet on
  Mac and iPhone can lock them with Touch ID or Face ID instead.

## Founders and maintainers

- Christopher McElvogue
- Matthew Gregorini

## License

Orien is free and open-source software, licensed under either of

- the Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE)), or
- the MIT License ([LICENSE-MIT](LICENSE-MIT)),

at your option. Unless you say otherwise, any contribution you submit for
inclusion is licensed the same way, without any additional terms.
