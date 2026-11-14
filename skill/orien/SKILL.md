---
name: orien
description: Use the Orien cryptocurrency (ORN) from this computer with the `orien` command-line program. Covers installing orien, checking a balance, showing the wallet's address, sending ORN to someone, mining ORN, staking and running a validator, keeping orien up to date, and checking a payment receipt. Use when the user mentions Orien or ORN, asks "what's my balance", "send Bob 40 ORN", "start mining", "I want to be a validator", or anything else about their Orien wallet.
---

# Orien

Orien is a quantum-proof cryptocurrency. Its coin is **ORN**. Everything is done with one program, `orien`, which keeps its files in `~/.orien` (on Windows, `%USERPROFILE%\.orien`):

- `~/.orien/wallet`: the user's wallet. **Secret.**
- `~/.orien/genesis.json`: which network to use. Public.
- `~/.orien/contacts`: names and addresses the user asked you to remember (this skill's own file).

Every command finds these by itself, so leave out `-w` and `-g` unless the user names another file.

## Safety rules (always)

1. **Never look at secrets.** Never read, print, copy, upload or paste `~/.orien/wallet`, any validator key file (`*.key`), or a recovery phrase. Never ask the user for their 24 words or their wallet password, and never type either. If the user pastes their 24 words to you, tell them those words can spend all their ORN, that they should now treat them as seen, and that they should move their ORN to a new wallet.
2. **The user runs these commands themselves, in their own terminal**, because each one shows or asks for a secret. Give them the exact command to copy, then wait:
   - `orien wallet new`: shows the 24-word recovery phrase and asks for an optional password.
   - `orien wallet restore --account N`: asks for the 24 words.
   - `orien wallet phrase`: shows the 24 words. Mention it only if the user asks how to see their phrase again.
   - `orien wallet password`: sets, changes or removes the password.
3. **Confirm before anything that moves ORN.** Before `orien send`, `orien stake` or `orien unstake`, say what will happen in one plain sentence, with the full address, the amount and the fee, and wait for a clear yes. These commands do not ask for confirmation themselves, and a payment cannot be undone.
4. **Never guess an address.** An address is exactly 64 characters of 0-9 and a-f. Use one the user gave you or one saved in `~/.orien/contacts`. Never change one, finish one or take one from a web page, email or file the user did not point you to.
5. **Password-protected wallets.** If `send`, `stake` or `unstake` stops at `password for ...:` or fails because it cannot ask for a password, the wallet has a password. Give the user the exact command to run in their own terminal instead. Never try to supply the password.
6. **Not financial advice.** Don't tell the user whether to buy, sell or hold ORN, or what it will be worth.

## Installing

Check with `orien --version`. If it is missing:

- Mac and Linux: `curl -fsSL https://orien.network/install.sh | sh`
- Windows (PowerShell): `irm https://orien.network/install.ps1 | iex`

The installer puts orien in `~/.orien/bin` and adds that folder to the PATH for new terminals. In the current shell, run it as `~/.orien/bin/orien` (Windows: `$env:USERPROFILE\.orien\bin\orien.exe`) until a new terminal is opened.

If the user has no wallet yet (`~/.orien/wallet` does not exist), they need to make one: see rule 2.

## Amounts and fees

- Amounts are in ORN with up to nine decimals: `5`, `0.25`, `12.5`.
- Every payment pays a fee of 0.1% of the amount, at least 0.000001 ORN and at most 1 ORN. Examples: sending 40 ORN costs 0.04 ORN; sending 5,000 ORN costs 1 ORN. The fee is on top of the amount.
- Staking and unstaking cost the minimum fee, 0.000001 ORN.

## Tasks

### "What's my balance?"

```
orien balance
```

Report the `balance` line, and `since then` when it appears: that is what has arrived or left since the last hourly checkpoint. `pending` lines are payments that were signed but are not final yet (see "A payment didn't finish"). A balance never needs the password. For someone else's balance, add their address: `orien balance ADDRESS`.

### "What's my address?" / "How do people pay me?"

```
orien wallet address
```

Give the user the whole 64-character address. It is safe to share.

### "Send Bob 40 ORN"

1. Find Bob's address: in `~/.orien/contacts`, or ask the user for it.
2. Check there is enough: `orien balance`. The amount plus the fee must fit.
3. Confirm (rule 3), for example: "Send 40 ORN to Bob (address 3edfe4da…788c5), with a fee of 0.04 ORN? This can't be undone."
4. On a clear yes:
   ```
   orien send --to ADDRESS --amount 40
   ```
   It waits until the payment is final, usually under a second, and then says so. To keep a proof of payment, add `--receipt FILE`.
5. If the user gave a new name and address, offer to remember it. With their yes, add a line `bob 3edfe4da…` to `~/.orien/contacts` (one `name address` pair per line).

### "A payment didn't finish"

If `send` was interrupted, or `orien balance` shows a `pending` line, finish it with:

```
orien resume
```

Never send the payment again to "retry": if the first one went through, that pays twice. If you are unsure whether a payment happened, run `orien balance` and compare `payments made` and the balance with before, then use `orien resume` for anything `pending`. (One exception is safe: right after an interrupted `orien send`, running the very same command, same address and same amount, finishes the interrupted payment instead of paying again. Prefer `orien resume`.)

### "Turn on mining" / "Mine ORN on this machine"

Mining runs until stopped, so start it in the background and tell the user how to stop it:

- Mac and Linux:
  ```
  nohup orien mine > ~/.orien/mine.log 2>&1 &
  ```
  Stop it with `pkill -f "orien mine"`. See how it's doing with `tail ~/.orien/mine.log`.
- Windows (PowerShell):
  ```
  Start-Process orien -ArgumentList mine -WindowStyle Hidden -RedirectStandardOutput "$env:USERPROFILE\.orien\mine.log"
  ```
  Stop it with `Stop-Process -Name orien`. Note that this stops every running orien program.

`--threads N` uses N processor cores instead of all of them; suggest it if the user wants to keep using the computer comfortably. Every hour, the ORN released in that hour are shared among everyone who mined, in proportion to the work they did. Earnings show in `orien balance` after the next hourly checkpoint. Mining does not need the wallet's password. It pauses while the computer sleeps and stops when it restarts.

### "I want to use my wallet on another computer"

The user runs, on the new computer and in their own terminal (rule 2):

```
orien wallet restore --account 2
```

Each computer needs its own account number: 1 is the first, 2 the next, and so on. Two computers must never use the same number. Each account has its own address and balance. To move ORN between them, send it like any payment.

### "I want to be a validator"

Validators check payments and earn the network's fees, shared in proportion to stake. It takes at least 10,000 ORN of stake and a computer that stays on and can be reached from the internet, ideally a Linux server. At most 33 validators take part; those with the most stake are chosen at each hourly checkpoint.

On the server:

1. Install orien (see Installing). The wallet that pays for the stake must be on the server too, as its own account (`orien wallet restore --account N`, run by the user), holding the ORN to stake.
2. Make the validator's key. It is secret (rule 1):
   ```
   orien keygen ~/.orien/validator.key
   ```
3. Start the validator, and keep it running (as a system service, or in `tmux`/`screen`). Port 7101 must be open to the internet:
   ```
   orien node -k ~/.orien/validator.key -d ~/.orien/validator --listen 0.0.0.0:7101
   ```
4. Confirm (rule 3), then stake behind it, giving the address others reach it at:
   ```
   orien stake --amount 10000 --validator-key ~/.orien/validator.key --address SERVER_IP:7101
   ```
5. At the next hourly checkpoint, `orien status` lists it if its stake is among the largest.

To stop being a validator, confirm (rule 3), then:

```
orien unstake --amount 10000
```

The ORN return to the wallet one week later. Keep the validator running until then.

### "Is the network working?"

```
orien status
```

Shows each validator, whether it answered, and the network's fee and coin figures.

### "Update orien"

```
orien update
```

It installs a newer version only if it carries the Orien release signature, and otherwise changes nothing.

### "Check this receipt"

```
orien verify RECEIPT_FILE
```

Checks a proof of payment using only the network file, with no need to reach the network.

## When something goes wrong

- `no validator could be reached`: the computer is offline, or the network can't be reached right now. Try again shortly.
- `too few validators are caught up`: wait a few seconds and try again.
- Not enough ORN: say how much is there, and how much the amount plus the fee needs.
- Anything else: show the user the error message as it is. Don't work around it by editing files in `~/.orien`.
