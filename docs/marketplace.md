# The Orien marketplace

A place on the website, at orien.network/marketplace, where people offer
work and things for ORN, and find them.

## In plain words

1. Maya signs in with a passkey (Face ID, a fingerprint, or her computer's
   own lock), so there is no password to remember and none to leak.
2. She posts a listing: "Logo design, 200 ORN", a description, and her Orien
   address, which is where she gets paid.
3. Sam browses, finds her listing and agrees the details with her.
4. Sam pays from his own wallet: the listing shows the amount and address,
   ready to copy, and later a button that opens Orien Wallet with the
   payment filled in.
5. Afterwards Sam leaves a review, so the next buyer can see Maya's record.

The marketplace never holds anyone's coins. It is a noticeboard with
accounts and reviews; every payment goes straight from one wallet to
another, so the site cannot lose or take anyone's money.

Nobody has to prove they own the address on their listing. Putting someone
else's address there only sends buyers' payments to its real owner, so
there is nothing to gain. Reviews belong to the marketplace account, not
the address, so a track record cannot be borrowed either.

## How it is built

- **`orien-market`**, a small server in this repository
  (`crates/market`). It keeps accounts, passkeys, listings and reviews in
  one SQLite file, and answers the website's pages at
  `orien.network/api/market/`. On cyphers-vps, behind nginx, like the site.
- **The pages** are part of the website (`web/src/app/marketplace`). The
  site stays static; the pages ask the server for listings as they load.
- **Passkeys** follow the WebAuthn standard (the `webauthn-rs` crate): the
  server keeps each account's public key only.

## Verified wallets

Before their first listing, a seller proves they hold the Orien wallet
buyers will pay: they send a small, exact amount (under 0.01 ORN) from it to
the marketplace's own address, and the marketplace sees it in the
validators' history within seconds. Listings are always paid to that
wallet, so a mistyped address can never send buyers' money astray.

The marketplace's own wallet is
`74fa1ab82c45ebeed539b03146243ddfc6f0ccb5163f45ea87d21692ba774b7a`. Its file
is kept on the founder's Mac, never on the server: the server only reads
what came in.

## The swap board

Swaps of ORN for Bitcoin run in Orien Wallet, which holds the keys and
watches the Bitcoin network; the website never touches either. What the
website adds, at orien.network/swap, is a place to find someone to swap
with, using the marketplace's accounts.

1. **An offer.** A seller with a verified wallet posts "500 ORN for 0.01
   BTC". Nothing is locked yet.
2. **Taking it.** A buyer, signed in and with a verified wallet too,
   presses Take in Orien Wallet. The offer is held for them alone for 30
   minutes.
3. **The codes pass through the board.** The seller's wallet sees the
   taker, locks the ORN for them and posts its offer code; the buyer's
   wallet reads it, pays the Bitcoin and posts its reply code; the seller's
   wallet reads that and the swap runs as it does today. Nobody copies
   anything. The board only carries the codes: it cannot take or move
   anyone's coins, and a dishonest board can only waste time.
4. **When the seller's wallet is not open**, nothing happens until it is;
   the hold lapses and the offer reopens.

The one cost a stranger can impose is a seller's ORN locked for 48 hours by
a buyer who takes and never pays. So: only verified wallets take offers, an
account holds one offer at a time, and an account that lets takes lapse
repeatedly is stopped from taking for a while. Profiles show completed
swaps, so sellers can see who has a record.

Swap offers launch with the marketplace.

## In pieces

1. The server: accounts with passkey sign-in, and listings to post, edit,
   remove and browse. Tested on its own.
2. The pages: browse, a listing, sign in, post and edit your own.
3. Paying and reviews: the amount and address to copy, and reviews by
   buyers. Then an `orien:` link that opens Orien Wallet with the payment
   filled in (a change to the app).
4. Live on cyphers-vps, with the founder's yes for what gets installed.

Later: messages between buyer and seller (spam and abuse make them the
hardest part to run well, so at first a listing says how to reach the
seller), pictures, reporting listings and removing them, and Bitcoin trades
as listings, using swaps.
