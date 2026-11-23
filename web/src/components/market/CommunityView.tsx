'use client'

import Link from 'next/link'
import { useSearchParams } from 'next/navigation'
import { useCallback, useEffect, useRef, useState } from 'react'
import { ArrowLeft, BadgeCheck, Flag, Flame, MessageCircle, MoreHorizontal, Search, SmilePlus, Trash2, Trophy } from 'lucide-react'
import { ask, pictureUrl, profileHref } from '@/lib/market'
import { checkpointHref, orn, short } from '@/lib/explorer'
import { useMe } from '@/lib/useMe'
import { AutoTextarea } from '@/components/AutoTextarea'
import { Avatar } from './Avatar'
import { ReportButton } from './ReportButton'
import { Notice } from './Shell'
import { Linked } from './Linked'
import { YouTube, youtubeId } from './YouTube'
import { openSignIn } from './SignInDialog'
import { EYEBROWS, NetworkAvatar } from './network'

type Person = { name: string; picture: string | null; banner?: string | null; tip_to?: string | null }
type Face = { account?: number; address?: string; name?: string; picture?: string | null; paid?: string }
type Reaction = { emoji: string; count: number; me: boolean }
type Post = {
  id: number
  kind: 'post' | 'record' | 'welcome' | 'milestone' | 'streak' | 'top_day' | 'swap' | 'supply' | 'validator'
  body: string
  data: Record<string, unknown> & { miners?: Face[]; miner?: Face }
  created: number
  parent: number | null
  author: Person | null
  subject: Person | null
  replies: number
  mine: boolean
  reactions: Reaction[]
  /** Tips paid to its author: how many, and how much in ORN's smallest units. */
  tips?: { count: number; orn: number }
}
export type Stats = { mined: string; streak: number; rank: number }
type MinerRow = { rank: number; name: string | null; picture: string | null; address: string | null; mined: string; streak: number }
type Board = { epoch: number; since: number; miners: MinerRow[] }
type You = { name: string; picture: string | null; verified: boolean; stats: Stats | null }
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

/** The newer kinds of network post this page can show: the feed sends
 * them only to apps that ask. */
const KINDS = 'swap,supply,validator'

/** Newer kinds' pictures, named for what they show, so a changed picture
 * never comes from a browser's saved copy of the one before. */
const PICTURE: Record<string, string> = {
  swap: 'swap-milky-way',
  supply: 'supply-north-america-nebula',
  validator: 'validator-helix-nebula',
}

/** A swap's coin amount, for reading: "0.5 SOL", "0.0123 BTC". */
const coinAmount = (amount: number, coin: string) =>
  `${amount >= 1 ? Number(amount.toFixed(2)).toLocaleString('en') : Number(amount.toPrecision(3)).toLocaleString('en', { maximumSignificantDigits: 3 })} ${coin === 'sol' ? 'SOL' : 'BTC'}`

/** Dollars, rounded for reading. */
const usdText = (usd: number) => (usd >= 100 ? `$${Math.round(usd).toLocaleString('en')}` : usd >= 1 ? `$${usd.toFixed(2)}` : `$${usd.toPrecision(2)}`)

/** "2h", "3d": the short time a feed uses. */
function since(seconds: number) {
  const s = Math.max(0, Math.floor(Date.now() / 1000 - seconds))
  if (s < 60) return 'now'
  if (s < 3600) return `${Math.floor(s / 60)}m`
  if (s < 86400) return `${Math.floor(s / 3600)}h`
  if (s < 86400 * 30) return `${Math.floor(s / 86400)}d`
  return new Date(seconds * 1000).toLocaleDateString('en', { day: 'numeric', month: 'short' })
}

