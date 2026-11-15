'use client'

import Link from 'next/link'
import { useEffect, useState } from 'react'
import { Search } from 'lucide-react'
import { ask, CATEGORIES, categoryLabel, formatOrn, type Listing } from '@/lib/market'
import { Avatar } from './Avatar'
import { Notice } from './Shell'

export function Browse() {
  const [category, setCategory] = useState('')
  const [search, setSearch] = useState('')
  const [query, setQuery] = useState('')
  const [listings, setListings] = useState<Listing[] | null>(null)
  const [more, setMore] = useState(false)
  const [trouble, setTrouble] = useState('')

  useEffect(() => {
    const wait = setTimeout(() => setQuery(search), 300)
    return () => clearTimeout(wait)
  }, [search])

  useEffect(() => {
    let current = true
    const params = new URLSearchParams()
    if (category) params.set('category', category)
    if (query.trim()) params.set('q', query.trim())
    ask<{ listings: Listing[]; more: boolean }>(`/listings?${params}`)
      .then((page) => {
        if (!current) return
        setListings(page.listings)
        setMore(page.more)
        setTrouble('')
      })
      .catch((error: Error) => current && setTrouble(error.message))
    return () => {
      current = false
    }
  }, [category, query])

  const loadMore = () => {
    const params = new URLSearchParams({ offset: String(listings?.length ?? 0) })
    if (category) params.set('category', category)
    if (query.trim()) params.set('q', query.trim())
    ask<{ listings: Listing[]; more: boolean }>(`/listings?${params}`)
      .then((page) => {
        setListings([...(listings ?? []), ...page.listings])
        setMore(page.more)
      })
      .catch((error: Error) => setTrouble(error.message))
  }

  return (
    <div className="space-y-6">
      <p className="text-lg leading-relaxed text-muted">
        Work and things offered for ORN. You pay the seller straight from your own wallet: the marketplace never holds
        anyone&apos;s coins.
      </p>
      <label className="flex h-12 items-center gap-3 rounded-xl border border-line bg-surface px-4 focus-within:border-pearl/60">
        <Search className="h-4 w-4 shrink-0 text-muted" aria-hidden />
        <input
          value={search}
          onChange={(event) => setSearch(event.target.value)}
          placeholder="Search listings"
          className="h-full w-full bg-transparent text-ink outline-none placeholder:text-muted"
        />
      </label>
      <div className="flex flex-wrap gap-2">
        {[{ id: '', label: 'Everything' }, ...CATEGORIES].map((c) => (
          <button
            key={c.id}
            type="button"
            onClick={() => setCategory(c.id)}
            className={`h-9 rounded-full border px-4 text-sm font-semibold transition ${
              category === c.id ? 'border-pearl/60 bg-raised text-ink' : 'border-line text-muted hover:text-ink'
            }`}
          >
            {c.label}
          </button>
        ))}
      </div>

      {trouble && <Notice tone="bad">{trouble}</Notice>}
      {listings && listings.length === 0 && (
        <Notice>
          Nothing here yet.{' '}
          <Link href="/marketplace/sell/" className="font-semibold text-ink underline underline-offset-4">
            Be the first to sell something.
          </Link>
        </Notice>
      )}
      <div className="grid gap-4 sm:grid-cols-2">
        {listings?.map((listing) => <Card key={listing.id} listing={listing} />)}
      </div>
      {more && (
        <button
          type="button"
          onClick={loadMore}
          className="mx-auto flex h-10 items-center rounded-xl border border-line bg-surface px-5 text-sm font-semibold text-ink hover:bg-raised"
        >
          Show more
        </button>
      )}
    </div>
  )
}

export function Card({ listing }: { listing: Listing }) {
  return (
    <Link
      href={`/marketplace/listing/?id=${listing.id}`}
      className="flex flex-col gap-3 rounded-2xl border border-line bg-surface p-5 transition hover:border-pearl/40 hover:bg-raised/50"
    >
      <p className="text-xs font-semibold uppercase tracking-[0.15em] text-muted">{categoryLabel(listing.category)}</p>
      <div className="-mt-1 flex items-start justify-between gap-3">
        <h2 className="text-lg font-bold leading-snug tracking-tight text-ink">{listing.title}</h2>
        {!listing.open && <span className="shrink-0 rounded-full bg-raised px-2.5 py-1 text-xs text-muted">Closed</span>}
      </div>
      <p className="line-clamp-2 text-sm leading-relaxed text-muted">{listing.description}</p>
      <div className="mt-auto flex items-end justify-between gap-3 pt-1">
        <span className="num text-lg font-bold text-ink">
          {formatOrn(listing.price)} ORN{listing.unit && <span className="text-sm font-medium text-muted"> {listing.unit}</span>}
        </span>
        <span className="flex min-w-0 items-center gap-2 text-xs text-muted">
          <Avatar name={listing.seller_name} picture={listing.seller_picture} size={22} />
          <span className="truncate">{listing.seller_name}</span>
        </span>
      </div>
    </Link>
  )
}
