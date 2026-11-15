'use client'

import Link from 'next/link'
import { useRouter, useSearchParams } from 'next/navigation'
import { useCallback, useEffect, useRef, useState } from 'react'
import { ArrowLeft, Ban, Send } from 'lucide-react'
import { ago, ask, MarketError, sentence, type Conversation, type Message } from '@/lib/market'
import { Avatar } from './Avatar'
import { Notice, SIGNED_IN_OR_OUT } from './Shell'

/** Every conversation, or one of them when the address names it. */
export function Messages() {
  const id = useSearchParams().get('c')
  return id ? <Thread id={id} /> : <Inbox />
}

function SignInFirst() {
  return (
    <Notice>
      <Link href="/marketplace/account/" className="font-semibold text-ink underline underline-offset-4">
        Sign in
      </Link>{' '}
      to see your messages.
    </Notice>
  )
}

function Inbox() {
  const [conversations, setConversations] = useState<Conversation[] | null>(null)
  const [signedOut, setSignedOut] = useState(false)
  const [trouble, setTrouble] = useState('')

  useEffect(() => {
    const load = () =>
      ask<{ conversations: Conversation[] }>('/conversations')
        .then((found) => setConversations(found.conversations))
        .catch((error) => {
          if (error instanceof MarketError && error.status === 401) setSignedOut(true)
          else setTrouble(sentence((error as Error).message))
        })
    void load()
    const timer = setInterval(load, 15000)
    return () => clearInterval(timer)
  }, [])

  if (signedOut) return <SignInFirst />
  if (trouble) return <Notice tone="bad">{trouble}</Notice>
  if (!conversations) return <p className="text-muted">Loading…</p>
  return (
    <div className="space-y-4">
      <h1 className="text-2xl font-bold tracking-tight text-ink">Messages</h1>
      {conversations.length === 0 ? (
        <Notice>No messages yet. To ask a seller something, open their listing and write to them there.</Notice>
      ) : (
        <ul className="overflow-hidden rounded-2xl border border-line bg-surface">
          {conversations.map((c) => (
            <li key={c.id} className="border-b border-line last:border-b-0">
              <Link href={`/marketplace/messages/?c=${c.id}`} className="flex items-center gap-4 p-4 transition hover:bg-raised/50">
                <Avatar name={c.with_name} picture={c.with_picture} size={44} />
                <div className="min-w-0 flex-1">
                  <div className="flex items-baseline gap-2">
                    <span className={`truncate ${c.unread ? 'font-bold text-ink' : 'font-semibold text-ink'}`}>{c.with_name}</span>
                    <span className="truncate text-xs text-muted">· {c.listing_title}</span>
                    <span className="ml-auto shrink-0 text-xs text-muted">{c.last && ago(c.last.created)}</span>
                  </div>
                  <p className={`truncate text-sm ${c.unread ? 'text-ink' : 'text-muted'}`}>{c.last?.body}</p>
                </div>
                {c.unread && <span className="h-2.5 w-2.5 shrink-0 rounded-full bg-pearl" aria-label="Unread" />}
              </Link>
            </li>
          ))}
        </ul>
      )}
      <p className="text-xs leading-relaxed text-muted">
        Messages are kept by the marketplace so both of you can read them, and can be read by its operator, as on most
        marketplaces. Never share your recovery phrase with anyone.
      </p>
    </div>
  )
}