/** A YouTube video a post links to, playable in place. */
function Video({ text }: { text: string }) {
  const id = youtubeId(text)
  return id ? <YouTube id={id} /> : null
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
        <div>
          <p className="num text-5xl font-bold leading-none tracking-tight text-ink">{String(post.data.miners)}</p>
          <p className="mt-2 text-ink">miners paid this hour, the most ever</p>
          <p className="mt-1 text-sm text-muted">The record before was {String(post.data.before)}.</p>
          <p className="mt-1">{checkpoint}</p>
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
            <p className="num text-3xl font-bold leading-none tracking-tight text-ink">
              {Number(post.data.orn) === 1 ? 'First ORN' : `${Number(post.data.orn).toLocaleString('en')} ORN`}
            </p>
            <p className="mt-1.5 text-ink">
              <Name person={post.subject} /> {Number(post.data.orn) === 1 ? 'mined their first ORN.' : 'passed it mining.'} 🎉
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
              <Name person={post.subject} /> has been paid at every checkpoint for {String(post.data.hours)} hours.
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
              {miner.name ? <Name person={{ name: miner.name }} /> : <span className="num">{short(miner.address ?? '')}</span>} mined the most of Orien
              Network’s members on {day}
            </p>
            <p className="num mt-1 text-2xl font-bold text-ink">{mined(String(post.data.mined))} ORN</p>
            <p className="mt-1 text-sm text-muted">
              Of {mined(String(post.data.total))} ORN paid to {String(post.data.miners)} miners that day.
            </p>
          </div>
        </div>
      )
    }
    case 'swap': {
      const coin = String(post.data.coin)
      const paid = coin === 'sol' ? Number(post.data.lamports) / 1e9 : Number(post.data.sats) / 1e8
      const ornAmount = Number(post.data.orn) / 1e9
      const usd = Number(post.data.usd)
      return (
        <div className="min-w-0">
          <p className="text-3xl font-bold leading-tight tracking-tight text-ink">
            Someone bought {ornAmount.toLocaleString('en', { maximumFractionDigits: 2 })} ORN
          </p>
          <p className="mt-1.5 text-lg text-ink">
            They paid {coinAmount(paid, coin)}, about {usdText(usd)}.
          </p>
          <p className="mt-1 text-sm text-muted">
            That&apos;s {coinAmount(paid / ornAmount, coin)} (about {usdText(usd / ornAmount)}) for each ORN.{' '}
            <Link href="/swap/#history" className="font-semibold text-ink hover:underline">
              See all swaps
            </Link>
          </p>
        </div>
      )
    }
    case 'supply':
      return (
        <div className="min-w-0">
          <p className="num text-3xl font-bold leading-none tracking-tight text-ink">{Number(post.data.orn).toLocaleString('en')} ORN mined</p>
          <p className="mt-1.5 text-ink">
            {Number(post.data.share).toLocaleString('en', { maximumFractionDigits: 2 })}% of all the ORN there will ever be.
          </p>
          <p className="mt-1 text-sm text-muted">Every ORN comes from mining, a little more every hour.</p>
        </div>
      )
    case 'validator':
      return (
        <div className="min-w-0">
          <p className="text-2xl font-bold leading-tight tracking-tight text-ink">A new validator took a seat</p>
          <p className="mt-1.5 text-ink">
            The network has {String(post.data.seats)} validators now, keeping its checkpoints together.
          </p>
          <p className="mt-1 text-sm text-muted">
            <a href="https://explorer.orien.network/map/?view=details" className="hover:text-ink hover:underline">
              See the network
            </a>
          </p>
        </div>
      )
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
          placeholder={parent ? 'Post your reply' : 'What’s happening?'}
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
      openSignIn()
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
      {post.tips && post.tips.count > 0 && (
        <span className="num ml-1 text-xs text-muted" title="Tips paid to its author from Orien Wallet">
          {post.tips.count} {post.tips.count === 1 ? 'tip' : 'tips'} ·{' '}
          {(post.tips.orn / 1e9).toLocaleString('en', { maximumFractionDigits: 2 })} ORN
        </span>
      )}
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
        <p className="whitespace-pre-line break-words text-[15px] leading-relaxed text-ink">
          <Linked text={reply.body} />
        </p>
        <Video text={reply.body} />
        <div className="mt-2">
          <ReactionBar post={reply} signedIn={!!me} onChange={onChange} />
        </div>
      </div>
    </div>
  )
}

