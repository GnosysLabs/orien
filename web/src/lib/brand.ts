export const SITE_NAME = 'Orien'
export const SITE_URL = 'https://orien.network'
export const TAGLINE = 'The cryptocurrency that settles in a blink, built for the quantum age'
export const SITE_DESCRIPTION =
  'Orien is a new cryptocurrency: payments are final in a fraction of a second, never wait in line, and use signatures built to stay safe from quantum computers. Zero coins at launch; every coin is earned.'
/** Where the Orien community talks. */
export const DISCORD = 'https://discord.gg/5FFRZAw3u9'
/** Orien on X. */
export const X_ACCOUNT = 'https://x.com/OrienNetwork'
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
