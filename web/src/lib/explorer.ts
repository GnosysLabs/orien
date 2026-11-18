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

export type Validator = { seat: number; name: string; up: boolean; stake: string; account: string | null }

export type Overview = {
  epoch: number
  epoch_started: number
  epoch_seconds: number
  mining: string
  validators: Validator[]
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
}

export type WalletProfile = { id: number; name: string; picture: string | null }

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

/** "a3f9c2…8b41". */
export const short = (address: string) => `${address.slice(0, 6)}…${address.slice(-4)}`

export const addressHref = (address: string) => `/explorer/address/?a=${address}`
export const checkpointHref = (epoch: number) => `/explorer/checkpoint/?n=${epoch}`

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