function PostCard({ post, me, onChange, startOpen = false }: { post: Post; me: You | null; onChange: () => void; startOpen?: boolean }) {
  const [open, setOpen] = useState(startOpen)
  const [replies, setReplies] = useState<Post[] | null>(null)
  const network = post.kind !== 'post'
  const banner = network ? null : post.author?.banner
  const eyebrow = network ? EYEBROWS[post.kind as keyof typeof EYEBROWS] : null
  const loadReplies = useCallback(async () => {
    const found = await ask<{ replies: Post[] }>(`/community/posts/${post.id}`).catch(() => ({ replies: [] }))
    setReplies(found.replies)
  }, [post.id])
  // Opened from a notification: the replies show at once.
  useEffect(() => {
    if (startOpen) void loadReplies()
  }, [startOpen, loadReplies])
  const changed = () => {
    if (open) void loadReplies()
    onChange()
  }

  return (
    <article className="relative overflow-hidden rounded-2xl border border-line bg-surface">
      {network && (
        <>
          <div className="pearl-fill absolute inset-x-0 top-0 h-0.5" aria-hidden />
          {/* A piece of space for each kind of news, in its own shape at the top
              of the card: it fades into the card towards the words, and fades out
              downwards, so replies opening below it sit on the plain card. */}
          {/* eslint-disable-next-line @next/next/no-img-element */}
          <img
            src={`/community/${PICTURE[post.kind] ?? post.kind}.jpg`}
            alt=""
            className="pointer-events-none absolute right-0 top-0 h-auto max-h-96 w-full object-cover opacity-80 [mask-composite:intersect] [mask-image:linear-gradient(to_left,black,transparent_90%),linear-gradient(to_bottom,black_55%,transparent)] sm:w-4/5 sm:[mask-image:linear-gradient(to_left,black_15%,rgb(0_0_0/0.55)_50%,transparent_95%),linear-gradient(to_bottom,black_55%,transparent)]"
          />
        </>
      )}
      {banner && (
        // The author's profile banner, fading into the card the same way.
        // eslint-disable-next-line @next/next/no-img-element
        <img
          src={pictureUrl(banner)}
          alt=""
          className="pointer-events-none absolute right-0 top-0 h-auto max-h-96 w-full object-cover opacity-70 [mask-composite:intersect] [mask-image:linear-gradient(to_left,black,transparent_90%),linear-gradient(to_bottom,black_55%,transparent)] sm:w-4/5 sm:[mask-image:linear-gradient(to_left,black_15%,rgb(0_0_0/0.55)_50%,transparent_95%),linear-gradient(to_bottom,black_55%,transparent)]"
        />
      )}
      <div className={`relative p-5 ${network || banner ? 'on-banner' : ''}`}>
        <header className="flex items-center gap-3">
          {network ? <NetworkAvatar size={40} /> : post.author && <Avatar name={post.author.name} picture={post.author.picture} size={44} />}
          <div className="min-w-0 flex-1">
            {network && eyebrow ? (
              <>
                <p className="text-[11px] font-bold uppercase tracking-[0.16em] text-pearl">{eyebrow}</p>
                <p className="text-sm text-muted">
                  <span className="font-semibold text-ink">Orien Network</span> · {since(post.created)}
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
          {network ? <Happening post={post} /> : <Linked text={post.body} />}
        </div>
        {!network && <Video text={post.body} />}

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

function YouCard({ you, onChange }: { you: You | null; onChange: () => void }) {
  if (!you) {
    return (
      <section className={`${card} p-4`}>
        <p className="font-bold text-ink">New here?</p>
        <p className="mt-1 text-sm leading-relaxed text-muted">Sign in to post and react.</p>
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
          <p className="truncate text-sm text-muted">{you.verified ? 'Verified wallet' : 'No verified wallet yet'}</p>
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
            : 'Verify a wallet to post and see your mining here.'}
        </p>
      )}
      {!you.verified && (
        <Link href="/account/" className={quiet}>
          Verify a wallet
        </Link>
      )}
    </section>
  )
}

const MEDALS = ['🥇', '🥈', '🥉']

type Member = { name: string; picture: string | null; verified: boolean; rank: number | null }

/** Finds members by name, as they type, each leading to their profile. */
function FindMembers() {
  const [text, setText] = useState('')
  const [found, setFound] = useState<Member[] | null>(null)
  useEffect(() => {
    const query = text.trim()
    if (query.length < 2) {
      setFound(null)
      return
    }
    let current = true
    const timer = setTimeout(() => {
      ask<{ members: Member[] }>(`/community/members?q=${encodeURIComponent(query)}`)
        .then((answer) => current && setFound(answer.members))
        .catch(() => current && setFound([]))
    }, 200)
    return () => {
      current = false
      clearTimeout(timer)
    }
  }, [text])
  return (
    <section className={card}>
      <label className="flex items-center gap-2.5 px-4">
        <Search className="h-4 w-4 shrink-0 text-muted" aria-hidden />
        <input
          type="search"
          value={text}
          onChange={(event) => setText(event.target.value)}
          placeholder="Find members"
          aria-label="Find members by name"
          className="h-12 min-w-0 flex-1 bg-transparent text-sm text-ink outline-none placeholder:text-muted"
        />
      </label>
      {found && (
        <div className="border-t border-line py-1.5">
          {found.length === 0 ? (
            <p className="px-4 py-2.5 text-sm text-muted">Nobody by that name.</p>
          ) : (
            found.map((member) => (
              <Link
                key={member.name}
                href={profileHref(member.name)}
                className="flex items-center gap-3 px-4 py-2 transition hover:bg-raised/40"
              >
                <Avatar name={member.name} picture={member.picture} size={32} />
                <span className="min-w-0 flex-1 truncate text-sm font-bold text-ink">{member.name}</span>
                {member.rank !== null ? (
                  <span className="num shrink-0 text-xs text-muted">
                    {member.rank <= 3 ? MEDALS[member.rank - 1] : `#${member.rank}`} miner
                  </span>
                ) : (
                  member.verified && <BadgeCheck className="h-4 w-4 shrink-0 text-go" aria-label="Verified" />
                )}
              </Link>
            ))
          )}
        </div>
      )}
    </section>
  )
}

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
          className="w-full px-4 py-3 text-left text-sm font-semibold text-pearl transition hover:bg-raised/40"
        >
          {all ? 'Show less' : 'Show more'}
        </button>
      )}
      <p className="border-t border-line px-4 py-3 text-xs leading-relaxed text-muted">
        Miners who have verified a wallet on their Orien Network account, by ORN mined. To be on the list,{' '}
        <Link href="/account/" className="font-semibold text-ink hover:underline">
          verify your wallet
        </Link>{' '}
        on your account.
      </p>
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
  // A notification opens one post: /community/?post=ID.
  const focus = Number(useSearchParams().get('post')) || null
  const [focused, setFocused] = useState<Post | null | undefined>(undefined)
  const loadFocused = useCallback(async () => {
    if (!focus) return
    const found = await ask<{ post: Post | null }>(`/community/posts/${focus}`).catch(() => ({ post: null }))
    setFocused(found.post)
  }, [focus])
  useEffect(() => {
    setFocused(undefined)
    void loadFocused()
    if (focus) window.scrollTo({ top: 0 })
  }, [loadFocused, focus, live.account])

  const path = useCallback(
    (before?: number) => {
      const query = new URLSearchParams()
      if (tab !== 'all') query.set('only', tab)
      query.set('kinds', KINDS)
      if (before) query.set('before', String(before))
      const q = query.toString()
      return `/community/feed${q ? `?${q}` : ''}`
    },
    [tab],
  )

  // The latest feed asked for; an answer for an earlier tab is dropped.
  const asked = useRef('')
  const loadFeed = useCallback(async () => {
    const wanted = path()
    asked.current = wanted
    try {
      const found = await ask<{ posts: Post[]; more: boolean }>(wanted)
      if (asked.current !== wanted) return
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
    <div className="mx-auto max-w-[1040px] space-y-5 pb-16 pt-5 sm:pt-7">
      <h1 className="px-1 text-3xl font-bold tracking-tight text-ink">Community</h1>

      <div className="grid items-start gap-6 lg:grid-cols-[minmax(0,1fr)_320px]">
        <main className="min-w-0 space-y-3">
          {focus ? (
            <>
              <Link href="/community/" className={`${quiet} w-fit`}>
                <ArrowLeft className="h-4 w-4" aria-hidden /> Back to the feed
              </Link>
              {focused === undefined && <p className="py-10 text-center text-muted">Loading…</p>}
              {focused === null && <p className={`${card} py-10 text-center text-muted`}>This post is gone.</p>}
              {focused && <PostCard key={focused.id} post={focused} me={you} onChange={loadFocused} startOpen />}
            </>
          ) : (
            <>
          <div className={`${card} flex overflow-hidden`}>
            {TABS.map(([key, label]) => (
              <button
                key={key}
                type="button"
                onClick={() => {
                  // The tab already open: fresh posts, without emptying it.
                  if (key === tab) return void loadFeed()
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
            </>
          )}
        </main>

        {/* Stays in view beside the feed; taller than the window, it scrolls on its own. */}
        <aside className="space-y-4 lg:sticky lg:top-24 lg:max-h-[calc(100vh-7rem)] lg:overflow-y-auto lg:pr-1 lg:[scrollbar-width:thin]">
          <FindMembers />
          <YouCard you={you} onChange={reload} />
          {board ? (
            <Miners board={board} />
          ) : (
            <p className={`${card} p-4 text-sm text-muted`}>The leaderboard appears once the network has been read.</p>
          )}
        </aside>
      </div>
    </div>
  )
}

/** Someone's mining, on their profile: their place among the members,
 * what they have mined and their streak, from the leaderboard. */
export function MiningCard({ stats }: { stats: Stats }) {
  return (
    <section className={card}>
      <h2 className="flex items-center gap-2 px-4 pb-1 pt-4 text-lg font-bold text-ink">
        <Trophy className="h-4 w-4 text-pearl" aria-hidden /> Mining
      </h2>
      <div className="grid grid-cols-3 divide-x divide-line text-center">
        {[
          [stats.rank <= 3 ? MEDALS[stats.rank - 1] : `#${stats.rank}`, 'top miner'],
          [mined(stats.mined), 'ORN mined'],
          [`${stats.streak}h`, 'in a row'],
        ].map(([value, label]) => (
          <div key={label} className="px-1 py-3">
            <p className="num text-base font-bold text-ink">{value}</p>
            <p className="text-[11px] text-muted">{label}</p>
          </div>
        ))}
      </div>
      <p className="border-t border-line px-4 py-3 text-xs leading-relaxed text-muted">
        Among members with a verified wallet, by ORN mined.{' '}
        <Link href="/community/" className="font-semibold text-ink hover:underline">
          See the top miners
        </Link>
      </p>
    </section>
  )
}

/** Someone's part of the community, on their profile: their own posts, and
 * the network's posts that name them. */
export function ProfileFeed({ name }: { name: string }) {
  const [posts, setPosts] = useState<Post[] | null>(null)
  const [more, setMore] = useState(false)
  const [you, setYou] = useState<You | null>(null)
  const live = useMe()
  const path = useCallback(
    (before?: number) => `/community/feed?about=${encodeURIComponent(name)}&kinds=${KINDS}${before ? `&before=${before}` : ''}`,
    [name],
  )
  const reload = useCallback(() => {
    ask<{ posts: Post[]; more: boolean }>(path())
      .then((found) => {
        setPosts(found.posts)
        setMore(found.more)
      })
      .catch(() => setPosts([]))
    ask<{ you: You | null }>('/community/board')
      .then((found) => setYou(found.you))
      .catch(() => undefined)
  }, [path])
  useEffect(() => {
    reload()
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
  if (posts === null) return <p className="px-1 text-sm text-muted">Loading…</p>
  if (posts.length === 0) {
    return <p className={`${card} px-5 py-8 text-center text-sm text-muted`}>Nothing in the community yet.</p>
  }
  return (
    <div className="space-y-3">
      {posts.map((post) => (
        <PostCard key={post.id} post={post} me={you} onChange={reload} />
      ))}
      {more && (
        <button type="button" onClick={() => void older()} className={`${quiet} w-full`}>
          Show more
        </button>
      )}
    </div>
  )
}
