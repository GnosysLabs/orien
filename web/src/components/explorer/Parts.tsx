'use client'

import { useEffect, useRef, useState } from 'react'
import { Search } from 'lucide-react'
import { Avatar } from '@/components/market/Avatar'
import { type NamedProfile, addressHref, findNames, label, searchTarget, short } from '@/lib/explorer'

/** The explorer's search: an address, a checkpoint number, or the name of
 * someone on the Market, whose wallets it offers as you type. */
export function ExplorerSearch({ initial = '' }: { initial?: string }) {
  const [text, setText] = useState(initial)
  const [found, setFound] = useState<NamedProfile[]>([])
  // The name last looked up, once its answer is in: to say when nobody has it.
  const [searched, setSearched] = useState('')
  const [open, setOpen] = useState(false)
  const [message, setMessage] = useState<string | null>(null)
  const box = useRef<HTMLFormElement>(null)

  // Names, a moment after typing stops.
  useEffect(() => {
    const query = text.trim()
    if (searchTarget(query) || query.replace(/^@/, '').length < 2) {
      setFound([])
      setSearched('')
      return
    }
    let live = true
    const timer = setTimeout(() => {
      void findNames(query).then((profiles) => {
        if (!live) return
        setFound(profiles)
        setSearched(query)
        setOpen(true)
      })
    }, 250)
    return () => {
      live = false
      clearTimeout(timer)
    }
  }, [text])

  // Closes the list on a click elsewhere.
  useEffect(() => {
    const close = (event: MouseEvent) => {
      if (!box.current?.contains(event.target as Node)) setOpen(false)
    }
    document.addEventListener('mousedown', close)
    return () => document.removeEventListener('mousedown', close)
  }, [])

  const wallets = found.flatMap((profile) => profile.wallets.map((wallet) => ({ profile, wallet })))
  const nobody =
    !message && wallets.length === 0 && searched !== '' && searched === text.trim()
      ? `Nobody on the Market is called “${searched.replace(/^@/, '')}”.`
      : null
  const note = message ?? nobody

  return (
    <form
      ref={box}
      className="relative"
      onSubmit={async (event) => {
        event.preventDefault()
        const query = text.trim()
        const target = searchTarget(query)
        if (target) {
          window.location.href = target
          return
        }
        const profiles = query.replace(/^@/, '').length >= 2 ? await findNames(query) : []
        const all = profiles.flatMap((profile) => profile.wallets)
        const exact = profiles.filter((p) => p.name.toLowerCase() === query.replace(/^@/, '').toLowerCase())
        // One wallet to go to: the only one found, or the only one of the exact name.
        const only = all.length === 1 ? all[0] : exact.length === 1 && exact[0].wallets.length === 1 ? exact[0].wallets[0] : null
        if (only) {
          window.location.href = addressHref(only.address)
          return
        }
        setFound(profiles)
        setOpen(true)
        setMessage(
          profiles.length === 0
            ? 'Nothing found. Search an address (64 letters and digits, 0-9 and a-f), a checkpoint number, or the name of someone on the Market.'
            : null,
        )
      }}
    >
      <div className="flex gap-2">
        <input
          value={text}
          onChange={(event) => {
            setText(event.target.value)
            setMessage(null)
          }}
          onFocus={() => setOpen(true)}
          onKeyDown={(event) => event.key === 'Escape' && setOpen(false)}
          placeholder="Address, name or checkpoint"
          spellCheck={false}
          autoCapitalize="off"
          autoCorrect="off"
          className="h-10 min-w-0 flex-1 rounded-xl border border-line bg-bg px-4 text-sm text-ink outline-none transition placeholder:text-muted focus:border-pearl"
        />
        <button
          type="submit"
          className="pearl-fill flex h-10 shrink-0 items-center gap-2 rounded-xl px-4 text-sm font-bold transition hover:brightness-105"
        >
          <Search className="h-4 w-4" aria-hidden /> Search
        </button>
      </div>
      {open && (wallets.length > 0 || note) && (
        <div className="absolute inset-x-0 top-full z-10 mt-2 overflow-hidden rounded-xl border border-line bg-surface shadow-2xl shadow-black/40">
          {note ? (
            <p className="px-4 py-3 text-sm text-muted">{note}</p>
          ) : (
            <ul className="max-h-80 divide-y divide-line overflow-y-auto">
              {wallets.map(({ profile, wallet }) => (
                <li key={wallet.address}>
                  <a
                    href={addressHref(wallet.address)}
                    className="flex items-center gap-3 px-4 py-3 transition hover:bg-raised"
                  >
                    <Avatar name={profile.name} picture={profile.picture} size={32} />
                    <span className="min-w-0 flex-1">
                      <span className="block truncate font-semibold text-ink">
                        {label({ name: profile.name, wallet_name: wallet.name })}
                      </span>
                      <span className="num block truncate text-sm text-muted">{short(wallet.address)}</span>
                    </span>
                  </a>
                </li>
              ))}
            </ul>
          )}
        </div>
      )}
    </form>
  )
}

/** A figure with its label. */
export function Stat({ label, value, note }: { label: string; value: React.ReactNode; note?: React.ReactNode }) {
  return (
    <div className="rounded-2xl border border-line bg-surface p-5 sm:p-6">
      <p className="text-xs font-semibold uppercase tracking-[0.14em] text-muted">{label}</p>
      <p className="num mt-3 text-2xl font-bold tracking-tight text-ink">{value}</p>
      {note && <p className="mt-2 text-sm leading-relaxed text-muted">{note}</p>}
    </div>
  )
}

export function Card({ title, aside, children }: { title: string; aside?: React.ReactNode; children: React.ReactNode }) {
  return (
    <section className="rounded-2xl border border-line bg-surface">
      <div className="flex flex-wrap items-center justify-between gap-3 border-b border-line px-5 py-4 sm:px-6">
        <h2 className="text-lg font-bold tracking-tight text-ink">{title}</h2>
        {aside}
      </div>
      {children}
    </section>
  )
}

export function Problem({ text }: { text: string }) {
  return <p className="rounded-2xl border border-line bg-surface p-6 text-center text-muted">{text}</p>
}

export function Loading() {
  return <p className="py-16 text-center text-muted">Asking the network…</p>
}
