'use client'

import { useCallback, useEffect, useState } from 'react'

export async function call<T>(path: string, method = 'GET', body?: unknown): Promise<T> {
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
export function usePoll<T>(path: string, every: number) {
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

export const count = (n: number) => n.toLocaleString('en')

/** A button that posts to the dashboard, first asking for a reason when `ask` is given. */
export function Action({
  label,
  ask,
  sure,
  path,
  onDone,
  danger,
}: {
  label: string
  ask?: string
  /** A yes/no question first, when no reason is needed. */
  sure?: string
  path: string
  onDone: () => void
  danger?: boolean
}) {
  const [busy, setBusy] = useState(false)
  const run = async () => {
    if (sure && !window.confirm(sure)) return
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
