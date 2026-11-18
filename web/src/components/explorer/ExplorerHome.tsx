'use client'

import { useEffect, useState } from 'react'
import Link from 'next/link'
import { ChevronRight } from 'lucide-react'
import { type Overview, ago, checkpointHref, explore, orn } from '@/lib/explorer'
import { Card, ExplorerSearch, Loading, Problem, Stat } from './Parts'

/** "23 min" until the next checkpoint, ticking. */
function useCountdown(overview: Overview | null) {
  const [now, setNow] = useState(() => Date.now() / 1000)
  useEffect(() => {
    const timer = setInterval(() => setNow(Date.now() / 1000), 1000)
    return () => clearInterval(timer)
  }, [])
  if (!overview) return null
  const left = overview.epoch_started + overview.epoch_seconds - now
  if (left <= 0) return 'any moment'
  const minutes = Math.floor(left / 60)
  return minutes >= 1 ? `in ${minutes} min` : `in ${Math.ceil(left)} s`
}

export function ExplorerHome() {
  const [overview, setOverview] = useState<Overview | null>(null)
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    let live = true
    const load = () =>
      explore<Overview>('/overview')
        .then((found) => {
          if (!live) return
          setOverview(found)
          setError(null)
        })
        .catch((e: Error) => live && setError(e.message))
    load()
    const timer = setInterval(load, 30_000)
    return () => {
      live = false
      clearInterval(timer)
    }
  }, [])

  const next = useCountdown(overview)
  const up = overview?.validators.filter((v) => v.up).length ?? 0

  return (
    <div className="space-y-10">
      <ExplorerSearch />

      {error && !overview && <Problem text={error} />}
      {!error && !overview && <Loading />}

      {overview && (
        <>
          <div className="grid grid-cols-1 gap-4 sm:grid-cols-2 lg:grid-cols-4">
            <Stat
              label="ORN released"
              value={overview.supply ? orn(overview.supply.released, 0) : '—'}
              note={overview.supply && `of ${orn(overview.supply.cap, 0)} there will ever be`}
            />
            <Stat
              label="New ORN this hour"
              value={overview.supply ? orn(overview.supply.per_hour, 0) : '—'}
              note="Shared among everyone mining"
            />
            <Stat
              label="Wallets holding ORN"
              value={overview.supply ? overview.supply.holding.toLocaleString('en') : '—'}
              note={overview.supply && `${overview.supply.holding_one.toLocaleString('en')} hold at least 1 ORN`}
            />
            <Stat label="Next checkpoint" value={next} note={`Checkpoint ${overview.epoch}, about every hour`} />
          </div>

          <Card
            title="Validators"
            aside={
              <span className="text-sm text-muted">
                {up} of {overview.validators.length} running
              </span>
            }
          >
            <ul className="divide-y divide-line">
              {overview.validators.map((validator) => (
                <li key={validator.seat} className="flex flex-wrap items-center gap-x-4 gap-y-1 px-5 py-4 sm:px-6">
                  <span
                    className={`h-2.5 w-2.5 shrink-0 rounded-full ${validator.up ? 'bg-go' : 'bg-stop'}`}
                    aria-hidden
                  />
                  <span className="min-w-0 flex-1">
                    <span className="block font-semibold text-ink">Validator {validator.seat + 1}</span>
                    <span className="block text-sm text-muted">
                      {validator.account ? 'Staked validator' : 'Launch validator'} ·{' '}
                      {validator.up ? 'Running' : 'Not answering'}
                    </span>
                  </span>
                  <span className="num text-sm text-ink">{orn(validator.stake, 0)} ORN at stake</span>
                </li>
              ))}
            </ul>
          </Card>

          <Card title="Latest checkpoints" aside={<span className="text-sm text-muted">One every hour</span>}>
            <div className="hidden grid-cols-[1.2fr_1fr_0.8fr_1fr_0.8fr_auto] gap-4 border-b border-line px-6 py-3 text-xs font-semibold uppercase tracking-[0.14em] text-muted md:grid">
              <span>Checkpoint</span>
              <span>Made</span>
              <span>Payments</span>
              <span>New ORN</span>
              <span>Signed</span>
              <span className="w-4" />
            </div>
            <ul className="divide-y divide-line">
              {overview.checkpoints.map((checkpoint) => (
                <li key={checkpoint.epoch}>
                  <Link
                    href={checkpointHref(checkpoint.epoch)}
                    className="grid grid-cols-[1fr_auto] items-center gap-x-4 gap-y-1 px-5 py-4 transition hover:bg-raised sm:px-6 md:grid-cols-[1.2fr_1fr_0.8fr_1fr_0.8fr_auto]"
                  >
                    <span className="num font-semibold text-ink">#{checkpoint.epoch}</span>
                    <span className="text-sm text-muted md:order-none">{ago(checkpoint.time)}</span>
                    <span className="num text-sm text-ink">
                      {checkpoint.payments.toLocaleString('en')}
                      <span className="text-muted md:hidden"> payments</span>
                    </span>
                    <span className="num text-sm text-ink">
                      {orn(checkpoint.released, 0)}
                      <span className="text-muted md:hidden"> new ORN</span>
                    </span>
                    <span className="num hidden text-sm text-ink md:block">
                      {checkpoint.signed.length} of {checkpoint.of}
                    </span>
                    <ChevronRight className="hidden h-4 w-4 text-muted md:block" aria-hidden />
                  </Link>
                </li>
              ))}
            </ul>
          </Card>
        </>
      )}
    </div>
  )
}
