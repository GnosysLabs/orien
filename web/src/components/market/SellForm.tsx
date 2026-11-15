'use client'

import Link from 'next/link'
import { useRouter, useSearchParams } from 'next/navigation'
import { useEffect, useState } from 'react'
import { ask, CATEGORIES, me, parseOrn, plainOrn, type Draft, type Listing } from '@/lib/market'
import { Notice } from './Shell'

const field = 'w-full rounded-xl border border-line bg-bg px-4 py-3 text-ink outline-none focus:border-pearl/60'

export function SellForm() {
  const router = useRouter()
  const id = useSearchParams().get('id')
  const [ready, setReady] = useState<'checking' | 'signed-out' | 'ready'>('checking')
  const [title, setTitle] = useState('')
  const [description, setDescription] = useState('')
  const [category, setCategory] = useState('design')
  const [price, setPrice] = useState('')
  const [unit, setUnit] = useState('')
  const [payTo, setPayTo] = useState('')
  const [contact, setContact] = useState('')
  const [trouble, setTrouble] = useState('')
  const [saving, setSaving] = useState(false)

  useEffect(() => {
    me()
      .then(async (found) => {
        if (!found) return setReady('signed-out')
        const editing = id && found.listings.find((l: Listing) => String(l.id) === id)
        if (editing) {
          setTitle(editing.title)
          setDescription(editing.description)
          setCategory(editing.category)
          setPrice(plainOrn(editing.price))
          setUnit(editing.unit)
          setPayTo(editing.pay_to)
          setContact(editing.contact)
        } else {
          // A new listing pays where the seller's last one did.
          const last = found.listings[0]
          if (last) {
            setPayTo(last.pay_to)
            setContact(last.contact)
          }
        }
        setReady('ready')
      })
      .catch((error: Error) => {
        setTrouble(error.message)
        setReady('ready')
      })
  }, [id])

  if (ready === 'checking') return <p className="text-muted">Loading…</p>
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
    const draft: Draft = { title, description, category, price: units, unit, pay_to: payTo, contact }
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
        <select value={category} onChange={(e) => setCategory(e.target.value)} className={field}>
          {CATEGORIES.map((c) => (
            <option key={c.id} value={c.id}>
              {c.label}
            </option>
          ))}
        </select>
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
      <label className="block space-y-2">
        <span className="text-sm font-semibold text-ink">Your Orien address, where buyers pay you</span>
        <input
          value={payTo}
          onChange={(e) => setPayTo(e.target.value)}
          required
          spellCheck={false}
          placeholder="64 letters and digits, from Orien Wallet or orien wallet address"
          className={`num text-sm ${field}`}
        />
      </label>
      <label className="block space-y-2">
        <span className="text-sm font-semibold text-ink">How buyers can reach you</span>
        <input
          value={contact}
          onChange={(e) => setContact(e.target.value)}
          maxLength={300}
          placeholder="An email, a Telegram or X handle, or a website"
          className={field}
        />
        <span className="block text-xs text-muted">Everyone can see this, so share only what you are happy to make public.</span>
      </label>
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
