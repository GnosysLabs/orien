'use client'

import Link from 'next/link'
import { useCallback, useEffect, useState } from 'react'
import { ago, formatOrn } from '@/lib/market'
import { Avatar } from '@/components/market/Avatar'

// The founder's dashboard. It only opens through `tailscale serve` on the
// server, which says who is signed in to Tailscale; the public site refuses
// /admin. Its numbers come from the marketplace server's /api/admin.

// Bar colour for the daily charts: a pearl violet that reads on the dark cards.
const BAR = '#8b74ee'

type Validator = {
  endpoint: string
  up: boolean
  name: string
  seat: number | null
  epoch: number
  closing: boolean
  accounts: number
  certificates: number
  keys_used: number
  keys_total: number
}

type Network = {
  error?: string
  validators: Validator[]
  epoch: number
  epoch_started: number
  epoch_seconds: number
  target: number | null
  first_target: number
  math?: string
}

type AdminAccount = {
  id: number
  name: string
  email: string | null
  picture: string | null
  wallet: string | null
  created: number
  suspended: string | null
  listings: number
  swaps_done: number
}

type AdminListing = {
  id: number
  title: string
  seller: number
  seller_name: string
  price: number
  unit: string
  created: number
  open: boolean
  removed: boolean
}

type Day = { date: string; signups: number; listings: number; messages: number; swaps: number }

type Overview = {
  login: string
  generated: number
  network: Network
  market: {
    accounts: number
    accounts_day: number
    accounts_week: number
    with_wallet: number
    suspended: number
    listings_open: number
    listings_week: number
    listings_removed: number
    conversations: number
    messages_day: number
    messages_week: number
    offers_open: number
    swaps_under_way: number
    swaps_done: number
    swaps_done_week: number
    reports_open: number
    daily: Day[]
    recent_accounts: AdminAccount[]
    recent_listings: AdminListing[]
  }
  server: { disk: { total: number; free: number } | null; services: { name: string; state: string }[] }
  /** Each validator asked over the network, wherever it runs. */
  validators: { seat: number; name: string; up: boolean; epoch: number }[] | null
}

type Report = {
  report: { id: number; reporter: number | null; reporter_name: string | null; kind: string; target: number; about: string; reason: string; created: number }
  owner: AdminAccount | null
  removed: boolean
}

async function call<T>(path: string, method = 'GET', body?: unknown): Promise<T> {
  const response = await fetch(`/api/admin${path}`, {
    method,
    cache: 'no-store',
    headers: body === undefined ? {} : { 'Content-Type': 'application/json' },
    body: body === undefined ? undefined : JSON.stringify(body),
  })
  const data = await response.json().catch(() => ({}))
  if (!response.ok) throw new Error(data.error ?? `The dashboard did not answer (${response.status}).`)
  return data as T
}

/** Fetches now and every `every` ms while the tab is visible; `reload` fetches again at once. */
function usePoll<T>(path: string, every: number) {
  const [data, setData] = useState<T | null>(null)
  const [error, setError] = useState('')
  const load = useCallback(
    () =>
      call<T>(path)
        .then((found) => {
          setData(found)
          setError('')
        })
        .catch((e: Error) => setError(e.message)),
    [path],
  )
  useEffect(() => {
    void load()
    const timer = setInterval(() => document.visibilityState === 'visible' && void load(), every)
    return () => clearInterval(timer)
  }, [load, every])
  return { data, error, reload: load }
}

const count = (n: number) => n.toLocaleString('en')
const short = (address: string) => `${address.slice(0, 6)}…${address.slice(-6)}`
const bytes = (n: number) => (n >= 1e12 ? `${(n / 1e12).toFixed(1)} TB` : `${Math.round(n / 1e9)} GB`)

