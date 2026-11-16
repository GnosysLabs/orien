# Swapping ORN for SOL

SOL is the main way to swap ORN; Bitcoin stays as a second option for people
who hold Bitcoin. A SOL swap works like a Bitcoin swap, with the Bitcoin
script replaced by `orien-lock`, a 4,016-byte program on Solana
(`programs/orien-lock`), and it finishes in about a minute.

## The program

`orien-lock` holds SOL in a 113-byte lock account until someone shows the
secret behind its SHA-256 hash (the same hash Orien's locks use), which pays
the seller and returns the account's deposit to the buyer, or until its
deadline, a Unix time, after which everything goes back to the buyer. Anyone
may send a claim or a refund; the coins only ever go to the two addresses
written in the lock. Instructions, by first byte:

| | Accounts | Data |
|---|---|---|
| 0 Lock | lock (writable, signer) | hash 32, seller 32, buyer 32, deadline i64 LE, amount u64 LE |
| 1 Claim | lock, seller, buyer (all writable) | secret 32; before the deadline only |
| 2 Refund | lock, buyer (writable) | none; at or after the deadline |

The buyer's wallet creates the lock account in the same transaction as
`Lock`: System Program `create_account` from the buyer, at a fresh keypair's
address, 113 bytes, owned by the program, funded with amount plus the
rent-exempt deposit. The secret appears in the claim transaction's data.

Devnet: `99zcMPAgZwXxoLDwor424zro9BGMATTsMk8djqov2qKf` (upgradeable, for
testing). Mainnet: deployed after review at exactly its size, then made
final so nobody can change it.

## A swap

The seller sells ORN for SOL and is paid at their own wallet. The buyer pays
from their own wallet (Phantom or any other), not from Orien Wallet, which
keeps no SOL balance. The steps are `orien_swap::sol::SolSwap`
(`crates/swap/src/sol.rs`); `step()` moves a swap as far as it can go now.

1. **The buyer takes an offer** on the swap board, in Orien Wallet, giving
   their Orien address (for the ORN).
2. **The seller's wallet locks the ORN** for the buyer behind a new secret's
   hash, for `ORN_EPOCHS` = 4 epochs, and posts the offer code
   (`orien-sol-offer:`), which carries the seller's payout address and the
   seller's fee address (below).
3. **The buyer's wallet checks the ORN lock**, proved as for Bitcoin, with at
   least `SOL_LOCK_SECONDS` + `MARGIN_SECONDS` (two hours) surely left. It
   makes a one-time address and a lock address for this swap, and posts the
   reply code at once (`orien-sol-reply:`): the lock address and the buyer's
   own wallet (the refund address, which must have held SOL before: Solana
   refuses tiny amounts to brand-new addresses). It shows "send exactly
   `to_pay` SOL" to `pay_address()`, with `pay_link()`, a `solana:` link for a
   QR code.
4. **When the SOL arrive**, the buyer's wallet locks them from the one-time
   address in one transaction: `create_account` and `Lock` (amount = the
   price, seller = the payout address, buyer = the refund address, deadline =
   Solana's clock plus `SOL_LOCK_SECONDS` = 1 hour), plus the seller's fee
   top-up. It keeps the rent minimum and two fees at the one-time address to
   pay for a refund and the final sweep.
5. **The seller's wallet checks the SOL lock**, finalized, through most of the
   services: the right hash, paying the payout address at least the price,
   at least `SAFETY_SECONDS` (10 minutes) left, and ending at least
   `MARGIN_SECONDS` (1 hour) before the ORN lock surely does. Then it claims,
   paying the fee from its fee key, which reveals the secret.
6. **The ORN reach the buyer**: the buyer's wallet reads the secret from the
   claim transaction and claims them; the seller's wallet claims them for the
   buyer too, `DELIVER_AFTER_SECONDS` (5 minutes) after the SOL were final;
   whichever comes first pays.
7. **Leftovers go home**: whatever is left at the one-time address goes to
   the refund address, leaving it at exactly nothing.
8. **If anyone stops**: SOL sent too late (less than two hours surely left on
   the ORN) goes straight back; a lock nobody claims by its deadline is
   refunded to the buyer's wallet; and the ORN go back to the seller when
   their lock ends.

### Fees, and why the seller has a fee key

Solana needs every transaction paid by an account that signs it and already
holds SOL. The seller's claim can't be paid by the lock account (the program
owns it), the buyer's one-time key, or the seller's payout wallet (it's in
Phantom). So the seller's wallet keeps a small fee key, its
`solana_secret()`, whose address goes in the offer. The buyer's lock tops it
up to the rent minimum plus `FEE_KEY_FEES` (20) fees when it holds less than
the minimum plus 5 fees: about 0.00099 SOL once per seller, then nothing for
a while. Each claim costs `FEE`, 5,000 lamports.

`to_pay` = the price + the lock's deposit (`rent(113)`, back to the buyer at
the claim or refund) + the one-time address's reserve (`rent(0)`, back at the
sweep) + three fees + the seller's top-up + `CUSHION` (10 fees, back at the
sweep). A completed swap costs the buyer the price, the top-up and four fees;
a refunded one, the top-up and five fees.

