'use client'

import Link from 'next/link'
import { useEffect, useState } from 'react'
import { usePathname } from 'next/navigation'
import { ArrowLeftRight, MessageCircle, Plus, Store, UserRound } from 'lucide-react'
import { me, type Account } from '@/lib/market'

/** Sent when this browser signs in or out, so every part of the page catches up. */
export const SIGNED_IN_OR_OUT = 'orien-market-account'

/** The bar the marketplace and the swap board share: the two, side by side,
 * then your messages and account, which serve both. */
export function MarketBar() {
  const pathname = usePathname()
  const [account, setAccount] = useState<Account | null | undefined>(undefined)
  const [unread, setUnread] = useState(0)
  useEffect(() => {
    const load = () =>
      me()
        .then((found) => {
          setAccount(found?.account ?? null)
          setUnread(found?.unread ?? 0)
        })
        .catch(() => setAccount(null))
    void load()
    window.addEventListener(SIGNED_IN_OR_OUT, load)
    const timer = setInterval(load, 30000)
    return () => {
      window.removeEventListener(SIGNED_IN_OR_OUT, load)
      clearInterval(timer)
    }
  }, [])
  const swapping = pathname.startsWith('/swap')
  return (
    <div className="flex flex-wrap items-center gap-3 pb-8 pt-10 sm:pt-14">
      <Link href={swapping ? '/swap/' : '/marketplace/'} className="flex items-center gap-2.5">
        {swapping ? (
          <ArrowLeftRight className="h-6 w-6 text-pearl" strokeWidth={1.75} aria-hidden />
        ) : (
          <Store className="h-6 w-6 text-pearl" strokeWidth={1.75} aria-hidden />
        )}
        <span className="text-xl font-bold tracking-tight text-ink sm:text-2xl">{swapping ? 'Buy & sell ORN' : 'Marketplace'}</span>
      </Link>
      <div className="ml-auto flex items-center gap-2">
        {account && (
          <Link
            href="/marketplace/messages/"
            aria-label={unread ? `Messages, ${unread} unread` : 'Messages'}
            className="relative flex h-10 items-center gap-2 rounded-xl border border-line bg-surface px-3 text-sm font-semibold text-ink transition hover:bg-raised"
          >
            <MessageCircle className="h-4 w-4" aria-hidden />
            <span className="hidden sm:inline">Messages</span>
            {unread > 0 && (
              <span className="flex h-5 min-w-5 items-center justify-center rounded-full bg-pearl px-1.5 text-[11px] font-bold text-bg">
                {unread}
              </span>
            )}
          </Link>
        )}
        <Link
          href="/marketplace/account/"
          className="flex h-10 items-center gap-2 rounded-xl border border-line bg-surface px-4 text-sm font-semibold text-ink transition hover:bg-raised"
        >
          <UserRound className="h-4 w-4" aria-hidden />
          {account ? account.name : 'Sign in'}
        </Link>
        {/* Buy & sell ORN has its own Sell tab. */}
        {!swapping && (
          <Link
            href="/marketplace/sell/"
            className="pearl-fill flex h-10 items-center gap-2 rounded-xl px-4 text-sm font-bold transition hover:brightness-105"
          >
            <Plus className="h-4 w-4" strokeWidth={2.5} aria-hidden /> Sell something
          </Link>
        )}
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