export default function AdminPage() {
  const overview = usePoll<Overview>('/overview', 30_000)
  const reports = usePoll<{ reports: Report[] }>('/reports', 30_000)
  const reload = () => {
    void overview.reload()
    void reports.reload()
  }
  const data = overview.data

  if (!data) {
    return (
      <div className="mx-auto max-w-[1200px] py-20">
        {overview.error ? <p className="text-center text-sm text-muted">{overview.error}</p> : <div className="h-96 animate-pulse rounded-2xl bg-surface" />}
      </div>
    )
  }
  const m = data.market

  return (
    <div className="mx-auto flex max-w-[1200px] flex-col gap-5 pb-16 pt-10">
      <header className="flex flex-wrap items-center justify-between gap-3">
        <div>
          <h1 className="text-2xl font-bold tracking-tight text-ink">Admin</h1>
          <p className="mt-1 text-xs text-muted">
            Signed in through Tailscale as {data.login} · updated {ago(data.generated)} · refreshes every 30s
          </p>
        </div>
        <a
          href="https://explorer.orien.network/"
          target="_blank"
          rel="noopener noreferrer"
          className="flex h-10 items-center rounded-xl border border-line bg-surface px-4 text-sm font-semibold text-ink transition hover:border-pearl"
        >
          Open the explorer
        </a>
      </header>
      {overview.error && <p className="rounded-xl border border-stop/40 bg-stop/10 px-4 py-2 text-sm text-ink">{overview.error}</p>}

      <Reports reports={reports.data?.reports ?? null} onChange={reload} />

      <h2 className="-mb-2 text-sm font-bold text-muted">Market and swaps</h2>
      <section className="grid grid-cols-2 gap-3 lg:grid-cols-4">
        <Tile label="Accounts" value={count(m.accounts)} sub={`${m.accounts_day} today · ${m.accounts_week} this week · ${m.with_wallet} with a wallet`} />
        <Tile label="Listings open" value={count(m.listings_open)} sub={`${m.listings_week} posted this week · ${m.listings_removed} taken down`} />
        <Tile label="Messages today" value={count(m.messages_day)} sub={`${count(m.messages_week)} this week · ${count(m.conversations)} conversations`} />
        <Tile
          label="Swaps done"
          value={count(m.swaps_done)}
          sub={`${m.swaps_done_week} this week · ${m.swaps_under_way} under way · ${m.offers_open} offers open`}
        />
      </section>
      <section className="grid gap-3 lg:grid-cols-3">
        <DailyChart title="Sign-ups per day" data={m.daily.map((d) => ({ date: d.date, value: d.signups }))} />
        <DailyChart title="Messages per day" data={m.daily.map((d) => ({ date: d.date, value: d.messages }))} />
        <DailyChart title="Swaps done per day" data={m.daily.map((d) => ({ date: d.date, value: d.swaps }))} />
      </section>
      <section className="flex flex-col gap-3">
        <Card title="Newest accounts">
          <AccountTable accounts={m.recent_accounts} onChange={reload} empty="No accounts yet." />
        </Card>
        <Card title="Newest listings">
          <Table
            head={['Listing', 'Seller', 'Price', 'Posted', '']}
            rows={m.recent_listings.map((l) => [
              <Link key="t" href={`/marketplace/listing/?id=${l.id}`} className="font-semibold text-ink hover:text-pearl">
                {l.title}
                {l.removed ? <span className="ml-1.5 text-xs font-normal text-stop">taken down</span> : !l.open && <span className="ml-1.5 text-xs font-normal text-muted">closed</span>}
              </Link>,
              l.seller_name,
              `${formatOrn(l.price)} ORN`,
              ago(l.created),
              l.removed ? '' : <Action key="a" label="Take down" ask="Why is it being taken down? (shown to no one but you)" path={`/listings/${l.id}/remove`} onDone={reload} />,
            ])}
            empty="No listings yet."
          />
        </Card>
      </section>

      <FindAccount onChange={reload} />

      <Traffic />

      <h2 className="-mb-2 text-sm font-bold text-muted">Validators</h2>
      <NetworkView network={data.network} />

      <h2 className="-mb-2 text-sm font-bold text-muted">The server</h2>
      <section className="grid gap-3 lg:grid-cols-2">
        <Card title="Disk">
          {data.server.disk ? (
            <>
              <Row label="Free">{bytes(data.server.disk.free)}</Row>
              <Row label="Total">{bytes(data.server.disk.total)}</Row>
              <Row label="Used">{Math.round((1 - data.server.disk.free / data.server.disk.total) * 100)}%</Row>
            </>
          ) : (
            <p className="text-sm text-muted">Could not read the disk.</p>
          )}
        </Card>
        <Card title="Validators">
          {data.validators ? (
            data.validators.map((v) => (
              <Row key={v.seat} label={v.name}>
                <span className={v.up ? 'text-go' : 'text-stop'}>{v.up ? `running · checkpoint ${v.epoch}` : 'not answering'}</span>
              </Row>
            ))
          ) : (
            <p className="text-sm text-muted">The network did not answer.</p>
          )}
        </Card>
        <Card title="Services on this server">
          {data.server.services.map((s) => (
            <Row key={s.name} label={s.name}>
              <span className={s.state === 'active' ? 'text-go' : s.state === 'unknown' ? 'text-muted' : 'text-stop'}>{s.state}</span>
            </Row>
          ))}
        </Card>
      </section>
    </div>
  )
}

