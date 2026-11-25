'use client'

import Link from 'next/link'
import { useCallback, useEffect, useRef, useState } from 'react'
import { ArrowLeft, Ban, Check, Flag, ImagePlus, Reply, Send, SmilePlus, Trash2, X } from 'lucide-react'
import { ago, ask, MarketError, profileHref, sentence } from '@/lib/market'
import { shrinkPicture } from '@/lib/shrink'
import { AutoTextarea } from '@/components/AutoTextarea'
import { Avatar } from './Avatar'
import { GifPicker, Photos, useDismiss, type Gif, type Picture } from './CommunityView'
import { ReportButton } from './ReportButton'
import { openSignIn } from './SignInDialog'
import { Notice } from './Shell'
import { ReactionChip, type ReactionCount } from './ReactionChip'

/** The group chat: one room for everyone on Orien Network, pinned first in
 * Messages, as in Orien Wallet. Anyone reads it; sending, replying and
 * reacting take a verified wallet, and every message is checked like a
 * post. */

type Author = { id: number; name: string; picture: string | null; verified: boolean; moderator?: boolean; banned?: boolean }
type RoomMessage = {
  id: number
  author: Author
  body: string
  pictures: Picture[]
  gif: Gif | null
  reply_to: { id: number; author_name: string; body: string } | null
  reactions: ReactionCount[]
  created: number
  mine: boolean
}
type You = { id: number; name: string; picture: string | null; verified: boolean; moderator: boolean; banned: boolean }
type Answer = {
  messages: RoomMessage[]
  more: boolean
  newest: number
  version: number
  changed?: (RoomMessage | { id: number; removed: true })[]
  reactions: string[]
  max: number
  max_pictures: number
  you: You | null
}

const PAGE = 50
const READ = 'room-read'
/** Sent when the group chat is read, so its row in Messages catches up. */
const ROOM_READ = 'orien-room-read'

const read = () => Number((typeof localStorage !== 'undefined' && localStorage.getItem(READ)) || 0)
function markRead(id: number) {
  if (id > read()) {
    localStorage.setItem(READ, String(id))
    window.dispatchEvent(new Event(ROOM_READ))
  }
}
const preview = (m: RoomMessage) => m.body || (m.pictures.length ? 'Sent a picture' : m.gif ? 'Sent a GIF' : '')

/** The Orien sunrise, as beside the wordmark, on the pearl. */
export function RoomIcon({ size }: { size: number }) {
  return (
    <span className="pearl-fill grid shrink-0 place-items-center rounded-full" style={{ width: size, height: size }}>
      <svg viewBox="0 0 512 512" fill="currentColor" className="h-[62%] w-[62%] text-bg" aria-hidden>
        <rect x="239" y="74" width="36" height="66" rx="18" />
        <rect x="239" y="74" width="36" height="66" rx="18" transform="rotate(-44 256 298)" />
        <rect x="239" y="74" width="36" height="66" rx="18" transform="rotate(44 256 298)" />
        <path d="M138 298 A118 118 0 0 1 374 298 Z" />
        <rect x="76" y="338" width="360" height="40" rx="20" />
        <rect x="156" y="406" width="200" height="36" rx="18" />
      </svg>
    </span>
  )
}

/** What this page has of the room, kept while the person moves between
 * conversations so coming back shows it at once. */
let kept: { messages: RoomMessage[]; more: boolean; version: number; you: You | null; reactions: string[] } | null = null

/** The group chat's row at the top of Messages: the newest message, and a
 * dot until it's read. */
