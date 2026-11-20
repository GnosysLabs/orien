'use client'

import Link from 'next/link'
import { useEffect, useState } from 'react'
import { LogOut, Mail, MessageCircle, UserRound } from 'lucide-react'
import { ask, me, takeWalletPass, NAME_PATTERN, NAME_RULE, profileHref, type Account, type Listing } from '@/lib/market'
import { Card } from './Browse'
import { ProfileEditor } from './ProfileEditor'
import { YourWallets } from './WalletCheck'
import { YourWebsites } from './YourWebsites'
import { WalletSignIn } from './WalletSignIn'
import { Notice, SIGNED_IN_OR_OUT } from './Shell'
import { useMe } from '@/lib/useMe'

type Found = { account: Account; listings: Listing[]; unread?: number }

export function AccountView() {
  const [found, setFound] = useState<Found | null | undefined>(undefined)
  const [trouble, setTrouble] = useState('')
  const reload = () =>
    me()
      .then((next) => {
        if ((next === null) !== (found === null || found === undefined)) window.dispatchEvent(new Event(SIGNED_IN_OR_OUT))
        setFound(next)
      })
      .catch((error: Error) => {
        setTrouble(error.message)
        setFound(null)
      })
  useEffect(() => {
    // A pass from Orien Wallet signs this browser in without an email code.
    void takeWalletPass().then(reload)
  }, [])

  // Signed out elsewhere, from the menu or another tab: show the sign-in.
  const live = useMe()
  useEffect(() => {
    if (live.account === null && found) setFound(null)
  }, [live.account])

  if (found === undefined) return <p className="text-muted">Loading…</p>
  if (found === null) return <SignIn onDone={reload} trouble={trouble} wallet />
  return <Mine found={found} onChange={reload} />
}

type Step = 'email' | 'code' | 'name'

