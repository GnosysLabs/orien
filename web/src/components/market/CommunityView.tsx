'use client'

import Link from 'next/link'
import { useCallback, useEffect, useRef, useState } from 'react'
import { Flag, Flame, MessageCircle, MoreHorizontal, Plus, SmilePlus, Trash2, Trophy, Users } from 'lucide-react'
import { ask, profileHref } from '@/lib/market'
import { checkpointHref, explore, hashrate as hashrateText, orn, short, type Overview } from '@/lib/explorer'
import { useMe } from '@/lib/useMe'
import { AutoTextarea } from '@/components/AutoTextarea'
import { Avatar } from './Avatar'
import { ReportButton } from './ReportButton'
import { Notice } from './Shell'

type Person = { name: string; picture: string | null }
type Face = { account?: number; address?: string; name?: string; picture?: string | null; paid?: string }
type Reaction = { emoji: string; count: number; me: boolean }
type Post = {
  id: number
  kind: 'post' | 'record' | 'welcome' | 'milestone' | 'streak' | 'top_day'
  body: string
  data: Record<string, unknown> & { miners?: Face[]; miner?: Face }
  created: number
  parent: number | null
  author: Person | null
  subject: Person | null
  replies: number
  mine: boolean
  reactions: Reaction[]
}
type Stats = { mined: string; streak: number; rank: number }
type MinerRow = { rank: number; name: string | null; picture: string | null; address: string | null; mined: string; streak: number; team: string | null }
type TeamRow = { rank: number; id: number; name: string; members: number; mined: string }
type Board = { epoch: number; since: number; miners: MinerRow[]; teams: TeamRow[] }
type You = { name: string; picture: string | null; verified: boolean; stats: Stats | null; team: { id: number; name: string } | null }
type Tab = 'all' | 'people' | 'network'

const REACTIONS = ['⛏️', '🔥', '❤️', '👏', '😂']
const MAX = 500
const TABS: [Tab, string][] = [
  ['all', 'All'],
  ['people', 'People'],
  ['network', 'Network'],
]

const pill = 'pearl-fill flex h-9 items-center justify-center rounded-full px-5 text-sm font-bold transition hover:brightness-105 disabled:opacity-50'
const quiet =
  'flex h-9 items-center justify-center gap-2 rounded-full border border-line px-4 text-sm font-semibold text-ink transition hover:border-pearl disabled:opacity-50'
const iconButton = 'flex h-8 w-8 items-center justify-center rounded-full text-muted transition hover:bg-raised hover:text-ink'
const card = 'rounded-2xl border border-line bg-surface'

const mined = (units: string) => orn(units, 2)

/** "2h", "3d": the short time a feed uses. */
function since(seconds: number) {
  const s = Math.max(0, Math.floor(Date.now() / 1000 - seconds))
  if (s < 60) return 'now'
  if (s < 3600) return `${Math.floor(s / 60)}m`
  if (s < 86400) return `${Math.floor(s / 3600)}h`
  if (s < 86400 * 30) return `${Math.floor(s / 86400)}d`
  return new Date(seconds * 1000).toLocaleDateString('en', { day: 'numeric', month: 'short' })
}

function Name({ person, className = '' }: { person: { name: string }; className?: string }) {
  return (
    <Link href={profileHref(person.name)} className={`font-bold text-ink hover:underline ${className}`}>
      {person.name}
    </Link>
  )
}

/** Closes when clicked outside or Escape is pressed. */
function useDismiss(open: boolean, close: () => void) {
  const box = useRef<HTMLDivElement>(null)
  useEffect(() => {
    if (!open) return
    const away = (event: MouseEvent | KeyboardEvent) => {
      if (event instanceof KeyboardEvent ? event.key === 'Escape' : !box.current?.contains(event.target as Node)) close()
    }
    document.addEventListener('mousedown', away)
    document.addEventListener('keydown', away)
    return () => {
      document.removeEventListener('mousedown', away)
      document.removeEventListener('keydown', away)
    }
  }, [open, close])
  return box
}

/** The network, as a poster: the Orien sun on living pearl. */
function NetworkAvatar({ size = 36 }: { size?: number }) {
  return (
    <span className="pearl-fill flex shrink-0 items-center justify-center rounded-full" style={{ width: size, height: size }} aria-hidden>
      <span className="orien-mark bg-bg" style={{ width: size * 0.56, height: size * 0.56 }} />
    </span>
  )
}

