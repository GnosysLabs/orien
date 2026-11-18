'use client'

import { useEffect, useState } from 'react'
import { useSearchParams } from 'next/navigation'
import Link from 'next/link'
import { Check, ChevronLeft, ChevronRight, Minus } from 'lucide-react'
import { type CheckpointInfo, addressHref, checkpointHref, explore, orn, short, when } from '@/lib/explorer'
import { Card, ExplorerSearch, Loading, Problem, Stat } from './Parts'

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
  const text = useSearchParams().get('n') ?? ''
  const epoch = /^\d{1,12}$/.test(text) ? Number(text) : null
  const [info, setInfo] = useState<CheckpointInfo | null>(null)
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
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
  }, [epoch])

  const checkpoint = info?.checkpoint

  return (
    <div className="space-y-10">
      <ExplorerSearch />

      <div className="flex flex-wrap items-center justify-between gap-4">
        <div>
          <h1 className="text-3xl font-bold tracking-tight text-ink">Checkpoint {epoch ?? ''}</h1>
          {checkpoint && <p className="mt-2 text-muted">Made {when(checkpoint.time)}</p>}
        </div>
        {epoch !== null && (
          <div className="flex gap-2">
            {epoch > 0 && (
              <Link href={checkpointHref(epoch - 1)} className={step}>
                <ChevronLeft className="h-4 w-4" aria-hidden /> Earlier
              </Link>
            )}
            {info && epoch < info.latest && (
              <Link href={checkpointHref(epoch + 1)} className={step}>
                Later <ChevronRight className="h-4 w-4" aria-hidden />
              </Link>
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
              label="Signed by"
              value={`${checkpoint.signed.length} of ${checkpoint.of}`}
              note="It needs more than two thirds of the stake. The signatures it was settled with are kept."
            />
          </div>

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
                            <Link href={addressHref(validator.account)} className="num hover:text-ink">
                              {short(validator.account)}
                            </Link>
                          </>
                        ) : (
                          'Launch validator'
                        )}{' '}
                        · {orn(validator.stake, 0)} ORN at stake
                      </span>
                    </span>
                    <span className={`flex items-center gap-1.5 text-sm ${validator.signed ? 'text-go' : 'text-muted'}`}>
                      {validator.signed ? (
                        <Check className="h-4 w-4" aria-hidden />
                      ) : (
                        <Minus className="h-4 w-4" aria-hidden />
                      )}
                      {validator.signed ? 'Signature kept' : 'Not kept'}
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
