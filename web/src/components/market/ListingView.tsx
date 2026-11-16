'use client'

import Link from 'next/link'
import { useSearchParams } from 'next/navigation'
import { useEffect, useState } from 'react'
import { ArrowLeft, Pencil, ShieldCheck } from 'lucide-react'
import { ask, categoryLabel, formatOrn, memberSince, perUnit, type Account, type Listing } from '@/lib/market'
import { Linked } from './Linked'
import { ProfileBanner } from './ProfileView'
import { Avatar } from './Avatar'
import { ContactLinks } from './ContactLinks'
import { MessageSeller } from './Messages'
import { Pictures } from './Pictures'
import { ReportButton } from './ReportButton'
import { Notice } from './Shell'

type Found = { listing: Listing; seller: Account; mine: boolean; signed_in: boolean }

export function ListingView() {
  const id = useSearchParams().get('id')
  const [found, setFound] = useState<Found | null>(null)
  const [trouble, setTrouble] = useState('')

  useEffect(() => {
    if (!id) return
    ask<Found>(`/listings/${encodeURIComponent(id)}`)
      .then(setFound)
      .catch((error: Error) => setTrouble(error.message === 'not found' ? 'This listing is gone or closed.' : error.message))
  }, [id])

  if (!id) return <Notice tone="bad">No listing was named.</Notice>
  if (trouble) return <Notice tone="bad">{trouble}</Notice>
  if (!found) return <p className="text-muted">Loading…</p>
  const { listing, seller, mine, signed_in } = found
  const card = 'rounded-2xl border border-line bg-surface p-6'

  return (
    <div className="space-y-6">
      <Link href="/marketplace/" className="inline-flex items-center gap-2 text-sm text-muted hover:text-ink">
        <ArrowLeft className="h-4 w-4" aria-hidden /> All listings
      </Link>
      <div className="grid items-start gap-8 lg:grid-cols-[minmax(0,1fr)_340px]">
        <div className="min-w-0 space-y-6">
          <div className="space-y-4">
            <p className="text-xs font-semibold uppercase tracking-[0.2em] text-muted">
              {categoryLabel(listing.category)}
              {!listing.open && ' · closed'}
            </p>
            <h1 className="text-balance text-3xl font-bold leading-tight tracking-tight text-ink sm:text-4xl">{listing.title}</h1>
            <Link href={`/marketplace/profile/?id=${seller.id}`} className="group inline-flex items-center gap-3">
              <Avatar name={seller.name} picture={seller.picture} size={40} />
              <span className="flex flex-col">
                <span className="flex items-center gap-1.5 font-semibold text-ink group-hover:underline">
                  {seller.name}
                </span>
                <span className="text-xs text-muted">{memberSince(seller.created)}</span>
              </span>
            </Link>
          </div>
          <Pictures pictures={listing.pictures} title={listing.title} />
          {listing.description && (
            <section className={card}>
              <h2 className="mb-3 text-lg font-bold tracking-tight text-ink">About this</h2>
              <p className="whitespace-pre-line leading-relaxed text-muted">
                <Linked text={listing.description} />
              </p>
            </section>
          )}
          <section className={`${card} relative isolate ${seller.banner ? 'on-banner pt-28 sm:min-h-56 sm:pt-6' : ''}`}>
            {seller.banner && <ProfileBanner banner={seller.banner} />}
            <h2 className="mb-4 text-lg font-bold tracking-tight text-ink">About the seller</h2>
            <Link href={`/marketplace/profile/?id=${seller.id}`} className="mb-4 flex items-center gap-3">
              <Avatar name={seller.name} picture={seller.picture} size={56} />
              <span className="flex flex-col">
                <span className="font-semibold text-ink hover:underline">{seller.name}</span>
                <span className="text-sm text-muted">See all they offer</span>
              </span>
            </Link>
            {seller.bio && <p className="mb-4 whitespace-pre-line leading-relaxed text-muted">{seller.bio}</p>}
            <ContactLinks contact={seller.contact} />
          </section>
        </div>

        <aside className="space-y-4 lg:sticky lg:top-24">
          <div className={`${card} space-y-5`}>
            <p className="num text-3xl font-bold text-ink">
              {formatOrn(listing.price)} ORN
              {listing.unit && <span className="ml-1.5 text-base font-medium text-muted">{perUnit(listing.unit)}</span>}
            </p>
            {mine ? (
              <div className="space-y-3">
                <p className="text-sm text-muted">This is your listing.</p>
                <Link
                  href={`/marketplace/sell/?id=${listing.id}`}
                  className="flex h-11 items-center justify-center gap-2 rounded-xl border border-line bg-bg text-sm font-semibold text-ink hover:bg-raised"
                >
                  <Pencil className="h-4 w-4" aria-hidden /> Edit your listing
                </Link>
              </div>
            ) : (
              <MessageSeller listing={listing.id} seller={seller.name} signedIn={signed_in} />
            )}
          </div>
          <p className="flex gap-2.5 px-1 text-sm leading-relaxed text-muted">
            <ShieldCheck className="mt-0.5 h-4 w-4 shrink-0 text-go" aria-hidden />
            <span>
              Agree the details in messages first. When you are ready, {mine ? 'you send' : `${seller.name} sends`} a payment
              request there, and the ORN go straight to {mine ? 'your' : 'their'} wallet.
            </span>
          </p>
          {!mine && (
            <div className="px-1">
              <ReportButton kind="listing" id={listing.id} what="this listing" signedIn={signed_in} />
            </div>
          )}
        </aside>
      </div>
    </div>
  )
}