type TrafficDay = {
  date: string
  pageviews: number
  visitors: number
  downloads: number
  paths: Record<string, number>
  referrers: Record<string, number>
  countries: Record<string, number>
  devices: Record<string, number>
  files: Record<string, number>
  /** Times play was pressed on a video on the site, by video. */
  plays?: Record<string, number>
}

const COUNTRIES = typeof Intl.DisplayNames === 'function' ? new Intl.DisplayNames(['en'], { type: 'region' }) : null
const countryName = (code: string) => {
  if (code === '??') return 'Unknown'
  try {
    return COUNTRIES?.of(code) ?? code
  } catch {
    return code
  }
}

/** Who visits orien.network, from the site's own cookie-free counts. */
function Traffic() {
  const { data, error } = usePoll<{ active_now: number; days: TrafficDay[] }>('/traffic', 30_000)
  if (!data) {
    return (
      <>
        <h2 className="-mb-2 text-sm font-bold text-muted">Traffic</h2>
        {error ? <p className="text-sm text-muted">{error}</p> : <div className="h-40 animate-pulse rounded-2xl bg-surface" />}
      </>
    )
  }
  const days = data.days
  const today = days[days.length - 1]
  const week = days.slice(-7)
  const fortnight = days.slice(-14)
  const sum = (list: TrafficDay[], key: 'visitors' | 'pageviews' | 'downloads') => list.reduce((n, d) => n + d[key], 0)
  const merge = (key: 'paths' | 'referrers' | 'countries' | 'devices' | 'files') => {
    const out: Record<string, number> = {}
    for (const d of week) for (const [k, n] of Object.entries(d[key])) out[k] = (out[k] ?? 0) + n
    return Object.entries(out).sort((a, b) => b[1] - a[1])
  }
  const plays = (list: TrafficDay[]) => list.reduce((n, d) => n + Object.values(d.plays ?? {}).reduce((a, b) => a + b, 0), 0)
  const site = 'https://orien.network'
  return (
    <>
      <h2 className="-mb-2 text-sm font-bold text-muted">Traffic</h2>
      <section className="grid grid-cols-2 gap-3 lg:grid-cols-5">
        <Tile label="Visitors today" value={count(today.visitors)} sub={`${count(sum(week, 'visitors'))} this week`} />
        <Tile label="Page views today" value={count(today.pageviews)} sub={`${count(sum(week, 'pageviews'))} this week`} />
        <Tile label="Active now" value={count(data.active_now)} sub="visitors in the last 5 minutes" />
        <Tile label="Wallet downloads today" value={count(today.downloads)} sub={`${count(sum(week, 'downloads'))} this week · ${count(sum(days, 'downloads'))} in 30 days`} />
        <div className="col-span-2 lg:col-span-1">
          <Tile label="Video plays today" value={count(plays([today]))} sub={`${count(plays(week))} this week · ${count(plays(days))} in 30 days`} />
        </div>
      </section>
      <section className="grid gap-3 lg:grid-cols-3">
        <DailyChart title="Visitors per day" data={fortnight.map((d) => ({ date: d.date, value: d.visitors }))} />
        <DailyChart title="Page views per day" data={fortnight.map((d) => ({ date: d.date, value: d.pageviews }))} />
        <DailyChart title="Wallet downloads per day" data={fortnight.map((d) => ({ date: d.date, value: d.downloads }))} />
      </section>
      <section className="grid gap-3 lg:grid-cols-3">
        <Card title="Top pages, 7 days">
          <Table head={['Page', 'Views']} rows={merge('paths').slice(0, 12).map(([p, n]) => [p, count(n)])} empty="No visits yet." />
        </Card>
        <Card title="Where visitors came from, 7 days">
          <Table head={['Source', 'Visitors']} rows={merge('referrers').slice(0, 12).map(([r, n]) => [r, count(n)])} empty="No visits yet." />
          <div className="mt-3">
            <Table head={['Download', 'Clicks']} rows={merge('files').map(([f, n]) => [f, count(n)])} empty="" />
          </div>
        </Card>
        <Card title="Countries and devices, 7 days">
          <Table head={['Country', 'Visitors']} rows={merge('countries').slice(0, 8).map(([c, n]) => [countryName(c), count(n)])} empty="No visits yet." />
          <div className="mt-3">
            <Table head={['Device', 'Visitors']} rows={merge('devices').map(([k, n]) => [k, count(n)])} empty="" />
          </div>
        </Card>
      </section>
      <div className="-mt-1 flex flex-wrap items-center justify-between gap-3 text-xs text-muted">
        <p className="max-w-3xl">
          Counted without cookies: a visitor is recognised for one day only (UTC), so weekly visitors are daily visitors
          added up. Bots that don&apos;t run JavaScript aren&apos;t counted, and neither are dashboard or tailnet visits. Your
          own visits to orien.network look like anyone else&apos;s: stop counting each browser you use, once.
        </p>
        <div className="flex gap-2">
          <a href={`${site}/?notrack=1`} target="_blank" rel="noreferrer" className="rounded-lg border border-line bg-raised px-3 py-1.5 font-semibold text-ink hover:border-pearl">
            Stop counting this browser
          </a>
          <a href={`${site}/?notrack=0`} target="_blank" rel="noreferrer" className="rounded-lg px-2 py-1.5 hover:text-ink">
            Count it again
          </a>
        </div>
      </div>
    </>
  )
}

