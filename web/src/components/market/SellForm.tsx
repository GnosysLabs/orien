'use client'

import { useMe } from '@/lib/useMe'
import { shrinkPicture } from '@/lib/shrink'
import Link from 'next/link'
import { useRouter, useSearchParams } from 'next/navigation'
import { useEffect, useState } from 'react'
import { ImagePlus, Star, X } from 'lucide-react'
import { ask, CATEGORIES, MarketError, me, parseOrn, perUnit, pictureUrl, plainOrn, PRICINGS, sentence, type Draft, type Listing } from '@/lib/market'
import { Notice } from './Shell'
import { AutoTextarea } from '@/components/AutoTextarea'

const field = 'w-full rounded-xl border border-line bg-bg px-4 py-3 text-ink outline-none focus:border-pearl/60'

/** A picture on the form: one already on the listing, or one picked to add. */
type Item = { saved: string } | { file: File; preview: string }

const MAX_PICTURES = 4

async function upload(listing: number, file: File): Promise<Listing> {
  const response = await fetch(`/api/market/listings/${listing}/pictures`, { method: 'POST', credentials: 'same-origin', body: await shrinkPicture(file) })
  const data = await response.json().catch(() => ({}))
  if (!response.ok) throw new MarketError(data.error ?? 'The picture could not be added.', response.status)
  return data.listing
}

