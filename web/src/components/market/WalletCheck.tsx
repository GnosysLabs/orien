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

type Named = { address: string; name: string }

/** The wallets you've proven are yours, and proving more: each by one
 * tiny payment from it. */
export function VerifiedWallets({ account, onChange }: { account: Account; onChange: () => void }) {
  const [wallets, setWallets] = useState<WalletProof[] | null>(null)
  const [named, setNamed] = useState<Named[]>([])
  const [choice, setChoice] = useState('')
  const [typed, setTyped] = useState('')
  const [busy, setBusy] = useState(false)
  const [trouble, setTrouble] = useState('')

  const load = () =>
    ask<{ wallets: WalletProof[] }>('/me/wallet')
      .then((found) => {
        setWallets((before) => {
          // A wallet newly verified changes the profile's badge.
          if (before && found.wallets.some((w) => w.verified && !before.find((b) => b.address === w.address)?.verified)) {
            onChange()
          }
          return found.wallets
        })
      })
      .catch(() => {})
  useEffect(() => {
    void load()
    ask<{ wallets: Named[] }>('/me/wallets')
      .then((found) => setNamed(found.wallets))
      .catch(() => {})
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [])
  // While a wallet's payment is awaited, look every few seconds.
  const waiting = wallets?.some((w) => !w.verified)
  useEffect(() => {
    if (!waiting) return
    const timer = setInterval(load, 5000)
    return () => clearInterval(timer)
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [waiting])

  const verify = async (address: string) => {
    setBusy(true)
    setTrouble('')
    try {
      await ask('/me/wallet/verify', 'POST', { address: address.trim() })
      setChoice('')
      setTyped('')
      await load()
    } catch (error) {
      setTrouble(sentence((error as Error).message))
    }
    setBusy(false)
  }
  const remove = async (address: string) => {
    try {
      await ask('/me/wallet/remove', 'POST', { address })
      await load()
      onChange()
    } catch (error) {
      setTrouble(sentence((error as Error).message))
    }
  }

  const listed = new Set((wallets ?? []).map((w) => w.address))
  const offered = named.filter((w) => !listed.has(w.address))
  const label = (w: WalletProof) =>
    w.name ?? named.find((n) => n.address === w.address)?.name ?? (w.address === account.wallet ? 'Paid-to wallet' : 'Wallet')

  return (
    <section className="space-y-4 rounded-2xl border border-line bg-surface p-6 sm:p-8">
      <div className="flex items-center gap-3">
        <BadgeCheck className="h-5 w-5 text-pearl" strokeWidth={1.75} aria-hidden />
        <h2 className="text-lg font-bold tracking-tight text-ink">Verified wallets</h2>
      </div>
      <p className="text-sm leading-relaxed text-muted">
        Prove a wallet is yours and people who pay you see it by name in Orien Wallet, as{' '}
        <strong className="text-ink">{account.name} – Savings</strong>, with your picture. Each wallet proves itself by
        sending a tiny amount from its balance to Orien Market: less than 0.001 ORN, fee included.{' '}
        <strong className="text-ink">
          Verifying ties the wallet to your name publicly: everyone will see it&apos;s yours, including its balance.
        </strong>
      </p>

      {wallets && wallets.length > 0 && (
        <ul className="divide-y divide-line rounded-xl border border-line bg-bg">
          {wallets.map((w) => (
            <li key={w.address} className="space-y-3 px-4 py-3">
              <div className="flex flex-wrap items-center gap-3">
                <span className="font-semibold text-ink">{label(w)}</span>
                <code className="num text-xs text-muted">{short(w.address)}</code>
                {w.verified ? (
                  <VerifiedBadge />
                ) : (
                  <span className="text-xs font-semibold text-muted">Waiting for its payment</span>
                )}
                <button
                  type="button"
                  onClick={() => remove(w.address)}
                  className="ml-auto text-xs font-semibold text-muted underline underline-offset-4 hover:text-ink"
                >
                  Remove
                </button>
              </div>
              {!w.verified && (
                <div className="space-y-3">
                  <p className="text-sm leading-relaxed text-muted">
                    In Orien Wallet, tap <strong className="text-ink">Verify</strong> and it sends this for you. Or send it
                    yourself, <strong className="text-ink">from this wallet</strong>: exactly{' '}
                    <strong className="num text-ink">{w.orn} ORN</strong> to Orien Market. This page shows Verified within
                    seconds of the payment.
                  </p>
                  <CopyText label="Orien Market's address" text={w.to} />
                  <CopyText label="Amount, in ORN" text={w.orn} />
                </div>
              )}
            </li>
          ))}
        </ul>
      )}

      <div className="flex flex-wrap items-center gap-3">
        {offered.length > 0 && (
          <select
            value={choice}
            onChange={(e) => setChoice(e.target.value)}
            className="h-10 rounded-xl border border-line bg-bg px-3 text-sm text-ink outline-none focus:border-pearl/60"
          >
            <option value="">Choose a wallet to verify…</option>
            {offered.map((w) => (
              <option key={w.address} value={w.address}>
                {w.name} ({short(w.address)})
              </option>
            ))}
            <option value="other">Another address…</option>
          </select>
        )}
        {(offered.length === 0 || choice === 'other') && (
          <input
            value={typed}
            onChange={(e) => setTyped(e.target.value)}
            spellCheck={false}
            placeholder="Orien address of a wallet to verify"
            className="num h-10 min-w-0 flex-1 rounded-xl border border-line bg-bg px-3 text-sm text-ink outline-none focus:border-pearl/60"
          />
        )}
        <button
          type="button"
          onClick={() => verify(choice && choice !== 'other' ? choice : typed)}
          disabled={busy || !((choice && choice !== 'other') || typed.trim())}
          className="pearl-fill flex h-10 items-center rounded-xl px-4 text-sm font-bold transition hover:brightness-105 disabled:opacity-40"
        >
          {busy ? 'One moment…' : 'Verify'}
        </button>
      </div>
      {trouble && <Notice tone="bad">{trouble}</Notice>}
    </section>
  )
}