function NetworkView({ network: n }: { network: Network }) {
  if (n.error) return <p className="rounded-xl border border-stop/40 bg-stop/10 px-4 py-3 text-sm text-ink">The validators did not answer: {n.error}</p>
  const up = n.validators.filter((v) => v.up).length
  const behind = n.validators.filter((v) => v.up && v.epoch < n.epoch).length
  const keysLeft = Math.min(...n.validators.filter((v) => v.up).map((v) => v.keys_total - v.keys_used))
  return (
    <>
      <section className="grid grid-cols-1 gap-3 sm:grid-cols-2">
        <Tile
          label="Validators answering"
          value={`${up} of ${n.validators.length}`}
          sub={behind ? `${behind} behind the latest epoch` : 'all in the same epoch'}
          bad={up < n.validators.length || behind > 0}
        />
        <Tile
          label="Signing keys left"
          value={Number.isFinite(keysLeft) ? count(keysLeft) : '–'}
          sub="on the validator with the fewest: each checkpoint and vote uses some"
          bad={Number.isFinite(keysLeft) && keysLeft < 5000}
        />
      </section>
      <Card title="Each validator">
        <Table
          head={['Validator', 'State', 'Accounts', 'Payments', 'Keys left']}
          rows={n.validators.map((v) =>
            v.up
              ? [
                  <span key="n" className="font-semibold text-ink">
                    {v.name} <span className="num text-xs font-normal text-muted">{v.endpoint}</span>
                  </span>,
                  <span key="s" className={v.epoch < n.epoch ? 'text-stop' : 'text-go'}>
                    {v.closing ? 'closing' : v.epoch < n.epoch ? 'behind' : 'running'}
                  </span>,
                  count(v.accounts),
                  count(v.certificates),
                  <span key="k" className={v.keys_used / v.keys_total > 0.8 ? 'text-stop' : ''}>
                    {count(v.keys_total - v.keys_used)} of {count(v.keys_total)}
                  </span>,
                ]
              : [<span key="n" className="num text-muted">{v.endpoint}</span>, <span key="s" className="text-stop">not answering</span>, '', '', ''],
          )}
          empty="No validators."
        />
      </Card>
    </>
  )
}