export function RoomRow({ active, onOpen }: { active: boolean; onOpen: () => void }) {
  const [last, setLast] = useState<RoomMessage | null>(kept?.messages.at(-1) ?? null)
  const [, setReadAt] = useState(0)
  useEffect(() => {
    const peek = () =>
      ask<Answer>('/room/messages?limit=1')
        .then((a) => setLast(a.messages.at(-1) ?? null))
        .catch(() => undefined)
    void peek()
    const timer = setInterval(() => document.visibilityState === 'visible' && void peek(), 15000)
    const caught = () => setReadAt(Date.now())
    window.addEventListener(ROOM_READ, caught)
    return () => {
      clearInterval(timer)
      window.removeEventListener(ROOM_READ, caught)
    }
  }, [])
  // Your own messages are never news to you.
  const unread = !!last && !active && !last.mine && last.id > read()
  return (
    <Link
      href="/messages/?c=room"
      scroll={false}
      onClick={onOpen}
      className={`flex items-center gap-3 border-b border-line/60 px-4 py-3.5 transition ${active ? 'bg-raised' : 'hover:bg-raised/50'}`}
    >
      <RoomIcon size={42} />
      <div className="min-w-0 flex-1">
        <div className="flex items-baseline gap-2">
          <span className={`truncate text-sm ${unread ? 'font-bold' : 'font-semibold'} text-ink`}>Orien</span>
          <span className="ml-auto shrink-0 text-[11px] text-muted">{last && ago(last.created)}</span>
        </div>
        <p className="truncate text-xs text-muted">Group chat · everyone on Orien Network</p>
        <p className={`truncate text-sm ${unread ? 'text-ink' : 'text-muted'}`}>
          {last ? `${last.author.name}: ${preview(last)}` : 'Say hello to everyone.'}
        </p>
      </div>
      {unread && <span className="h-2.5 w-2.5 shrink-0 rounded-full bg-pearl" aria-label="Unread" />}
    </Link>
  )
}

const time = (seconds: number) => new Date(seconds * 1000).toLocaleTimeString([], { hour: 'numeric', minute: '2-digit' })
const day = (seconds: number) => new Date(seconds * 1000).toLocaleDateString([], { weekday: 'long', month: 'short', day: 'numeric' })

/** The open group chat, as on Discord: everyone's face and name on the
 * left, new messages at the bottom, live every few seconds. */
