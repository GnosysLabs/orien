'use client'

import { WalletNetworkGlyph } from '@/components/WalletNetworkGlyph'
import { useEffect, useRef, useState } from 'react'
import { BadgeCheck, MoreHorizontal, Plus, Wallet } from 'lucide-react'
import { CopyText } from '@/components/CopyText'
import { ask, sentence, type Account, type WalletProof } from '@/lib/market'
import { Notice } from './Shell'

const short = (address: string) => `${address.slice(0, 8)}…${address.slice(-8)}`

/** A small green "Verified" mark for a wallet proven to be its owner's. */
export function VerifiedBadge() {
  return (
    <span className="inline-flex items-center gap-1 rounded-full bg-go/15 px-2 py-0.5 text-xs font-semibold text-go">
      <BadgeCheck className="h-3.5 w-3.5" strokeWidth={2.25} aria-hidden /> Verified
    </span>
  )
}

type Named = { address: string; name: string }

/** One row's place in the list: a proof (verified or waiting), or the wallet
 * paid at from before verifying existed. */
type Row = { address: string; name: string | null; proof: WalletProof | null }

/** Your wallets, in one place: which ones are proven yours, which one buyers
 * pay, and adding another, which proves it with one tiny payment. */
export function YourWallets({ account, onChange }: { account: Account; onChange: () => void }) {
  const [proofs, setProofs] = useState<WalletProof[] | null>(null)
  const [named, setNamed] = useState<Named[]>([])
  const [adding, setAdding] = useState(false)
  const [trouble, setTrouble] = useState('')

  // A link to /account/#wallets: this section comes after the page loads,
  // too late for the browser to go to it by itself.
  useEffect(() => {
    if (window.location.hash === '#wallets') {
      requestAnimationFrame(() => document.getElementById('wallets')?.scrollIntoView({ behavior: 'smooth', block: 'start' }))
    }
  }, [])

  const load = () =>
    ask<{ wallets: WalletProof[] }>('/me/wallet')
      .then((found) => {
        setProofs((before) => {
          // A wallet newly verified can change the account: tell the page.
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
  const waiting = proofs?.some((w) => !w.verified)
  useEffect(() => {
    if (!waiting) return
    const timer = setInterval(load, 5000)
    return () => clearInterval(timer)
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [waiting])

  const act = async (run: () => Promise<unknown>) => {
    setTrouble('')
    try {
      await run()
      await load()
      onChange()
    } catch (error) {
      setTrouble(sentence((error as Error).message))
    }
  }

  const nameOf = (address: string, proof: WalletProof | null) =>
    proof?.name ?? named.find((n) => n.address === address)?.name ?? null
  const rows: Row[] = (proofs ?? []).map((p) => ({ address: p.address, name: nameOf(p.address, p), proof: p }))
  if (account.wallet && !rows.some((r) => r.address === account.wallet)) {
    rows.unshift({ address: account.wallet, name: nameOf(account.wallet, null), proof: null })
  }

  return (
    <section id="wallets" className="scroll-mt-24 space-y-4 rounded-2xl border border-line bg-surface p-6 sm:p-8">
      <div className="flex items-center gap-3">
        <Wallet className="h-5 w-5 text-pearl" strokeWidth={1.75} aria-hidden />
        <h2 className="text-lg font-bold tracking-tight text-ink">Your wallets</h2>
      </div>
      <p className="text-sm leading-relaxed text-muted">
        Buyers pay you straight to the wallet marked <strong className="text-ink">Paid here</strong>. Verified wallets show
        your name and picture when people pay you in Orien Wallet. To sell, you need one.
      </p>

      {proofs !== null && rows.length > 0 && (
        <ul className="divide-y divide-line rounded-xl border border-line bg-bg">
          {rows.map((row) => (
            <WalletRow
              key={row.address}
              row={row}
              paidHere={row.address === account.wallet}
              onPayHere={() => act(() => ask('/me/wallet', 'POST', { address: row.address }))}
              onVerify={() => act(() => ask('/me/wallet/verify', 'POST', { address: row.address }))}
              onRemove={() => act(() => ask('/me/wallet/remove', 'POST', { address: row.address }))}
            />
          ))}
        </ul>
      )}

      {adding ? (
        <AddWallet
          offered={named.filter((n) => !rows.some((r) => r.address === n.address))}
          onAdd={(address) =>
            act(async () => {
              await ask('/me/wallet/verify', 'POST', { address })
              setAdding(false)
            })
          }
          onCancel={() => setAdding(false)}
        />
      ) : (
        <button
          type="button"
          onClick={() => setAdding(true)}
          className="flex h-10 items-center gap-2 rounded-xl border border-line bg-bg px-4 text-sm font-semibold text-ink transition hover:border-pearl"
        >
          <Plus className="h-4 w-4" aria-hidden /> Add a wallet
        </button>
      )}
      {trouble && <Notice tone="bad">{trouble}</Notice>}
    </section>
  )
}

function WalletRow({
  row,
  paidHere,
  onPayHere,
  onVerify,
  onRemove,
}: {
  row: Row
  paidHere: boolean
  onPayHere: () => void
  onVerify: () => void
  onRemove: () => void
}) {
  const [menu, setMenu] = useState(false)
  const box = useRef<HTMLDivElement>(null)
  useEffect(() => {
    if (!menu) return
    const close = (event: MouseEvent) => {
      if (!box.current?.contains(event.target as Node)) setMenu(false)
    }
    document.addEventListener('mousedown', close)
    return () => document.removeEventListener('mousedown', close)
  }, [menu])
  const verified = row.proof?.verified ?? false
  const waiting = row.proof !== null && !verified

  return (
    <li className={`space-y-3 px-4 py-3 ${row.proof?.solana_devnet ? 'network-wallet-pair' : ''}`}>
      <div className="flex flex-wrap items-center gap-3">
        <span className="font-semibold text-ink"><WalletNetworkGlyph />{row.name ?? 'Wallet'}</span>
        <code className="num text-xs text-muted">{short(row.address)}</code>
        {verified ? (
          <VerifiedBadge />
        ) : waiting ? (
          <span className="text-xs font-semibold text-muted">Waiting for its payment</span>
        ) : (
          <span className="text-xs font-semibold text-warn">Not verified yet</span>
        )}
        <span className="ml-auto flex items-center gap-3">
          {paidHere ? (
            <span className="text-xs font-semibold text-pearl">Paid here</span>
          ) : (
            verified && (
              <button type="button" onClick={onPayHere} className="flex h-8 items-center rounded-lg border border-line bg-surface px-3 text-xs font-semibold text-ink transition hover:border-pearl">
                Get paid here
              </button>
            )
          )}
          {row.proof === null && (
            <button type="button" onClick={onVerify} className="flex h-8 items-center rounded-lg border border-line bg-surface px-3 text-xs font-semibold text-ink transition hover:border-pearl">
              Verify
            </button>
          )}
          {row.proof !== null && (
            <div ref={box} className="relative">
              <button
                type="button"
                aria-label="More"
                onClick={() => setMenu(!menu)}
                className="flex h-7 w-7 items-center justify-center rounded-lg text-muted transition hover:bg-raised hover:text-ink"
              >
                <MoreHorizontal className="h-4 w-4" aria-hidden />
              </button>
              {menu && (
                <div className="absolute right-0 top-full z-10 mt-1 w-40 rounded-xl border border-line bg-surface p-1 shadow-2xl shadow-black/40">
                  <button
                    type="button"
                    onClick={() => {
                      setMenu(false)
                      onRemove()
                    }}
                    className="w-full rounded-lg px-3 py-2 text-left text-sm font-semibold text-ink transition hover:bg-raised"
                  >
                    Remove
                  </button>
                </div>
              )}
            </div>
          )}
        </span>
      </div>
      {row.proof?.solana_devnet && (
        <div className="flex flex-wrap items-center gap-3 border-t border-line pt-3">
          <span className="font-semibold text-ink"><WalletNetworkGlyph network="SOL" />{row.name ?? 'Wallet'}</span>
          <a href={`https://explorer.solana.com/address/${row.proof.solana_devnet}?cluster=devnet`} title={row.proof.solana_devnet} className="num text-xs text-muted hover:underline">{short(row.proof.solana_devnet)}</a>
          <VerifiedBadge />
        </div>
      )}
      {waiting && row.proof && (
        <div className="space-y-3">
          <p className="text-sm leading-relaxed text-muted">
            In Orien Wallet, tap <strong className="text-ink">Verify</strong> and it sends this for you. Or send it yourself,{' '}
            <strong className="text-ink">from this wallet</strong>: exactly{' '}
            <strong className="num text-ink">{row.proof.orn} ORN</strong> to Orien Network. This shows Verified within
            seconds of the payment.
          </p>
          <CopyText label="Orien Network's address" text={row.proof.to} />
          <CopyText label="Amount, in ORN" text={row.proof.orn} />
        </div>
      )}
    </li>
  )
}

/** Adding a wallet: one from your Orien Wallet apps, or an address. */
function AddWallet({ offered, onAdd, onCancel }: { offered: Named[]; onAdd: (address: string) => void; onCancel: () => void }) {
  const [choice, setChoice] = useState(offered[0]?.address ?? 'other')
  const [typed, setTyped] = useState('')
  const address = choice === 'other' ? typed.trim() : choice
  return (
    <div className="space-y-3 rounded-xl border border-line bg-bg p-4">
      <h3 className="font-semibold text-ink">Add a wallet</h3>
      <p className="text-sm leading-relaxed text-muted">
        The wallet proves it&apos;s yours by sending a tiny amount from its balance to Orien Network: less than 0.001 ORN,
        fee included.{' '}
        <strong className="text-ink">Everyone will then see it&apos;s yours, including its balance.</strong> Don&apos;t
        have any ORN yet? Mine for an hour in Orien Wallet, or buy a little on Buy &amp; sell.
      </p>
      <div className="flex flex-wrap items-center gap-3">
        {offered.length > 0 && (
          <label className="inline-flex items-center"><WalletNetworkGlyph /><select aria-label="Orien wallet"
            value={choice}
            onChange={(e) => setChoice(e.target.value)}
            className="h-10 rounded-xl border border-line bg-surface px-3 text-sm text-ink outline-none focus:border-pearl/60"
          >
            {offered.map((w) => (
              <option key={w.address} value={w.address}>
                {w.name} ({short(w.address)})
              </option>
            ))}
            <option value="other">Another address…</option>
          </select></label>
        )}
        {choice === 'other' && (
          <input
            value={typed}
            onChange={(e) => setTyped(e.target.value)}
            spellCheck={false}
            placeholder="Orien address: 64 letters and digits"
            className="num h-10 min-w-0 flex-1 rounded-xl border border-line bg-surface px-3 text-sm text-ink outline-none focus:border-pearl/60"
          />
        )}
      </div>
      <div className="flex items-center gap-4">
        <button
          type="button"
          onClick={() => onAdd(address)}
          disabled={!address}
          className="pearl-fill flex h-10 items-center rounded-xl px-5 text-sm font-bold transition hover:brightness-105 disabled:opacity-40"
        >
          Verify this wallet
        </button>
        <button type="button" onClick={onCancel} className="text-sm font-semibold text-muted hover:text-ink">
          Cancel
        </button>
      </div>
    </div>
  )
}