function Reports({ reports, onChange }: { reports: Report[] | null; onChange: () => void }) {
  if (reports === null) return null
  return (
    <section className={`rounded-2xl border p-4 ${reports.length ? 'border-stop/40 bg-stop/5' : 'border-line bg-surface'}`}>
      <h2 className="mb-3 text-sm font-bold text-ink">
        {reports.length ? `Reports to look at: ${reports.length}` : 'Reports'}
      </h2>
      {reports.length === 0 ? (
        <p className="text-sm text-muted">Nothing reported. When someone reports a listing, a profile, a message or a community post, it shows here.</p>
      ) : (
        <ul className="space-y-3">
          {reports.map(({ report: r, owner, removed }) => (
            <li key={r.id} className="space-y-2 rounded-xl border border-line bg-surface p-4">
              <div className="flex flex-wrap items-baseline justify-between gap-2">
                <p className="text-sm text-muted">
                  <span className="font-semibold text-ink">{r.kind === 'listing' ? 'Listing' : r.kind === 'account' ? 'Profile' : r.kind === 'post' ? 'Community post' : 'Message'}</span>{' '}
                  reported by {r.reporter_name ?? 'a deleted account'} · {ago(r.created)}
                </p>
                <span className="num text-xs text-muted">report {r.id}</span>
              </div>
              <p className="whitespace-pre-line text-ink">
                {r.kind === 'listing' ? (
                  <Link href={`/marketplace/listing/?id=${r.target}`} className="font-semibold hover:text-pearl">
                    {r.about}
                  </Link>
                ) : r.kind === 'account' ? (
                  <Link href={`/marketplace/profile/?id=${r.target}`} className="font-semibold hover:text-pearl">
                    {r.about}
                  </Link>
                ) : (
                  <span className="italic">“{r.about}”</span>
                )}
              </p>
              <p className="text-sm text-muted">
                <span className="font-semibold text-ink">Why:</span> {r.reason || 'no reason given'}
              </p>
              {owner && (
                <p className="text-sm text-muted">
                  {r.kind === 'message' ? 'Sent by' : r.kind === 'listing' || r.kind === 'post' ? 'Posted by' : 'Account'}{' '}
                  <Link href={`/marketplace/profile/?id=${owner.id}`} className="font-semibold text-ink hover:text-pearl">
                    {owner.name}
                  </Link>{' '}
                  · {owner.email ?? 'no email'} · joined {ago(owner.created)}
                  {owner.suspended && <span className="text-stop"> · suspended</span>}
                </p>
              )}
              <div className="flex flex-wrap gap-2 pt-1">
                {r.kind === 'listing' && !removed && (
                  <Action label="Take the listing down" ask="Why is it being taken down? (shown to no one but you)" path={`/listings/${r.target}/remove`} onDone={onChange} danger />
                )}
                {r.kind === 'post' && !removed && (
                  <Action label="Take the post down" path={`/posts/${r.target}/remove`} onDone={onChange} danger />
                )}
                {owner && !owner.suspended && (
                  <Action
                    label={`Suspend ${owner.name}`}
                    ask={`Suspend ${owner.name}? They are signed out, cannot sign in, and their listings and offers come down. Why?`}
                    path={`/accounts/${owner.id}/suspend`}
                    onDone={onChange}
                    danger
                  />
                )}
                <Action label="Dealt with, nothing to do" path={`/reports/${r.id}/handled`} onDone={onChange} />
              </div>
            </li>
          ))}
        </ul>
      )}
    </section>
  )
}