/** A miner's face: their picture when they have a profile, else the sun. */
function FaceAvatar({ face, size }: { face: Face; size: number }) {
  if (face.name) return <Avatar name={face.name} picture={face.picture ?? null} size={size} />
  return (
    <span
      title={face.address ? short(face.address) : undefined}
      className="flex shrink-0 items-center justify-center rounded-full border border-line bg-bg"
      style={{ width: size, height: size }}
    >
      <span className="orien-mark pearl-fill" style={{ width: size * 0.5, height: size * 0.5 }} aria-hidden />
    </span>
  )
}

/** Up to five faces, overlapping, and how many more. */
function FaceRow({ faces, count, size = 40 }: { faces: Face[]; count: number; size?: number }) {
  const shown = faces.slice(0, 5)
  const more = count - shown.length
  return (
    <div className="flex items-center">
      {shown.map((face, index) => (
        <span key={face.account ?? face.address ?? index} className="rounded-full ring-[3px] ring-surface" style={{ marginLeft: index ? -size / 4 : 0 }}>
          {face.name ? (
            <Link href={profileHref(face.name)} title={face.name}>
              <FaceAvatar face={face} size={size} />
            </Link>
          ) : (
            <FaceAvatar face={face} size={size} />
          )}
        </span>
      ))}
      {more > 0 && (
        <span
          className="num flex items-center justify-center rounded-full border border-line bg-raised text-xs font-bold text-ink ring-[3px] ring-surface"
          style={{ width: size, height: size, marginLeft: -size / 4 }}
        >
          +{more}
        </span>
      )}
    </div>
  )
}

/** "Cyph3r, Alice and 3 others". */
function names(faces: Face[], count: number) {
  const named = faces.filter((f) => f.name).map((f) => f.name as string)
  const others = count - named.length
  if (named.length === 0) return null
  const list = named.slice(0, 3)
  const rest = named.length - list.length + others
  const head = list.length === 1 ? list[0] : `${list.slice(0, -1).join(', ')} and ${list[list.length - 1]}`
  return rest > 0 ? `${head} and ${rest} ${rest === 1 ? 'other' : 'others'}` : head
}

/** The eyebrow over each kind of the network's card. One colour for all of
 * them: the site's own living pearl. */
const EYEBROWS: Record<Exclude<Post['kind'], 'post'>, string> = {
  record: 'New record',
  welcome: 'Welcome',
  milestone: 'Milestone',
  streak: 'On a streak',
  top_day: 'Top miner of the day',
}

/** What the network says, as a card of its own kind. */
function Happening({ post }: { post: Post }) {
  const epoch = Number(post.data.epoch)
  const checkpoint = (
    <a href={checkpointHref(epoch)} className="text-xs text-muted hover:text-ink hover:underline">
      checkpoint {epoch}
    </a>
  )
  switch (post.kind) {
    case 'record':
      return (
        <div className="flex items-end justify-between gap-4">
          <div>
            <p className="num text-5xl font-bold leading-none tracking-tight text-ink">{String(post.data.miners)}</p>
            <p className="mt-2 text-ink">miners paid this hour, the most ever</p>
            <p className="mt-1 text-sm text-muted">The record before was {String(post.data.before)}.</p>
          </div>
          <div className="text-right">{checkpoint}</div>
        </div>
      )
    case 'welcome': {
      const faces = post.data.miners ?? []
      const count = Number(post.data.count)
      const who = names(faces, count)
      const from = Number(post.data.from)
      const span = (
        <>
          checkpoints{' '}
          <a href={checkpointHref(from)} className="hover:text-ink hover:underline">
            {from}
          </a>{' '}
          to{' '}
          <a href={checkpointHref(epoch)} className="hover:text-ink hover:underline">
            {epoch}
          </a>
        </>
      )
      return (
        <div className="space-y-3">
          <FaceRow faces={faces} count={count} size={44} />
          <p className="text-lg font-semibold text-ink">
            {count === 1 ? 'A new miner joined' : `${count} new miners joined`} in the last six hours
          </p>
          <p className="text-sm text-muted">
            {who ? <>Say hi to {who}. </> : null}
            {count === 1 ? 'Their' : 'Their'} first ORN were paid between {span}.
          </p>
        </div>
      )
    }
    case 'milestone':
      return post.subject ? (
        <div className="flex items-center gap-4">
          <Avatar name={post.subject.name} picture={post.subject.picture} size={56} />
          <div className="min-w-0">
            <p className="num text-3xl font-bold leading-none tracking-tight text-ink">{Number(post.data.orn).toLocaleString('en')} ORN</p>
            <p className="mt-1.5 text-ink">
              <Name person={post.subject} /> passed it mining. 🎉
            </p>
            <p className="mt-1">{checkpoint}</p>
          </div>
        </div>
      ) : null
    case 'streak':
      return post.subject ? (
        <div className="flex items-center gap-4">
          <Avatar name={post.subject.name} picture={post.subject.picture} size={56} />
          <div className="min-w-0">
            <p className="num text-3xl font-bold leading-none tracking-tight text-ink">
              {String(post.data.hours)} hours in a row 🔥
            </p>
            <p className="mt-1.5 text-ink">
              <Name person={post.subject} /> has been paid at every checkpoint for {Number(post.data.hours) >= 48 ? `${Number(post.data.hours) / 24} days` : 'a whole day'}.
            </p>
            <p className="mt-1">{checkpoint}</p>
          </div>
        </div>
      ) : null
    case 'top_day': {
      const miner = post.data.miner ?? {}
      const day = new Date(`${post.data.day}T00:00:00Z`).toLocaleDateString('en', { day: 'numeric', month: 'long', timeZone: 'UTC' })
      return (
        <div className="flex items-center gap-4">
          <FaceAvatar face={miner} size={56} />
          <div className="min-w-0">
            <p className="text-lg font-semibold text-ink">
              {miner.name ? <Name person={{ name: miner.name }} /> : <span className="num">{short(miner.address ?? '')}</span>} mined the most on {day}
            </p>
            <p className="num mt-1 text-2xl font-bold text-ink">{mined(String(post.data.mined))} ORN</p>
            <p className="mt-1 text-sm text-muted">
              Of {mined(String(post.data.total))} ORN paid to {String(post.data.miners)} miners that day.
            </p>
          </div>
        </div>
      )
    }
    default:
      return null
  }
}

