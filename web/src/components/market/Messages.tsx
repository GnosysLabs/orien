'use client'

import Link from 'next/link'
import { useRouter, useSearchParams } from 'next/navigation'
import { useCallback, useEffect, useRef, useState } from 'react'
import { ArrowLeft, Ban, CheckCircle2, MessageCircle, Send, Wallet } from 'lucide-react'
import { CopyText } from '@/components/CopyText'
import {
  ago,
  ask,
  formatOrn,
  MarketError,
  parseOrn,
  perUnit,
  plainOrn,
  sentence,
  type Conversation,
  type Message,
} from '@/lib/market'
import { Avatar } from './Avatar'
import { Notice, SIGNED_IN_OR_OUT } from './Shell'
import { AutoTextarea } from '@/components/AutoTextarea'

/** Conversations on the left and the open one on the right; one at a time on a phone. */
export function Messages() {
  const open = useSearchParams().get('c')
  const [conversations, setConversations] = useState<Conversation[] | null>(null)
  const [signedOut, setSignedOut] = useState(false)
  const [trouble, setTrouble] = useState('')

  const loadList = useCallback(
    () =>
      ask<{ conversations: Conversation[] }>('/conversations')
        .then((found) => setConversations(found.conversations))
        .catch((error) => {
          if (error instanceof MarketError && error.status === 401) setSignedOut(true)
          else setTrouble(sentence((error as Error).message))
        }),
    [],
  )

  useEffect(() => {
    void loadList()
    const timer = setInterval(loadList, 10000)
    return () => clearInterval(timer)
  }, [loadList])

  if (signedOut) {
    return (
      <Notice>
        <Link href="/marketplace/account/" className="font-semibold text-ink underline underline-offset-4">
          Sign in
        </Link>{' '}
        to see your messages.
      </Notice>
    )
  }
  if (trouble) return <Notice tone="bad">{trouble}</Notice>
  if (!conversations) return <p className="text-muted">Loading…</p>

  return (
    <div className="space-y-3">
      <div className="grid h-[calc(100dvh-13rem)] min-h-[520px] overflow-hidden rounded-2xl border border-line bg-surface lg:grid-cols-[320px_minmax(0,1fr)]">
        <aside className={`min-h-0 flex-col border-line lg:flex lg:border-r ${open ? 'hidden' : 'flex'}`}>
          <h1 className="border-b border-line px-5 py-4 text-lg font-bold tracking-tight text-ink">Messages</h1>
          {conversations.length === 0 ? (
            <p className="p-5 text-sm leading-relaxed text-muted">
              No messages yet. To ask a seller something, open their listing and write to them there.
            </p>
          ) : (
            <ul className="min-h-0 flex-1 overflow-y-auto">
              {conversations.map((c) => (
                <li key={c.id}>
                  <Link
                    href={`/marketplace/messages/?c=${c.id}`}
                    className={`flex items-center gap-3 border-b border-line/60 px-4 py-3.5 transition ${
                      String(c.id) === open ? 'bg-raised' : 'hover:bg-raised/50'
                    }`}
                  >
                    <Avatar name={c.with_name} picture={c.with_picture} size={42} />
                    <div className="min-w-0 flex-1">
                      <div className="flex items-baseline gap-2">
                        <span className={`truncate text-sm ${c.unread ? 'font-bold' : 'font-semibold'} text-ink`}>{c.with_name}</span>
                        <span className="ml-auto shrink-0 text-[11px] text-muted">{c.last && ago(c.last.created)}</span>
                      </div>
                      <p className="truncate text-xs text-muted">{c.listing_title}</p>
                      <p className={`truncate text-sm ${c.unread ? 'text-ink' : 'text-muted'}`}>{c.last?.body}</p>
                    </div>
                    {c.unread && <span className="h-2.5 w-2.5 shrink-0 rounded-full bg-pearl" aria-label="Unread" />}
                  </Link>
                </li>
              ))}
            </ul>
          )}
        </aside>
        <section className={`min-h-0 flex-col lg:flex ${open ? 'flex' : 'hidden'}`}>
          {open ? (
            <Thread key={open} id={open} onChange={loadList} />
          ) : (
            <div className="flex flex-1 flex-col items-center justify-center gap-3 p-8 text-center text-muted">
              <MessageCircle className="h-8 w-8 text-pearl/50" strokeWidth={1.5} aria-hidden />
              <p>Pick a conversation to read it.</p>
            </div>
          )}
        </section>
      </div>
      <p className="px-1 text-xs leading-relaxed text-muted">
        Messages are kept by the marketplace so both of you can read them, and can be read by its operator, as on most
        marketplaces. Never share your recovery phrase with anyone.
      </p>
    </div>
  )
}