/** A button that posts to the dashboard, first asking for a reason when `ask` is given. */
function Action({ label, ask, path, onDone, danger }: { label: string; ask?: string; path: string; onDone: () => void; danger?: boolean }) {
  const [busy, setBusy] = useState(false)
  const run = async () => {
    let reason = ''
    if (ask) {
      const typed = window.prompt(ask)
      if (typed === null) return
      reason = typed
    }
    setBusy(true)
    try {
      await call(path, 'POST', { reason })
      onDone()
    } catch (e) {
      window.alert((e as Error).message)
    }
    setBusy(false)
  }
  return (
    <button
      type="button"
      onClick={run}
      disabled={busy}
      className={`h-8 whitespace-nowrap rounded-lg border px-3 text-xs font-semibold transition disabled:opacity-50 ${
        danger ? 'border-stop/40 text-stop hover:bg-stop/10' : 'border-line text-ink hover:bg-raised'
      }`}
    >
      {busy ? 'One moment…' : label}
    </button>
  )
}

function AccountTable({ accounts, onChange, empty }: { accounts: AdminAccount[]; onChange: () => void; empty: string }) {
  return (
    <Table
      head={['Account', 'Wallet', 'Listings', 'Swaps', 'Joined', '']}
      rows={accounts.map((a) => [
        <Link key="n" href={`/marketplace/profile/?id=${a.id}`} className="inline-flex min-w-0 items-center gap-2 hover:text-pearl">
          <Avatar name={a.name} picture={a.picture} size={24} />
          <span className="min-w-0 truncate">
            <span className="font-semibold text-ink">{a.name}</span> <span className="text-xs text-muted">{a.email ?? ''}</span>
            {a.suspended && <span className="ml-1.5 text-xs text-stop">suspended</span>}
          </span>
        </Link>,
        a.wallet ? short(a.wallet) : '–',
        count(a.listings),
        count(a.swaps_done),
        ago(a.created),
        a.suspended ? (
          <Action key="u" label="Lift suspension" path={`/accounts/${a.id}/unsuspend`} onDone={onChange} />
        ) : (
          <Action
            key="s"
            label="Suspend"
            ask={`Suspend ${a.name}? They are signed out, cannot sign in, and their listings and offers come down. Why?`}
            path={`/accounts/${a.id}/suspend`}
            onDone={onChange}
            danger
          />
        ),
      ])}
      empty={empty}
    />
  )
}

function FindAccount({ onChange }: { onChange: () => void }) {
  const [typed, setTyped] = useState('')
  const [found, setFound] = useState<AdminAccount[] | null>(null)
  const [error, setError] = useState('')
  const search = async (event?: React.FormEvent) => {
    event?.preventDefault()
    setError('')
    try {
      const answer = await call<{ accounts: AdminAccount[] }>(`/accounts?q=${encodeURIComponent(typed)}`)
      setFound(answer.accounts)
    } catch (e) {
      setError((e as Error).message)
    }
  }
  return (
    <Card title="Find an account">
      <form onSubmit={search} className="mb-3 flex gap-2">
        <input
          value={typed}
          onChange={(e) => setTyped(e.target.value)}
          placeholder="Name, email, wallet address or account number"
          className="h-10 flex-1 rounded-xl border border-line bg-bg px-3 text-sm text-ink outline-none focus:border-pearl/60"
        />
        <button type="submit" className="pearl-fill h-10 rounded-xl px-4 text-sm font-bold">
          Search
        </button>
      </form>
      {error && <p className="text-sm text-stop">{error}</p>}
      {found && (
        <AccountTable
          accounts={found}
          onChange={() => {
            onChange()
            void search()
          }}
          empty="No account matches."
        />
      )}
    </Card>
  )
}

function Tile({ label, value, sub, bad }: { label: string; value: string; sub: string; bad?: boolean }) {
  return (
    <div className={`rounded-2xl border bg-surface p-4 ${bad ? 'border-stop/50' : 'border-line'}`}>
      <p className="text-xs text-muted">{label}</p>
      <p className="num mt-1 text-2xl font-semibold text-ink">{value}</p>
      <p className="num mt-1 text-xs text-muted">{sub}</p>
    </div>
  )
}

function Card({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <section className="rounded-2xl border border-line bg-surface p-4">
      <h2 className="mb-3 text-sm font-bold text-ink">{title}</h2>
      {children}
    </section>
  )
}

