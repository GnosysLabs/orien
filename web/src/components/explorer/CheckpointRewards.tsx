'use client'

import { useEffect, useState } from 'react'
import { Wallet } from 'lucide-react'
import { Avatar } from '@/components/market/Avatar'
import { type Rewards, type WalletProfile, addressHref, label, namesFor, orn, short } from '@/lib/explorer'
import { Card, Stat } from './Parts'

/** "12.34%", or "<0.01%". */
function percent(part: number): string {
  const value = part * 100
  if (value > 0 && value < 0.01) return '<0.01%'
  return `${value.toFixed(2)}%`
}

/** "80% harder", "12% easier" or "About the same". */
function change(harder: number): string {
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
        <Avatar name={profile.name} picture={profile.picture} size={32} />
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
            <span className="block truncate font-semibold text-ink">{label(profile)}</span>
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

  const { miners, shares } = rewards
  const counted = miners.length > 0 && miners.every((m) => m.shares !== null)
  const perShare = rewards.known && shares && shares > 0 ? (BigInt(rewards.mined) / BigInt(shares)).toString() : null
  const cols = counted
    ? 'md:grid-cols-[3rem_1.8fr_0.8fr_0.8fr_1.2fr]'
    : 'md:grid-cols-[3rem_1.8fr_0.8fr_1.2fr]'

  return (
    <>
      <div className="grid grid-cols-1 gap-4 sm:grid-cols-2 lg:grid-cols-4">
        <Stat
          label="Shares found"
          value={shares === null ? '—' : shares.toLocaleString('en')}
          note={
            shares === null
              ? 'Not known for this hour: mining’s difficulty changed by the most it can at once.'
              : !rewards.known
                ? `The network aims for ${(rewards.wanted ?? 1000).toLocaleString('en')} an hour.`
                : `By ${miners.length.toLocaleString('en')} ${miners.length === 1 ? 'wallet' : 'wallets'}. The network aims for ${(rewards.wanted ?? 1000).toLocaleString('en')} an hour.`
          }
        />
        <Stat
          label="Paid to miners"
          value={rewards.known ? `${orn(rewards.mined, 2)} ORN` : '—'}
          note="Shared out by shares found"
        />
        <Stat
          label="Per share"
          value={perShare ? `${orn(perShare, 4)} ORN` : '—'}
          note={rewards.math ? `Mined with ${rewards.math === 'Blake3' ? 'BLAKE3' : rewards.math}` : undefined}
        />
        <Stat
          label="The next hour"
          value={rewards.harder === null ? '—' : change(rewards.harder)}
          note={
            rewards.harder === null
              ? 'Mining switched to new math, with a difficulty set for it.'
              : 'More shares than aimed for make mining harder; fewer make it easier.'
          }
        />
      </div>

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
              ? `Validators’ records of who earned what start at checkpoint ${rewards.since}, so who found this hour’s shares isn’t known.`
              : 'The validators didn’t answer about this checkpoint’s rewards just now. Try again in a moment.'}
          </p>
        ) : miners.length === 0 ? (
          <p className="px-5 py-5 leading-relaxed text-muted sm:px-6">
            Nobody found a share this hour, so its new ORN went to the miners of the next one.
          </p>
        ) : (
          <>
            <div className={`${HEAD} ${cols}`}>
              <span>#</span>
              <span>Wallet</span>
              {counted && <span className="text-right">Shares</span>}
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
                    {counted && (
                      <span className="num hidden text-right text-sm text-ink md:block">
                        {miner.shares!.toLocaleString('en')}
                      </span>
                    )}
                    <span className="num hidden text-right text-sm text-muted md:block">{percent(miner.part)}</span>
                    <span className="num text-right text-sm text-ink">
                      {orn(miner.amount, 2)} ORN
                      <span className="block text-muted md:hidden">
                        {counted
                          ? `${miner.shares!.toLocaleString('en')} ${miner.shares === 1 ? 'share' : 'shares'}`
                          : percent(miner.part)}
                      </span>
                    </span>
                  </a>
                </li>
              ))}
            </ol>
            <p className="border-t border-line px-5 py-4 text-sm leading-relaxed text-muted sm:px-6">
              A share is a small proof that a computer did mining work. At each checkpoint the new ORN are split between
              wallets by how many shares each found that hour.
              {counted &&
                ' Validators don’t keep the shares once the hour closes, so each wallet’s count is worked out from what it earned and how the difficulty moved. The counts add up exactly.'}
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
