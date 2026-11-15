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

## Sellers' wallets

Listings are always paid to the wallet linked to the seller's account.
Nothing has to be proven: naming someone else's wallet only sends that
person the buyers' payments, so there is nothing to gain from it, and any
number of accounts may name the same wallet (nobody can block its owner).

- Signing in from Orien Wallet links the app's own address, for free. It
  cannot be mistyped and may be brand new.
- An address pasted on the website is accepted once the network shows it
  has held ORN before. A mistyped address never has, so this catches typos.

Until 2026-11-15 sellers proved their wallet by paying a tiny exact amount
to the marketplace's own wallet,
`74fa1ab82c45ebeed539b03146243ddfc6f0ccb5163f45ea87d21692ba774b7a`. Its file
is kept on the founder's Mac; nothing uses it now.

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

## Launch

The marketplace, the swap board and the board in Orien Wallet go live
together, with the founder's yes for what is installed:

1. **Build** `orien-market` for Linux (`cargo zigbuild --release --target
   x86_64-unknown-linux-gnu.2.31 -p orien-market`).
2. **On cyphers-vps**, nothing touching the validators:
   - a system user `orien-market`, and `/var/lib/orien-market` for its
     database and pictures;
   - `/usr/local/bin/orien-market`;
   - `/etc/orien-market/` holding the Resend and OpenRouter keys (copied from
     `~/.config/orien/` without being shown) and a copy of the genesis file,
     readable by `orien-market` only;
   - the service `deploy/orien-market.service`, enabled and started;
   - nginx: `deploy/nginx-orien-market-limits.conf` into `conf.d`, the two
     `orien-market*.conf` snippets into `snippets`, and one `include` line
     in the HTTPS server block; `nginx -t`, then a reload.
3. **Check** from outside: the board answers at
   `https://orien.network/api/market/swaps`, and a sign-in code arrives.
4. **Publish the site** with `MARKETPLACE=1 bash deploy/site.sh`: the pages,
   the Marketplace link in the menus and the Privacy link in the footer.
5. **Release Orien Wallet** with the board (the wallet session), and add
   changelog entries for both.
6. **Push** to GitHub.

Undoing it: stop the service and publish the site without `MARKETPLACE=1`.

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

## The admin dashboard

https://cyphers-vps.yakalo-lizard.ts.net:8448/admin/, from any device on the
founder's tailnet. It shows reports (with take down, suspend and "dealt
with"), the validators and the epoch, mining difficulty, signing keys left,
marketplace and swap numbers, account search, and the server's disk and
services.

- `orien-market` serves it on 127.0.0.1:7201 (`--admin-listen`), with the
  page from the published site (`--admin-site`).
- `tailscale serve --bg --https=8448 http://127.0.0.1:7201` puts it on the
  tailnet only. Tailscale adds the visitor's login as Tailscale-User-Login;
  a release build refuses any request without it. `--admin-logins` can
  narrow it to named logins.
- The public site answers 404 for /admin (deploy/nginx-orien-market.conf),
  and never forwards to port 7201.