export function SellForm() {
  const router = useRouter()
  const id = useSearchParams().get('id')
  const [ready, setReady] = useState<'checking' | 'signed-out' | 'no-wallet' | 'ready'>('checking')
  const [title, setTitle] = useState('')
  const [description, setDescription] = useState('')
  const [category, setCategory] = useState('design')
  const [price, setPrice] = useState('')
  const [unit, setUnit] = useState('')
  const [payTo, setPayTo] = useState('')
  const [pictures, setPictures] = useState<Item[]>([])
  const [original, setOriginal] = useState<string[]>([])
  const [trouble, setTrouble] = useState('')
  const [saving, setSaving] = useState(false)

  // Signed out elsewhere, from the menu or another tab: say so.
  const live = useMe()
  useEffect(() => {
    if (live.account === null) setReady('signed-out')
  }, [live.account])

  useEffect(() => {
    me()
      .then(async (found) => {
        if (!found) return setReady('signed-out')
        if (!found.account.wallet || !found.account.wallet_verified) return setReady('no-wallet')
        setPayTo(found.account.wallet)
        const editing = id && found.listings.find((l: Listing) => String(l.id) === id)
        if (editing) {
          setTitle(editing.title)
          setDescription(editing.description)
          setCategory(editing.category)
          setPrice(plainOrn(editing.price))
          setUnit(editing.unit)
          setPictures(editing.pictures.map((saved) => ({ saved })))
          setOriginal(editing.pictures)
        }
        setReady('ready')
      })
      .catch((error: Error) => {
        setTrouble(error.message)
        setReady('ready')
      })
  }, [id])

  if (ready === 'checking') return <p className="text-muted">Loading…</p>
  if (ready === 'no-wallet') {
    return (
      <Notice>
        To sell, first{' '}
        <Link href="/account/" className="font-semibold text-ink underline underline-offset-4">
          add and verify a wallet
        </Link>{' '}
        under Your wallets, so buyers know their payments reach you. Verifying sends less than 0.001 ORN from it; don&apos;t
        have any yet? Mine for an hour in Orien Wallet, or buy a little on Buy &amp; sell.
      </Notice>
    )
  }
  if (ready === 'signed-out') {
    return (
      <Notice>
        To sell something,{' '}
        <Link href="/account/" className="font-semibold text-ink underline underline-offset-4">
          sign in or make an account
        </Link>{' '}
        first. It takes a few seconds, with your email.
      </Notice>
    )
  }

  const save = async (event: React.FormEvent) => {
    event.preventDefault()
    const units = parseOrn(price)
    if (units === null) return setTrouble('Write the price in ORN, like 200 or 12.5.')
    const draft: Draft = { title, description, category, price: units, unit: perUnit(unit), pay_to: payTo }
    setSaving(true)
    setTrouble('')
    let listing: number | null = null
    try {
      const saved = id
        ? await ask<{ listing: Listing }>(`/listings/${encodeURIComponent(id)}`, 'PUT', draft)
        : await ask<{ listing: Listing }>('/listings', 'POST', draft)
      listing = saved.listing.id
      // Pictures taken off, then the new ones added, then the first made the cover.
      const kept = new Set(pictures.flatMap((p) => ('saved' in p ? [p.saved] : [])))
      for (const name of original.filter((name) => !kept.has(name))) {
        await ask(`/listings/${listing}/pictures/${name}`, 'DELETE')
      }
      let names = [...kept]
      let cover: string | null = null
      for (const [n, item] of pictures.entries()) {
        let name = 'saved' in item ? item.saved : null
        if (!name && 'file' in item) {
          const after = await upload(listing, item.file)
          name = after.pictures.find((p) => !names.includes(p)) ?? null
          if (name) names = [...names, name]
        }
        if (n === 0) cover = name
      }
      if (cover) await ask(`/listings/${listing}/pictures/${cover}`, 'POST')
      router.push(`/marketplace/listing/?id=${listing}`)
    } catch (error) {
      const message = sentence((error as Error).message)
      setTrouble(listing && !id ? `Your listing is posted, but: ${message}` : message)
      // What was saved is now this listing: saving again edits it.
      if (listing && !id) router.replace(`/marketplace/sell/?id=${listing}`)
      setSaving(false)
    }
  }

  return (
    <form onSubmit={save} className="space-y-5 rounded-2xl border border-line bg-surface p-6 sm:p-8">
      <h1 className="text-2xl font-bold tracking-tight text-ink">{id ? 'Edit your listing' : 'Sell something'}</h1>
      <label className="flex flex-col gap-2">
        <span className="text-sm font-semibold text-ink">What are you offering?</span>
        <input value={title} onChange={(e) => setTitle(e.target.value)} maxLength={100} required placeholder="Logo design" className={field} />
      </label>
      <label className="flex flex-col gap-2">
        <span className="text-sm font-semibold text-ink">Describe it</span>
        <AutoTextarea
          value={description}
          onChange={(e) => setDescription(e.target.value)}
          maxLength={5000}
          rows={6}
          placeholder="What the buyer gets, how long it takes, anything they should know."
          className={field}
        />
      </label>
      <label className="flex flex-col gap-2">
        <span className="text-sm font-semibold text-ink">Category</span>
        <span className="relative block">
          <select
            value={category}
            onChange={(e) => setCategory(e.target.value)}
            className={field}
          >
            {CATEGORIES.map((c) => (
              <option key={c.id} value={c.id}>
                {c.label}
              </option>
            ))}
          </select>
        </span>
      </label>
      <div className="space-y-2">
        <span className="text-sm font-semibold text-ink">Pictures</span>
        <p className="text-sm text-muted">
          Up to four: examples of your work, or the thing you are selling. The first is the one shown on your listing&apos;s card.
        </p>
        <div className="flex flex-wrap gap-3">
          {pictures.map((item, n) => {
            const src = 'saved' in item ? pictureUrl(item.saved) : item.preview
            return (
              <div key={src} className="relative w-36 overflow-hidden rounded-xl border border-line">
                {/* eslint-disable-next-line @next/next/no-img-element */}
                <img src={src} alt="" className="aspect-video w-full object-cover" />
                {n === 0 ? (
                  <span className="absolute left-2 top-2 rounded-full bg-bg/85 px-2 py-0.5 text-[11px] font-semibold text-ink">Cover</span>
                ) : (
                  <button
                    type="button"
                    onClick={() => setPictures([item, ...pictures.filter((_, k) => k !== n)])}
                    className="absolute left-2 top-2 flex items-center gap-1 rounded-full bg-bg/85 px-2 py-0.5 text-[11px] font-semibold text-ink hover:bg-bg"
                  >
                    <Star className="h-3 w-3" aria-hidden /> Make cover
                  </button>
                )}
                <button
                  type="button"
                  onClick={() => setPictures(pictures.filter((_, k) => k !== n))}
                  aria-label="Remove this picture"
                  className="absolute right-2 top-2 flex h-6 w-6 items-center justify-center rounded-full bg-bg/85 text-ink hover:bg-bg"
                >
                  <X className="h-3.5 w-3.5" aria-hidden />
                </button>
              </div>
            )
          })}
          {pictures.length < MAX_PICTURES && (
            <label className="flex aspect-video w-36 cursor-pointer flex-col items-center justify-center gap-1.5 rounded-xl border border-dashed border-line text-sm text-muted transition hover:border-pearl/60 hover:text-ink">
              <ImagePlus className="h-5 w-5" aria-hidden />
              Add
              <input
                type="file"
                accept="image/*"
                multiple
                className="hidden"
                onChange={(e) => {
                  const files = [...(e.target.files ?? [])].slice(0, MAX_PICTURES - pictures.length)
                  setPictures([...pictures, ...files.map((file) => ({ file, preview: URL.createObjectURL(file) }))])
                  e.target.value = ''
                }}
              />
            </label>
          )}
        </div>
      </div>
      <div className="grid gap-5 sm:grid-cols-2">
        <label className="flex flex-col gap-2">
          <span className="text-sm font-semibold text-ink">How is it priced?</span>
          <div className="relative">
            <select
              value={PRICINGS.includes(unit) ? unit : 'other'}
              onChange={(e) => setUnit(e.target.value === 'other' ? 'per ' : e.target.value)}
              className={field}
            >
              <option value="">Fixed price</option>
              {PRICINGS.filter(Boolean).map((p) => (
                <option key={p} value={p}>
                  {p.charAt(0).toUpperCase() + p.slice(1)}
                </option>
              ))}
              <option value="other">Other…</option>
            </select>
          </div>
          {!PRICINGS.includes(unit) && (
            <input
              value={unit}
              onChange={(e) => setUnit(e.target.value)}
              maxLength={30}
              placeholder="per page"
              aria-label="Priced per what?"
              className={field}
            />
          )}
        </label>
        <label className="flex flex-col gap-2">
          <span className="text-sm font-semibold text-ink">{unit.trim() ? `Rate in ORN, ${perUnit(unit)}` : 'Price in ORN'}</span>
          <input value={price} onChange={(e) => setPrice(e.target.value)} inputMode="decimal" required placeholder="200" className={`num ${field}`} />
        </label>
      </div>
      <p className="rounded-xl border border-line bg-bg px-4 py-3 text-sm text-muted">
        Buyers pay you straight to your wallet,{' '}
        <code className="num text-ink">
          {payTo.slice(0, 8)}…{payTo.slice(-8)}
        </code>
        .
      </p>
      {trouble && <Notice tone="bad">{trouble}</Notice>}
      <button
        type="submit"
        disabled={saving}
        className="pearl-fill flex h-11 items-center rounded-xl px-6 text-sm font-bold transition hover:brightness-105 disabled:opacity-60"
      >
        {saving ? 'Saving…' : id ? 'Save changes' : 'Post your listing'}
      </button>
    </form>
  )
}
