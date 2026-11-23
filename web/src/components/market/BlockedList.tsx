'use client'

import Link from 'next/link'
import { useEffect, useState } from 'react'
import { ArrowLeft } from 'lucide-react'
import { ask, MarketError, sentence } from '@/lib/market'
import { Avatar } from './Avatar'
import { Notice } from './Shell'

type Blocked = { id: number; name: string; picture: string | null; blocked_at: number }

/** The people you've blocked, newest first, each with an Unblock button.
 * Blocked people and you can't see each other until you unblock them. */
export function BlockedList() {
  const [people, setPeople] = useState<Blocked[] | null>(null)
  const [signedOut, setSignedOut] = useState(false)
  const [trouble, setTrouble] = useState('')
  const [busy, setBusy] = useState<number | null>(null)

  useEffect(() => {
    ask<{ blocked: Blocked[] }>('/me/blocked')
      .then((found) => setPeople(found.blocked))
      .catch((error) => {
        if (error instanceof MarketError && error.status === 401) setSignedOut(true)
        else setTrouble(sentence((error as Error).message))
      })
  }, [])

  const unblock = async (person: Blocked) => {
    setBusy(person.id)
    setTrouble('')
    try {
      await ask(`/accounts/${person.id}/block`, 'POST', { block: false })
      setPeople((list) => list?.filter((p) => p.id !== person.id) ?? null)
    } catch (error) {
      setTrouble(sentence((error as Error).message))
    }
    setBusy(null)
  }

  return (
    <div className="space-y-6">
      <div className="space-y-3">
        <Link href="/account/" className="inline-flex items-center gap-1.5 text-sm font-semibold text-muted hover:text-ink">
          <ArrowLeft className="h-4 w-4" aria-hidden /> Your account
        </Link>
        <h1 className="text-3xl font-bold tracking-tight text-ink">Blocked people</h1>
        <p className="leading-relaxed text-muted">
          You and the people you block can’t see each other’s profiles, posts or listings, and they can’t message you.
          Unblock someone to see each other again.
        </p>
      </div>
      {signedOut ? (
        <Notice>
          <Link href="/account/" className="font-semibold text-ink underline underline-offset-4">
            Sign in
          </Link>{' '}
          to see who you’ve blocked.
        </Notice>
      ) : trouble && !people ? (
        <Notice tone="bad">{trouble}</Notice>
      ) : !people ? (
        <p className="text-muted">Loading…</p>
      ) : (
        <div className="space-y-3">
          {trouble && <Notice tone="bad">{trouble}</Notice>}
          {people.length === 0 ? (
            <p className="rounded-2xl border border-line bg-surface p-6 text-muted">You haven’t blocked anyone.</p>
          ) : (
            <ul className="divide-y divide-line overflow-hidden rounded-2xl border border-line bg-surface">
              {people.map((person) => (
                <li key={person.id} className="flex items-center gap-4 px-5 py-4">
                  <Avatar name={person.name} picture={person.picture} size={44} />
                  <span className="min-w-0 flex-1 truncate font-semibold text-ink">{person.name}</span>
                  <button
                    type="button"
                    onClick={() => unblock(person)}
                    disabled={busy !== null}
                    className="flex h-10 shrink-0 items-center rounded-xl border border-line bg-bg px-4 text-sm font-semibold text-ink transition hover:bg-raised disabled:opacity-60"
                  >
                    {busy === person.id ? 'Unblocking…' : 'Unblock'}
                  </button>
                </li>
              ))}
            </ul>
          )}
        </div>
      )}
    </div>
  )
}
