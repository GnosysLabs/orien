'use client'

import Link from 'next/link'
import { useState, type FormEvent } from 'react'
import { CopyText } from '@/components/CopyText'
import { call, usePoll } from '../admin-client'

type Campaign = { wallet: string; budget: number; remaining: number }
type Payout = {
  id: number
  recipient: 'inviter' | 'invitee'
  name: string | null
  address: string
  sequence: number | null
  suspended: boolean
}
type Referrals = {
  campaign: Campaign | null
  payouts: Payout[]
  required_funding: number
  reward: number
}

const orn = (units: number) => (units / 1e9).toLocaleString('en', { maximumFractionDigits: 9 }) + ' ORN'
const button = 'min-h-10 rounded-xl border border-line bg-bg px-4 text-sm font-semibold text-ink hover:border-pearl disabled:opacity-50'

export default function AdminReferrals() {
  const { data, error, reload } = usePoll<Referrals>('/referrals', 30_000)
  const [address, setAddress] = useState('')
  const [busy, setBusy] = useState('')
  const [actionError, setActionError] = useState('')

  async function perform(key: string, path: string, body: unknown = {}) {
    setBusy(key)
    setActionError('')
    try {
      await call(path, 'POST', body)
      await reload()
    } catch (error) {
      setActionError((error as Error).message)
    } finally {
      setBusy('')
    }
  }

  function openCampaign(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    if (busy || !window.confirm('Open the referral program using this funding wallet?')) return
    void perform('start', '/referrals', { address: address.trim() })
  }

  const outstanding = data?.payouts.filter((payout) => payout.sequence === null) ?? []

  return (
    <div className="mx-auto flex max-w-[1000px] flex-col gap-5 pb-16 pt-10">
      <header className="flex flex-wrap items-center justify-between gap-3">
        <h1 className="text-2xl font-bold tracking-tight text-ink">Referrals</h1>
        <Link href="/admin/" className="text-sm font-semibold text-pearl hover:underline">Back to Admin</Link>
      </header>

      {(error || actionError) && <p role="alert" className="rounded-xl border border-stop/40 p-3 text-sm text-stop">{actionError || error}</p>}
      {!data && !error && <p className="text-sm text-muted">Loading referrals…</p>}

      {data?.campaign && (
        <section className="grid gap-4 rounded-2xl border border-line bg-surface p-5">
          <div className="grid gap-3 sm:grid-cols-3">
            <div><p className="text-sm text-muted">Reward budget</p><b>{orn(data.campaign.budget)}</b></div>
            <div><p className="text-sm text-muted">Available for new rewards</p><b>{orn(data.campaign.remaining)}</b></div>
            <div><p className="text-sm text-muted">Awaiting payment</p><b>{orn(outstanding.length * data.reward)}</b></div>
          </div>
          <CopyText text={data.campaign.wallet} label="Campaign funding wallet" />
        </section>
      )}

      {data && !data.campaign && (
        <form onSubmit={openCampaign} className="grid gap-3 rounded-2xl border border-line bg-surface p-5">
          <h2 className="font-semibold text-ink">Open the campaign</h2>
          <p className="text-sm text-muted">
            Use a dedicated wallet with at least {orn(data.required_funding)}. That covers rewards for 100 referrals and the payment fees.
          </p>
          <label htmlFor="funding-wallet" className="text-sm text-ink">Funding wallet address</label>
          <input
            id="funding-wallet"
            value={address}
            onChange={(event) => setAddress(event.target.value)}
            className="min-w-0 rounded-xl border border-line bg-bg px-3 py-2 font-mono text-sm"
            autoComplete="off"
            spellCheck={false}
            required
          />
          <button type="submit" disabled={!!busy || !/^[A-Fa-f0-9]{64}$/.test(address.trim())} className={`${button} justify-self-start`}>
            {busy === 'start' ? 'Checking wallet…' : 'Open campaign'}
          </button>
        </form>
      )}

      {data?.campaign && (
        <section className="grid gap-3">
          <h2 className="font-semibold text-ink">Referral payouts</h2>
          <p className="text-sm text-muted">
            Send each reward from the campaign wallet. Use Check payment after the network finalizes it.
          </p>
          {data.payouts.length === 0 && <p className="text-sm text-muted">No referrals have qualified yet.</p>}
          {data.payouts.map((payout) => {
            const key = `${payout.id}-${payout.recipient}`
            return (
              <article key={key} className="grid gap-3 rounded-2xl border border-line bg-surface p-4">
                <div className="flex flex-wrap items-center justify-between gap-2">
                  <div>
                    <p className="font-semibold text-ink">{payout.name ?? 'Closed account'}</p>
                    <p className="text-xs text-muted">Referral {payout.id} · {payout.recipient === 'inviter' ? 'Inviter' : 'New member'} · {orn(data.reward)}</p>
                  </div>
                  {payout.sequence === null ? (
                    <button
                      type="button"
                      className={button}
                      disabled={!!busy}
                      onClick={() => void perform(key, `/referrals/${payout.id}/${payout.recipient}/confirm`)}
                    >
                      {busy === key ? 'Checking…' : 'Check payment'}
                    </button>
                  ) : <span className="text-sm font-semibold text-go">Paid · payment #{payout.sequence}</span>}
                </div>
                {payout.suspended && <p className="text-sm text-stop">This member’s account is suspended.</p>}
                {payout.sequence === null && <CopyText text={payout.address} label="Recipient’s verified wallet" />}
              </article>
            )
          })}
        </section>
      )}
    </div>
  )
}