export function SignIn({
  onDone,
  trouble: first,
  title = 'Sign in to Orien Network',
  blurb,
  wallet = false,
}: {
  onDone: () => void
  trouble: string
  /** For signing in on the way to something else, such as another website. */
  title?: string
  blurb?: React.ReactNode
  /** Offer signing in with Orien Wallet, through its browser extension. */
  wallet?: boolean
}) {
  const [step, setStep] = useState<Step>('email')
  const [email, setEmail] = useState('')
  const [code, setCode] = useState('')
  const [name, setName] = useState('')
  const [busy, setBusy] = useState(false)
  const [trouble, setTrouble] = useState(first)

  const run = async (work: () => Promise<void>) => {
    setBusy(true)
    setTrouble('')
    try {
      await work()
    } catch (error) {
      setTrouble((error as Error).message)
    }
    setBusy(false)
  }

  const sendCode = (event: React.FormEvent) => {
    event.preventDefault()
    void run(async () => {
      const { sent } = await ask<{ sent: string }>('/email/start', 'POST', { email })
      setEmail(sent)
      setCode('')
      setStep('code')
    })
  }

  const finish = (event: React.FormEvent) => {
    event.preventDefault()
    void run(async () => {
      const answer = await ask<{ needs_name?: boolean }>('/email/finish', 'POST', { email, code, name })
      if (answer.needs_name) setStep('name')
      else onDone()
    })
  }

  const field = 'h-11 w-full rounded-xl border border-line bg-bg px-4 text-ink outline-none focus:border-pearl/60'
  const button =
    'pearl-fill flex h-11 w-full items-center justify-center rounded-xl px-6 text-sm font-bold transition hover:brightness-105 disabled:opacity-60'

  return (
    <section className="mx-auto max-w-[440px] rounded-2xl border border-line bg-surface p-6 sm:p-8">
      <Mail className="mb-4 h-6 w-6 text-pearl" strokeWidth={1.75} aria-hidden />
      {step === 'email' && (
        <form onSubmit={sendCode} className="space-y-4">
          <h1 className="text-2xl font-bold tracking-tight text-ink">{title}</h1>
          <p className="leading-relaxed text-muted">
            {blurb ?? (
              <>
                One account for your profile, the Market and messages. We&apos;ll email you a 6-digit code; no password
                needed, and new emails make an account.
              </>
            )}
          </p>
          {wallet && (
            <>
              <WalletSignIn className={button} />
              <p className="text-center text-sm text-muted">or with your email</p>
            </>
          )}
          <input
            type="email"
            value={email}
            onChange={(e) => setEmail(e.target.value)}
            required
            autoFocus
            autoComplete="email"
            placeholder="you@example.com"
            className={field}
          />
          <button type="submit" disabled={busy} className={button}>
            {busy ? 'Sending…' : 'Email me a code'}
          </button>
        </form>
      )}
      {step === 'code' && (
        <form onSubmit={finish} className="space-y-4">
          <h1 className="text-2xl font-bold tracking-tight text-ink">Check your email</h1>
          <p className="leading-relaxed text-muted">
            We sent a code to <strong className="font-semibold text-ink">{email}</strong>. It works for 15 minutes.
          </p>
          <input
            value={code}
            onChange={(e) => setCode(e.target.value.replace(/\D/g, '').slice(0, 6))}
            required
            autoFocus
            inputMode="numeric"
            autoComplete="one-time-code"
            placeholder="123456"
            className={`num text-center text-xl tracking-[0.4em] ${field}`}
          />
          <button type="submit" disabled={busy || code.length !== 6} className={button}>
            {busy ? 'Checking…' : 'Sign in'}
          </button>
          <p className="flex items-center justify-center gap-3 text-sm text-muted">
            Wrong email or no code?
            <button
              type="button"
              onClick={() => setStep('email')}
              className="flex h-8 items-center rounded-lg border border-line px-3 text-xs font-semibold text-ink transition hover:border-pearl"
            >
              Start again
            </button>
          </p>
        </form>
      )}
      {step === 'name' && (
        <form onSubmit={finish} className="space-y-4">
          <h1 className="text-2xl font-bold tracking-tight text-ink">Welcome! Choose a username</h1>
          <p className="leading-relaxed text-muted">
            It&apos;s what people see, and your profile&apos;s address: orien.network/u/{name || 'username'}. Use {NAME_RULE}.
            You can change it later.
          </p>
          <input
            value={name}
            onChange={(e) => setName(e.target.value.replace(/\s/g, '_'))}
            maxLength={20}
            minLength={3}
            pattern={NAME_PATTERN}
            title={`Your username: ${NAME_RULE}`}
            required
            autoFocus
            spellCheck={false}
            autoCapitalize="off"
            placeholder="Username"
            className={field}
          />
          <button type="submit" disabled={busy} className={button}>
            Make my account
          </button>
        </form>
      )}
      {trouble && (
        <div className="mt-4">
          <Notice tone="bad">{trouble.charAt(0).toUpperCase() + trouble.slice(1)}{trouble.endsWith('.') ? '' : '.'}</Notice>
        </div>
      )}
    </section>
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
        <h1 className="text-2xl font-bold tracking-tight text-ink sm:text-3xl">Your account</h1>
        <div className="ml-auto flex flex-wrap items-center gap-2">
          <Link
            href={profileHref(found.account.name)}
            className="flex h-9 items-center gap-2 rounded-lg border border-line px-3 text-sm font-semibold text-ink transition hover:border-pearl"
          >
            <UserRound className="h-4 w-4" aria-hidden /> View profile
          </Link>
          <Link
            href="/marketplace/messages/"
            className="flex h-9 items-center gap-2 rounded-lg border border-line px-3 text-sm font-semibold text-ink transition hover:border-pearl"
          >
            <MessageCircle className="h-4 w-4" aria-hidden /> Messages
            {(found.unread ?? 0) > 0 && (
              <span className="flex h-5 min-w-5 items-center justify-center rounded-full bg-pearl px-1.5 text-[11px] font-bold text-bg">
                {found.unread}
              </span>
            )}
          </Link>
          <button
            type="button"
            onClick={signOut}
            className="flex h-9 items-center gap-2 rounded-lg border border-line px-3 text-sm font-semibold text-muted transition hover:border-pearl hover:text-ink"
          >
            <LogOut className="h-4 w-4" aria-hidden /> Sign out
          </button>
        </div>
      </div>
      <ProfileEditor account={found.account} onSaved={onChange} />
      <YourWallets account={found.account} onChange={onChange} />
      <h2 className="pt-2 text-xl font-bold tracking-tight text-ink">Your listings on the Market</h2>
      {trouble && <Notice tone="bad">{trouble}</Notice>}
      {found.listings.length === 0 ? (
        <Notice>
          You have no listings yet.{' '}
          <Link href="/marketplace/sell/" className="font-semibold text-ink underline underline-offset-4">
            Sell something.
          </Link>
        </Notice>
      ) : (
        <div className="grid gap-5 sm:grid-cols-2 lg:grid-cols-3">
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
      <YourWebsites />
      <DeleteAccount name={found.account.name} onDeleted={onChange} />
    </div>
  )
}

/** Deleting the account, at the very bottom, behind typing one's name. */
function DeleteAccount({ name, onDeleted }: { name: string; onDeleted: () => void }) {
  const [open, setOpen] = useState(false)
  const [typed, setTyped] = useState('')
  const [trouble, setTrouble] = useState('')
  if (!open) {
    return (
      <div className="border-t border-line pt-6">
        <button type="button" onClick={() => setOpen(true)} className="flex h-9 items-center rounded-lg border border-line px-3 text-sm font-semibold text-muted transition hover:border-stop hover:text-stop">
          Delete my account
        </button>
      </div>
    )
  }
  const remove = async (event: React.FormEvent) => {
    event.preventDefault()
    setTrouble('')
    try {
      await ask('/me/delete', 'POST', { confirm: typed })
      onDeleted()
    } catch (error) {
      setTrouble((error as Error).message)
    }
  }
  return (
    <form onSubmit={remove} className="space-y-3 rounded-2xl border border-stop/40 bg-stop/5 p-6">
      <h2 className="text-lg font-bold tracking-tight text-ink">Delete your account</h2>
      <p className="text-sm leading-relaxed text-muted">
        This deletes your account and everything you made here: your profile, listings and pictures, your conversations and
        messages, and your swap offers. It cannot be undone. Your Orien wallet and coins are not touched.
      </p>
      <label className="flex flex-col gap-2">
        <span className="text-sm text-ink">
          Type your name, <strong>{name}</strong>, to confirm
        </span>
        <input
          value={typed}
          onChange={(e) => setTyped(e.target.value)}
          className="h-11 w-full rounded-xl border border-line bg-bg px-4 text-ink outline-none focus:border-stop/60"
        />
      </label>
      {trouble && <Notice tone="bad">{trouble}</Notice>}
      <div className="flex items-center gap-4">
        <button
          type="submit"
          disabled={typed.trim() !== name}
          className="h-10 rounded-xl bg-stop px-4 text-sm font-bold text-bg disabled:opacity-40"
        >
          Delete everything
        </button>
        <button type="button" onClick={() => setOpen(false)} className="text-sm text-muted hover:text-ink">
          Cancel
        </button>
      </div>
    </form>
  )
}
