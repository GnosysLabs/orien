'use client'

import Link from 'next/link'
import { useEffect, useState } from 'react'
import { Search } from 'lucide-react'
import { ask, CATEGORIES, categoryLabel, formatOrn, type Listing } from '@/lib/market'
import { Avatar } from './Avatar'
import { Cover } from './Pictures'
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
      <h1 className="text-balance text-4xl font-bold leading-[1.1] tracking-tight text-ink sm:text-5xl">Buy and sell for ORN</h1>
      <p className="max-w-[680px] text-lg leading-relaxed text-muted">
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
      <div className="grid gap-5 sm:grid-cols-2 lg:grid-cols-3">
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
      className="group flex flex-col overflow-hidden rounded-2xl border border-line bg-surface transition hover:border-pearl/40"
    >
      <div className="relative overflow-hidden">
        <div className="transition duration-300 group-hover:scale-[1.03]">
          <Cover pictures={listing.pictures} category={listing.category} />
        </div>
        {!listing.open && (
          <span className="absolute left-3 top-3 rounded-full bg-bg/80 px-2.5 py-1 text-xs font-semibold text-muted">Closed</span>
        )}
      </div>
      <div className="flex flex-1 flex-col gap-3 p-4">
        <span className="flex min-w-0 items-center gap-2 text-xs text-muted">
          <Avatar name={listing.seller_name} picture={listing.seller_picture} size={22} />
          <span className="truncate font-semibold text-ink">{listing.seller_name}</span>
        </span>
        <h2 className="line-clamp-2 font-semibold leading-snug text-ink">{listing.title}</h2>
        <div className="mt-auto flex items-baseline justify-between gap-2 border-t border-line pt-3">
          <span className="text-xs uppercase tracking-[0.12em] text-muted">{categoryLabel(listing.category)}</span>
          <span className="num font-bold text-ink">
            {formatOrn(listing.price)} ORN{listing.unit && <span className="text-xs font-medium text-muted"> {listing.unit}</span>}
          </span>
        </div>
      </div>
    </Link>
  )
}
