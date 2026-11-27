'use client'

import { WalletNetworkGlyph } from '@/components/WalletNetworkGlyph'
import { useEffect, useState } from 'react'
import { OfficialTag, WalletFace } from './Official'
import { ArrowRight, Wallet } from 'lucide-react'
import { type Top, type WalletProfile, addressHref, explore, label, namesFor, orn, short, topHref } from '@/lib/explorer'
import { Card, Loading, Problem } from './Parts'

/** "12.34%", or "<0.01%". */
function percent(share: number): string {
  const value = share * 100
  if (value > 0 && value < 0.01) return '<0.01%'
  return `${value.toFixed(2)}%`
}

/** The largest wallets, with their names where someone on the Market showed
 * they hold one. `limit` shows only the first few, with a way to the rest. */
export function TopWallets({ limit }: { limit?: number }) {
  const [top, setTop] = useState<Top | null>(null)
  const [names, setNames] = useState<Record<string, WalletProfile>>({})
  const [error, setError] = useState<string | null>(null)
  const [more, setMore] = useState('/explorer/top/')

  useEffect(() => {
    setMore(topHref())
    let live = true
    explore<Top>('/top')
      .then((found) => {
        if (!live) return
        setTop(found)
        void namesFor(found.holders.map((h) => h.address)).then((n) => live && setNames(n))
      })
      .catch((e: Error) => live && setError(e.message))
    return () => {
      live = false
    }
  }, [])

  const fewer = top && !limit && top.holders.length < 100

  const holders = top ? (limit ? top.holders.slice(0, limit) : top.holders) : []

  return (
    <Card
      title={limit ? 'Top wallets' : 'Ranked by ORN held'}
      aside={top && <span className="text-sm text-muted">As of checkpoint {Math.max(top.epoch - 1, 0)}</span>}
    >
      {error && !top && (
        <div className="p-5 sm:p-6">
          <Problem text={error} />
        </div>
      )}
      {!error && !top && <Loading />}
      {top && (
        <>
          <div className="hidden grid-cols-[3rem_1.6fr_1.2fr_0.7fr_1fr_0.9fr] gap-4 border-b border-line px-6 py-3 text-xs font-semibold uppercase tracking-[0.14em] text-muted md:grid">
            <span>#</span>
            <span>Wallet</span>
            <span className="text-right">Holds</span>
            <span className="text-right">Share</span>
            <span className="text-right">Staked</span>
            <span className="text-right">Payments sent</span>
          </div>
          <ol className="divide-y divide-line">
            {holders.map((holder, index) => {
              const profile = names[holder.address]
              return (
                <li key={holder.address}>
                  <a
                    href={addressHref(holder.address)}
                    className="grid grid-cols-[1.25rem_1fr_auto] items-center gap-x-3 sm:gap-x-4 gap-y-1 px-5 py-4 transition hover:bg-raised sm:px-6 md:grid-cols-[3rem_1.6fr_1.2fr_0.7fr_1fr_0.9fr]"
                  >
                    <span className="num text-sm text-muted">{index + 1}</span>
                    <span className="flex min-w-0 items-center gap-2.5 sm:gap-3">
                      {profile ? (
                        <WalletFace profile={profile} size={32} />
                      ) : (
                        <span className="hidden h-8 w-8 shrink-0 items-center justify-center rounded-full border border-line bg-raised sm:flex" aria-hidden>
                          <Wallet className="h-3.5 w-3.5 text-muted" />
                        </span>
                      )}
                      <span className="min-w-0">
                        {profile ? (
                          <>
                            <span className="block truncate font-semibold text-ink">
                              <WalletNetworkGlyph />{label(profile)}
                              {profile.official && <OfficialTag />}
                            </span>
                            <span className="num block truncate text-sm text-muted">{short(holder.address)}</span>
                          </>
                        ) : (
                          // As tall as a named row's two lines, so every row is the same.
                          <span className="num flex min-h-11 items-center truncate text-ink">{short(holder.address)}</span>
                        )}
                      </span>
                    </span>
                    <span className="num text-right text-sm text-ink">
                      {orn(holder.holds, 2)} ORN
                      <span className="block text-muted md:hidden">{percent(holder.share)}</span>
                    </span>
                    <span className="num hidden text-right text-sm text-muted md:block">{percent(holder.share)}</span>
                    <span className="num hidden text-right text-sm text-muted md:block">
                      {holder.staked === '0' ? '–' : `${orn(holder.staked, 0)} ORN`}
                    </span>
                    <span className="num hidden text-right text-sm text-ink md:block">
                      {holder.sent.toLocaleString('en')}
                    </span>
                  </a>
                </li>
              )
            })}
          </ol>
          {limit && top.holders.length > limit && (
            <div className="border-t border-line p-5 sm:px-6">
              <a
                href={more}
                className="flex h-10 w-full items-center justify-center gap-2 rounded-xl border border-line bg-bg px-4 text-sm font-semibold text-ink transition hover:border-pearl sm:w-auto sm:justify-self-start"
              >
                {top.holders.length < 100 ? `See all ${top.holders.length}` : 'See the top 100'} <ArrowRight className="h-4 w-4" aria-hidden />
              </a>
            </div>
          )}
          {fewer && (
            <p className="border-t border-line px-5 py-4 text-sm text-muted sm:px-6">
              Only {top.holders.length} wallets hold ORN so far, so all of them are listed.
            </p>
          )}
          {!limit && (
            <p className="border-t border-line px-5 py-4 text-sm text-muted sm:px-6">
              Holds counts everything a wallet has: spendable, staked, and on its way back from staking. Share is of all
              the ORN in wallets. Names are shown where someone on Orien Network showed they hold the wallet.
            </p>
          )}
        </>
      )}
    </Card>
  )
}
