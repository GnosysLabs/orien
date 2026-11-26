'use client'

import { useEffect, useState } from 'react'
import { CopyText } from '@/components/CopyText'

export function ReferralInvite() {
  const [code, setCode] = useState('')

  useEffect(() => {
    const value = new URLSearchParams(window.location.search).get('ref')?.toUpperCase() ?? ''
    if (/^[A-F0-9]{12}$/.test(value)) setCode(value)
  }, [])

  if (!code) return null

  return (
    <section className="mb-6 grid gap-3 rounded-2xl border border-line bg-surface p-5 sm:p-6">
      <h2 className="text-xl font-bold tracking-tight text-ink">You’ve been invited to Orien</h2>
      <p className="text-sm leading-relaxed text-muted">
        <span>Download Orien Wallet and create an Orien Network account.</span>{' '}
        <span>Open Settings → Referrals → Enter code and apply this code within 12 hours of signing up.</span>
      </p>
      <p className="text-sm leading-relaxed text-muted">
        <span>You and your inviter each earn 7 ORN after your verified wallet earns mining rewards in 10 hourly checkpoints.</span>{' '}
        <span>Wallet verification uses less than 0.001 ORN from your wallet.</span>
      </p>
      <CopyText text={code} label="Referral code" />
    </section>
  )
}
