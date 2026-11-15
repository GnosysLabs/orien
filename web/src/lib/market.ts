// Talking to the marketplace server, and passkeys in the browser.

export type Account = {
  id: number
  name: string
  bio: string
  picture: string | null
  /** A wide picture across the top of the profile. */
  banner?: string | null
  wallet: string | null
  contact: Contact
  created: number
}

export type Contact = { email: string; telegram: string; x: string }

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
  listing: number
  listing_title: string
  price: number
  unit: string
  pay_to: string
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
  sats: number
  state: string
  created: number
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
    throw new MarketError(data.error ?? 'The marketplace is not answering right now. Try again in a moment.', response.status)
  }
  return data as T
}

/** The signed-in account and its listings, or null when signed out. */
export async function me(): Promise<{ account: Account; listings: Listing[]; unread: number } | null> {
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

// Passkeys. The server speaks WebAuthn's JSON, with binary fields in
// base64url; the browser's API wants them as bytes.

const toBytes = (text: string) => {
  const base64 = text.replace(/-/g, '+').replace(/_/g, '/').padEnd(Math.ceil(text.length / 4) * 4, '=')
  return Uint8Array.from(atob(base64), (c) => c.charCodeAt(0))
}

const toText = (buffer: ArrayBuffer | null) =>
  buffer === null
    ? null
    : btoa(String.fromCharCode(...new Uint8Array(buffer)))
        .replace(/\+/g, '-')
        .replace(/\//g, '_')
        .replace(/=+$/, '')

type Json = Record<string, any> // eslint-disable-line @typescript-eslint/no-explicit-any

export function passkeysWork(): boolean {
  return typeof window !== 'undefined' && !!window.PublicKeyCredential
}

/** Makes a passkey for a new account, as the server's options describe. */
export async function createPasskey(options: Json): Promise<Json> {
  const key = options.publicKey
  const publicKey: PublicKeyCredentialCreationOptions = {
    ...key,
    challenge: toBytes(key.challenge),
    user: { ...key.user, id: toBytes(key.user.id) },
    excludeCredentials: (key.excludeCredentials ?? []).map((c: Json) => ({ ...c, id: toBytes(c.id) })),
  }
  const credential = (await navigator.credentials.create({ publicKey })) as PublicKeyCredential | null
  if (!credential) throw new Error('No passkey was made.')
  const response = credential.response as AuthenticatorAttestationResponse
  return {
    id: credential.id,
    rawId: toText(credential.rawId),
    type: credential.type,
    extensions: credential.getClientExtensionResults(),
    response: {
      attestationObject: toText(response.attestationObject),
      clientDataJSON: toText(response.clientDataJSON),
      transports: response.getTransports?.() ?? [],
    },
  }
}

/** Signs in with any passkey this device holds for the marketplace. */
export async function usePasskey(options: Json): Promise<Json> {
  const key = options.publicKey
  const publicKey: PublicKeyCredentialRequestOptions = {
    ...key,
    challenge: toBytes(key.challenge),
    allowCredentials: (key.allowCredentials ?? []).map((c: Json) => ({ ...c, id: toBytes(c.id) })),
  }
  const credential = (await navigator.credentials.get({ publicKey, mediation: options.mediation })) as PublicKeyCredential | null
  if (!credential) throw new Error('No passkey was chosen.')
  const response = credential.response as AuthenticatorAssertionResponse
  return {
    id: credential.id,
    rawId: toText(credential.rawId),
    type: credential.type,
    extensions: credential.getClientExtensionResults(),
    response: {
      authenticatorData: toText(response.authenticatorData),
      clientDataJSON: toText(response.clientDataJSON),
      signature: toText(response.signature),
      userHandle: toText(response.userHandle),
    },
  }
}

/** What a failed passkey step means, in plain words. */
export function passkeyTrouble(error: unknown): string {
  if (error instanceof MarketError) return error.message
  if (error instanceof DOMException && error.name === 'NotAllowedError') {
    return 'The passkey step was cancelled or timed out. Try again when you are ready.'
  }
  return error instanceof Error ? error.message : 'Something went wrong with the passkey.'
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
