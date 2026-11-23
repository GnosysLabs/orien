export const SITE_NAME = 'Orien'
export const SITE_URL = 'https://orien.network'
export const TAGLINE = 'The cryptocurrency that settles in a blink, built for the quantum age'
export const SITE_DESCRIPTION =
  'Orien is a new cryptocurrency: payments are final in a fraction of a second, never wait in line, and use signatures built to stay safe from quantum computers. Zero coins at launch; every coin is earned.'
/** Where the Orien community talks. */
export const DISCORD = 'https://discord.gg/5FFRZAw3u9'
/** Orien on X. */
export const X_ACCOUNT = 'https://x.com/OrienNetwork'
export const YOUTUBE = 'https://www.youtube.com/@OrienNetwork'
/** The introduction video on YouTube, the same as the one on the home page. */
export const INTRODUCTION_VIDEO = 'https://www.youtube.com/watch?v=ddp3STGELd4'
/** Orien's source code, public on GitHub. */
export const SOURCE_CODE = 'https://github.com/GnosysLabs/orien'
/** Orien Wallet for iPhone's public beta, through Apple's TestFlight. */
export const IPHONE_BETA = 'https://testflight.apple.com/join/butuQ4Ms'
export const OPERATOR = 'Gnosys Labs LLC'
export const OPERATOR_URL = 'https://gnosyslabs.xyz'

/** Whether this build of the site includes the marketplace, the swap board
 * and the privacy page, which need the marketplace's server. Set by
 * `MARKETPLACE=1 bash deploy/site.sh`. */
export const MARKETPLACE_LIVE = process.env.NEXT_PUBLIC_MARKETPLACE === '1'

/** The first epoch whose mining shares RandomX judges. Set with the release
 * that switches; the network code's LIVE_RANDOMX_FROM is the same number. */
export const RANDOMX_FROM = 21

/** The first epoch in which anyone may claim a locked payment for its payee.
 * The network code's LIVE_OPEN_CLAIMS_FROM is the same number. */
export const OPEN_CLAIMS_FROM = 23

/** The first epoch that aims for 10,000 shares an hour instead of 1,000.
 * The network code's LIVE_MORE_SHARES_FROM is the same number. */
export const MORE_SHARES_FROM = 112

/** The first epoch that aims for 50,000 shares an hour instead of 10,000.
 * The network code's LIVE_MOST_SHARES_FROM is the same number. */
export const MOST_SHARES_FROM = 123

/** Orien Wallet's extension in the Chrome Web Store. */
export const CHROME_EXTENSION = 'https://chromewebstore.google.com/detail/orien-wallet/oogifphigefggbdlcomcohbjfbgomcmn'

/** Whether everyone sees Sign in with Orien Wallet. Until Orien Wallet's
 * release with it, only people who already have its extension do. */
export const WALLET_SIGN_IN_FOR_EVERYONE = true

/** Whether the Sign in with Orien Wallet developer docs (/developers/sign-in/
 * and its demo) are shown and linked. Off until the Chrome and Firefox
 * extensions that can do it are approved in their stores. To show them
 * again: set this to true, and copy apps/wallet/signin-js/demo.html to
 * public/developers/sign-in/demo/index.html with its script src set to
 * "/orien-sign-in.js". */
export const WALLET_SIGN_IN_DOCS = false

/** Whether the community page is open and linked. Until then it says it opens soon. */
export const COMMUNITY_LIVE = true
