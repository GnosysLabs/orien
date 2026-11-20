// What the explorer pages read from the server. Amounts are in ORN's
// smallest units, sent as text so large figures stay exact.

export type Checkpoint = {
  epoch: number
  time: number
  payments: number
  released: string
  agreed: number[]
  of: number
  round: number
  state: string
  digest: string
}

export type Validator = {
  seat: number
  name: string
  up: boolean
  /** The hour (epoch) it is working on. */
  epoch: number
  /** Of the latest checkpoints, how many it holds too. */
  agreed: number
  of: number
  stake: string
  account: string | null
}

export type Overview = {
  epoch: number
  epoch_started: number
  epoch_seconds: number
  mining: string
  validators: Validator[]
  /** Wallets paid for mining at checkpoint `epoch`, and at least once in the
   * `hours` checkpoints up to it. */
  miners: { epoch: number; last_hour: number; last_day: number; hours: number } | null
  /** The network's mining speed, estimated: tries a second over `hours`
   * hours up to checkpoint `epoch`. */
  hashrate: { epoch: number; per_second: number; hours: number } | null
  supply: {
    cap: string
    released: string
    per_hour: string
    in_wallets: string
    staked: string
    stakers: number
    holding: number
    holding_one: number
    burned: string
  } | null
  checkpoints: Checkpoint[]
}

export type Entry = {
  epoch: number
  time: number | null
  kind: 'received' | 'sent' | 'mined' | 'fees' | 'claimed' | 'refunded' | 'unstaked'
  amount: string
  counterparty: string | null
}

export type AddressInfo = {
  address: string
  balance: string
  staked: string
  returning: string
  payments_made: number
  activity: Entry[]
  more: boolean
  since: number
  since_time: number | null
}

export type CheckpointInfo = {
  checkpoint: Checkpoint
  previous: string
  validators: { seat: number; stake: string; account: string | null; agreed: boolean }[]
  latest: number
  rewards?: Rewards
}

/** Who earned what at a checkpoint: mining rewards by work shares, and fee
 * shares to stakers. Amounts in units. */
export type Rewards = {
  /** Whether validators' records cover this checkpoint. */
  known: boolean
  /** The first checkpoint they cover. */
  since: number | null
  math: string | null
  /** How much harder mining got for the next hour (negative: easier). */
  harder: number | null
  mined: string
  miners: { address: string; amount: string; part: number }[]
  fees: string
  stakers: { address: string; amount: string }[]
}

export type WalletProfile = { id: number; name: string; picture: string | null; wallet_name?: string | null }

/** "Chris – Savings", or "Chris": as Orien Wallet names people it pays. */
export const label = (profile: { name: string; wallet_name?: string | null }) =>
  profile.wallet_name ? `${profile.name} – ${profile.wallet_name}` : profile.name

export type Holder = { address: string; holds: string; staked: string; sent: number; share: number }
export type Top = { epoch: number; in_wallets: string; holding: number; holders: Holder[] }

/** A Market profile with wallets it showed it holds, as the name search finds it. */
export type NamedProfile = {
  id: number
  name: string
  picture: string | null
  wallets: { address: string; name: string | null }[]
}

/** Market profiles whose name holds `query`, with their wallets. */
export async function findNames(query: string): Promise<NamedProfile[]> {
  const text = query.trim().replace(/^@/, '')
  if (text.length < 2) return []
  try {
    const response = await fetch(`/api/market/profiles?q=${encodeURIComponent(text)}`)
    if (!response.ok) return []
    const data = await response.json()
    return (data.profiles ?? []).filter((p: NamedProfile) => p.wallets.length > 0)
  } catch {
    return []
  }
}

export class ExplorerError extends Error {
  constructor(
    message: string,
    public status: number,
  ) {
    super(message)
  }
}

export async function explore<T>(path: string): Promise<T> {
  const response = await fetch(`/api/explorer${path}`)
  const data = await response.json().catch(() => ({}))
  if (!response.ok) {
    throw new ExplorerError(
      response.status === 404 ? 'Not found.' : (data.error ?? 'The network is not answering right now. Try again in a moment.'),
      response.status,
    )
  }
  return data as T
}

