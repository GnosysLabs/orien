export const SITE_NAME = 'Orien'
export const SITE_URL = 'https://orien.network'
export const TAGLINE = 'The cryptocurrency that settles in a blink, built for the quantum age'
export const SITE_DESCRIPTION =
  'Orien is a new cryptocurrency: payments are final in a fraction of a second, never wait in line, and use signatures built to stay safe from quantum computers. Zero coins at launch; every coin is earned.'
export const OPERATOR = 'GnosysLabs LLC'
export const OPERATOR_URL = 'https://gnosyslabs.xyz'

/** Whether this build of the site includes the marketplace, the swap board
 * and the privacy page, which need the marketplace's server. Set by
 * `MARKETPLACE=1 bash deploy/site.sh`. */
export const MARKETPLACE_LIVE = process.env.NEXT_PUBLIC_MARKETPLACE === '1'

/** The first epoch whose mining shares RandomX judges. Set with the release
 * that switches; the network code's LIVE_RANDOMX_FROM is the same number. */
export const RANDOMX_FROM = 0