function Composer({ me, parent, onPosted, autoFocus = false }: { me: You; parent?: number; onPosted: () => void; autoFocus?: boolean }) {
  const [body, setBody] = useState('')
  const [busy, setBusy] = useState(false)
  const [trouble, setTrouble] = useState('')
  const send = async (event: React.FormEvent) => {
    event.preventDefault()
    setBusy(true)
    setTrouble('')
    try {
      await ask('/community/posts', 'POST', { body, parent })
      setBody('')
      onPosted()
    } catch (error) {
      setTrouble((error as Error).message)
    }
    setBusy(false)
  }
  const left = MAX - body.length
  return (
    <form onSubmit={send} className="flex gap-3">
      <Avatar name={me.name} picture={me.picture} size={parent ? 32 : 44} />
      <div className="min-w-0 flex-1 space-y-2">
        <AutoTextarea
          value={body}
          onChange={(e) => setBody(e.target.value.slice(0, MAX))}
          rows={1}
          autoFocus={autoFocus}
          placeholder={parent ? 'Post your reply' : 'What’s happening on Orien?'}
          className={`w-full resize-none bg-transparent py-2 text-ink outline-none placeholder:text-muted ${parent ? 'text-[15px]' : 'text-lg'}`}
        />
        {trouble && <Notice tone="bad">{trouble.charAt(0).toUpperCase() + trouble.slice(1)}</Notice>}
        <div className="flex items-center justify-end gap-3">
          {body.length > 0 && <span className={`num text-xs ${left < 50 ? 'text-ink' : 'text-muted'}`}>{left}</span>}
          <button type="submit" disabled={busy || !body.trim()} className={pill}>
            {busy ? 'Posting…' : parent ? 'Reply' : 'Post'}
          </button>
        </div>
      </div>
    </form>
  )
}

function ReactionBar({ post, signedIn, onChange }: { post: Post; signedIn: boolean; onChange: () => void }) {
  const [open, setOpen] = useState(false)
  const close = useCallback(() => setOpen(false), [])
  const box = useDismiss(open, close)
  const toggle = async (emoji: string) => {
    setOpen(false)
    if (!signedIn) {
      window.location.href = '/account/'
      return
    }
    await ask(`/community/posts/${post.id}/react`, 'POST', { emoji }).catch(() => undefined)
    onChange()
  }
  return (
    <div className="flex flex-wrap items-center gap-1.5">
      {post.reactions.map((r) => (
        <button
          key={r.emoji}
          type="button"
          onClick={() => void toggle(r.emoji)}
          aria-pressed={r.me}
          className={`flex h-7 items-center gap-1 rounded-full border px-2 text-sm transition ${
            r.me ? 'border-pearl/70 bg-raised text-ink' : 'border-line text-muted hover:border-pearl/60 hover:text-ink'
          }`}
        >
          <span aria-hidden>{r.emoji}</span>
          <span className="num text-xs font-semibold">{r.count}</span>
        </button>
      ))}
      <div ref={box} className="relative">
        <button type="button" onClick={() => setOpen(!open)} aria-label="React" title="React" className={iconButton}>
          <SmilePlus className="h-[18px] w-[18px]" aria-hidden />
        </button>
        {open && (
          <div className="absolute bottom-10 left-0 z-20 flex gap-1 rounded-full border border-line bg-surface p-1 shadow-2xl shadow-black/50">
            {REACTIONS.map((emoji) => (
              <button
                key={emoji}
                type="button"
                onClick={() => void toggle(emoji)}
                className="flex h-9 w-9 items-center justify-center rounded-full text-xl transition hover:scale-110 hover:bg-raised"
              >
                {emoji}
              </button>
            ))}
          </div>
        )}
      </div>
    </div>
  )
}