/** Market names for the verified wallets among `addresses`. */
export async function namesFor(addresses: string[]): Promise<Record<string, WalletProfile>> {
  const unique = [...new Set(addresses)].slice(0, 100)
  if (unique.length === 0) return {}
  try {
    const response = await fetch('/api/market/wallets/lookup', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ addresses: unique }),
    })
    if (!response.ok) return {}
    const data = await response.json()
    return data.profiles ?? {}
  } catch {
    return {}
  }
}

const UNITS = BigInt(1_000_000_000)

/** Units as ORN, grouped, with at most `digits` decimals (all nine by default). */
export function orn(units: string | number, digits = 9): string {
  const value = BigInt(units)
  let whole = value / UNITS
  let fraction = value % UNITS
  if (digits < 9) {
    const step = BigInt(10) ** BigInt(9 - digits)
    fraction = (fraction + step / BigInt(2)) / step
    if (fraction >= BigInt(10) ** BigInt(digits)) {
      whole += BigInt(1)
      fraction = BigInt(0)
    }
  }
  const text = digits === 0 ? '' : fraction.toString().padStart(digits, '0').replace(/0+$/, '')
  const grouped = whole.toString().replace(/\B(?=(\d{3})+(?!\d))/g, ',')
  return text ? `${grouped}.${text}` : grouped
}

/** Tries a second as "3.16 MH/s". */
export function hashrate(perSecond: number): string {
  const units = ['H/s', 'kH/s', 'MH/s', 'GH/s', 'TH/s']
  let value = perSecond
  let unit = 0
  while (value >= 1000 && unit < units.length - 1) {
    value /= 1000
    unit += 1
  }
  const digits = value >= 100 ? 0 : value >= 10 ? 1 : 2
  return `${value.toFixed(digits)} ${units[unit]}`
}

/** "a3f9c2…8b41". */
export const short = (address: string) => `${address.slice(0, 6)}…${address.slice(-4)}`

/** On explorer.orien.network, short addresses (/address/ABC/); anywhere
 * else, such as on this computer, the pages' own (/explorer/address/?a=ABC). */
const own = () => typeof window !== 'undefined' && window.location.hostname.startsWith('explorer.')

export const EXPLORER_URL = 'https://explorer.orien.network/'
export const homeHref = () => (own() ? '/' : '/explorer/')
export const addressHref = (address: string) => (own() ? `/address/${address}/` : `/explorer/address/?a=${address}`)
export const topHref = () => (own() ? '/top/' : '/explorer/top/')
export const checkpointHref = (epoch: number) => (own() ? `/checkpoint/${epoch}/` : `/explorer/checkpoint/?n=${epoch}`)

/** What an explorer page was opened for: the part after /address/ or
 * /checkpoint/ on explorer.orien.network, or the `key` in the query. */
export function opened(kind: 'address' | 'checkpoint', key: string): string {
  if (typeof window === 'undefined') return ''
  const match = window.location.pathname.match(new RegExp(`^/${kind}/([^/]+)`))
  return decodeURIComponent(match?.[1] ?? new URLSearchParams(window.location.search).get(key) ?? '')
}

/** "just now", "5 min ago", "3 h ago", or a date. */
export function ago(seconds: number): string {
  const gone = Date.now() / 1000 - seconds
  if (gone < 60) return 'just now'
  if (gone < 3600) return `${Math.floor(gone / 60)} min ago`
  if (gone < 86400) return `${Math.floor(gone / 3600)} h ago`
  return when(seconds)
}

/** "18 Nov, 15:12". */
export const when = (seconds: number) =>
  new Date(seconds * 1000).toLocaleString('en-GB', { day: 'numeric', month: 'short', hour: '2-digit', minute: '2-digit' })

/** What the search box was given: an address, a checkpoint number, or neither. */
export function searchTarget(text: string): string | null {
  const query = text.trim().toLowerCase()
  if (/^[0-9a-f]{64}$/.test(query)) return addressHref(query)
  if (/^\d{1,12}$/.test(query)) return checkpointHref(Number(query))
  return null
}
