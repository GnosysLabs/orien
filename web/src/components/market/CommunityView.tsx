'use client'

import Link from 'next/link'
import { useCallback, useEffect, useState } from 'react'
import { Flame, MessageCircle, Plus, Sparkles, Trash2, Trophy, Users } from 'lucide-react'
import { ask, ago, profileHref } from '@/lib/market'
import { checkpointHref, orn, short } from '@/lib/explorer'
import { useMe } from '@/lib/useMe'
import { AutoTextarea } from '@/components/AutoTextarea'
import { Wordmark } from '@/components/Wordmark'
import { Avatar } from './Avatar'
import { ReportButton } from './ReportButton'
import { Notice } from './Shell'

type Person = { name: string; picture: string | null }
type Reaction = { emoji: string; count: number; me: boolean }
type Post = {
  id: number
  kind: 'post' | 'record' | 'new_miner' | 'new_miners' | 'milestone'
  body: string
  data: Record<string, unknown>
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

const REACTIONS = ['⛏️', '🔥', '❤️', '👏', '😂']
const MAX = 500

const small =
  'flex h-9 items-center justify-center gap-2 rounded-lg border border-line px-3 text-sm font-semibold text-ink transition hover:border-pearl disabled:opacity-60'
const main =
  'pearl-fill flex h-10 items-center justify-center gap-2 rounded-xl px-5 text-sm font-bold transition hover:brightness-105 disabled:opacity-60'
const card = 'rounded-2xl border border-line bg-surface'

/** ORN, rounded for a leaderboard. */
const mined = (units: string) => orn(units, 2)

function NameLink({ person }: { person: Person }) {
  return (
    <Link href={profileHref(person.name)} className="font-semibold text-ink hover:text-pearl">
      {person.name}
    </Link>
  )
}

/** What the network says, in words. */
function Happening({ post }: { post: Post }) {
  const epoch = Number(post.data.epoch)
  const at = (
    <a href={checkpointHref(epoch)} className="font-semibold text-ink hover:text-pearl">
      checkpoint {epoch}
    </a>
  )
  switch (post.kind) {
    case 'record':
      return (
        <p>
          <span className="font-semibold text-ink">A new record:</span> {String(post.data.miners)} miners were paid at {at}, beating{' '}
          {String(post.data.before)}.
        </p>
      )
    case 'new_miner':
      return post.subject ? (
        <p>
          <NameLink person={post.subject} /> was paid for mining for the first time, at {at}. Welcome!
        </p>
      ) : null
    case 'new_miners': {
      const count = Number(post.data.count)
      return (
        <p>
          {count === 1 ? 'A new miner was' : `${count} new miners were`} paid for the first time at {at}.
        </p>
      )
    }
    case 'milestone':
      return post.subject ? (
        <p>
          <NameLink person={post.subject} /> passed{' '}
          <span className="font-semibold text-ink">{Number(post.data.orn).toLocaleString('en')} ORN</span> mined.
        </p>
      ) : null
    default:
      return null
  }
}

function Composer({ parent, onPosted, placeholder }: { parent?: number; onPosted: () => void; placeholder: string }) {
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
  return (
    <form onSubmit={send} className="space-y-3">
      <AutoTextarea
        value={body}
        onChange={(e) => setBody(e.target.value.slice(0, MAX))}
        rows={parent ? 2 : 3}
        placeholder={placeholder}
        className="w-full rounded-xl border border-line bg-bg px-4 py-3 text-ink outline-none focus:border-pearl/60"
      />
      {trouble && <Notice tone="bad">{trouble.charAt(0).toUpperCase() + trouble.slice(1)}</Notice>}
      <div className="flex items-center justify-between gap-3">
        <span className={`num text-xs ${body.length > MAX - 50 ? 'text-ink' : 'text-muted'}`}>
          {body.length}/{MAX}
        </span>
        <button type="submit" disabled={busy || !body.trim()} className={parent ? small : main}>
          {busy ? 'Posting…' : parent ? 'Reply' : 'Post'}
        </button>
      </div>
    </form>
  )
}

function Reactions({ post, signedIn, onChange }: { post: Post; signedIn: boolean; onChange: () => void }) {
  const counts = new Map(post.reactions.map((r) => [r.emoji, r]))
  const toggle = async (emoji: string) => {
    if (!signedIn) {
      window.location.href = '/account/'
      return
    }
    await ask(`/community/posts/${post.id}/react`, 'POST', { emoji }).catch(() => undefined)
    onChange()
  }
  return (
    <div className="flex flex-wrap gap-1.5">
      {REACTIONS.map((emoji) => {
        const r = counts.get(emoji)
        return (
          <button
            key={emoji}
            type="button"
            onClick={() => void toggle(emoji)}
            aria-pressed={r?.me ?? false}
            className={`flex h-8 items-center gap-1.5 rounded-full border px-2.5 text-sm transition ${
              r?.me ? 'border-pearl bg-raised text-ink' : 'border-line text-muted hover:border-pearl/60'
            }`}
          >
            <span aria-hidden>{emoji}</span>
            {r && <span className="num text-xs font-semibold">{r.count}</span>}
          </button>
        )
      })}
    </div>
  )
}

function PostCard({ post, me, onChange }: { post: Post; me: You | null; onChange: () => void }) {
  const [open, setOpen] = useState(false)
  const [replies, setReplies] = useState<Post[] | null>(null)
  const network = post.kind !== 'post'
  const loadReplies = useCallback(async () => {
    const found = await ask<{ replies: Post[] }>(`/community/posts/${post.id}`).catch(() => ({ replies: [] }))
    setReplies(found.replies)
  }, [post.id])
  const remove = async () => {
    if (!confirm('Delete this post?')) return
    await ask(`/community/posts/${post.id}`, 'DELETE').catch(() => undefined)
    onChange()
  }

  return (
    <article className={`${card} space-y-4 p-5 sm:p-6`}>
      <header className="flex items-center gap-3">
        {network ? (
          <span className="flex h-9 w-9 shrink-0 items-center justify-center rounded-full border border-line bg-bg">
            <Sparkles className="h-4 w-4 text-pearl" aria-hidden />
          </span>
        ) : (
          post.author && <Avatar name={post.author.name} picture={post.author.picture} size={36} />
        )}
        <div className="min-w-0 flex-1">
          <p className="truncate text-sm">
            {network ? (
              <span className="inline-flex items-center gap-1.5 font-semibold text-ink">
                <Wordmark className="text-xs" /> network
              </span>
            ) : (
              post.author && <NameLink person={post.author} />
            )}
          </p>
          <p className="text-xs text-muted">{ago(post.created)}</p>
        </div>
        {post.mine && (
          <button type="button" onClick={() => void remove()} aria-label="Delete this post" title="Delete" className="flex h-9 w-9 items-center justify-center rounded-lg text-muted transition hover:bg-raised hover:text-ink">
            <Trash2 className="h-4 w-4" aria-hidden />
          </button>
        )}
        {!network && !post.mine && <ReportButton kind="post" id={post.id} what="this post" signedIn={!!me} icon />}
      </header>

      <div className="leading-relaxed text-muted">
        {network ? <Happening post={post} /> : <p className="whitespace-pre-line break-words text-ink">{post.body}</p>}
      </div>

      <div className="flex flex-wrap items-center justify-between gap-3">
        <Reactions post={post} signedIn={!!me} onChange={onChange} />
        <button
          type="button"
          onClick={() => {
            setOpen(!open)
            if (!open && replies === null) void loadReplies()
          }}
          className="flex h-8 items-center gap-1.5 rounded-lg px-2 text-sm font-semibold text-muted transition hover:bg-raised hover:text-ink"
        >
          <MessageCircle className="h-4 w-4" aria-hidden />
          {post.replies > 0 ? `${post.replies} ${post.replies === 1 ? 'reply' : 'replies'}` : 'Reply'}
        </button>
      </div>

      {open && (
        <div className="space-y-4 border-t border-line pt-4">
          {replies?.map((reply) => (
            <div key={reply.id} className="flex gap-3">
              {reply.author && <Avatar name={reply.author.name} picture={reply.author.picture} size={28} />}
              <div className="min-w-0 flex-1 space-y-1">
                <p className="flex flex-wrap items-center gap-x-2 text-sm">
                  {reply.author && <NameLink person={reply.author} />}
                  <span className="text-xs text-muted">{ago(reply.created)}</span>
                  {reply.mine ? (
                    <button
                      type="button"
                      onClick={async () => {
                        if (!confirm('Delete this reply?')) return
                        await ask(`/community/posts/${reply.id}`, 'DELETE').catch(() => undefined)
                        void loadReplies()
                        onChange()
                      }}
                      className="ml-auto text-xs text-muted hover:text-ink"
                    >
                      Delete
                    </button>
                  ) : (
                    <span className="ml-auto">
                      <ReportButton kind="post" id={reply.id} what="this reply" signedIn={!!me} icon />
                    </span>
                  )}
                </p>
                <p className="whitespace-pre-line break-words text-sm leading-relaxed text-ink">{reply.body}</p>
              </div>
            </div>
          ))}
          {me?.verified ? (
            <Composer
              parent={post.id}
              placeholder="Write a reply"
              onPosted={() => {
                void loadReplies()
                onChange()
              }}
            />
          ) : (
            <p className="text-sm text-muted">{me ? 'Verify a wallet to reply.' : 'Sign in to reply.'}</p>
          )}
        </div>
      )}
    </article>
  )
}

function YouCard({ you, onChange }: { you: You | null; onChange: () => void }) {
  if (!you) {
    return (
      <section className={`${card} space-y-3 p-5`}>
        <h2 className="font-bold text-ink">Join in</h2>
        <p className="text-sm leading-relaxed text-muted">Sign in to post, react and join a team.</p>
        <Link href="/account/" className={main}>
          Sign in
        </Link>
      </section>
    )
  }
  return (
    <section className={`${card} space-y-4 p-5`}>
      <div className="flex items-center gap-3">
        <Avatar name={you.name} picture={you.picture} size={40} />
        <div className="min-w-0">
          <Link href={profileHref(you.name)} className="block truncate font-bold text-ink hover:text-pearl">
            {you.name}
          </Link>
          <p className="text-xs text-muted">{you.team ? `Team ${you.team.name}` : 'No team yet'}</p>
        </div>
      </div>
      {you.stats ? (
        <dl className="grid grid-cols-3 gap-2 text-center">
          {[
            ['Rank', `#${you.stats.rank}`],
            ['ORN mined', mined(you.stats.mined)],
            ['Hours in a row', String(you.stats.streak)],
          ].map(([label, value]) => (
            <div key={label} className="rounded-xl border border-line bg-bg px-2 py-3">
              <dd className="num text-sm font-bold text-ink">{value}</dd>
              <dt className="mt-1 text-[11px] text-muted">{label}</dt>
            </div>
          ))}
        </dl>
      ) : (
        <p className="text-sm leading-relaxed text-muted">
          {you.verified
            ? 'Mine with one of your verified wallets and your figures show here after the next checkpoint.'
            : 'Verify one of your wallets to post, join a team, and see what you have mined here.'}
        </p>
      )}
      {!you.verified && (
        <Link href="/account/" className={small}>
          Verify a wallet
        </Link>
      )}
      {you.team && (
        <button
          type="button"
          className={`${small} w-full`}
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

function Miners({ board }: { board: Board }) {
  const [all, setAll] = useState(false)
  const rows = all ? board.miners : board.miners.slice(0, 10)
  return (
    <section className={`${card} overflow-hidden`}>
      <h2 className="flex items-center gap-2 border-b border-line px-5 py-4 font-bold text-ink">
        <Trophy className="h-4 w-4 text-pearl" aria-hidden /> Top miners
      </h2>
      <ol className="divide-y divide-line">
        {rows.map((row) => (
          <li key={row.rank} className="flex items-center gap-3 px-5 py-3">
            <span className="num w-6 shrink-0 text-sm text-muted">{row.rank}</span>
            {row.name ? (
              <Avatar name={row.name} picture={row.picture} size={28} />
            ) : (
              <span className="h-7 w-7 shrink-0 rounded-full border border-line bg-bg" aria-hidden />
            )}
            <span className="min-w-0 flex-1">
              {row.name ? (
                <Link href={profileHref(row.name)} className="block truncate text-sm font-semibold text-ink hover:text-pearl">
                  {row.name}
                </Link>
              ) : (
                <a href={`https://explorer.orien.network/address/${row.address}/`} className="num block truncate text-sm text-muted hover:text-ink">
                  {short(row.address ?? '')}
                </a>
              )}
              <span className="block truncate text-xs text-muted">
                {row.streak > 0 && (
                  <span className="inline-flex items-center gap-1">
                    <Flame className="h-3 w-3" aria-hidden />
                    {row.streak} h
                  </span>
                )}
                {row.streak > 0 && row.team && ' · '}
                {row.team}
              </span>
            </span>
            <span className="num shrink-0 text-sm text-ink">{mined(row.mined)}</span>
          </li>
        ))}
      </ol>
      {board.miners.length > 10 && (
        <div className="border-t border-line p-3">
          <button type="button" onClick={() => setAll(!all)} className={`${small} w-full`}>
            {all ? 'Show the top 10' : `Show the top ${board.miners.length}`}
          </button>
        </div>
      )}
      <p className="border-t border-line px-5 py-3 text-xs leading-relaxed text-muted">
        Miners with a verified wallet on their profile, by ORN mined since checkpoint {board.since}, as of checkpoint{' '}
        {board.epoch}. An account counts all its verified wallets.
      </p>
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
    <section className={`${card} overflow-hidden`}>
      <div className="flex items-center gap-2 border-b border-line px-5 py-3">
        <h2 className="flex flex-1 items-center gap-2 font-bold text-ink">
          <Users className="h-4 w-4 text-pearl" aria-hidden /> Teams
        </h2>
        {canJoin && !making && (
          <button type="button" onClick={() => setMaking(true)} className={small}>
            <Plus className="h-4 w-4" aria-hidden /> Start a team
          </button>
        )}
      </div>
      {making && (
        <form
          className="space-y-3 border-b border-line p-5"
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
            className="h-10 w-full rounded-xl border border-line bg-bg px-4 text-sm text-ink outline-none focus:border-pearl/60"
          />
          <div className="flex gap-2">
            <button type="submit" disabled={busy} className={main}>
              {busy ? 'Starting…' : 'Start it'}
            </button>
            <button type="button" onClick={() => setMaking(false)} className={small}>
              Cancel
            </button>
          </div>
        </form>
      )}
      {trouble && (
        <div className="border-b border-line p-4">
          <Notice tone="bad">{trouble.charAt(0).toUpperCase() + trouble.slice(1)}</Notice>
        </div>
      )}
      {board.teams.length === 0 ? (
        <p className="px-5 py-4 text-sm text-muted">No teams yet. Start the first one.</p>
      ) : (
        <ol className="divide-y divide-line">
          {board.teams.map((team) => (
            <li key={team.id} className="px-5 py-3">
              <div className="flex items-center gap-3">
                <span className="num w-6 shrink-0 text-sm text-muted">{team.rank}</span>
                <button
                  type="button"
                  onClick={() =>
                    void run(async () => {
                      if (members?.id === team.id) return setMembers(null)
                      const found = await ask<{ members: Person[] }>(`/community/teams/${team.id}`)
                      setMembers({ id: team.id, list: found.members })
                    })
                  }
                  className="min-w-0 flex-1 text-left"
                >
                  <span className="block truncate text-sm font-semibold text-ink hover:text-pearl">{team.name}</span>
                  <span className="block text-xs text-muted">
                    {team.members} {team.members === 1 ? 'member' : 'members'}
                  </span>
                </button>
                <span className="num shrink-0 text-sm text-ink">{mined(team.mined)}</span>
              </div>
              {members?.id === team.id && (
                <div className="mt-3 space-y-2 pl-9">
                  {members.list.map((person) => (
                    <div key={person.name} className="flex items-center gap-2">
                      <Avatar name={person.name} picture={person.picture} size={22} />
                      <NameLink person={person} />
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
                      className={`${small} mt-2`}
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

/** /community/: the feed, the leaderboards and teams. */
export function CommunityView() {
  const [posts, setPosts] = useState<Post[] | null>(null)
  const [more, setMore] = useState(false)
  const [board, setBoard] = useState<Board | null>(null)
  const [you, setYou] = useState<You | null>(null)
  const [trouble, setTrouble] = useState('')
  const live = useMe()

  const loadFeed = useCallback(async () => {
    try {
      const found = await ask<{ posts: Post[]; more: boolean }>('/community/feed')
      setPosts(found.posts)
      setMore(found.more)
      setTrouble('')
    } catch (error) {
      setTrouble((error as Error).message)
    }
  }, [])
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
    const found = await ask<{ posts: Post[]; more: boolean }>(`/community/feed?before=${last.id}`).catch(() => null)
    if (found) {
      setPosts([...(posts ?? []), ...found.posts])
      setMore(found.more)
    }
  }

  return (
    <div className="space-y-8 pb-16 pt-10 sm:pt-14">
      <header className="space-y-3">
        <h1 className="text-3xl font-bold tracking-tight text-ink sm:text-4xl">Community</h1>
        <p className="max-w-2xl leading-relaxed text-muted">
          What the network is doing, and what people are saying about it. Be kind; criticism is welcome, scams and abuse
          are not.
        </p>
      </header>

      <div className="grid items-start gap-6 lg:grid-cols-[minmax(0,1fr)_340px]">
        <div className="space-y-4">
          {you?.verified ? (
            <section className={`${card} p-5 sm:p-6`}>
              <Composer placeholder="Say something to the Orien community" onPosted={reload} />
            </section>
          ) : (
            <Notice>
              {you ? (
                <>
                  Verify one of your wallets to post.{' '}
                  <Link href="/account/" className="font-semibold text-ink underline underline-offset-4">
                    Your account
                  </Link>
                </>
              ) : (
                <>
                  <Link href="/account/" className="font-semibold text-ink underline underline-offset-4">
                    Sign in
                  </Link>{' '}
                  to post and react.
                </>
              )}
            </Notice>
          )}
          {trouble && <Notice tone="bad">{trouble}</Notice>}
          {posts === null && !trouble && <p className="text-muted">Loading…</p>}
          {posts?.length === 0 && <Notice>Nothing here yet. The network posts after each checkpoint.</Notice>}
          {posts?.map((post) => <PostCard key={post.id} post={post} me={you} onChange={reload} />)}
          {more && (
            <button type="button" onClick={() => void older()} className={`${small} w-full`}>
              Show older posts
            </button>
          )}
        </div>

        <aside className="space-y-4">
          <YouCard you={you} onChange={reload} />
          {board ? (
            <>
              <Miners board={board} />
              <Teams board={board} you={you} onChange={reload} />
            </>
          ) : (
            <p className={`${card} p-5 text-sm text-muted`}>The leaderboards appear once the network has been read.</p>
          )}
        </aside>
      </div>
    </div>
  )
}
