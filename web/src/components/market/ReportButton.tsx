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
}: {
  kind: 'listing' | 'account'
  id: number
  what: string
  signedIn: boolean
}) {
  const [open, setOpen] = useState(false)
  const [reason, setReason] = useState('')
  const [state, setState] = useState<'idle' | 'sending' | 'sent'>('idle')
  const [trouble, setTrouble] = useState('')

  if (state === 'sent') return <p className="text-xs text-muted">Thanks. We will look at it.</p>
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
        <button type="button" onClick={() => setOpen(false)} className="text-xs text-muted hover:text-ink">
          Cancel
        </button>
      </div>
    </form>
  )
}