/** The "…" menu on a post: delete your own, report another's. */
function PostMenu({ post, signedIn, onRemoved }: { post: Post; signedIn: boolean; onRemoved: () => void }) {
  const [open, setOpen] = useState(false)
  const [reporting, setReporting] = useState(false)
  const close = useCallback(() => {
    setOpen(false)
    setReporting(false)
  }, [])
  const box = useDismiss(open || reporting, close)
  if (post.kind !== 'post') return null
  return (
    <div ref={box} className="relative">
      <button type="button" onClick={() => setOpen(!open)} aria-label="More" className={iconButton}>
        <MoreHorizontal className="h-[18px] w-[18px]" aria-hidden />
      </button>
      {open && (
        <div className="absolute right-0 top-9 z-20 w-44 overflow-hidden rounded-xl border border-line bg-surface p-1 shadow-2xl shadow-black/50">
          {post.mine ? (
            <button
              type="button"
              onClick={async () => {
                setOpen(false)
                if (!confirm('Delete this post?')) return
                await ask(`/community/posts/${post.id}`, 'DELETE').catch(() => undefined)
                onRemoved()
              }}
              className="flex w-full items-center gap-3 rounded-lg px-3 py-2 text-sm font-semibold text-stop transition hover:bg-raised"
            >
              <Trash2 className="h-4 w-4" aria-hidden /> Delete
            </button>
          ) : (
            <button
              type="button"
              onClick={() => {
                setOpen(false)
                setReporting(true)
              }}
              className="flex w-full items-center gap-3 rounded-lg px-3 py-2 text-sm font-semibold text-ink transition hover:bg-raised"
            >
              <Flag className="h-4 w-4" aria-hidden /> Report
            </button>
          )}
        </div>
      )}
      {reporting && (
        <div className="absolute right-0 top-9 z-20 w-80 max-w-[calc(100vw-3rem)] rounded-xl border border-line bg-surface p-2 shadow-2xl shadow-black/50">
          <ReportButton kind="post" id={post.id} what={post.parent ? 'this reply' : 'this post'} signedIn={signedIn} />
        </div>
      )}
    </div>
  )
}

function Reply({ reply, me, onChange }: { reply: Post; me: You | null; onChange: () => void }) {
  return (
    <div className="flex gap-3 py-3">
      {reply.author && <Avatar name={reply.author.name} picture={reply.author.picture} size={32} />}
      <div className="min-w-0 flex-1">
        <div className="flex items-start gap-2">
          <p className="min-w-0 flex-1 truncate text-[15px]">
            {reply.author && <Name person={reply.author} />} <span className="text-muted">· {since(reply.created)}</span>
          </p>
          <PostMenu post={reply} signedIn={!!me} onRemoved={onChange} />
        </div>
        <p className="whitespace-pre-line break-words text-[15px] leading-relaxed text-ink">{reply.body}</p>
        <div className="mt-2">
          <ReactionBar post={reply} signedIn={!!me} onChange={onChange} />
        </div>
      </div>
    </div>
  )
}

