'use client'

import { WalletNetworkGlyph } from '@/components/WalletNetworkGlyph'
import { useEffect, useState } from 'react'
import { OfficialTag, WalletFace } from './Official'
import { Wallet } from 'lucide-react'
import { type Rewards, type WalletProfile, addressHref, label, namesFor, orn, short } from '@/lib/explorer'
import { Card } from './Parts'

/** "12.34%", or "<0.01%". */
function percent(part: number): string {
  const value = part * 100
  if (value > 0 && value < 0.01) return '<0.01%'
  return `${value.toFixed(2)}%`
}

/** "80% harder", "4× harder", "12% easier" or "About the same". */
export function change(harder: number): string {
  // Twice as hard or more reads better as "4× harder"; likewise easier.
  const ratio = 1 + harder
  if (ratio >= 2) return `${Number(ratio.toFixed(1))}× harder`
  if (ratio > 0 && ratio <= 0.5) return `${Number((1 / ratio).toFixed(1))}× easier`
  const value = Math.round(Math.abs(harder) * 100)
  if (value === 0) return 'About the same'
  return `${value}% ${harder > 0 ? 'harder' : 'easier'}`
}

/** A wallet: its name and picture where someone on the Market showed they
 * hold it, its address otherwise. */
function Who({ address, profile }: { address: string; profile?: WalletProfile }) {
  return (
    <span className="flex min-w-0 items-center gap-2.5 sm:gap-3">
      {profile ? (
        <WalletFace profile={profile} size={32} />
      ) : (
        <span
          className="hidden h-8 w-8 shrink-0 items-center justify-center rounded-full border border-line bg-raised sm:flex"
          aria-hidden
        >
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
            <span className="num block truncate text-sm text-muted">{short(address)}</span>
          </>
        ) : (
          // As tall as a named row's two lines, so every row is the same.
          <span className="num flex min-h-11 items-center truncate text-ink">{short(address)}</span>
        )}
      </span>
    </span>
  )
}

const HEAD = 'hidden gap-4 border-b border-line px-6 py-3 text-xs font-semibold uppercase tracking-[0.14em] text-muted md:grid'

/** The checkpoint's mining rewards, wallet by wallet, and any fees shared
 * with stakers. */
export function CheckpointRewards({ rewards, epoch }: { rewards: Rewards; epoch: number }) {
  const [names, setNames] = useState<Record<string, WalletProfile>>({})

  useEffect(() => {
    let live = true
    const addresses = [...rewards.miners, ...rewards.stakers].map((m) => m.address)
    void namesFor(addresses).then((found) => live && setNames(found))
    return () => {
      live = false
    }
  }, [rewards])

  const { miners } = rewards
  const cols = 'md:grid-cols-[3rem_1.8fr_0.8fr_1.2fr]'

  return (
    <>
      <Card
        title="Mining rewards"
        aside={
          rewards.known &&
          miners.length > 0 && (
            <span className="text-sm text-muted">
              {miners.length.toLocaleString('en')} {miners.length === 1 ? 'wallet' : 'wallets'}
            </span>
          )
        }
      >
        {!rewards.known ? (
          <p className="px-5 py-5 leading-relaxed text-muted sm:px-6">
            {rewards.since !== null && epoch < rewards.since
              ? `Validators’ records of who earned what start at checkpoint ${rewards.since}, so who mined this hour isn’t known.`
              : 'The validators didn’t answer about this checkpoint’s rewards just now. Try again in a moment.'}
          </p>
        ) : miners.length === 0 ? (
          <p className="px-5 py-5 leading-relaxed text-muted sm:px-6">
            Nobody mined this hour, so its new ORN went to the miners of the next one.
          </p>
        ) : (
          <>
            <div className={`${HEAD} ${cols}`}>
              <span>#</span>
              <span>Wallet</span>
              <span className="text-right">Part</span>
              <span className="text-right">Earned</span>
            </div>
            <ol className="divide-y divide-line">
              {miners.map((miner, index) => (
                <li key={miner.address}>
                  <a
                    href={addressHref(miner.address)}
                    className={`grid grid-cols-[1.25rem_1fr_auto] items-center gap-x-3 gap-y-1 px-5 py-4 transition hover:bg-raised sm:gap-x-4 sm:px-6 ${cols}`}
                  >
                    <span className="num text-sm text-muted">{index + 1}</span>
                    <Who address={miner.address} profile={names[miner.address]} />
                    <span className="num hidden text-right text-sm text-muted md:block">{percent(miner.part)}</span>
                    <span className="num text-right text-sm text-ink">
                      {orn(miner.amount, 2)} ORN
                      <span className="block text-muted md:hidden">{percent(miner.part)}</span>
                    </span>
                  </a>
                </li>
              ))}
            </ol>
            <p className="border-t border-line px-5 py-4 text-sm leading-relaxed text-muted sm:px-6">
              At each checkpoint the hour’s new ORN are split between wallets by how much mining work each did. Part is
              each wallet’s share of what was paid. When there’s more mining than the network aims for, the next hour
              gets harder; with less, easier.
            </p>
          </>
        )}
      </Card>

      {rewards.stakers.length > 0 && (
        <Card
          title="Fees shared with stakers"
          aside={<span className="text-sm text-muted">{orn(rewards.fees, 2)} ORN</span>}
        >
          <div className={`${HEAD} md:grid-cols-[3rem_1.8fr_1.2fr]`}>
            <span>#</span>
            <span>Wallet</span>
            <span className="text-right">Earned</span>
          </div>
          <ol className="divide-y divide-line">
            {rewards.stakers.map((staker, index) => (
              <li key={staker.address}>
                <a
                  href={addressHref(staker.address)}
                  className="grid grid-cols-[1.25rem_1fr_auto] items-center gap-x-3 gap-y-1 px-5 py-4 transition hover:bg-raised sm:gap-x-4 sm:px-6 md:grid-cols-[3rem_1.8fr_1.2fr]"
                >
                  <span className="num text-sm text-muted">{index + 1}</span>
                  <Who address={staker.address} profile={names[staker.address]} />
                  <span className="num text-right text-sm text-ink">{orn(staker.amount, 4)} ORN</span>
                </a>
              </li>
            ))}
          </ol>
          <p className="border-t border-line px-5 py-4 text-sm leading-relaxed text-muted sm:px-6">
            Payment fees are shared at each checkpoint between wallets staking ORN behind a validator, by how much each
            has staked.
          </p>
        </Card>
      )}
    </>
  )
}