function Thread({ id }: { id: string }) {
  const [found, setFound] = useState<{ conversation: Conversation; messages: Message[]; me: number } | null>(null)
  const [signedOut, setSignedOut] = useState(false)
  const [trouble, setTrouble] = useState('')
  const [draft, setDraft] = useState('')
  const [sending, setSending] = useState(false)
  const bottom = useRef<HTMLDivElement>(null)

  const load = useCallback(
    () =>
      ask<{ conversation: Conversation; messages: Message[]; me: number }>(`/conversations/${encodeURIComponent(id)}`)
        .then((next) => {
          setFound((before) => {
            if (!before || before.messages.length !== next.messages.length) {
              setTimeout(() => bottom.current?.scrollIntoView({ block: 'end' }), 0)
            }
            return next
          })
          // Reading changes the unread count in the bar.
          window.dispatchEvent(new Event(SIGNED_IN_OR_OUT))
        })
        .catch((error) => {
          if (error instanceof MarketError && error.status === 401) setSignedOut(true)
          else setTrouble(error instanceof MarketError && error.status === 404 ? 'There is no such conversation of yours.' : sentence(error.message))
        }),
    [id],
  )

  useEffect(() => {
    void load()
    const timer = setInterval(load, 5000)
    return () => clearInterval(timer)
  }, [load])

  if (signedOut) return <SignInFirst />
  if (trouble && !found) return <Notice tone="bad">{trouble}</Notice>
  if (!found) return <p className="text-muted">Loading…</p>
  const { conversation: c, messages, me } = found

  const send = async (event: React.FormEvent) => {
    event.preventDefault()
    if (!draft.trim()) return
    setSending(true)
    setTrouble('')
    try {
      await ask(`/conversations/${c.id}/messages`, 'POST', { body: draft })
      setDraft('')
      await load()
    } catch (error) {
      setTrouble(sentence((error as Error).message))
    }
    setSending(false)
  }

  const toggleBlock = async () => {
    const block = !c.blocked
    if (block && !window.confirm(`Block ${c.with_name}? They will not be able to message you.`)) return
    await ask(`/conversations/${c.id}/block`, 'POST', { block }).catch(() => {})
    await load()
  }

  return (
    <div className="space-y-4">
      <Link href="/marketplace/messages/" className="inline-flex items-center gap-2 text-sm text-muted hover:text-ink">
        <ArrowLeft className="h-4 w-4" aria-hidden /> All messages
      </Link>
      <div className="flex flex-wrap items-center gap-3 rounded-2xl border border-line bg-surface p-4">
        <Link href={`/marketplace/profile/?id=${c.with}`} className="flex min-w-0 items-center gap-3">
          <Avatar name={c.with_name} picture={c.with_picture} size={44} />
          <span className="flex min-w-0 flex-col">
            <span className="truncate font-semibold text-ink">{c.with_name}</span>
            <span className="text-xs text-muted">{c.buying ? 'Seller' : 'Buyer'}</span>
          </span>
        </Link>
        <Link
          href={`/marketplace/listing/?id=${c.listing}`}
          className="ml-auto max-w-full truncate rounded-xl border border-line bg-bg px-3 py-2 text-sm font-semibold text-ink hover:bg-raised"
        >
          {c.listing_title}
        </Link>
        <button
          type="button"
          onClick={toggleBlock}
          className="flex h-9 items-center gap-1.5 rounded-lg px-3 text-sm font-semibold text-muted hover:bg-raised hover:text-ink"
        >
          <Ban className="h-4 w-4" aria-hidden /> {c.blocked ? 'Unblock' : 'Block'}
        </button>
      </div>

      <div className="space-y-3 rounded-2xl border border-line bg-surface p-4 sm:p-6">
        {messages.map((m) => {
          const mine = m.sender === me
          return (
            <div key={m.id} className={`flex flex-col ${mine ? 'items-end' : 'items-start'}`}>
              <p
                className={`max-w-[85%] whitespace-pre-line break-words rounded-2xl px-4 py-2.5 leading-relaxed ${
                  mine ? 'bg-pearl/20 text-ink' : 'bg-raised text-ink'
                }`}
              >
                {m.body}
              </p>
              <span className="mt-1 px-1 text-[11px] text-muted">{ago(m.created)}</span>
            </div>
          )
        })}
        <div ref={bottom} />
      </div>

      {c.blocked ? (
        <Notice>You have blocked {c.with_name}. Unblock them to write.</Notice>
      ) : (
        <form onSubmit={send} className="space-y-3">
          <textarea
            value={draft}
            onChange={(e) => setDraft(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === 'Enter' && !e.shiftKey) {
                e.preventDefault()
                e.currentTarget.form?.requestSubmit()
              }
            }}
            maxLength={2000}
            rows={3}
            placeholder={`Write to ${c.with_name}`}
            className="w-full rounded-xl border border-line bg-bg px-4 py-3 text-ink outline-none focus:border-pearl/60"
          />
          {trouble && <Notice tone="bad">{trouble}</Notice>}
          <button
            type="submit"
            disabled={sending || !draft.trim()}
            className="pearl-fill flex h-11 items-center gap-2 rounded-xl px-6 text-sm font-bold transition hover:brightness-105 disabled:opacity-60"
          >
            <Send className="h-4 w-4" aria-hidden /> {sending ? 'Sending…' : 'Send'}
          </button>
        </form>
      )}
    </div>
  )
}

/** On a listing: the first message to its seller. */
export function MessageSeller({ listing, seller, signedIn }: { listing: number; seller: string; signedIn: boolean }) {
  const router = useRouter()
  const [draft, setDraft] = useState('')
  const [sending, setSending] = useState(false)
  const [trouble, setTrouble] = useState('')

  if (!signedIn) {
    return (
      <Notice>
        <Link href="/marketplace/account/" className="font-semibold text-ink underline underline-offset-4">
          Sign in
        </Link>{' '}
        to message {seller}.
      </Notice>
    )
  }

  const send = async (event: React.FormEvent) => {
    event.preventDefault()
    setSending(true)
    setTrouble('')
    try {
      const sent = await ask<{ conversation: number }>(`/listings/${listing}/messages`, 'POST', { body: draft })
      router.push(`/marketplace/messages/?c=${sent.conversation}`)
    } catch (error) {
      setTrouble(sentence((error as Error).message))
      setSending(false)
    }
  }

  return (
    <form onSubmit={send} className="space-y-3">
      <textarea
        value={draft}
        onChange={(e) => setDraft(e.target.value)}
        maxLength={2000}
        rows={3}
        required
        placeholder={`Hi ${seller}, I'm interested. Could you…`}
        className="w-full rounded-xl border border-line bg-bg px-4 py-3 text-ink outline-none focus:border-pearl/60"
      />
      {trouble && <Notice tone="bad">{trouble}</Notice>}
      <button
        type="submit"
        disabled={sending || !draft.trim()}
        className="pearl-fill flex h-11 items-center gap-2 rounded-xl px-6 text-sm font-bold transition hover:brightness-105 disabled:opacity-60"
      >
        <Send className="h-4 w-4" aria-hidden /> {sending ? 'Sending…' : `Message ${seller}`}
      </button>
    </form>
  )
}