export function RoomThread() {
  const [messages, setMessages] = useState<RoomMessage[] | null>(kept?.messages ?? null)
  const [more, setMore] = useState(kept?.more ?? false)
  const [you, setYou] = useState<You | null>(kept?.you ?? null)
  const [reactions, setReactions] = useState<string[]>(kept?.reactions ?? [])
  const [trouble, setTrouble] = useState('')
  const [replying, setReplying] = useState<RoomMessage | null>(null)
  const [flash, setFlash] = useState<number | null>(null)
  const version = useRef<number | null>(kept?.version ?? null)
  const log = useRef<HTMLDivElement>(null)
  const atBottom = useRef(true)
  const olderBusy = useRef(false)

  const take = useCallback((a: Answer) => {
    setYou(a.you)
    if (a.reactions?.length) setReactions(a.reactions)
    version.current = a.version
  }, [])

  // Newest first look, then only what's new and what changed since.
  const poll = useCallback(async () => {
    try {
      const had = kept?.messages ?? []
      if (!had.length || version.current === null) {
        const a = await ask<Answer>(`/room/messages?limit=${PAGE}`)
        take(a)
        setMessages(a.messages)
        setMore(a.more)
        kept = { messages: a.messages, more: a.more, version: a.version, you: a.you, reactions: a.reactions }
        return
      }
      const newest = had.at(-1)!.id
      const a = await ask<Answer>(`/room/messages?after=${newest}&changes=${version.current}&limit=100`)
      take(a)
      let next = had
      for (const c of a.changed ?? []) {
        if ('removed' in c) next = next.filter((m) => m.id !== c.id)
        else next = next.map((m) => (m.id === c.id ? c : m))
      }
      const fresh = a.messages.filter((m) => m.id > newest)
      if (fresh.length) next = [...next, ...fresh]
      kept = { ...kept!, messages: next, version: a.version, you: a.you, reactions: a.reactions?.length ? a.reactions : kept!.reactions }
      if (next !== had) setMessages(next)
      setTrouble('')
    } catch (error) {
      setTrouble(sentence((error as Error).message))
    }
  }, [take])

  useEffect(() => {
    void poll()
    const timer = setInterval(() => document.visibilityState === 'visible' && void poll(), 3500)
    return () => clearInterval(timer)
  }, [poll])

  // Keeps to the bottom while you're there, and marks what you've seen read.
  useEffect(() => {
    const el = log.current
    if (!el || !messages) return
    if (atBottom.current) el.scrollTop = el.scrollHeight
    const last = messages.at(-1)
    if (last) markRead(last.id)
  }, [messages])

  const older = async () => {
    const el = log.current
    if (!more || olderBusy.current || !messages?.length || !el) return
    olderBusy.current = true
    try {
      const a = await ask<Answer>(`/room/messages?before=${messages[0].id}&limit=${PAGE}`)
      const below = el.scrollHeight - el.scrollTop
      const next = [...a.messages, ...(kept?.messages ?? [])]
      kept = { ...kept!, messages: next, more: a.more }
      setMore(a.more)
      setMessages(next)
      requestAnimationFrame(() => {
        el.scrollTop = el.scrollHeight - below
      })
    } catch {
      // Tried again on the next scroll.
    }
    olderBusy.current = false
  }

  const onScroll = () => {
    const el = log.current
    if (!el) return
    atBottom.current = el.scrollHeight - el.scrollTop - el.clientHeight < 80
    if (el.scrollTop < 300) void older()
  }

  const jump = (id: number) => {
    const row = log.current?.querySelector(`[data-rm="${id}"]`)
    if (!row) return
    row.scrollIntoView({ block: 'center', behavior: 'smooth' })
    setFlash(id)
    setTimeout(() => setFlash(null), 1200)
  }

  const can = !!you && you.verified && !you.banned
  const startReply = (m: RoomMessage) => setReplying(m)

  return (
    <>
      <header className="flex items-center gap-3 border-b border-line px-4 py-3">
        <Link href="/messages/" scroll={false} className="text-muted hover:text-ink lg:hidden" aria-label="All messages">
          <ArrowLeft className="h-5 w-5" />
        </Link>
        <RoomIcon size={38} />
        <span className="flex min-w-0 flex-col">
          <span className="truncate font-semibold text-ink">Orien</span>
          <span className="text-xs text-muted">Everyone on Orien Network</span>
        </span>
        {you?.moderator && <BansButton onChange={() => void poll()} />}
      </header>

      <div ref={log} onScroll={onScroll} className="min-h-0 flex-1 overflow-y-auto px-2 py-4 sm:px-3">
        {!messages ? (
          <p className="p-4 text-muted">Loading…</p>
        ) : (
          <>
            {!more && (
              <p className="mx-auto mb-2 mt-4 max-w-sm text-center text-xs text-muted">
                This is the start of the Orien group chat. Be kind; everything here is public.
              </p>
            )}
            {messages.length === 0 && <p className="mt-10 text-center text-sm text-muted">No messages yet. Say hello to everyone.</p>}
            {messages.map((m, i) => {
              const prev = messages[i - 1]
              const newDay = !prev || day(prev.created) !== day(m.created)
              const cont = !newDay && !!prev && prev.author.id === m.author.id && m.created - prev.created < 300 && !m.reply_to
              return (
                <div key={m.id}>
                  {newDay && (
                    <div className="my-4 flex items-center gap-3 px-2 text-[11px] font-semibold text-muted">
                      <span className="h-px flex-1 bg-line" />
                      {day(m.created)}
                      <span className="h-px flex-1 bg-line" />
                    </div>
                  )}
                  <Row
                    message={m}
                    cont={cont}
                    you={you}
                    can={can}
                    reactions={reactions}
                    flash={flash === m.id}
                    onJump={jump}
                    onReply={startReply}
                    onChange={() => void poll()}
                    onTrouble={setTrouble}
                  />
                </div>
              )
            })}
          </>
        )}
      </div>

      {trouble && (
        <div className="px-4 pb-2">
          <Notice tone="bad">{trouble}</Notice>
        </div>
      )}
      {can ? (
        <Composer
          replying={replying}
          onCancelReply={() => setReplying(null)}
          onSent={() => {
            setReplying(null)
            atBottom.current = true
            void poll()
          }}
        />
      ) : (
        <div className="flex flex-col items-center gap-3 border-t border-line px-4 py-5 text-center text-sm text-muted">
          {!you ? (
            <>
              <p>Sign in to Orien Network to chat. Anyone can read along.</p>
              <button type="button" onClick={openSignIn} className="pearl-fill h-10 rounded-full px-5 text-sm font-bold">
                Sign in
              </button>
            </>
          ) : you.banned ? (
            <p>You can&apos;t send in the group chat right now. You can still read along.</p>
          ) : (
            <>
              <p>Verify one of your wallets to chat. Anyone can read along.</p>
              <Link href="/account/" className="pearl-fill flex h-10 items-center rounded-full px-5 text-sm font-bold">
                Verify a wallet
              </Link>
            </>
          )}
        </div>
      )}
    </>
  )
}

const tool = 'grid h-7 w-8 place-items-center rounded-lg text-muted transition hover:bg-raised hover:text-ink'

/** One message: face and name unless it follows the same person's, then
 * its words, photos or GIF, and reactions. Its tools show on hover. */
