'use client'

import Link from 'next/link'
import { useSearchParams } from 'next/navigation'
import { useEffect, useState } from 'react'
import { ArrowLeft, Wallet } from 'lucide-react'
import { ask, me, memberSince, pictureUrl, type Account, type Listing } from '@/lib/market'
import { Linked } from './Linked'
import { Avatar } from './Avatar'
import { ContactLinks } from './ContactLinks'
import { Card } from './Browse'
import { ReportButton } from './ReportButton'
import { Notice } from './Shell'

export function ProfileView() {
  const id = useSearchParams().get('id')
  const [found, setFound] = useState<{ account: Account; listings: Listing[] } | null>(null)
  const [trouble, setTrouble] = useState('')
  const [viewer, setViewer] = useState<number | null>(null)

  useEffect(() => {
    me()
      .then((found) => setViewer(found?.account.id ?? null))
      .catch(() => {})
  }, [])

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
      <section
        className={`relative isolate flex flex-col items-center gap-4 rounded-2xl border border-line bg-surface p-8 text-center sm:flex-row sm:items-start sm:text-left ${
          account.banner ? 'pt-28 sm:min-h-56 sm:pt-8' : ''
        }`}
      >
        {account.banner && <ProfileBanner banner={account.banner} />}
        {viewer !== account.id && (
          <div className="absolute right-3 top-3 z-10">
            <ReportButton kind="account" id={account.id} what={account.name} signedIn={viewer !== null} icon />
          </div>
        )}
        <div className="relative shrink-0 rounded-full ring-4 ring-surface">
          <Avatar name={account.name} picture={account.picture} size={112} />
        </div>
        {/* relative: drawn above the banner behind it. */}
        <div className="relative min-w-0 space-y-2">
          <h1 className="text-3xl font-bold tracking-tight text-ink">{account.name}</h1>
          {account.wallet && (
            <p className="inline-flex items-center gap-1.5 rounded-full border border-line bg-bg px-3 py-1 text-xs font-semibold text-muted">
              <Wallet className="h-3.5 w-3.5" strokeWidth={2.25} aria-hidden /> Paid to{' '}
              <span className="num">
                {account.wallet.slice(0, 6)}…{account.wallet.slice(-6)}
              </span>
            </p>
          )}
          <p className="text-sm text-muted">{memberSince(account.created)}</p>
          {account.bio && (
            <p className="whitespace-pre-line pt-2 leading-relaxed text-muted">
              <Linked text={account.bio} />
            </p>
          )}
          <div className="pt-3">
            <ContactLinks contact={account.contact} />
          </div>
        </div>
      </section>
      <section className="space-y-4">
        <h2 className="text-xl font-bold tracking-tight text-ink">What {account.name} offers</h2>
        {listings.length === 0 ? (
          <Notice>Nothing on offer right now.</Notice>
        ) : (
          <div className="grid gap-5 sm:grid-cols-2 lg:grid-cols-3">
            {listings.map((listing) => (
              <Card key={listing.id} listing={listing} />
            ))}
          </div>
        )}
      </section>
    </div>
  )
}

/** The profile's banner, fading into the card: down to the content on a
 * phone, leftwards into the name on a wider screen; or, with `top`, down
 * from the top at every size. */
export function ProfileBanner({ banner, top = false }: { banner: string; top?: boolean }) {
  return (
    // Clipped to the card's corners here, not by the card, so a menu can
    // open over the card's edge.
    <div className="pointer-events-none absolute inset-0 -z-10 overflow-hidden rounded-2xl">
      {/* eslint-disable-next-line @next/next/no-img-element */}
      <img
        src={pictureUrl(banner)}
        alt=""
        className={
          top
            ? // Across the top of a box whose text runs its full width.
              'absolute inset-x-0 top-0 h-36 w-full object-cover opacity-90 [mask-image:linear-gradient(to_bottom,black_30%,transparent)]'
            : 'absolute right-0 top-0 h-40 w-full object-cover opacity-90 [mask-image:linear-gradient(to_bottom,black_35%,transparent)] sm:h-full sm:w-3/4 sm:[mask-image:linear-gradient(to_left,black_30%,transparent)]'
        }
      />
    </div>
  )
}
