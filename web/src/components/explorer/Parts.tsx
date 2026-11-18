'use client'

import { useState } from 'react'
import { useRouter } from 'next/navigation'
import { Search } from 'lucide-react'
import { searchTarget } from '@/lib/explorer'

/** The explorer's search: an address or a checkpoint number. */
export function ExplorerSearch({ initial = '' }: { initial?: string }) {
  const router = useRouter()
  const [text, setText] = useState(initial)
  const [wrong, setWrong] = useState(false)
  return (
    <form
      onSubmit={(event) => {
        event.preventDefault()
        const target = searchTarget(text)
        setWrong(!target)
        if (target) router.push(target)
      }}
    >
      <div className="flex gap-3">
        <input
          value={text}
          onChange={(event) => {
            setText(event.target.value)
            setWrong(false)
          }}
          placeholder="An address, or a checkpoint number"
          spellCheck={false}
          autoCapitalize="off"
          autoCorrect="off"
          className="num h-12 min-w-0 flex-1 rounded-xl border border-line bg-bg px-4 text-sm text-ink outline-none transition placeholder:font-sans placeholder:text-muted focus:border-pearl"
        />
        <button
          type="submit"
          className="pearl-fill flex h-12 shrink-0 items-center gap-2 rounded-xl px-5 text-sm font-bold transition hover:brightness-105"
        >
          <Search className="h-4 w-4" aria-hidden /> Search
        </button>
      </div>
      {wrong && (
        <p className="mt-3 text-sm text-stop">
          An address is 64 letters and digits (0-9 and a-f). A checkpoint is a number, such as 90.
        </p>
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