function Row({
  message: m,
  cont,
  you,
  can,
  reactions,
  flash,
  onJump,
  onReply,
  onChange,
  onTrouble,
}: {
  message: RoomMessage
  cont: boolean
  you: You | null
  can: boolean
  reactions: string[]
  flash: boolean
  onJump: (id: number) => void
  onReply: (m: RoomMessage) => void
  onChange: () => void
  onTrouble: (text: string) => void
}) {
  const [picking, setPicking] = useState(false)
  const [reporting, setReporting] = useState(false)
  const [armed, setArmed] = useState<'delete' | 'ban' | null>(null)
  // Phones have no hover: a tap on the message shows its tools.
  const [tapped, setTapped] = useState(false)
  const closePicker = useCallback(() => setPicking(false), [])
  const pickerBox = useDismiss(picking, closePicker)
  const mod = !!you?.moderator
  const a = m.author

  useEffect(() => {
    if (!armed) return
    const timer = setTimeout(() => setArmed(null), 4000)
    return () => clearTimeout(timer)
  }, [armed])

  const run = async (work: () => Promise<unknown>) => {
    try {
      await work()
    } catch (error) {
      onTrouble(sentence((error as Error).message))
    }
    onChange()
  }
  const needs = () => {
    if (can) return true
    if (!you) openSignIn()
    else onTrouble(you.banned ? 'You can’t send in the group chat right now.' : 'Verify one of your wallets to chat, reply or react.')
    return false
  }
  const react = (emoji: string) => {
    setPicking(false)
    if (needs()) void run(() => ask(`/room/messages/${m.id}/react`, 'POST', { emoji }))
  }
  const remove = () => {
    if (armed !== 'delete') return setArmed('delete')
    setArmed(null)
    void run(() => ask(`/room/messages/${m.id}`, 'DELETE'))
  }
  const ban = () => {
    if (a.banned) return void run(() => ask(`/room/bans/${a.id}`, 'DELETE'))
    if (armed !== 'ban') return setArmed('ban')
    setArmed(null)
    void run(() => ask('/room/bans', 'POST', { account: a.id }))
  }

  return (
    <div
      data-rm={m.id}
      onClick={(event) => {
        if (window.matchMedia('(hover: none)').matches && !(event.target as HTMLElement).closest('a,button,form')) setTapped(!tapped)
      }}
      className={`group relative grid grid-cols-[40px_minmax(0,1fr)] gap-x-3 rounded-xl px-2.5 transition-colors ${cont ? 'py-0.5' : 'mt-2 pb-0.5 pt-2'} ${
        flash ? 'bg-raised' : 'hover:bg-raised/40'
      }`}
    >
      {m.reply_to && (
        <button
          type="button"
          onClick={() => onJump(m.reply_to!.id)}
          className="col-start-2 mb-1 flex min-w-0 items-center gap-1.5 text-left text-xs text-muted hover:text-ink"
        >
          <Reply className="h-3 w-3 shrink-0 -scale-x-100" aria-hidden />
          <b className="shrink-0 font-semibold">{m.reply_to.author_name}</b>
          <span className="truncate">{m.reply_to.body}</span>
        </button>
      )}
      {cont ? (
        <span className="pt-1 text-center text-[10px] text-muted opacity-0 group-hover:opacity-100">{time(m.created)}</span>
      ) : (
        <Link href={profileHref(a.name)} className="self-start" aria-label={a.name}>
          <Avatar name={a.name} picture={a.picture} size={40} />
        </Link>
      )}
      <div className="min-w-0">
        {!cont && (
          <div className="flex min-w-0 items-baseline gap-2">
            <Link href={profileHref(a.name)} className="truncate text-sm font-semibold text-ink hover:underline">
              {a.name}
            </Link>
            {a.moderator && <span className="shrink-0 rounded-full bg-white/[0.08] px-2 py-px text-[10.5px] font-semibold text-muted">Moderator</span>}
            {a.banned && <span className="shrink-0 rounded-full bg-stop/15 px-2 py-px text-[10.5px] font-semibold text-stop">Banned</span>}
            <span className="shrink-0 text-[11px] text-muted">{time(m.created)}</span>
          </div>
        )}
        {m.body && <p className="whitespace-pre-wrap break-words text-[15px] leading-relaxed text-ink [overflow-wrap:anywhere]">{m.body}</p>}
        {m.pictures.length > 0 && <Photos pictures={m.pictures} small />}
        {m.gif && (
          <a href={m.gif.url} target="_blank" rel="noopener noreferrer" className="mt-2 block w-fit max-w-full overflow-hidden rounded-xl border border-line bg-bg">
            {/* eslint-disable-next-line @next/next/no-img-element */}
            <img src={m.gif.url} alt="GIF" width={m.gif.width} height={m.gif.height} loading="lazy" className="block max-h-72 w-auto max-w-full" />
          </a>
        )}
        {m.reactions.length > 0 && (
          <div className="mt-1.5 flex flex-wrap gap-1.5">
            {m.reactions.map((r) => (
              <ReactionChip key={r.emoji} r={r} onClick={() => react(r.emoji)} small below />
            ))}
          </div>
        )}
        {reporting && (
          <div className="mt-2 max-w-sm">
            <ReportButton kind="room" id={m.id} what="this message" signedIn={!!you} />
          </div>
        )}
      </div>

      <div
        ref={pickerBox}
        className={`absolute -top-3.5 right-2.5 z-10 gap-0.5 rounded-xl border border-line bg-surface p-[3px] shadow-lg shadow-black/40 ${
          picking || armed || tapped ? 'flex' : 'hidden group-hover:flex'
        }`}
      >
        {picking && (
          <div className="absolute right-full top-1/2 mr-1.5 flex -translate-y-1/2 gap-1 rounded-full border border-line bg-surface p-1 shadow-2xl shadow-black/50">
            {reactions.map((emoji) => (
              <button
                key={emoji}
                type="button"
                onClick={() => react(emoji)}
                className="flex h-9 w-9 items-center justify-center rounded-full text-xl transition hover:scale-110 hover:bg-raised"
              >
                {emoji}
              </button>
            ))}
          </div>
        )}
        <button type="button" onClick={() => needs() && setPicking(!picking)} className={tool} title="React" aria-label="React">
          <SmilePlus className="h-4 w-4" aria-hidden />
        </button>
        <button type="button" onClick={() => needs() && onReply(m)} className={tool} title="Reply" aria-label="Reply">
          <Reply className="h-4 w-4" aria-hidden />
        </button>
        {!m.mine && (
          <button
            type="button"
            onClick={() => (you ? setReporting(!reporting) : openSignIn())}
            className={tool}
            title="Report"
            aria-label="Report"
          >
            <Flag className="h-4 w-4" aria-hidden />
          </button>
        )}
        {(m.mine || mod) && (
          <button
            type="button"
            onClick={remove}
            className={`${tool} text-stop hover:text-stop`}
            title={armed === 'delete' ? 'Click again to delete' : 'Delete'}
            aria-label="Delete"
          >
            {armed === 'delete' ? <Check className="h-4 w-4" aria-hidden /> : <Trash2 className="h-4 w-4" aria-hidden />}
          </button>
        )}
        {mod && !m.mine && !a.moderator && (
          <button
            type="button"
            onClick={ban}
            className={`${tool} ${a.banned ? '' : 'text-stop hover:text-stop'}`}
            title={a.banned ? `Let ${a.name} send again` : armed === 'ban' ? `Click again to ban ${a.name} from the chat` : `Ban ${a.name} from the chat`}
            aria-label={a.banned ? 'Unban' : 'Ban'}
          >
            {a.banned || armed === 'ban' ? <Check className="h-4 w-4" aria-hidden /> : <Ban className="h-4 w-4" aria-hidden />}
          </button>
        )}
      </div>
    </div>
  )
}

