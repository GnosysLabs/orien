'use client'

import Link from 'next/link'
import { useEffect, useState } from 'react'
import { KeyRound, LogOut } from 'lucide-react'
import {
  ask,
  createPasskey,
  me,
  passkeysWork,
  passkeyTrouble,
  usePasskey,
  type Account,
  type Listing,
} from '@/lib/market'
import { Card } from './Browse'
import { Notice } from './Shell'

type Found = { account: Account; listings: Listing[] }

export function AccountView() {
  const [found, setFound] = useState<Found | null | undefined>(undefined)
  const [trouble, setTrouble] = useState('')
  const reload = () =>
    me()
      .then(setFound)
      .catch((error: Error) => {
        setTrouble(error.message)
        setFound(null)
      })
  useEffect(() => {
    void reload()
  }, [])

  if (found === undefined) return <p className="text-muted">Loading…</p>
  if (found === null) return <SignIn onDone={reload} trouble={trouble} />
  return <Mine found={found} onChange={reload} />
}

function SignIn({ onDone, trouble: first }: { onDone: () => void; trouble: string }) {
  const [name, setName] = useState('')
  const [busy, setBusy] = useState(false)
  const [trouble, setTrouble] = useState(first)

  if (!passkeysWork()) {
    return <Notice tone="bad">This browser cannot use passkeys. Try a current Safari, Chrome, Edge or Firefox.</Notice>
  }

  const signIn = async () => {
    setBusy(true)
    setTrouble('')
    try {
      const { id, options } = await ask<{ id: string; options: Record<string, unknown> }>('/signin/start', 'POST')
      const credential = await usePasskey(options)
      await ask('/signin/finish', 'POST', { id, credential })
      onDone()
    } catch (error) {
      setTrouble(passkeyTrouble(error))
    }
    setBusy(false)
  }

  const signUp = async (event: React.FormEvent) => {
    event.preventDefault()
    setBusy(true)
    setTrouble('')
    try {
      const { id, options } = await ask<{ id: string; options: Record<string, unknown> }>('/signup/start', 'POST', { name })
      const credential = await createPasskey(options)
      await ask('/signup/finish', 'POST', { id, credential })
      onDone()
    } catch (error) {
      setTrouble(passkeyTrouble(error))
    }
    setBusy(false)
  }

  return (
    <div className="space-y-6">
      <section className="rounded-2xl border border-line bg-surface p-6 sm:p-8">
        <div className="mb-3 flex items-center gap-3">
          <KeyRound className="h-5 w-5 text-pearl" strokeWidth={1.75} aria-hidden />
          <h1 className="text-xl font-bold tracking-tight text-ink">Sign in</h1>
        </div>
        <p className="mb-5 leading-relaxed text-muted">
          With the passkey you made here: Face ID, a fingerprint, or your computer&apos;s own lock. No password.
        </p>
        <button
          type="button"
          onClick={signIn}
          disabled={busy}
          className="pearl-fill flex h-11 items-center rounded-xl px-6 text-sm font-bold transition hover:brightness-105 disabled:opacity-60"
        >
          Sign in with a passkey
        </button>
      </section>
      <form onSubmit={signUp} className="rounded-2xl border border-line bg-surface p-6 sm:p-8">
        <h2 className="mb-3 text-xl font-bold tracking-tight text-ink">New here? Make an account</h2>
        <p className="mb-5 leading-relaxed text-muted">
          Pick the name buyers will see. Your device then makes a passkey for the marketplace, which is all you need to
          sign in from then on.
        </p>
        <div className="flex flex-wrap gap-3">
          <input
            value={name}
            onChange={(e) => setName(e.target.value)}
            maxLength={50}
            required
            placeholder="Your name or your business's"
            className="h-11 min-w-0 flex-1 rounded-xl border border-line bg-bg px-4 text-ink outline-none focus:border-pearl/60"
          />
          <button
            type="submit"
            disabled={busy}
            className="flex h-11 items-center rounded-xl border border-line bg-raised px-6 text-sm font-bold text-ink transition hover:bg-line disabled:opacity-60"
          >
            Make my account
          </button>
        </div>
      </form>
      {trouble && <Notice tone="bad">{trouble}</Notice>}
    </div>
  )
}

function Mine({ found, onChange }: { found: Found; onChange: () => void }) {
  const [trouble, setTrouble] = useState('')
  const signOut = async () => {
    await ask('/signout', 'POST').catch(() => {})
    onChange()
  }
  const setOpen = async (listing: Listing, open: boolean) => {
    setTrouble('')
    try {
      await ask(`/listings/${listing.id}/open`, 'POST', { open })
      onChange()
    } catch (error) {
      setTrouble((error as Error).message)
    }
  }
  return (
    <div className="space-y-6">
      <div className="flex flex-wrap items-center gap-3">
        <h1 className="text-2xl font-bold tracking-tight text-ink">{found.account.name}</h1>
        <button
          type="button"
          onClick={signOut}
          className="ml-auto flex h-9 items-center gap-2 rounded-lg px-3 text-sm font-semibold text-muted hover:bg-raised hover:text-ink"
        >
          <LogOut className="h-4 w-4" aria-hidden /> Sign out
        </button>
      </div>
      {trouble && <Notice tone="bad">{trouble}</Notice>}
      {found.listings.length === 0 ? (
        <Notice>
          You have no listings yet.{' '}
          <Link href="/marketplace/sell/" className="font-semibold text-ink underline underline-offset-4">
            Sell something.
          </Link>
        </Notice>
      ) : (
        <div className="grid gap-4 sm:grid-cols-2">
          {found.listings.map((listing) => (
            <div key={listing.id} className="flex flex-col gap-2">
              <Card listing={listing} />
              <div className="flex gap-2">
                <Link
                  href={`/marketplace/sell/?id=${listing.id}`}
                  className="flex h-9 items-center rounded-lg border border-line px-3 text-sm font-semibold text-ink hover:bg-raised"
                >
                  Edit
                </Link>
                <button
                  type="button"
                  onClick={() => setOpen(listing, !listing.open)}
                  className="flex h-9 items-center rounded-lg border border-line px-3 text-sm font-semibold text-ink hover:bg-raised"
                >
                  {listing.open ? 'Close' : 'Reopen'}
                </button>
              </div>
            </div>
          ))}
        </div>
      )}
    </div>
  )
}