function PostCard({ post, me, onChange }: { post: Post; me: You | null; onChange: () => void }) {
  const [open, setOpen] = useState(false)
  const [replies, setReplies] = useState<Post[] | null>(null)
  const network = post.kind !== 'post'
  const eyebrow = network ? EYEBROWS[post.kind as keyof typeof EYEBROWS] : null
  const loadReplies = useCallback(async () => {
    const found = await ask<{ replies: Post[] }>(`/community/posts/${post.id}`).catch(() => ({ replies: [] }))
    setReplies(found.replies)
  }, [post.id])
  const changed = () => {
    if (open) void loadReplies()
    onChange()
  }

  return (
    <article className="relative overflow-hidden rounded-2xl border border-line bg-surface">
      {network && (
        <>
          <div className="pearl-fill absolute inset-x-0 top-0 h-0.5" aria-hidden />
          {/* A piece of space for each kind of news, fading into the card towards the words. */}
          {/* eslint-disable-next-line @next/next/no-img-element */}
          <img
            src={`/community/${post.kind}.jpg`}
            alt=""
            className="pointer-events-none absolute right-0 top-0 h-full w-full object-cover opacity-80 [mask-image:linear-gradient(to_left,black,transparent_85%)] sm:w-3/5 sm:[mask-image:linear-gradient(to_left,black_20%,transparent)]"
          />
        </>
      )}
      <div className={`relative p-5 ${network ? 'on-banner' : ''}`}>
        <header className="flex items-center gap-3">
          {network ? <NetworkAvatar size={40} /> : post.author && <Avatar name={post.author.name} picture={post.author.picture} size={44} />}
          <div className="min-w-0 flex-1">
            {network && eyebrow ? (
              <>
                <p className="pearl-text text-[11px] font-bold uppercase tracking-[0.16em]">{eyebrow}</p>
                <p className="text-sm text-muted">
                  <span className="font-semibold text-ink">Orien</span> · {since(post.created)}
                </p>
              </>
            ) : (
              post.author && (
                <>
                  <Name person={post.author} className="block truncate text-[15px]" />
                  <p className="text-sm text-muted">{since(post.created)}</p>
                </>
              )
            )}
          </div>
          <PostMenu post={post} signedIn={!!me} onRemoved={onChange} />
        </header>

        <div className={`mt-4 ${network ? '' : 'whitespace-pre-line break-words text-[17px] leading-relaxed text-ink'}`}>
          {network ? <Happening post={post} /> : post.body}
        </div>

        <div className="mt-4 flex flex-wrap items-center gap-x-4 gap-y-2">
          <button
            type="button"
            onClick={() => {
              setOpen(!open)
              if (!open && replies === null) void loadReplies()
            }}
            className={`flex h-8 items-center gap-1.5 rounded-full px-2 text-sm transition hover:bg-raised hover:text-ink ${open ? 'text-ink' : 'text-muted'}`}
          >
            <MessageCircle className="h-[18px] w-[18px]" aria-hidden />
            {post.replies > 0 && <span className="num text-xs font-semibold">{post.replies}</span>}
          </button>
          <ReactionBar post={post} signedIn={!!me} onChange={onChange} />
        </div>

        {open && (
          <div className="mt-3 border-t border-line">
            <div className="divide-y divide-line">{replies?.map((reply) => <Reply key={reply.id} reply={reply} me={me} onChange={changed} />)}</div>
            <div className="pt-3">
              {me?.verified ? (
                <Composer me={me} parent={post.id} autoFocus onPosted={changed} />
              ) : (
                <p className="text-sm text-muted">
                  <Link href="/account/" className="font-semibold text-ink hover:underline">
                    {me ? 'Verify a wallet' : 'Sign in'}
                  </Link>{' '}
                  to reply.
                </p>
              )}
            </div>
          </div>
        )}
      </div>
    </article>
  )
}

/** Miners this hour, ORN this hour, hashrate, and the next checkpoint, live. */
function LiveStrip() {
  const [overview, setOverview] = useState<Overview | null>(null)
  const [now, setNow] = useState(() => Date.now() / 1000)
  useEffect(() => {
    let live = true
    const load = () => explore<Overview>('/overview').then((o) => live && setOverview(o)).catch(() => undefined)
    load()
    const refresh = setInterval(load, 30_000)
    const tick = setInterval(() => setNow(Date.now() / 1000), 1000)
    return () => {
      live = false
      clearInterval(refresh)
      clearInterval(tick)
    }
  }, [])
  const left = overview ? overview.epoch_started + overview.epoch_seconds - now : 0
  const countdown =
    left <= 0 ? 'any moment' : `${Math.floor(left / 60)}:${String(Math.floor(left % 60)).padStart(2, '0')}`
  const cells: [string, string][] = [
    ['Miners this hour', overview?.miners ? overview.miners.last_hour.toLocaleString('en') : '—'],
    ['ORN this hour', overview?.supply ? orn(overview.supply.per_hour, 0) : '—'],
    ['Hashrate', overview?.hashrate ? hashrateText(overview.hashrate.per_second) : '—'],
    ['Next checkpoint', overview ? countdown : '—'],
  ]
  return (
    <div className={`${card} grid grid-cols-2 divide-line sm:grid-cols-4 sm:divide-x`}>
      {cells.map(([label, value]) => (
        <div key={label} className="px-4 py-3">
          <p className="flex items-center gap-1.5 text-[11px] font-semibold uppercase tracking-[0.14em] text-muted">
            {label === 'Miners this hour' && <span className="h-1.5 w-1.5 rounded-full bg-go" aria-hidden />}
            {label}
          </p>
          <p className="num mt-0.5 text-xl font-bold text-ink">{value}</p>
        </div>
      ))}
    </div>
  )
}