function Row({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div className="flex justify-between gap-3 border-t border-line/50 py-1.5 text-sm first-of-type:border-t-0">
      <span className="text-muted">{label}</span>
      <span className="num text-right text-ink">{children}</span>
    </div>
  )
}

function Table({ head, rows, empty }: { head: string[]; rows: React.ReactNode[][]; empty: string }) {
  if (!rows.length) return <p className="text-sm text-muted">{empty}</p>
  return (
    <div className="overflow-x-auto">
      <table className="w-full text-sm">
        <thead>
          <tr>
            {head.map((h, i) => (
              <th key={i} className={`whitespace-nowrap pb-1.5 text-xs font-medium text-muted ${i ? 'pl-3 text-right' : 'text-left'}`}>
                {h}
              </th>
            ))}
          </tr>
        </thead>
        <tbody>
          {rows.map((r, i) => (
            <tr key={i} className="border-t border-line/50">
              {r.map((cell, j) => (
                // The first column takes the room that is left and cuts off
                // long names with "…", so the numbers stay in view.
                <td key={j} className={`whitespace-nowrap py-1.5 text-muted ${j ? 'num pl-3 text-right' : 'w-full max-w-0 truncate'}`}>
                  {cell}
                </td>
              ))}
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  )
}

// A 14-day column chart: thin columns with rounded tops on one baseline,
// hairline gridlines at 0 / half / max, and the value on hover.
function DailyChart({ title, data }: { title: string; data: { date: string; value: number }[] }) {
  const [hover, setHover] = useState<number | null>(null)
  const top = niceMax(Math.max(...data.map((d) => d.value)))
  const day = (date: string) => new Date(`${date}T00:00:00Z`).toLocaleDateString(undefined, { month: 'short', day: 'numeric', timeZone: 'UTC' })
  const total = data.reduce((s, d) => s + d.value, 0)
  return (
    <section className="rounded-2xl border border-line bg-surface p-4">
      <div className="mb-2 flex items-baseline justify-between">
        <h2 className="text-sm font-bold text-ink">{title}</h2>
        <span className="num text-xs text-muted">14 days: {count(total)}</span>
      </div>
      <div className="relative flex h-36 pl-8">
        {[0, 0.5, 1].map((f) => (
          <div key={f} className="pointer-events-none absolute left-8 right-0 border-t border-line/60" style={{ bottom: `${f * 100}%` }}>
            <span className="num absolute -left-8 -top-2 w-7 text-right text-[10px] text-muted">{count(top * f)}</span>
          </div>
        ))}
        <div className="relative flex flex-1 items-end gap-[2px]">
          {data.map((d, i) => (
            <div key={d.date} className="relative flex h-full flex-1 items-end justify-center" onMouseEnter={() => setHover(i)} onMouseLeave={() => setHover(null)}>
              <div
                className="w-full max-w-6 rounded-t-[4px] transition-opacity"
                style={{ height: `${(d.value / top) * 100}%`, minHeight: d.value > 0 ? 2 : 0, background: BAR, opacity: hover === null || hover === i ? 1 : 0.45 }}
              />
              {hover === i && (
                <div
                  className={`pointer-events-none absolute top-0 z-10 whitespace-nowrap rounded-lg border border-line bg-bg px-2 py-1 text-xs shadow-xl ${
                    i > (data.length * 2) / 3 ? 'right-0' : i < data.length / 3 ? 'left-0' : 'left-1/2 -translate-x-1/2'
                  }`}
                >
                  <span className="text-muted">{day(d.date)}</span> <span className="num font-semibold text-ink">{count(d.value)}</span>
                </div>
              )}
            </div>
          ))}
        </div>
      </div>
      <div className="num mt-1 flex justify-between pl-8 text-[10px] text-muted">
        <span>{day(data[0].date)}</span>
        <span>{day(data[data.length - 1].date)}</span>
      </div>
    </section>
  )
}

// A clean axis maximum: 1, 2 or 5 times a power of ten at or above the data.
function niceMax(v: number) {
  if (v <= 0) return 1
  const p = 10 ** Math.floor(Math.log10(v))
  return [1, 2, 5, 10].map((m) => m * p).find((m) => m >= v)!
}
