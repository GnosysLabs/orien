// Talking to the marketplace server.

export type Account = {
  id: number
  name: string
  bio: string
  picture: string | null
  /** A wide picture across the top of the profile. */
  banner?: string | null
  wallet: string | null
  /** Whether that wallet proved it is theirs, by a small payment. */
  wallet_verified?: boolean
  contact: Contact
  created: number
  /** Who may start a direct conversation with them. Messages about their
   * listings always come through. */
  messages_from?: MessagesFrom
}

export type MessagesFrom = 'everyone' | 'talked' | 'nobody'

export type Contact = { telegram: string; x: string; discord: string }

/** A wallet its owner proved is theirs, by its name in their Orien Wallet. */
export type VerifiedWallet = { address: string; name: string | null; solana_devnet?: string }

/** What verifying a wallet takes: `amount` from `address` to `to`. */
export type WalletProof = {
  solana_devnet?: string
  address: string
  /** Its name in your Orien Wallet, if an app has said. */
  name: string | null
  amount: number
  orn: string
  to: string
  verified: boolean
}

export type Message = {
  id: number
  sender: number
  body: string
  kind: 'text' | 'request' | 'paid'
  amount: number | null
  created: number
}

export type Conversation = {
  id: number
  /** Null in a direct conversation, which is about no listing. */
  listing: number | null
  listing_title: string | null
  price: number | null
  unit: string | null
  pay_to: string | null
  with: number
  with_name: string
  with_picture: string | null
  buying: boolean
  last: Message | null
  unread: boolean
  updated: number
  blocked: boolean
}

export type SwapOffer = {
  id: number
  seller: number
  seller_name: string
  seller_picture: string | null
  seller_done: number
  orn: number
  /** What the buyer pays with. */
  coin: 'sol' | 'btc'
  /** The price of a Bitcoin offer; 0 for a SOL one. */
  sats: number
  /** The price of a SOL offer; 0 for a Bitcoin one. */
  lamports: number
  state: string
  created: number
  /** Whether the maker's Orien Wallet is open: only then can the offer be
   * taken. Missing on older servers: counted as open. */
  seller_online?: boolean
  /** "sell" (offering ORN for SOL/BTC) or "buy" (offering SOL/BTC for ORN). */
  side?: 'sell' | 'buy'
}

/** Lamports as SOL, at most four decimals: 250000000 → "0.25". */
export function formatSol(lamports: number): string {
  const sol = lamports / 1_000_000_000
  return sol.toLocaleString('en', { maximumFractionDigits: sol < 0.01 ? 6 : 4 })
}

/** Satoshis as BTC: 1000000 → "0.01". */
export function formatBtc(sats: number): string {
  const whole = Math.floor(sats / 100_000_000)
  const fraction = String(sats % 100_000_000).padStart(8, '0').replace(/0+$/, '')
  return fraction ? `${whole}.${fraction}` : String(whole)
}

/** "0.01" or ".01" → 1000000 satoshis, or null if it is not an amount of BTC. */
export function parseBtc(text: string): number | null {
  const match = text.trim().replace(/,/g, '').match(/^(\d*)(?:\.(\d{1,8}))?$/)
  if (!match || (!match[1] && !match[2])) return null
  const sats = Number(match[1] || '0') * 100_000_000 + Number((match[2] ?? '').padEnd(8, '0'))
  return sats > 0 && Number.isSafeInteger(sats) ? sats : null
}

/** "just now", "5 min ago", "3 h ago", or a date. */
export function ago(seconds: number): string {
  const gone = Date.now() / 1000 - seconds
  if (gone < 60) return 'just now'
  if (gone < 3600) return `${Math.floor(gone / 60)} min ago`
  if (gone < 86400) return `${Math.floor(gone / 3600)} h ago`
  return new Date(seconds * 1000).toLocaleDateString('en', { day: 'numeric', month: 'short' })
}

/** An error message from the server as a sentence. */
export const sentence = (text: string) => text.charAt(0).toUpperCase() + text.slice(1) + (text.endsWith('.') ? '' : '.')

export const pictureUrl = (name: string) => `/api/market/images/${name}`

/** A profile's page: /u/Cyph3r/. */
export const profileHref = (name: string) => `/u/${encodeURIComponent(name)}/`