function YouCard({ you, onChange }: { you: You | null; onChange: () => void }) {
  if (!you) {
    return (
      <section className={`${card} p-4`}>
        <p className="font-bold text-ink">New here?</p>
        <p className="mt-1 text-sm leading-relaxed text-muted">Sign in to post, react and join a team.</p>
        <Link href="/account/" className={`${pill} mt-3`}>
          Sign in
        </Link>
      </section>
    )
  }
  return (
    <section className={`${card} space-y-3 p-4`}>
      <div className="flex items-center gap-3">
        <Avatar name={you.name} picture={you.picture} size={44} />
        <div className="min-w-0 flex-1">
          <Name person={you} className="block truncate" />
          <p className="truncate text-sm text-muted">{you.team ? `Team ${you.team.name}` : 'No team yet'}</p>
        </div>
      </div>
      {you.stats ? (
        <div className="grid grid-cols-3 divide-x divide-line rounded-xl border border-line text-center">
          {[
            [`#${you.stats.rank}`, 'rank'],
            [mined(you.stats.mined), 'ORN mined'],
            [`${you.stats.streak}h`, 'in a row'],
          ].map(([value, label]) => (
            <div key={label} className="px-1 py-2.5">
              <p className="num text-sm font-bold text-ink">{value}</p>
              <p className="text-[11px] text-muted">{label}</p>
            </div>
          ))}
        </div>
      ) : (
        <p className="text-sm leading-relaxed text-muted">
          {you.verified
            ? 'Mine with a verified wallet and your figures show here after the next checkpoint.'
            : 'Verify a wallet to post, join a team and see your mining here.'}
        </p>
      )}
      {!you.verified && (
        <Link href="/account/" className={quiet}>
          Verify a wallet
        </Link>
      )}
      {you.team && (
        <button
          type="button"
          className={`${quiet} w-full`}
          onClick={async () => {
            if (!confirm(`Leave ${you.team!.name}?`)) return
            await ask('/community/teams/leave', 'POST').catch(() => undefined)
            onChange()
          }}
        >
          Leave {you.team.name}
        </button>
      )}
    </section>
  )
}

const MEDALS = ['🥇', '🥈', '🥉']

function Miners({ board }: { board: Board }) {
  const [all, setAll] = useState(false)
  const rows = all ? board.miners : board.miners.slice(0, 8)
  return (
    <section className={card}>
      <h2 className="flex items-center gap-2 px-4 pb-1 pt-4 text-lg font-bold text-ink">
        <Trophy className="h-4 w-4 text-pearl" aria-hidden /> Top miners
      </h2>
      {rows.length === 0 ? (
        <p className="px-4 pb-4 pt-2 text-sm text-muted">Nobody with a verified wallet has been paid for mining yet.</p>
      ) : (
        <ol>
          {rows.map((row) => (
            <li key={row.rank} className="flex items-center gap-3 px-4 py-2.5 transition hover:bg-raised/40">
              <span className={`num w-6 shrink-0 text-center text-sm ${row.rank <= 3 ? 'text-base' : 'text-muted'}`}>
                {row.rank <= 3 ? MEDALS[row.rank - 1] : row.rank}
              </span>
              <Avatar name={row.name ?? ''} picture={row.picture} size={32} />
              <span className="min-w-0 flex-1">
                <Link href={profileHref(row.name ?? '')} className="block truncate text-sm font-bold text-ink hover:underline">
                  {row.name}
                </Link>
                <span className="flex items-center gap-2 truncate text-xs text-muted">
                  {row.streak > 0 && (
                    <span className="inline-flex items-center gap-0.5">
                      <Flame className="h-3 w-3" aria-hidden />
                      {row.streak}h
                    </span>
                  )}
                  {row.team && <span className="truncate rounded-md border border-line px-1.5 text-[11px]">{row.team}</span>}
                </span>
              </span>
              <span className="num shrink-0 text-sm font-semibold text-ink">{mined(row.mined)}</span>
            </li>
          ))}
        </ol>
      )}
      {board.miners.length > 8 && (
        <button
          type="button"
          onClick={() => setAll(!all)}
          className="w-full rounded-b-2xl px-4 py-3 text-left text-sm font-semibold text-pearl transition hover:bg-raised/40"
        >
          {all ? 'Show less' : 'Show more'}
        </button>
      )}
    </section>
  )
}