type Found = { conversation: Conversation; messages: Message[]; me: number }

function Thread({ id, onChange }: { id: string; onChange: () => void }) {
  const [found, setFound] = useState<Found | null>(null)
  const [trouble, setTrouble] = useState('')
  const [draft, setDraft] = useState('')
  const [sending, setSending] = useState(false)
  const [asking, setAsking] = useState<string | null>(null)
  const scroller = useRef<HTMLDivElement>(null)

  const load = useCallback(
    () =>
      ask<Found>(`/conversations/${encodeURIComponent(id)}`)
        .then((next) => {
          setFound((before) => {
            if (!before || before.messages.length !== next.messages.length) {
              setTimeout(() => scroller.current?.scrollTo({ top: scroller.current.scrollHeight }), 0)
            }
            return next
          })
          window.dispatchEvent(new Event(SIGNED_IN_OR_OUT))
        })
        .catch((error) =>
          setTrouble(error instanceof MarketError && error.status === 404 ? 'There is no such conversation of yours.' : sentence(error.message)),
        ),
    [id],
  )

  useEffect(() => {
    void load()
    const timer = setInterval(load, 4000)
    return () => clearInterval(timer)
  }, [load])

  if (trouble && !found) return <div className="p-6"><Notice tone="bad">{trouble}</Notice></div>
  if (!found) return <p className="p-6 text-muted">Loading…</p>
  const { conversation: c, messages, me } = found
  const selling = !c.buying

  const act = async (path: string, body: unknown) => {
    setSending(true)
    setTrouble('')
    try {
      await ask(`/conversations/${c.id}/${path}`, 'POST', body)
      await load()
      onChange()
      return true
    } catch (error) {
      setTrouble(sentence((error as Error).message))
      return false
    } finally {
      setSending(false)
    }
  }

  const send = async (event: React.FormEvent) => {
    event.preventDefault()
    if (draft.trim() && (await act('messages', { body: draft }))) setDraft('')
  }

  const requestPayment = async (event: React.FormEvent) => {
    event.preventDefault()
    const amount = parseOrn(asking ?? '')
    if (amount === null) return setTrouble('Write the amount in ORN, like 200 or 12.5.')
    if (await act('request', { amount })) setAsking(null)
  }

  const toggleBlock = async () => {
    if (!c.blocked && !window.confirm(`Block ${c.with_name}? They will not be able to message you.`)) return
    await act('block', { block: !c.blocked })
  }

  // A request counts as paid once the buyer has said so after it.
  const paidAfter = (request: Message) => messages.some((m) => m.kind === 'paid' && m.id > request.id && m.amount === request.amount)

  return (
    <>
      <header className="flex flex-wrap items-center gap-3 border-b border-line px-4 py-3">
        <Link href="/marketplace/messages/" className="text-muted hover:text-ink lg:hidden" aria-label="All messages">
          <ArrowLeft className="h-5 w-5" />
        </Link>
        <Link href={`/marketplace/profile/?id=${c.with}`} className="flex min-w-0 items-center gap-3">
          <Avatar name={c.with_name} picture={c.with_picture} size={38} />
          <span className="flex min-w-0 flex-col">
            <span className="truncate font-semibold text-ink hover:underline">{c.with_name}</span>
            <span className="text-xs text-muted">{c.buying ? 'Seller' : 'Buyer'}</span>
          </span>
        </Link>
        <div className="ml-auto flex items-center gap-2">
          {selling && !c.blocked && (
            <button
              type="button"
              onClick={() => setAsking(asking === null ? plainOrn(c.price) : null)}
              className="pearl-fill flex h-9 items-center gap-1.5 rounded-lg px-3 text-sm font-bold"
            >
              <Wallet className="h-4 w-4" aria-hidden /> Ask for payment
            </button>
          )}
          <button
            type="button"
            onClick={toggleBlock}
            title={c.blocked ? 'Unblock' : 'Block'}
            className="flex h-9 items-center gap-1.5 rounded-lg px-2.5 text-sm font-semibold text-muted hover:bg-raised hover:text-ink"
          >
            <Ban className="h-4 w-4" aria-hidden />
            <span className="hidden sm:inline">{c.blocked ? 'Unblock' : 'Block'}</span>
          </button>
        </div>
      </header>

      <Link
        href={`/marketplace/listing/?id=${c.listing}`}
        className="flex items-center justify-between gap-3 border-b border-line bg-bg/40 px-4 py-2.5 text-sm hover:bg-raised/40"
      >
        <span className="truncate text-muted">
          About <span className="font-semibold text-ink">{c.listing_title}</span>
        </span>
        <span className="num shrink-0 font-semibold text-ink">
          {formatOrn(c.price)} ORN{c.unit && <span className="font-normal text-muted"> {perUnit(c.unit)}</span>}
        </span>
      </Link>

      {asking !== null && (
        <form onSubmit={requestPayment} className="flex flex-wrap items-center gap-3 border-b border-line bg-raised/40 px-4 py-3">
          <span className="text-sm text-ink">Ask {c.with_name} to pay</span>
          <input
            value={asking}
            onChange={(e) => setAsking(e.target.value)}
            inputMode="decimal"
            autoFocus
            className="num h-10 w-32 rounded-lg border border-line bg-bg px-3 text-ink outline-none focus:border-pearl/60"
          />
          <span className="text-sm text-muted">ORN</span>
          <button type="submit" disabled={sending} className="pearl-fill h-10 rounded-lg px-4 text-sm font-bold disabled:opacity-60">
            Send request
          </button>
          <button type="button" onClick={() => setAsking(null)} className="text-sm text-muted hover:text-ink">
            Cancel
          </button>
        </form>
      )}

      <div ref={scroller} className="min-h-0 flex-1 space-y-3 overflow-y-auto px-4 py-5">
        {messages.map((m) => {
          const mine = m.sender === me
          if (m.kind === 'request' && m.amount !== null) {
            return <RequestCard key={m.id} message={m} c={c} mine={mine} paid={paidAfter(m)} onPaid={() => act('paid', { request: m.id })} />
          }
          if (m.kind === 'paid' && m.amount !== null) {
            return (
              <div key={m.id} className="mx-auto flex max-w-md items-center gap-3 rounded-2xl border border-go/30 bg-go/10 px-4 py-3 text-sm">
                <CheckCircle2 className="h-5 w-5 shrink-0 text-go" aria-hidden />
                <span className="text-ink">
                  {mine ? 'You said' : `${c.with_name} says`} {mine ? 'you' : 'they'} paid{' '}
                  <span className="num font-semibold">{formatOrn(m.amount)} ORN</span>.
                  {!mine && ' Check your wallet to be sure it arrived.'}
                </span>
              </div>
            )
          }
          return (
            <div key={m.id} className={`flex flex-col ${mine ? 'items-end' : 'items-start'}`}>
              <p
                className={`max-w-[80%] whitespace-pre-line break-words rounded-2xl px-4 py-2.5 leading-relaxed text-ink ${
                  mine ? 'rounded-br-md bg-pearl/20' : 'rounded-bl-md bg-raised'
                }`}
              >
                {m.body}
              </p>
              <span className="mt-1 px-1 text-[11px] text-muted">{ago(m.created)}</span>
            </div>
          )
        })}
      </div>

      {trouble && <div className="px-4 pb-2"><Notice tone="bad">{trouble}</Notice></div>}
      {c.blocked ? (
        <p className="border-t border-line px-4 py-4 text-sm text-muted">You have blocked {c.with_name}. Unblock them to write.</p>
      ) : (
        <form onSubmit={send} className="flex items-end gap-2 border-t border-line p-3">
          <AutoTextarea
            value={draft}
            onChange={(e) => setDraft(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === 'Enter' && !e.shiftKey) {
                e.preventDefault()
                e.currentTarget.form?.requestSubmit()
              }
            }}
            maxLength={2000}
            rows={1}
            maxRows={6}
            placeholder={`Write to ${c.with_name}`}
            className="max-h-40 min-h-11 flex-1 resize-none rounded-xl border border-line bg-bg px-4 py-2.5 text-ink outline-none focus:border-pearl/60"
          />
          <button
            type="submit"
            disabled={sending || !draft.trim()}
            aria-label="Send"
            className="pearl-fill flex h-11 w-11 shrink-0 items-center justify-center rounded-xl disabled:opacity-50"
          >
            <Send className="h-4 w-4" aria-hidden />
          </button>
        </form>
      )}
    </>
  )
}

