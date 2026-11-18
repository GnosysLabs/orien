'use client'

import { useEffect, useState } from 'react'
import { Check, ChevronLeft, ChevronRight, Minus } from 'lucide-react'
import { type CheckpointInfo, addressHref, checkpointHref, explore, opened, orn, short, when } from '@/lib/explorer'
import { CheckpointRewards } from './CheckpointRewards'
import { Card, Loading, Problem, Stat } from './Parts'

const step =
  'flex h-10 items-center gap-1.5 rounded-xl border border-line bg-bg px-4 text-sm font-semibold text-ink transition hover:border-pearl'

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

  return (
    <div className="space-y-10">
      <div className="flex flex-wrap items-center justify-between gap-4">
        <div>
          <h1 className="text-3xl font-bold tracking-tight text-ink">Checkpoint {epoch ?? ''}</h1>
          {checkpoint && <p className="mt-2 text-muted">Made {when(checkpoint.time)}</p>}
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
          <div className="grid grid-cols-1 gap-4 sm:grid-cols-3">
            <Stat label="Payments" value={checkpoint.payments.toLocaleString('en')} note="Made final during its hour" />
            <Stat label="New ORN" value={orn(checkpoint.released, 2)} note="Released to miners with it" />
            <Stat
              label="Agreed by"
              value={`${checkpoint.agreed.length} of ${checkpoint.of}`}
              note="Validators holding this same checkpoint. It is final once validators with more than two thirds of the say have signed it."
            />
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