function Teams({ board, you, onChange }: { board: Board; you: You | null; onChange: () => void }) {
  const [making, setMaking] = useState(false)
  const [name, setName] = useState('')
  const [trouble, setTrouble] = useState('')
  const [busy, setBusy] = useState(false)
  const [members, setMembers] = useState<{ id: number; list: Person[] } | null>(null)
  const canJoin = !!you?.verified && !you.team
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
  return (
    <section className={card}>
      <div className="flex items-center gap-2 px-4 pb-1 pt-4">
        <h2 className="flex flex-1 items-center gap-2 text-lg font-bold text-ink">
          <Users className="h-4 w-4 text-pearl" aria-hidden /> Teams
        </h2>
        {canJoin && !making && (
          <button type="button" onClick={() => setMaking(true)} className={`${quiet} h-8 px-3 text-xs`}>
            <Plus className="h-3.5 w-3.5" aria-hidden /> Start one
          </button>
        )}
      </div>
      {making && (
        <form
          className="space-y-2 px-4 py-3"
          onSubmit={(event) => {
            event.preventDefault()
            void run(async () => {
              await ask('/community/teams', 'POST', { name })
              setMaking(false)
              setName('')
              onChange()
            })
          }}
        >
          <input
            value={name}
            onChange={(e) => setName(e.target.value)}
            maxLength={30}
            required
            autoFocus
            placeholder="Team name"
            className="h-10 w-full rounded-xl border border-line bg-bg px-3 text-sm text-ink outline-none focus:border-pearl/60"
          />
          <div className="flex gap-2">
            <button type="submit" disabled={busy} className={pill}>
              {busy ? 'Starting…' : 'Start'}
            </button>
            <button type="button" onClick={() => setMaking(false)} className={quiet}>
              Cancel
            </button>
          </div>
        </form>
      )}
      {trouble && (
        <div className="px-4 py-2">
          <Notice tone="bad">{trouble.charAt(0).toUpperCase() + trouble.slice(1)}</Notice>
        </div>
      )}
      {board.teams.length === 0 ? (
        <p className="px-4 pb-4 pt-2 text-sm text-muted">No teams yet. Start the first one.</p>
      ) : (
        <ol className="pb-2">
          {board.teams.map((team) => (
            <li key={team.id}>
              <button
                type="button"
                onClick={() =>
                  void run(async () => {
                    if (members?.id === team.id) return setMembers(null)
                    const found = await ask<{ members: Person[] }>(`/community/teams/${team.id}`)
                    setMembers({ id: team.id, list: found.members })
                  })
                }
                className="flex w-full items-center gap-3 px-4 py-2.5 text-left transition hover:bg-raised/40"
              >
                <span className={`num w-6 shrink-0 text-center text-sm ${team.rank <= 3 ? 'text-base' : 'text-muted'}`}>
                  {team.rank <= 3 ? MEDALS[team.rank - 1] : team.rank}
                </span>
                <span className="min-w-0 flex-1">
                  <span className="block truncate text-sm font-bold text-ink">{team.name}</span>
                  <span className="block text-xs text-muted">
                    {team.members} {team.members === 1 ? 'member' : 'members'}
                  </span>
                </span>
                <span className="num shrink-0 text-sm font-semibold text-ink">{mined(team.mined)}</span>
              </button>
              {members?.id === team.id && (
                <div className="space-y-2 px-4 pb-3 pl-12">
                  {members.list.map((person) => (
                    <div key={person.name} className="flex items-center gap-2 text-sm">
                      <Avatar name={person.name} picture={person.picture} size={22} />
                      <Name person={person} />
                    </div>
                  ))}
                  {canJoin && (
                    <button
                      type="button"
                      disabled={busy}
                      onClick={() =>
                        void run(async () => {
                          await ask(`/community/teams/${team.id}/join`, 'POST')
                          setMembers(null)
                          onChange()
                        })
                      }
                      className={`${quiet} mt-1 h-8 px-3 text-xs`}
                    >
                      Join {team.name}
                    </button>
                  )}
                </div>
              )}
            </li>
          ))}
        </ol>
      )}
    </section>
  )
}

