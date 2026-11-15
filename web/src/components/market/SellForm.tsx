'use client'

import Link from 'next/link'
import { useRouter, useSearchParams } from 'next/navigation'
import { useEffect, useState } from 'react'
import { ChevronDown } from 'lucide-react'
import { ask, CATEGORIES, me, parseOrn, plainOrn, type Draft, type Listing } from '@/lib/market'
import { Notice } from './Shell'

const field = 'w-full rounded-xl border border-line bg-bg px-4 py-3 text-ink outline-none focus:border-pearl/60'

export function SellForm() {
  const router = useRouter()
  const id = useSearchParams().get('id')
  const [ready, setReady] = useState<'checking' | 'signed-out' | 'no-wallet' | 'no-contact' | 'ready'>('checking')
  const [title, setTitle] = useState('')
  const [description, setDescription] = useState('')
  const [category, setCategory] = useState('design')
  const [price, setPrice] = useState('')
  const [unit, setUnit] = useState('')
  const [payTo, setPayTo] = useState('')
  const [trouble, setTrouble] = useState('')
  const [saving, setSaving] = useState(false)

  useEffect(() => {
    me()
      .then(async (found) => {
        if (!found) return setReady('signed-out')
        if (!found.account.wallet) return setReady('no-wallet')
        const { email, telegram, x } = found.account.contact
        if (!email && !telegram && !x) return setReady('no-contact')
        setPayTo(found.account.wallet)
        const editing = id && found.listings.find((l: Listing) => String(l.id) === id)
        if (editing) {
          setTitle(editing.title)
          setDescription(editing.description)
          setCategory(editing.category)
          setPrice(plainOrn(editing.price))
          setUnit(editing.unit)
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
        Before your first listing,{' '}
        <Link href="/marketplace/account/" className="font-semibold text-ink underline underline-offset-4">
          verify your Orien wallet
        </Link>
        , so buyers&apos; payments are sure to reach you. It takes a minute.
      </Notice>
    )
  }
  if (ready === 'no-contact') {
    return (
      <Notice>
        Before your first listing,{' '}
        <Link href="/marketplace/account/" className="font-semibold text-ink underline underline-offset-4">
          add a way for buyers to reach you
        </Link>{' '}
        to your profile: an email, Telegram or X.
      </Notice>
    )
  }
  if (ready === 'signed-out') {
    return (
      <Notice>
        To sell something,{' '}
        <Link href="/marketplace/account/" className="font-semibold text-ink underline underline-offset-4">
          sign in or make an account
        </Link>{' '}
        first. It takes a few seconds, with a passkey.
      </Notice>
    )
  }

  const save = async (event: React.FormEvent) => {
    event.preventDefault()
    const units = parseOrn(price)
    if (units === null) return setTrouble('Write the price in ORN, like 200 or 12.5.')
    const draft: Draft = { title, description, category, price: units, unit, pay_to: payTo }
    setSaving(true)
    setTrouble('')
    try {
      const saved = id
        ? await ask<{ listing: Listing }>(`/listings/${encodeURIComponent(id)}`, 'PUT', draft)
        : await ask<{ listing: Listing }>('/listings', 'POST', draft)
      router.push(`/marketplace/listing/?id=${saved.listing.id}`)
    } catch (error) {
      setTrouble((error as Error).message)
      setSaving(false)
    }
  }

  return (
    <form onSubmit={save} className="space-y-5 rounded-2xl border border-line bg-surface p-6 sm:p-8">
      <h1 className="text-2xl font-bold tracking-tight text-ink">{id ? 'Edit your listing' : 'Sell something'}</h1>
      <label className="block space-y-2">
        <span className="text-sm font-semibold text-ink">What are you offering?</span>
        <input value={title} onChange={(e) => setTitle(e.target.value)} maxLength={100} required placeholder="Logo design" className={field} />
      </label>
      <label className="block space-y-2">
        <span className="text-sm font-semibold text-ink">Describe it</span>
        <textarea
          value={description}
          onChange={(e) => setDescription(e.target.value)}
          maxLength={5000}
          rows={6}
          placeholder="What the buyer gets, how long it takes, anything they should know."
          className={field}
        />
      </label>
      <label className="block space-y-2">
        <span className="text-sm font-semibold text-ink">Category</span>
        <span className="relative block">
          <select
            value={category}
            onChange={(e) => setCategory(e.target.value)}
            className={`appearance-none pr-12 ${field}`}
          >
            {CATEGORIES.map((c) => (
              <option key={c.id} value={c.id}>
                {c.label}
              </option>
            ))}
          </select>
          <ChevronDown
            className="pointer-events-none absolute right-4 top-1/2 h-4 w-4 -translate-y-1/2 text-muted"
            strokeWidth={2.25}
            aria-hidden
          />
        </span>
      </label>
      <div className="grid gap-5 sm:grid-cols-2">
        <label className="block space-y-2">
          <span className="text-sm font-semibold text-ink">Price in ORN</span>
          <input value={price} onChange={(e) => setPrice(e.target.value)} inputMode="decimal" required placeholder="200" className={`num ${field}`} />
        </label>
        <label className="block space-y-2">
          <span className="text-sm font-semibold text-ink">Per (optional)</span>
          <input value={unit} onChange={(e) => setUnit(e.target.value)} maxLength={30} placeholder="per hour" className={field} />
        </label>
      </div>
      <p className="rounded-xl border border-line bg-bg px-4 py-3 text-sm text-muted">
        Buyers pay you straight to your verified wallet,{' '}
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
