'use client'

import Link from 'next/link'
import { useSearchParams } from 'next/navigation'
import { useEffect, useState } from 'react'
import { ArrowLeft, Pencil, Wallet } from 'lucide-react'
import { CopyText } from '@/components/CopyText'
import { ask, categoryLabel, formatOrn, plainOrn, type Listing } from '@/lib/market'
import { Avatar } from './Avatar'
import { Notice } from './Shell'

export function ListingView() {
  const id = useSearchParams().get('id')
  const [found, setFound] = useState<{ listing: Listing; mine: boolean } | null>(null)
  const [trouble, setTrouble] = useState('')

  useEffect(() => {
    if (!id) return
    ask<{ listing: Listing; mine: boolean }>(`/listings/${encodeURIComponent(id)}`)
      .then(setFound)
      .catch((error: Error) => setTrouble(error.message === 'not found' ? 'This listing is gone or closed.' : error.message))
  }, [id])

  if (!id) return <Notice tone="bad">No listing was named.</Notice>
  if (trouble) return <Notice tone="bad">{trouble}</Notice>
  if (!found) return <p className="text-muted">Loading…</p>
  const { listing, mine } = found

  return (
    <div className="space-y-6">
      <Link href="/marketplace/" className="inline-flex items-center gap-2 text-sm text-muted hover:text-ink">
        <ArrowLeft className="h-4 w-4" aria-hidden /> All listings
      </Link>
      <article className="rounded-2xl border border-line bg-surface p-6 sm:p-8">
        <p className="mb-3 text-xs font-semibold uppercase tracking-[0.2em] text-muted">
          {categoryLabel(listing.category)}
          {!listing.open && ' · closed'}
        </p>
        <h1 className="text-balance text-3xl font-bold leading-tight tracking-tight text-ink">{listing.title}</h1>
        <p className="num mt-4 text-2xl font-bold text-ink">
          {formatOrn(listing.price)} ORN{listing.unit && <span className="text-base font-medium text-muted"> {listing.unit}</span>}
        </p>
        <Link
          href={`/marketplace/profile/?id=${listing.seller}`}
          className="mt-4 inline-flex items-center gap-3 rounded-xl border border-line bg-bg py-2 pl-2 pr-4 transition hover:bg-raised"
        >
          <Avatar name={listing.seller_name} picture={listing.seller_picture} size={36} />
          <span className="flex flex-col">
            <span className="text-sm font-semibold text-ink">{listing.seller_name}</span>
            <span className="text-xs text-muted">See their profile</span>
          </span>
        </Link>
        {listing.description && (
          <div className="mt-6 whitespace-pre-line leading-relaxed text-muted">{listing.description}</div>
        )}
        {mine && (
          <Link
            href={`/marketplace/sell/?id=${listing.id}`}
            className="mt-6 inline-flex h-10 items-center gap-2 rounded-xl border border-line bg-bg px-4 text-sm font-semibold text-ink hover:bg-raised"
          >
            <Pencil className="h-4 w-4" aria-hidden /> Edit your listing
          </Link>
        )}
      </article>

      {listing.contact && (
        <section className="rounded-2xl border border-line bg-surface p-6 sm:p-8">
          <h2 className="mb-3 text-xl font-bold tracking-tight text-ink">Talk to the seller first</h2>
          <p className="mb-4 leading-relaxed text-muted">Agree the details before you pay. The seller can be reached at:</p>
          <p className="whitespace-pre-line break-words rounded-xl border border-line bg-bg px-4 py-3 text-ink">
            {listing.contact}
          </p>
        </section>
      )}

      <section className="rounded-2xl border border-line bg-surface p-6 sm:p-8">
        <div className="mb-3 flex items-center gap-3">
          <Wallet className="h-5 w-5 text-pearl" strokeWidth={1.75} aria-hidden />
          <h2 className="text-xl font-bold tracking-tight text-ink">Paying</h2>
        </div>
        <div className="space-y-4 leading-relaxed text-muted">
          <p>
            Send the ORN from your own wallet, in Orien Wallet or with the <strong className="text-ink">orien</strong>{' '}
            program. A payment is final in about a second and cannot be undone, so pay only once you trust the seller
            and have agreed what you get.
          </p>
          <CopyText label="Amount" text={`${plainOrn(listing.price)} ORN`} />
          <CopyText label="The seller's Orien address" text={listing.pay_to} />
          <CopyText label="Or with orien" text={`orien send --to ${listing.pay_to} --amount ${plainOrn(listing.price)}`} />
        </div>
      </section>
    </div>
  )
}
