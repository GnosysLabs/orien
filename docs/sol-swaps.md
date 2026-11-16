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

The seller sells ORN for SOL; the buyer pays SOL.

1. **The buyer takes an offer** on the swap board, in Orien Wallet, giving
   their Orien address (for the ORN) as today.
2. **The seller's wallet locks the ORN** for the buyer on Orien behind a new
   secret's hash, for `ORN_EPOCHS_SOL` = 3 epochs, and posts the offer code.
   For SOL it carries the seller's SOL address (to be paid at) in place of
   the Bitcoin key.
3. **The buyer's wallet checks the ORN lock** (proved, as for Bitcoin, with at
   least 2 epochs left) and locks the SOL: amount = the price, seller = the
   offer's SOL address, buyer = the buyer's own SOL address, deadline = now
   plus `SOL_LOCK_SECONDS` = 1 hour, which must end at least one epoch before
   the ORN lock. It posts the reply code: the lock account's address.
4. **The seller's wallet checks the SOL lock** on Solana at `finalized`
   commitment: owned by the program, state 1, the right hash, seller address
   and amount, and a deadline that ends before the ORN lock with the margin.
   It then claims, revealing the secret, while at least `SOL_SAFETY_SECONDS`
   = 10 minutes remain.
5. **The ORN reach the buyer**: the seller's wallet delivers them with the
   secret (open claims), and the buyer's wallet, reading the secret from the
   claim transaction, claims them too; whichever comes first pays.
6. **If anyone stops**: no claim by the deadline means the buyer's wallet
   sends `Refund` and gets the SOL back; the ORN lock ends later and the ORN
   go back to the seller.

Reading Solana uses three RPC services and needs two of them to agree, as
the Bitcoin side now does, with the same rest-on-429 back-off.

## The smallest swap

0.15 SOL, about $10 when set, matching the 0.00015 BTC Bitcoin minimum, and
far above the ~0.0009 SOL an unused Solana address needs to exist.

## Who builds what

- **Network and website thread**: `orien-lock` (done, tested in LiteSVM and on
  devnet); the swap crate's Solana side (`crates/swap/src/sol.rs`: offer and
  reply codes, the RPC quorum, the seller's and buyer's steps, tests); the
  swap board's coin and price; the website, with SOL first and Bitcoin
  second.
- **Apps thread**: a SOL address in Orien Wallet, from the recovery phrase,
  with its balance and send; the SOL swap screens, SOL first and "Swap with
  Bitcoin instead" second, with approve-at-posting as for Bitcoin.

## The swap board

Offers get a `coin`, `sol` or `btc`, and a `price` in that coin's smallest
unit (lamports or satoshis). Existing offers are `btc` with their `sats`.
Takes from wallets that can't swap SOL are refused for SOL offers.
