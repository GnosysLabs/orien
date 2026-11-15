'use client'

import Link from 'next/link'
import { useSearchParams } from 'next/navigation'
import { useEffect, useState } from 'react'
import { ArrowLeft, BadgeCheck } from 'lucide-react'
import { ask, memberSince, type Account, type Listing } from '@/lib/market'
import { Avatar } from './Avatar'
import { Card } from './Browse'
import { Notice } from './Shell'

export function ProfileView() {
  const id = useSearchParams().get('id')
  const [found, setFound] = useState<{ account: Account; listings: Listing[] } | null>(null)
  const [trouble, setTrouble] = useState('')

  useEffect(() => {
    if (!id) return
    ask<{ account: Account; listings: Listing[] }>(`/profiles/${encodeURIComponent(id)}`)
      .then(setFound)
      .catch((error: Error) => setTrouble(error.message === 'not found' ? 'There is no such seller.' : error.message))
  }, [id])

  if (!id) return <Notice tone="bad">No seller was named.</Notice>
  if (trouble) return <Notice tone="bad">{trouble}</Notice>
  if (!found) return <p className="text-muted">Loading…</p>
  const { account, listings } = found

  return (
    <div className="space-y-8">
      <Link href="/marketplace/" className="inline-flex items-center gap-2 text-sm text-muted hover:text-ink">
        <ArrowLeft className="h-4 w-4" aria-hidden /> All listings
      </Link>
      <section className="flex flex-col items-center gap-4 rounded-2xl border border-line bg-surface p-8 text-center sm:flex-row sm:items-start sm:text-left">
        <Avatar name={account.name} picture={account.picture} size={112} />
        <div className="min-w-0 space-y-2">
          <h1 className="text-3xl font-bold tracking-tight text-ink">{account.name}</h1>
          {account.wallet && (
            <p className="inline-flex items-center gap-1.5 rounded-full border border-go/30 bg-go/10 px-3 py-1 text-xs font-semibold text-go">
              <BadgeCheck className="h-3.5 w-3.5" strokeWidth={2.5} aria-hidden /> Verified wallet
            </p>
          )}
          <p className="text-sm text-muted">{memberSince(account.created)}</p>
          {account.bio && <p className="whitespace-pre-line pt-2 leading-relaxed text-muted">{account.bio}</p>}
        </div>
      </section>
      <section className="space-y-4">
        <h2 className="text-xl font-bold tracking-tight text-ink">What {account.name} offers</h2>
        {listings.length === 0 ? (
          <Notice>Nothing on offer right now.</Notice>
        ) : (
          <div className="grid gap-4 sm:grid-cols-2">
            {listings.map((listing) => (
              <Card key={listing.id} listing={listing} />
            ))}
          </div>
        )}
      </section>
    </div>
  )
}