/** A photo on a message about to be sent: uploading, or uploaded. */
type Upload = { key: string; preview: string; picture?: Picture }

function Composer({
  replying,
  onCancelReply,
  onSent,
}: {
  replying: RoomMessage | null
  onCancelReply: () => void
  onSent: () => void
}) {
  const [body, setBody] = useState('')
  const [photos, setPhotos] = useState<Upload[]>([])
  const [gif, setGif] = useState<Gif | null>(null)
  const [picking, setPicking] = useState(false)
  const [busy, setBusy] = useState(false)
  const [trouble, setTrouble] = useState('')
  const chooser = useRef<HTMLInputElement>(null)
  const form = useRef<HTMLFormElement>(null)
  const uploading = photos.some((p) => !p.picture)
  const focus = () => form.current?.querySelector('textarea')?.focus()
  useEffect(() => {
    if (replying) focus()
  }, [replying])

  const addPhotos = async (files: FileList | null) => {
    if (!files) return
    setTrouble('')
    for (const file of Array.from(files).slice(0, 4 - photos.length)) {
      const key = `${file.name}-${Math.random()}`
      setPhotos((had) => [...had, { key, preview: URL.createObjectURL(file) }])
      try {
        const response = await fetch('/api/market/community/pictures', { method: 'POST', credentials: 'same-origin', body: await shrinkPicture(file) })
        const data = await response.json().catch(() => ({}))
        if (!response.ok) throw new Error(data.error ?? 'That photo could not be added.')
        setPhotos((had) => had.map((p) => (p.key === key ? { ...p, picture: data as Picture } : p)))
      } catch (error) {
        setPhotos((had) => had.filter((p) => p.key !== key))
        setTrouble(sentence((error as Error).message))
      }
    }
    if (chooser.current) chooser.current.value = ''
  }

  const send = async (event: React.FormEvent) => {
    event.preventDefault()
    const pictures = photos.flatMap((p) => (p.picture ? [p.picture.name] : []))
    if (busy || uploading || (!body.trim() && !pictures.length && !gif)) return
    setBusy(true)
    setTrouble('')
    try {
      await ask('/room/messages', 'POST', {
        body,
        ...(pictures.length ? { pictures } : {}),
        ...(gif ? { gif } : {}),
        ...(replying ? { reply_to: replying.id } : {}),
      })
      setBody('')
      setPhotos([])
      setGif(null)
      onSent()
    } catch (error) {
      setTrouble(error instanceof MarketError ? sentence(error.message) : sentence((error as Error).message))
    }
    setBusy(false)
    focus()
  }

  const ready = !busy && !uploading && (body.trim().length > 0 || photos.length > 0 || !!gif)
  return (
    <form ref={form} onSubmit={send} className="space-y-2 border-t border-line p-3">
      {trouble && <Notice tone="bad">{trouble}</Notice>}
      {replying && (
        <div className="flex items-center gap-2.5 rounded-xl bg-raised/60 py-2 pl-3.5 pr-2 text-[13px] text-muted">
          <Reply className="h-3.5 w-3.5 shrink-0 -scale-x-100" aria-hidden />
          <span className="min-w-0 flex-1 truncate">
            Replying to <b className="font-semibold text-ink">{replying.author.name}</b>
          </span>
          <button type="button" onClick={onCancelReply} aria-label="Stop replying" className="grid h-7 w-7 place-items-center rounded-full hover:bg-raised hover:text-ink">
            <X className="h-4 w-4" aria-hidden />
          </button>
        </div>
      )}
      {photos.length > 0 && (
        <div className="flex gap-2">
          {photos.map((p) => (
            <div key={p.key} className="relative h-16 w-16 overflow-hidden rounded-xl border border-line bg-bg">
              {/* eslint-disable-next-line @next/next/no-img-element */}
              <img src={p.preview} alt="" className={`h-full w-full object-cover ${p.picture ? '' : 'opacity-50'}`} />
              <button
                type="button"
                onClick={() => setPhotos((had) => had.filter((x) => x.key !== p.key))}
                aria-label="Remove photo"
                className="absolute right-1 top-1 grid h-[22px] w-[22px] place-items-center rounded-full bg-black/70 text-white"
              >
                <X className="h-3 w-3" aria-hidden />
              </button>
            </div>
          ))}
        </div>
      )}
      {gif && (
        <div className="relative w-fit overflow-hidden rounded-xl border border-line bg-bg">
          {/* eslint-disable-next-line @next/next/no-img-element */}
          <img src={gif.preview} alt="GIF" className="block h-24 w-auto" />
          <button
            type="button"
            onClick={() => setGif(null)}
            aria-label="Remove GIF"
            className="absolute right-1 top-1 grid h-[22px] w-[22px] place-items-center rounded-full bg-black/70 text-white"
          >
            <X className="h-3 w-3" aria-hidden />
          </button>
        </div>
      )}
      {picking && (
        <GifPicker
          onPick={(picked) => {
            setGif(picked)
            setPhotos([])
            setPicking(false)
          }}
          onClose={() => setPicking(false)}
        />
      )}
      <div className="flex items-end gap-2">
        <input ref={chooser} type="file" accept="image/jpeg,image/png,image/webp,image/gif" multiple className="hidden" onChange={(e) => void addPhotos(e.target.files)} />
        <button
          type="button"
          onClick={() => chooser.current?.click()}
          disabled={!!gif || photos.length >= 4}
          aria-label="Add photos"
          title="Photos"
          className="grid h-11 w-9 shrink-0 place-items-center rounded-xl text-muted transition hover:text-ink disabled:opacity-40"
        >
          <ImagePlus className="h-[18px] w-[18px]" aria-hidden />
        </button>
        <button
          type="button"
          onClick={() => setPicking(!picking)}
          disabled={photos.length > 0}
          aria-label="Add a GIF"
          title="GIF"
          className="grid h-11 w-9 shrink-0 place-items-center rounded-xl text-muted transition hover:text-ink disabled:opacity-40"
        >
          <span className="rounded border border-current px-1 text-[10px] font-bold leading-[14px]" aria-hidden>
            GIF
          </span>
        </button>
        <AutoTextarea
          value={body}
          onChange={(e) => setBody(e.target.value.slice(0, 500))}
          onKeyDown={(e) => {
            if (e.key === 'Enter' && !e.shiftKey) {
              e.preventDefault()
              e.currentTarget.form?.requestSubmit()
            }
            if (e.key === 'Escape' && replying) onCancelReply()
          }}
          rows={1}
          maxRows={6}
          placeholder="Message everyone"
          className="max-h-40 min-h-11 flex-1 resize-none rounded-xl border border-line bg-bg px-4 py-2.5 text-ink outline-none focus:border-pearl/60"
        />
        <button type="submit" disabled={!ready} aria-label="Send" className="pearl-fill flex h-11 w-11 shrink-0 items-center justify-center rounded-xl disabled:opacity-50">
          <Send className="h-4 w-4" aria-hidden />
        </button>
      </div>
    </form>
  )
}