Every account must end a transaction holding nothing or at least the rent
minimum. The one-time address keeps its reserve until the sweep, which
empties it exactly; the refund address must already exist.

## Reading and writing Solana

`crates/swap/src/solana.rs`, shared by the swap logic and Orien Wallet:

- **Reading** goes through several RPC services, as the Bitcoin explorers do:
  answers reused for 10 seconds, a busy (429) or failing (5xx) service left
  alone for a while, and anything that decides money taken only when most
  services answer, using the least favourable answer. On the main network
  these are Solana's own public service and PublicNode, the two that answer
  with no account; a third that needs a free account is the next step.
- **Writing**: legacy transactions are built and signed in that file, byte by
  byte (a transfer, a new account, and orien-lock's Lock, Claim and Refund),
  so neither the wallet nor the iPhone app needs Solana's own libraries. A
  live test on devnet (`cargo test -p orien-swap solana_devnet -- --ignored`)
  funds, locks, has a wrong secret refused, claims, and reads the secret back.

## The seller's fee key

Orien Wallet derives each account's Solana key from that account's seed, as
it does its vault keys: `blake3::derive_key("orien wallet solana 2026-11-16
ed25519", seed)`, through `Wallet::solana_secret()` in `crates/net/src/keys.rs`.
It comes back with the 24 words, and it's only used as the seller's fee key.
Orien's phrase is not BIP39 (its check is BLAKE3, on purpose), so the key is
not one Phantom would show for the same words.

## The smallest swap

0.01 SOL (`MIN_LAMPORTS`). Solana's fees are fractions of a cent, so unlike
Bitcoin there's no need for a large minimum; the only real floor is that a
payout address that has never held SOL needs at least the rent minimum,
about 0.00089 SOL, to exist, and 0.01 SOL is comfortably above it.

## Who builds what

- **Network and website thread**: `orien-lock` (done, tested in LiteSVM and on
  devnet); `crates/swap/src/solana.rs` and `sol.rs` (done: the services, the
  transactions, the offer and reply codes, the seller's and buyer's steps,
  whole-swap tests against a pretend Solana that runs the real transactions);
  the swap board's coin and price; the website, with SOL first and Bitcoin
  second.
- **Apps thread**: the SOL swap screens, SOL first and "Swap with Bitcoin
  instead" second: the payout address when posting, the refund address and
  the pay box with its QR code when buying, approve-at-posting as for
  Bitcoin; and `solana_secret()` for the seller's fee key.

## The swap board

Offers get a `coin`, `sol` or `btc`, and a `price` in that coin's smallest
unit (lamports or satoshis). Existing offers are `btc` with their `sats`.
Takes from wallets that can't swap SOL are refused for SOL offers.