/** /community/: the network and its people, as a feed of cards. */
export function CommunityView() {
  const [tab, setTab] = useState<Tab>('all')
  const [posts, setPosts] = useState<Post[] | null>(null)
  const [more, setMore] = useState(false)
  const [board, setBoard] = useState<Board | null>(null)
  const [you, setYou] = useState<You | null>(null)
  const [trouble, setTrouble] = useState('')
  const live = useMe()

  const path = useCallback(
    (before?: number) => {
      const query = new URLSearchParams()
      if (tab !== 'all') query.set('only', tab)
      if (before) query.set('before', String(before))
      const q = query.toString()
      return `/community/feed${q ? `?${q}` : ''}`
    },
    [tab],
  )

  const loadFeed = useCallback(async () => {
    try {
      const found = await ask<{ posts: Post[]; more: boolean }>(path())
      setPosts(found.posts)
      setMore(found.more)
      setTrouble('')
    } catch (error) {
      setTrouble((error as Error).message)
    }
  }, [path])
  const loadBoard = useCallback(async () => {
    const found = await ask<{ board: Board | null; you: You | null }>('/community/board').catch(() => null)
    if (found) {
      setBoard(found.board)
      setYou(found.you)
    }
  }, [])
  const reload = useCallback(() => {
    void loadFeed()
    void loadBoard()
  }, [loadFeed, loadBoard])

  useEffect(() => {
    reload()
    const timer = setInterval(reload, 60_000)
    return () => clearInterval(timer)
  }, [reload, live.account])

  const older = async () => {
    const last = posts?.[posts.length - 1]
    if (!last) return
    const found = await ask<{ posts: Post[]; more: boolean }>(path(last.id)).catch(() => null)
    if (found) {
      setPosts([...(posts ?? []), ...found.posts])
      setMore(found.more)
    }
  }

  return (
    <div className="mx-auto max-w-[1040px] space-y-5 pb-16 pt-4 sm:pt-6">
      <div className="flex flex-wrap items-end justify-between gap-3 px-1">
        <h1 className="text-3xl font-bold tracking-tight text-ink">Community</h1>
        <p className="text-sm text-muted">Be kind. Criticism welcome; scams and abuse are not.</p>
      </div>
      <LiveStrip />

      <div className="grid items-start gap-6 lg:grid-cols-[minmax(0,1fr)_320px]">
        <main className="min-w-0 space-y-3">
          <div className={`${card} flex overflow-hidden`}>
            {TABS.map(([key, label]) => (
              <button
                key={key}
                type="button"
                onClick={() => {
                  setTab(key)
                  setPosts(null)
                }}
                className={`relative flex-1 py-3 text-sm font-semibold transition hover:bg-raised/40 ${tab === key ? 'text-ink' : 'text-muted'}`}
              >
                {label}
                {tab === key && <span className="absolute inset-x-1/3 bottom-0 h-1 rounded-full bg-pearl" aria-hidden />}
              </button>
            ))}
          </div>

          {you?.verified ? (
            <div className="rounded-2xl p-px pearl-fill">
              <div className="rounded-[15px] bg-surface p-4">
                <Composer me={you} onPosted={reload} />
              </div>
            </div>
          ) : (
            <div className={`${card} px-5 py-4 text-sm text-muted`}>
              <Link href="/account/" className="font-semibold text-ink hover:underline">
                {you ? 'Verify a wallet' : 'Sign in'}
              </Link>{' '}
              {you ? 'to post. Anyone signed in can react.' : 'to post and react.'}
            </div>
          )}

          {trouble && <Notice tone="bad">{trouble}</Notice>}
          {posts === null && !trouble && <p className="py-10 text-center text-muted">Loading…</p>}
          {posts?.length === 0 && <p className={`${card} py-10 text-center text-muted`}>Nothing here yet.</p>}
          {posts?.map((post) => <PostCard key={post.id} post={post} me={you} onChange={reload} />)}
          {more && (
            <button type="button" onClick={() => void older()} className={`${quiet} w-full`}>
              Show more
            </button>
          )}
        </main>

        <aside className="space-y-4">
          <YouCard you={you} onChange={reload} />
          {board ? (
            <>
              <Miners board={board} />
              <Teams board={board} you={you} onChange={reload} />
            </>
          ) : (
            <p className={`${card} p-4 text-sm text-muted`}>The leaderboards appear once the network has been read.</p>
          )}
        </aside>
      </div>
    </div>
  )
}