type BanRow = { account: number; name: string; picture: string | null; banned_at: number; by: string | null }

/** Moderators: who can't send in the chat, each with Unban. */
function BansButton({ onChange }: { onChange: () => void }) {
  const [open, setOpen] = useState(false)
  const [bans, setBans] = useState<BanRow[] | null>(null)
  const close = useCallback(() => setOpen(false), [])
  const holder = useDismiss(open, close)
  const load = () =>
    ask<{ bans: BanRow[] }>('/room/bans')
      .then((a) => setBans(a.bans))
      .catch(() => setBans([]))
  const lift = async (account: number) => {
    await ask(`/room/bans/${account}`, 'DELETE').catch(() => undefined)
    await load()
    onChange()
  }
  return (
    <div ref={holder} className="relative ml-auto">
      <button
        type="button"
        onClick={() => {
          if (!open) void load()
          setOpen(!open)
        }}
        className={`flex h-9 items-center gap-1.5 rounded-lg px-3 text-sm font-semibold transition hover:bg-raised hover:text-ink ${open ? 'bg-raised text-ink' : 'text-muted'}`}
      >
        <Ban className="h-4 w-4" aria-hidden /> Banned
      </button>
      {open && (
        <div className="absolute right-0 top-full z-20 mt-2 w-80 max-w-[calc(100vw-3rem)] overflow-hidden rounded-xl border border-line bg-surface shadow-2xl shadow-black/60">
          <p className="border-b border-line px-4 py-3 text-sm font-bold text-ink">Banned from the group chat</p>
          <div className="max-h-80 overflow-y-auto p-1.5">
            {!bans ? (
              <p className="p-3 text-sm text-muted">Loading…</p>
            ) : bans.length === 0 ? (
              <p className="p-5 text-center text-sm text-muted">Nobody is banned.</p>
            ) : (
              bans.map((b) => (
                <div key={b.account} className="flex items-center gap-3 rounded-lg px-2.5 py-2">
                  <Avatar name={b.name} picture={b.picture} size={34} />
                  <div className="min-w-0 flex-1">
                    <p className="truncate text-sm font-semibold text-ink">{b.name}</p>
                    <p className="truncate text-xs text-muted">
                      Banned {ago(b.banned_at)}
                      {b.by ? ` by ${b.by}` : ''}
                    </p>
                  </div>
                  <button type="button" onClick={() => void lift(b.account)} className="h-8 rounded-lg border border-line px-3 text-xs font-semibold text-ink hover:bg-raised">
                    Unban
                  </button>
                </div>
              ))
            )}
          </div>
        </div>
      )}
    </div>
  )
}
