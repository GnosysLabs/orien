'use client'

import { useEffect, useState } from 'react'
import { BadgeCheck, Wallet } from 'lucide-react'
import { CopyText } from '@/components/CopyText'
import { ask, plainOrn, type Account } from '@/lib/market'
import { Notice } from './Shell'

type Waiting = { from: string; pay_to: string; amount: number }

const short = (address: string) => `${address.slice(0, 8)}…${address.slice(-8)}`

/** Proving the seller holds the wallet their listings are paid to. */
export function WalletCheck({ account, onVerified }: { account: Account; onVerified: () => void }) {
  const [address, setAddress] = useState('')
  const [waiting, setWaiting] = useState<Waiting | null>(null)
  const [changing, setChanging] = useState(false)
  const [busy, setBusy] = useState(false)
  const [trouble, setTrouble] = useState('')

  // A check already under way, say after a reload.
  useEffect(() => {
    ask<{ waiting?: Waiting }>('/me/wallet')
      .then((found) => found.waiting && setWaiting(found.waiting))
      .catch(() => {})
  }, [])

  // While waiting, look for the payment every few seconds.
  useEffect(() => {
    if (!waiting) return
    const timer = setInterval(() => {
      ask<{ waiting?: Waiting; wallet?: string | null }>('/me/wallet')
        .then((found) => {
          if (found.wallet && !found.waiting) {
            setWaiting(null)
            setChanging(false)
            onVerified()
          }
        })
        .catch(() => {})
    }, 4000)
    return () => clearInterval(timer)
  }, [waiting, onVerified])

  const start = async (event: React.FormEvent) => {
    event.preventDefault()
    setBusy(true)
    setTrouble('')
    try {
      const found = await ask<{ waiting: Waiting }>('/me/wallet', 'POST', { address })
      setWaiting(found.waiting)
    } catch (error) {
      const message = (error as Error).message
      setTrouble(message.charAt(0).toUpperCase() + message.slice(1) + (message.endsWith('.') ? '' : '.'))
    }
    setBusy(false)
  }

  const box = 'space-y-4 rounded-2xl border border-line bg-surface p-6 sm:p-8'

  if (account.wallet && !changing && !waiting) {
    return (
      <section className={box}>
        <div className="flex flex-wrap items-center gap-3">
          <BadgeCheck className="h-5 w-5 text-go" strokeWidth={2} aria-hidden />
          <h2 className="text-lg font-bold tracking-tight text-ink">Wallet verified</h2>
          <code className="num text-sm text-muted">{short(account.wallet)}</code>
          <button
            type="button"
            onClick={() => setChanging(true)}
            className="ml-auto text-sm font-semibold text-muted underline underline-offset-4 hover:text-ink"
          >
            Use a different wallet
          </button>
        </div>
        <p className="text-sm leading-relaxed text-muted">Buyers pay your listings straight to this wallet.</p>
      </section>
    )
  }

  if (waiting) {
    return (
      <section className={box}>
        <div className="flex items-center gap-3">
          <Wallet className="h-5 w-5 text-pearl" strokeWidth={1.75} aria-hidden />
          <h2 className="text-lg font-bold tracking-tight text-ink">Send this from your wallet</h2>
        </div>
        <p className="leading-relaxed text-muted">
          From <strong className="font-semibold text-ink">{short(waiting.from)}</strong>, send exactly this amount to the
          marketplace, in Orien Wallet or with orien. It proves the wallet is yours. Keep this page open: it notices the
          payment within seconds.
        </p>
        <CopyText label="Amount, exactly" text={`${plainOrn(waiting.amount)} ORN`} />
        <CopyText label="To the marketplace's address" text={waiting.pay_to} />
        <CopyText label="Or with orien" text={`orien send --to ${waiting.pay_to} --amount ${plainOrn(waiting.amount)}`} />
        <p className="flex items-center gap-2 text-sm text-muted">
          <span className="h-2 w-2 animate-pulse rounded-full bg-pearl" aria-hidden /> Waiting for the payment…
        </p>
        <button
          type="button"
          onClick={() => setWaiting(null)}
          className="text-sm font-semibold text-muted underline underline-offset-4 hover:text-ink"
        >
          Use another address
        </button>
      </section>
    )
  }

  return (
    <form onSubmit={start} className={box}>
      <div className="flex items-center gap-3">
        <Wallet className="h-5 w-5 text-pearl" strokeWidth={1.75} aria-hidden />
        <h2 className="text-lg font-bold tracking-tight text-ink">
          {account.wallet ? 'Use a different wallet' : 'Verify your wallet to start selling'}
        </h2>
      </div>
      <p className="leading-relaxed text-muted">
        Buyers pay you straight to your Orien wallet. To make sure no payment goes astray, you prove it is yours once, by
        sending a tiny amount from it, less than a hundredth of an ORN.
      </p>
      <input
        value={address}
        onChange={(e) => setAddress(e.target.value)}
        required
        spellCheck={false}
        placeholder="Your Orien address: 64 letters and digits"
        className="num w-full rounded-xl border border-line bg-bg px-4 py-3 text-sm text-ink outline-none focus:border-pearl/60"
      />
      {trouble && <Notice tone="bad">{trouble}</Notice>}
      <div className="flex items-center gap-4">
        <button
          type="submit"
          disabled={busy}
          className="pearl-fill flex h-11 items-center rounded-xl px-6 text-sm font-bold transition hover:brightness-105 disabled:opacity-60"
        >
          {busy ? 'One moment…' : 'Verify this wallet'}
        </button>
        {changing && (
          <button type="button" onClick={() => setChanging(false)} className="text-sm font-semibold text-muted hover:text-ink">
            Cancel
          </button>
        )}
      </div>
    </form>
  )
}