/** The rule for names, as the server keeps it: a username. */
export const NAME_RULE = '3 to 20 letters, numbers or underscores, with no spaces'
export const NAME_PATTERN = '[A-Za-z0-9_]{3,20}'

/** "Member since March 2026". */
export const memberSince = (seconds: number) =>
  `Member since ${new Date(seconds * 1000).toLocaleDateString('en', { month: 'long', year: 'numeric' })}`

export type Listing = {
  id: number
  seller: number
  seller_name: string
  seller_picture: string | null
  title: string
  description: string
  category: string
  price: number
  unit: string
  pay_to: string
  created: number
  updated: number
  open: boolean
  pictures: string[]
}

export type Draft = {
  title: string
  description: string
  category: string
  price: number
  unit: string
  pay_to: string
}

export const CATEGORIES: { id: string; label: string }[] = [
  { id: 'design', label: 'Design' },
  { id: 'software', label: 'Software' },
  { id: 'teaching', label: 'Teaching' },
  { id: 'goods', label: 'Goods' },
  { id: 'other', label: 'Other' },
]

export const categoryLabel = (id: string) => CATEGORIES.find((c) => c.id === id)?.label ?? id

export class MarketError extends Error {
  constructor(
    message: string,
    public status: number,
  ) {
    super(message)
  }
}

export async function ask<T>(path: string, method = 'GET', body?: unknown): Promise<T> {
  const response = await fetch(`/api/market${path}`, {
    method,
    credentials: 'same-origin',
    headers: body === undefined ? {} : { 'Content-Type': 'application/json' },
    body: body === undefined ? undefined : JSON.stringify(body),
  })
  const data = await response.json().catch(() => ({}))
  if (!response.ok) {
    throw new MarketError(data.error ?? 'Orien Network is not answering right now. Try again in a moment.', response.status)
  }
  return data as T
}

/** The signed-in account and its listings, or null when signed out. */
/** Signs this browser in with a pass from Orien Wallet, if its Safari
 * extension put one in the address's fragment (#pass=…). The pass is taken
 * out of the address at once; it works once, for a minute. True when it
 * signed this browser in. */
export async function takeWalletPass(): Promise<boolean> {
  const pass = new URLSearchParams(window.location.hash.slice(1)).get('pass')
  if (!pass) return false
  history.replaceState(null, '', window.location.pathname + window.location.search)
  return ask('/oidc/pass/use', 'POST', { pass }).then(
    () => true,
    () => false,
  )
}

export async function me(): Promise<{ account: Account; listings: Listing[]; unread: number; notices?: number } | null> {
  try {
    return await ask('/me')
  } catch (error) {
    if (error instanceof MarketError && error.status === 401) return null
    throw error
  }
}

// ORN has nine decimals.
const UNITS = BigInt(1_000_000_000)

/** 200000000000 → "200", 1500000000 → "1.5". */
export function formatOrn(units: number): string {
  const value = BigInt(units)
  const whole = value / UNITS
  const fraction = (value % UNITS).toString().padStart(9, '0').replace(/0+$/, '')
  const grouped = whole.toString().replace(/\B(?=(\d{3})+(?!\d))/g, ',')
  return fraction ? `${grouped}.${fraction}` : grouped
}

/** The same amount as `orien send --amount` takes it: no grouping. */
export function plainOrn(units: number): string {
  return formatOrn(units).replace(/,/g, '')
}

/** "1.5" or ".5" → units, or null if it is not an amount of ORN. */
export function parseOrn(text: string): number | null {
  const match = text.trim().replace(/,/g, '').match(/^(\d*)(?:\.(\d{1,9}))?$/)
  if (!match || (!match[1] && !match[2])) return null
  const units = BigInt(match[1] || '0') * UNITS + BigInt((match[2] ?? '').padEnd(9, '0'))
  if (units <= BigInt(0) || units > BigInt(Number.MAX_SAFE_INTEGER)) return null
  return Number(units)
}

/** How a listing can be priced: empty for a fixed price. */
export const PRICINGS = ['', 'per hour', 'per day', 'per week', 'per month', 'per item']

/** A listing's unit as it reads after the price: "hour" → "per hour",
 * "/hr" stays as written, empty stays empty. */
export function perUnit(unit: string): string {
  const text = unit.trim()
  if (!text) return ''
  return /^(per\b|\/|each\b|a\b|an\b)/i.test(text) ? text : `per ${text}`
}
