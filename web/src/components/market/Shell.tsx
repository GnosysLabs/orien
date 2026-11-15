'use client'

import Link from 'next/link'
import { useEffect, useState } from 'react'
import { Plus, Store, UserRound } from 'lucide-react'
import { me, type Account } from '@/lib/market'

/** Sent when this browser signs in or out, so every part of the page catches up. */
export const SIGNED_IN_OR_OUT = 'orien-market-account'

/** The marketplace's own bar: its name, selling, and your account. */
export function MarketBar() {
  const [account, setAccount] = useState<Account | null | undefined>(undefined)
  useEffect(() => {
    const load = () =>
      me()
        .then((found) => setAccount(found?.account ?? null))
        .catch(() => setAccount(null))
    void load()
    window.addEventListener(SIGNED_IN_OR_OUT, load)
    return () => window.removeEventListener(SIGNED_IN_OR_OUT, load)
  }, [])
  return (
    <div className="flex flex-wrap items-center gap-3 pb-6 pt-10 sm:pt-14">
      <Link href="/marketplace/" className="flex items-center gap-2.5">
        <Store className="h-6 w-6 text-pearl" strokeWidth={1.75} aria-hidden />
        <span className="text-2xl font-bold tracking-tight text-ink sm:text-3xl">Marketplace</span>
      </Link>
      <div className="ml-auto flex items-center gap-2">
        <Link
          href="/marketplace/account/"
          className="flex h-10 items-center gap-2 rounded-xl border border-line bg-surface px-4 text-sm font-semibold text-ink transition hover:bg-raised"
        >
          <UserRound className="h-4 w-4" aria-hidden />
          {account ? account.name : 'Sign in'}
        </Link>
        <Link
          href="/marketplace/sell/"
          className="pearl-fill flex h-10 items-center gap-2 rounded-xl px-4 text-sm font-bold transition hover:brightness-105"
        >
          <Plus className="h-4 w-4" strokeWidth={2.5} aria-hidden /> Sell something
        </Link>
      </div>
    </div>
  )
}

export function Notice({ tone = 'plain', children }: { tone?: 'plain' | 'bad'; children: React.ReactNode }) {
  return (
    <p
      className={`rounded-xl border px-4 py-3 text-sm ${
        tone === 'bad' ? 'border-stop/40 bg-stop/10 text-ink' : 'border-line bg-surface text-muted'
      }`}
    >
      {children}
    </p>
  )
}
