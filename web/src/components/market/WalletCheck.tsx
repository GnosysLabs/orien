'use client'

import { useState } from 'react'
import { Wallet } from 'lucide-react'
import { ask, sentence, type Account } from '@/lib/market'
import { Notice } from './Shell'

const short = (address: string) => `${address.slice(0, 8)}…${address.slice(-8)}`

/** The Orien wallet buyers pay this seller's listings to. */
export function WalletCheck({ account, onVerified }: { account: Account; onVerified: () => void }) {
  const [address, setAddress] = useState('')
  const [changing, setChanging] = useState(false)
  const [busy, setBusy] = useState(false)
  const [trouble, setTrouble] = useState('')

  const link = async (event: React.FormEvent) => {
    event.preventDefault()
    setBusy(true)
    setTrouble('')
    try {
      await ask('/me/wallet', 'POST', { address: address.trim() })
      setAddress('')
      setChanging(false)
      onVerified()
    } catch (error) {
      setTrouble(sentence((error as Error).message))
    }
    setBusy(false)
  }

  const box = 'space-y-4 rounded-2xl border border-line bg-surface p-6 sm:p-8'

  if (account.wallet && !changing) {
    return (
      <section className={box}>
        <div className="flex flex-wrap items-center gap-3">
          <Wallet className="h-5 w-5 text-pearl" strokeWidth={1.75} aria-hidden />
          <h2 className="text-lg font-bold tracking-tight text-ink">Paid to</h2>
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

  return (
    <form onSubmit={link} className={box}>
      <div className="flex items-center gap-3">
        <Wallet className="h-5 w-5 text-pearl" strokeWidth={1.75} aria-hidden />
        <h2 className="text-lg font-bold tracking-tight text-ink">
          {account.wallet ? 'Use a different wallet' : 'Add your wallet to start selling'}
        </h2>
      </div>
      <p className="leading-relaxed text-muted">
        Buyers pay you straight to your Orien wallet. The easiest way is to sign in from Orien Wallet, which adds itself.
        Or paste its address here.
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
          {busy ? 'One moment…' : 'Use this wallet'}
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
