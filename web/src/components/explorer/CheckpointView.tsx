'use client'

import { useEffect, useState } from 'react'
import { Check, ChevronLeft, ChevronRight, Minus } from 'lucide-react'
import { type CheckpointInfo, addressHref, ago, checkpointHref, explore, opened, orn, short, when } from '@/lib/explorer'
import { CheckpointRewards, change } from './CheckpointRewards'
import { Card, Loading, Problem } from './Parts'

const step =
  'flex h-10 items-center gap-1.5 rounded-xl border border-line bg-bg px-4 text-sm font-semibold text-ink transition hover:border-pearl'

/** One figure in the summary: a label, the figure, and a few words on it. */
function Fact({ label, value, note, wide }: { label: string; value: React.ReactNode; note?: React.ReactNode; wide?: boolean }) {
  return (
    <div className={`bg-surface px-5 py-5 sm:px-6 ${wide ? 'col-span-2 lg:col-span-1' : ''}`}>
      <p className="text-xs font-semibold uppercase tracking-[0.14em] text-muted">{label}</p>
      <p className="num mt-2 text-xl font-bold tracking-tight text-ink">{value}</p>
      {note && <p className="mt-1 text-sm text-muted">{note}</p>}
    </div>
  )
}

function Hash({ label, value }: { label: string; value: string }) {
  return (
    <div className="space-y-1.5 px-5 py-4 sm:px-6">
      <p className="text-sm text-muted">{label}</p>
      <p className="num break-all text-sm text-ink">{value}</p>
    </div>
  )
}

export function CheckpointView() {
  const [text, setText] = useState<string | null>(null)
  useEffect(() => setText(opened('checkpoint', 'n')), [])
  const epoch = text !== null && /^\d{1,12}$/.test(text) ? Number(text) : null
  const [info, setInfo] = useState<CheckpointInfo | null>(null)
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    if (text === null) return
    setInfo(null)
    setError(null)
    if (epoch === null) {
      setError('That is not a checkpoint number.')
      return
    }
    let live = true
    explore<CheckpointInfo>(`/checkpoints/${epoch}`)
      .then((found) => live && setInfo(found))
      .catch((e: Error) =>
        live && setError(e.message === 'Not found.' ? `Checkpoint ${epoch} hasn't been made yet.` : e.message),
      )
    return () => {
      live = false
    }
  }, [epoch, text])

  const checkpoint = info?.checkpoint
  const rewards = info?.rewards

  return (
    <div className="space-y-10">
      <div className="flex flex-wrap items-center justify-between gap-4">
        <div>
          <h1 className="text-3xl font-bold tracking-tight text-ink">Checkpoint {epoch ?? ''}</h1>
        </div>
        {epoch !== null && (
          <div className="flex gap-2">
            {epoch > 0 && (
              <a href={checkpointHref(epoch - 1)} className={step}>
                <ChevronLeft className="h-4 w-4" aria-hidden /> Earlier
              </a>
            )}
            {info && epoch < info.latest && (
              <a href={checkpointHref(epoch + 1)} className={step}>
                Later <ChevronRight className="h-4 w-4" aria-hidden />
              </a>
            )}
          </div>
        )}
      </div>

      {error && <Problem text={error} />}
      {!error && !info && <Loading />}

      {info && checkpoint && (
        <>
          <div className="grid grid-cols-2 gap-px overflow-hidden rounded-2xl border border-line bg-line lg:grid-cols-5">
            <Fact
              wide
              label="New ORN"
              value={`${orn(checkpoint.released, 2)} ORN`}
              note={
                rewards?.known
                  ? `To ${rewards.miners.length.toLocaleString('en')} ${rewards.miners.length === 1 ? 'miner' : 'miners'}`
                  : 'Released to miners'
              }
            />
            <Fact label="Payments" value={checkpoint.payments.toLocaleString('en')} note="Made final" />
            <Fact
              label="Mining"
              value={rewards?.math ? (rewards.math === 'Blake3' ? 'BLAKE3' : rewards.math) : '—'}
              note={rewards?.harder != null ? `Next hour ${change(rewards.harder).toLowerCase()}` : 'Next hour: new math'}
            />
            <Fact
              label="Agreed by"
              value={`${checkpoint.agreed.length} of ${checkpoint.of}`}
              note="Validators"
            />
            <Fact label="Made" value={ago(checkpoint.time)} note={when(checkpoint.time)} />
          </div>

          {info.rewards && <CheckpointRewards rewards={info.rewards} epoch={checkpoint.epoch} />}

          {info.validators.length > 0 && (
            <Card title="Validators that hour">
              <ul className="divide-y divide-line">
                {info.validators.map((validator) => (
                  <li key={validator.seat} className="flex flex-wrap items-center gap-x-4 gap-y-1 px-5 py-4 sm:px-6">
                    <span className="min-w-0 flex-1">
                      <span className="block font-semibold text-ink">Validator {validator.seat + 1}</span>
                      <span className="block text-sm text-muted">
                        {validator.account ? (
                          <>
                            Staked by{' '}
                            <a href={addressHref(validator.account)} className="num hover:text-ink">
                              {short(validator.account)}
                            </a>{' '}
                            · {orn(validator.stake, 0)} ORN staked
                          </>
                        ) : (
                          'Launch validator'
                        )}
                      </span>
                    </span>
                    <span className={`flex items-center gap-1.5 text-sm ${validator.agreed ? 'text-go' : 'text-muted'}`}>
                      {validator.agreed ? (
                        <Check className="h-4 w-4" aria-hidden />
                      ) : (
                        <Minus className="h-4 w-4" aria-hidden />
                      )}
                      {validator.agreed ? 'Agrees' : 'Didn’t answer'}
                    </span>
                  </li>
                ))}
              </ul>
              <p className="border-t border-line px-5 py-4 text-sm leading-relaxed text-muted sm:px-6">
                A checkpoint is final once validators with more than two thirds of the say have signed it.
              </p>
            </Card>
          )}

          <Card title="For checking">
            <div className="divide-y divide-line">
              <Hash label="State after it: every account's balance, as one fingerprint" value={checkpoint.state} />
              <Hash label="This checkpoint's fingerprint" value={checkpoint.digest} />
              {epoch !== 0 && <Hash label="The checkpoint before it" value={info.previous} />}
            </div>
          </Card>
        </>
      )}
    </div>
  )
}
