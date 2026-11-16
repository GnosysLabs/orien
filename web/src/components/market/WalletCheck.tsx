'use client'

import { useEffect, useState } from 'react'
import { BadgeCheck, Wallet } from 'lucide-react'
import { CopyText } from '@/components/CopyText'
import { ask, sentence, type Account, type WalletProof } from '@/lib/market'
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
          {account.wallet_verified && <VerifiedBadge />}
          <button
            type="button"
            onClick={() => setChanging(true)}
            className="ml-auto text-sm font-semibold text-muted underline underline-offset-4 hover:text-ink"
          >
            Use a different wallet
          </button>
        </div>
        <p className="text-sm leading-relaxed text-muted">Buyers pay your listings straight to this wallet.</p>
        {account.wallet_verified ? (
          <p className="text-sm leading-relaxed text-muted">
            It&apos;s verified as yours, so your name and picture show beside it in other people&apos;s Orien Wallet.
          </p>
        ) : (
          <Verify onVerified={onVerified} />
        )}
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

/** A small green "Verified" mark for a wallet proven to be its owner's. */
export function VerifiedBadge() {
  return (
    <span className="inline-flex items-center gap-1 rounded-full bg-go/15 px-2 py-0.5 text-xs font-semibold text-go">
      <BadgeCheck className="h-3.5 w-3.5" strokeWidth={2.25} aria-hidden /> Verified
    </span>
  )
}

/** Proving the wallet is yours: one small payment from it. */
function Verify({ onVerified }: { onVerified: () => void }) {
  const [proof, setProof] = useState<WalletProof | null>(null)
  const [busy, setBusy] = useState(false)
  const [trouble, setTrouble] = useState('')

  // A proof asked for before is shown again; once one is waiting, the page
  // looks every few seconds for its payment.
  useEffect(() => {
    ask<{ proof: WalletProof | null }>('/me/wallet')
      .then((found) => setProof(found.proof))
      .catch(() => {})
  }, [])
  useEffect(() => {
    if (!proof || proof.verified) return
    const timer = setInterval(() => {
      ask<{ verified: boolean }>('/me/wallet')
        .then((found) => found.verified && onVerified())
        .catch(() => {})
    }, 5000)
    return () => clearInterval(timer)
  }, [proof, onVerified])

  const start = async () => {
    setBusy(true)
    setTrouble('')
    try {
      setProof((await ask<{ proof: WalletProof }>('/me/wallet/verify', 'POST')).proof)
    } catch (error) {
      setTrouble(sentence((error as Error).message))
    }
    setBusy(false)
  }

  return (
    <div className="space-y-3 rounded-xl border border-line bg-bg p-4">
      <h3 className="font-semibold text-ink">Verify it&apos;s yours</h3>
      <p className="text-sm leading-relaxed text-muted">
        A verified wallet gets a <strong className="text-ink">Verified</strong> badge on your profile, and your name and
        picture show beside it in other people&apos;s Orien Wallet. To prove it&apos;s yours, the wallet sends a tiny
        amount from its balance to Orien Market: less than 0.001 ORN, fee included.
      </p>
      {proof ? (
        <div className="space-y-3">
          <p className="text-sm leading-relaxed text-muted">
            In Orien Wallet, tap <strong className="text-ink">Verify</strong> on your profile and it sends this for you. Or
            send it yourself, <strong className="text-ink">from this wallet</strong>: exactly{' '}
            <strong className="num text-ink">{proof.orn} ORN</strong> to this address.
          </p>
          <CopyText label="Orien Market's address" text={proof.to} />
          <CopyText label="Amount, in ORN" text={proof.orn} />
          <p className="text-sm text-muted">This page shows Verified within seconds of the payment.</p>
        </div>
      ) : (
        <button
          type="button"
          onClick={start}
          disabled={busy}
          className="flex h-10 items-center rounded-xl border border-line bg-surface px-4 text-sm font-semibold text-ink transition hover:border-pearl disabled:opacity-60"
        >
          {busy ? 'One moment…' : 'Verify this wallet'}
        </button>
      )}
      {trouble && <Notice tone="bad">{trouble}</Notice>}
    </div>
  )
}