/** A seller's payment request, in the chat. The buyer pays from here. */
function RequestCard({
  message,
  c,
  mine,
  paid,
  onPaid,
}: {
  message: Message
  c: Conversation
  mine: boolean
  paid: boolean
  onPaid: () => void
}) {
  const [paying, setPaying] = useState(false)
  const amount = message.amount ?? 0
  return (
    <div className="mx-auto w-full max-w-md overflow-hidden rounded-2xl border border-pearl/30 bg-raised">
      <div className="flex items-center gap-3 px-5 py-4">
        <Wallet className="h-5 w-5 shrink-0 text-pearl" aria-hidden />
        <div className="flex-1">
          <p className="text-xs uppercase tracking-[0.12em] text-muted">Payment request</p>
          <p className="num text-xl font-bold text-ink">{formatOrn(amount)} ORN</p>
        </div>
        {paid && (
          <span className="flex items-center gap-1 rounded-full bg-go/15 px-2.5 py-1 text-xs font-semibold text-go">
            <CheckCircle2 className="h-3.5 w-3.5" aria-hidden /> Paid
          </span>
        )}
      </div>
      {mine ? (
        !paid && <p className="border-t border-line px-5 py-3 text-sm text-muted">Waiting for {c.with_name} to pay.</p>
      ) : (
        !paid &&
        (paying ? (
          <div className="space-y-3 border-t border-line px-5 py-4 text-sm text-muted">
            <p>
              Send exactly this from Orien Wallet or with orien. It goes straight to {c.with_name}&apos;s wallet, and
              cannot be undone.
            </p>
            <CopyText label="Amount" text={`${plainOrn(amount)} ORN`} />
            <CopyText label={`${c.with_name}'s address`} text={c.pay_to} />
            <CopyText label="Or with orien" text={`orien send --to ${c.pay_to} --amount ${plainOrn(amount)}`} />
            <button type="button" onClick={onPaid} className="pearl-fill h-11 w-full rounded-xl text-sm font-bold">
              I&apos;ve paid
            </button>
          </div>
        ) : (
          <div className="border-t border-line px-5 py-3">
            <button type="button" onClick={() => setPaying(true)} className="pearl-fill h-11 w-full rounded-xl text-sm font-bold">
              Pay {formatOrn(amount)} ORN
            </button>
          </div>
        ))
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
      <AutoTextarea
        value={draft}
        onChange={(e) => setDraft(e.target.value)}
        maxLength={2000}
        rows={3}
        required
        placeholder={`Hi ${seller}, I'm interested. Could you…`}
        aria-label={`Message ${seller}`}
        className="w-full rounded-xl border border-line bg-bg px-4 py-3 text-ink outline-none focus:border-pearl/60"
      />
      {trouble && <Notice tone="bad">{trouble}</Notice>}
      <button
        type="submit"
        disabled={sending || !draft.trim()}
        className="pearl-fill flex h-11 w-full items-center justify-center gap-2 rounded-xl px-6 text-sm font-bold transition hover:brightness-105 disabled:opacity-60"
      >
        <Send className="h-4 w-4" aria-hidden /> {sending ? 'Sending…' : `Message ${seller}`}
      </button>
    </form>
  )
}
