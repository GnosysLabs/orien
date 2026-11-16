'use client'

import Link from 'next/link'
import { useState } from 'react'
import { Flag } from 'lucide-react'
import { ask, sentence } from '@/lib/market'
import { AutoTextarea } from '@/components/AutoTextarea'

/** A quiet "Report" link that opens a short form, for something that
 * should not be on the marketplace. */
export function ReportButton({
  kind,
  id,
  what,
  signedIn,
  icon = false,
}: {
  kind: 'listing' | 'account'
  id: number
  what: string
  signedIn: boolean
  /** Just a flag, opening the form in a small panel below it. */
  icon?: boolean
}) {
  const [open, setOpen] = useState(false)

  if (icon) {
    return (
      <div className="relative">
        <button
          type="button"
          onClick={() => setOpen(!open)}
          aria-label={`Report ${what}`}
          title={`Report ${what}`}
          className={`flex h-9 w-9 items-center justify-center rounded-lg transition hover:bg-raised hover:text-ink ${open ? 'bg-raised text-ink' : 'text-muted'}`}
        >
          <Flag className="h-4 w-4" aria-hidden />
        </button>
        {open && (
          <div className="absolute right-0 top-11 z-20 w-80 max-w-[calc(100vw-3rem)] rounded-xl border border-line bg-surface p-1 text-left shadow-2xl shadow-black/40">
            <ReportForm kind={kind} id={id} what={what} signedIn={signedIn} onCancel={() => setOpen(false)} />
          </div>
        )}
      </div>
    )
  }
  if (!open) {
    return (
      <button
        type="button"
        onClick={() => setOpen(true)}
        className="inline-flex items-center gap-1.5 text-xs text-muted transition hover:text-ink"
      >
        <Flag className="h-3.5 w-3.5" aria-hidden /> Report {what}
      </button>
    )
  }
  return <ReportForm kind={kind} id={id} what={what} signedIn={signedIn} onCancel={() => setOpen(false)} />
}

/** The report form itself: what is wrong, sent to the marketplace. */
function ReportForm({
  kind,
  id,
  what,
  signedIn,
  onCancel,
}: {
  kind: 'listing' | 'account'
  id: number
  what: string
  signedIn: boolean
  onCancel: () => void
}) {
  const [reason, setReason] = useState('')
  const [state, setState] = useState<'idle' | 'sending' | 'sent'>('idle')
  const [trouble, setTrouble] = useState('')
  if (state === 'sent') return <p className="p-3 text-xs text-muted">Thanks. We will look at it.</p>
  if (!signedIn) {
    return (
      <p className="text-xs text-muted">
        <Link href="/marketplace/account/" className="font-semibold text-ink underline underline-offset-4">
          Sign in
        </Link>{' '}
        to report {what}.
      </p>
    )
  }

  const send = async (event: React.FormEvent) => {
    event.preventDefault()
    setState('sending')
    setTrouble('')
    try {
      await ask('/reports', 'POST', { kind, id, reason })
      setState('sent')
    } catch (error) {
      setTrouble(sentence((error as Error).message))
      setState('idle')
    }
  }

  return (
    <form onSubmit={send} className="space-y-2 rounded-xl border border-line bg-bg p-3">
      <p className="text-xs font-semibold text-ink">What is wrong with {what}?</p>
      <AutoTextarea
        value={reason}
        onChange={(e) => setReason(e.target.value)}
        maxLength={1000}
        rows={3}
        placeholder="A scam, something illegal, pretending to be someone…"
        className="w-full rounded-lg border border-line bg-surface px-3 py-2 text-sm text-ink outline-none focus:border-pearl/60"
      />
      {trouble && <p className="text-xs text-stop">{trouble}</p>}
      <div className="flex items-center gap-3">
        <button type="submit" disabled={state === 'sending'} className="h-8 rounded-lg bg-raised px-3 text-xs font-semibold text-ink hover:bg-line">
          Send report
        </button>
        <button type="button" onClick={onCancel} className="text-xs text-muted hover:text-ink">
          Cancel
        </button>
      </div>
    </form>
  )
}
