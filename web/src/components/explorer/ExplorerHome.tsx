'use client'

import { useEffect, useState } from 'react'
import { ChevronRight } from 'lucide-react'
import { type Overview, addressHref, ago, checkpointHref, explore, orn, short } from '@/lib/explorer'
import { Card, Fact, Facts, Loading, Problem } from './Parts'
import { TopWallets } from './TopWallets'

/** A table's column headings, on wide screens. */
const HEAD =
  'hidden gap-4 border-b border-line px-6 py-3 text-xs font-semibold uppercase tracking-[0.14em] text-muted md:grid'

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
      {error && !overview && <Problem text={error} />}
      {!error && !overview && <Loading />}

      {overview && (
        <>
          <Facts className="grid-cols-2 lg:grid-cols-5">
            <Fact
              wide
              label="ORN released"
              value={overview.supply ? orn(overview.supply.released, 0) : '—'}
              note={overview.supply && `Of ${orn(overview.supply.cap, 0)} there will ever be`}
            />
            <Fact
              label="New ORN this hour"
              value={overview.supply ? orn(overview.supply.per_hour, 0) : '—'}
              note="Shared among miners"
            />
            <Fact
              label="Miners last hour"
              value={overview.miners ? overview.miners.last_hour.toLocaleString('en') : '—'}
              note={
                overview.miners &&
                `${overview.miners.last_day.toLocaleString('en')} in the last ${overview.miners.hours === 24 ? '24 hours' : `${overview.miners.hours} hours`}`
              }
            />
            <Fact
              label="Wallets holding ORN"
              value={overview.supply ? overview.supply.holding.toLocaleString('en') : '—'}
              note={overview.supply && `${overview.supply.holding_one.toLocaleString('en')} hold at least 1 ORN`}
            />
            <Fact label="Next checkpoint" value={next} note={`Checkpoint ${overview.epoch}`} />
          </Facts>

          <Card
            title="Validators"
            aside={
              <span className="text-sm text-muted">
                {up} of {overview.validators.length} running
              </span>
            }
          >
            <div className={`${HEAD} md:grid-cols-[1.3fr_1.3fr_1fr_1fr_7rem]`}>
              <span>Validator</span>
              <span>Kind</span>
              <span>Working on</span>
              <span>Agreed, last {overview.validators[0]?.of ?? 24}</span>
              <span className="text-right">Status</span>
            </div>
            <ul className="divide-y divide-line">
              {overview.validators.map((validator) => {
                const behind = validator.up && validator.epoch < overview.epoch
                return (
                  <li
                    key={validator.seat}
                    className="grid grid-cols-[1fr_auto] items-center gap-x-4 gap-y-1 px-5 py-4 sm:px-6 md:grid-cols-[1.3fr_1.3fr_1fr_1fr_7rem]"
                  >
                    <span className="flex min-w-0 items-center gap-3">
                      <span
                        className={`h-2.5 w-2.5 shrink-0 rounded-full ${validator.up ? 'bg-go' : 'bg-stop'}`}
                        aria-hidden
                      />
                      <span className="font-semibold text-ink">Validator {validator.seat + 1}</span>
                    </span>
                    <span className={`text-right text-sm font-semibold md:hidden ${validator.up ? 'text-go' : 'text-stop'}`}>
                      {validator.up ? 'Running' : 'Not answering'}
                    </span>
                    <span className="col-span-2 pl-[1.375rem] text-sm text-muted md:col-span-1 md:pl-0">
                      {validator.account ? (
                        <>
                          Staked by{' '}
                          <a href={addressHref(validator.account)} className="num hover:text-ink">
                            {short(validator.account)}
                          </a>{' '}
                          · {orn(validator.stake, 0)} ORN
                        </>
                      ) : (
                        'Launch validator'
                      )}
                      <span className="md:hidden">
                        {' '}
                        · agreed on {validator.agreed} of the last {validator.of} checkpoints
                      </span>
                    </span>
                    <span className="hidden text-sm md:block">
                      {!validator.up ? (
                        <span className="text-muted">–</span>
                      ) : behind ? (
                        <span className="text-warn">Catching up</span>
                      ) : (
                        <span className="text-ink">Checkpoint {validator.epoch}</span>
                      )}
                    </span>
                    <span className="num hidden text-sm text-ink md:block">
                      {validator.agreed} of {validator.of}
                    </span>
                    <span className={`hidden text-right text-sm font-semibold md:block ${validator.up ? 'text-go' : 'text-stop'}`}>
                      {validator.up ? 'Running' : 'Not answering'}
                    </span>
                  </li>
                )
              })}
            </ul>
            {overview.validators.some((v) => !v.account) && (
              <p className="border-t border-line px-5 py-4 text-sm leading-relaxed text-muted sm:px-6">
                Launch validators have run the network since it started. Their say in each checkpoint was set at launch
                and isn&apos;t made of ORN, so it adds nothing to the ORN in existence. Anyone can also stake ORN to run a
                validator.
              </p>
            )}
          </Card>

          <TopWallets limit={10} />

          <Card title="Latest checkpoints" aside={<span className="text-sm text-muted">One every hour</span>}>
            <div className="hidden grid-cols-[1.2fr_1fr_0.8fr_1fr_0.8fr_auto] gap-4 border-b border-line px-6 py-3 text-xs font-semibold uppercase tracking-[0.14em] text-muted md:grid">
              <span>Checkpoint</span>
              <span>Made</span>
              <span>Payments</span>
              <span>New ORN</span>
              <span>Agreed by</span>
              <span className="w-4" />
            </div>
            <ul className="divide-y divide-line">
              {overview.checkpoints.map((checkpoint) => (
                <li key={checkpoint.epoch}>
                  <a
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
                      {checkpoint.agreed.length} of {checkpoint.of}
                    </span>
                    <ChevronRight className="hidden h-4 w-4 text-muted md:block" aria-hidden />
                  </a>
                </li>
              ))}
            </ul>
          </Card>
        </>
      )}
    </div>
  )
}
