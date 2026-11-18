'use client'

import { Fragment, useEffect, useRef, useState } from 'react'
import { ArrowUp, MessageCircleQuestion, RotateCcw, X } from 'lucide-react'
import { DISCORD } from '@/lib/brand'

// The site-wide chat bubble: a small chat with the AI assistant (/api/ask),
// the same Orien expert as the Discord bot, which answers from the site's own
// pages, the paper and the changelog. The chat lives only in this tab.

type Msg = { role: 'user' | 'assistant'; content: string }

// Give up if the reply stalls this long, so the chat never hangs silently.
const STALL_MS = 45_000

const SUGGESTIONS = ['How do I start mining?', 'What makes Orien quantum-proof?', 'How do I get ORN?', 'How do I keep my wallet safe?']

/** **bold**, `code` and links, within one line. */
function inline(text: string) {
  const parts: React.ReactNode[] = []
  const pattern = /\*\*([^*]+)\*\*|`([^`]+)`|\[([^\]]+)\]\((https?:\/\/[^)\s]+)\)|(https?:\/\/[^\s)]+)/g
  let last = 0
  let match: RegExpExecArray | null
  while ((match = pattern.exec(text))) {
    if (match.index > last) parts.push(text.slice(last, match.index))
    const [, bold, code, label, href, bare] = match
    if (bold) parts.push(<strong key={match.index} className="font-semibold text-ink">{bold}</strong>)
    else if (code) parts.push(<code key={match.index} className="rounded bg-bg px-1 py-0.5 text-[0.85em]">{code}</code>)
    else {
      const url = href ?? bare
      const own = /^https:\/\/(explorer\.)?orien\.network/.test(url)
      parts.push(
        <a
          key={match.index}
          href={url}
          target={own ? undefined : '_blank'}
          rel={own ? undefined : 'noopener noreferrer'}
          className="text-pearl underline underline-offset-2 hover:text-ink"
        >
          {label ?? url}
        </a>,
      )
    }
    last = match.index + match[0].length
  }
  if (last < text.length) parts.push(text.slice(last))
  return parts
}

/** The little Markdown the assistant writes: paragraphs, lists, bold, links. */
function Answer({ text }: { text: string }) {
  const blocks: React.ReactNode[] = []
  let list: { ordered: boolean; items: string[] } | null = null
  const flush = () => {
    if (!list) return
    const Tag = list.ordered ? 'ol' : 'ul'
    blocks.push(
      <Tag key={blocks.length} className={`space-y-1 pl-5 ${list.ordered ? 'list-decimal' : 'list-disc'}`}>
        {list.items.map((item, i) => (
          <li key={i}>{inline(item)}</li>
        ))}
      </Tag>,
    )
    list = null
  }
  for (const raw of text.split('\n')) {
    const line = raw.trim()
    const numbered = line.match(/^\d+[.)]\s+(.*)$/)
    const bullet = line.match(/^[-*•]\s+(.*)$/)
    if (numbered || bullet) {
      const ordered = !!numbered
      if (list && list.ordered !== ordered) flush()
      list ??= { ordered, items: [] }
      list.items.push((numbered ?? bullet)![1])
    } else if (line) {
      flush()
      blocks.push(<p key={blocks.length}>{inline(line.replace(/^#+\s*/, ''))}</p>)
    } else {
      flush()
    }
  }
  flush()
  return <div className="space-y-2">{blocks.map((block, i) => <Fragment key={i}>{block}</Fragment>)}</div>
}

export function AskOrien() {
  const [open, setOpen] = useState(false)
  const [messages, setMessages] = useState<Msg[]>([])
  const [input, setInput] = useState('')
  const [busy, setBusy] = useState(false)
  const scroller = useRef<HTMLDivElement>(null)
  const field = useRef<HTMLTextAreaElement>(null)

  useEffect(() => {
    scroller.current?.scrollTo({ top: scroller.current.scrollHeight })
  }, [messages, open])

  // Phones: the chat covers the screen, sized to the part above the
  // keyboard (the visual viewport), and the page behind it doesn't scroll.
  // The keyboard isn't opened by itself there, so the suggestions show first.
  const [vv, setVv] = useState<{ h: number; top: number } | null>(null)
  useEffect(() => {
    if (!open) return
    const phone = window.matchMedia('(max-width: 639px)').matches
    if (!phone) {
      field.current?.focus()
      return
    }
    const v = window.visualViewport
    const fit = () => setVv(v ? { h: v.height, top: v.offsetTop } : null)
    fit()
    v?.addEventListener('resize', fit)
    v?.addEventListener('scroll', fit)
    const before = document.body.style.overflow
    document.body.style.overflow = 'hidden'
    return () => {
      v?.removeEventListener('resize', fit)
      v?.removeEventListener('scroll', fit)
      document.body.style.overflow = before
      setVv(null)
    }
  }, [open])

  async function ask(text: string) {
    const question = text.trim()
    if (!question || busy) return
    const history: Msg[] = [...messages, { role: 'user', content: question }]
    setMessages([...history, { role: 'assistant', content: '' }])
    setInput('')
    setBusy(true)
    const write = (content: string) => setMessages((m) => [...m.slice(0, -1), { role: 'assistant', content }])
    const abort = new AbortController()
    let stall = setTimeout(() => abort.abort(), STALL_MS)
    try {
      const res = await fetch('/api/ask', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ messages: history.slice(-12) }),
        signal: abort.signal,
      })
      if (!res.ok || !res.body) {
        write((await res.json().catch(() => null))?.error ?? 'Something went wrong. Please try again.')
        return
      }
      const reader = res.body.getReader()
      const decoder = new TextDecoder()
      let reply = ''
      for (;;) {
        const { done, value } = await reader.read()
        if (done) break
        clearTimeout(stall)
        stall = setTimeout(() => abort.abort(), STALL_MS)
        reply += decoder.decode(value, { stream: true })
        write(reply)
      }
      if (!reply.trim()) write('Sorry, I couldn’t answer that. Please try again.')
    } catch {
      write(
        abort.signal.aborted
          ? 'The assistant is taking too long to answer. Please try again.'
          : 'I couldn’t reach the assistant. Check your connection and try again.',
      )
    } finally {
      clearTimeout(stall)
      setBusy(false)
    }
  }

  function clear() {
    setMessages([])
    field.current?.focus()
  }

  return (
    <>
      {!open && (
        // Above the phone's bottom bar; in the corner on larger screens.
        <div className="fixed bottom-[calc(5.5rem+env(safe-area-inset-bottom))] right-4 z-40 print:hidden sm:right-6 lg:bottom-6">
          <button
            type="button"
            onClick={() => setOpen(true)}
            aria-label="Ask about Orien"
            className="pearl-fill flex h-12 items-center gap-2 rounded-full px-4 text-sm font-bold shadow-xl shadow-black/40 transition hover:brightness-105"
          >
            <MessageCircleQuestion className="h-5 w-5" aria-hidden />
            <span className="max-sm:hidden">Ask</span>
          </button>
        </div>
      )}

      {open && (
        <section
          aria-label="Ask about Orien"
          style={vv ? { height: vv.h, transform: `translateY(${vv.top}px)` } : undefined}
          className="fixed z-[60] flex flex-col overflow-hidden bg-surface print:hidden max-sm:inset-x-0 max-sm:top-0 max-sm:h-[100dvh] sm:bottom-6 sm:right-6 sm:max-h-[min(640px,85dvh)] sm:w-[380px] sm:rounded-2xl sm:border sm:border-line sm:shadow-2xl sm:shadow-black/60"
        >
          <header className="flex items-center gap-3 border-b border-line px-4 py-3 max-sm:pt-[max(12px,env(safe-area-inset-top))]">
            <span className="pearl-fill grid h-8 w-8 shrink-0 place-items-center rounded-lg">
              <MessageCircleQuestion className="h-4 w-4" aria-hidden />
            </span>
            <div className="min-w-0 flex-1">
              <p className="font-bold leading-tight text-ink">Ask about Orien</p>
              <p className="text-xs text-muted">AI assistant · answers from Orien&apos;s own pages</p>
            </div>
            {messages.length > 0 && !busy && (
              <button
                type="button"
                onClick={clear}
                aria-label="Start over"
                title="Start over"
                className="grid h-8 w-8 place-items-center rounded-lg text-muted transition hover:bg-raised hover:text-ink"
              >
                <RotateCcw className="h-4 w-4" aria-hidden />
              </button>
            )}
            <button
              type="button"
              onClick={() => setOpen(false)}
              aria-label="Close"
              className="grid h-8 w-8 place-items-center rounded-lg text-muted transition hover:bg-raised hover:text-ink"
            >
              <X className="h-4 w-4" aria-hidden />
            </button>
          </header>

          <div ref={scroller} className="flex min-h-[220px] flex-1 flex-col gap-3 overflow-y-auto px-4 py-4 text-sm">
            {messages.length === 0 ? (
              <>
                <p className="leading-relaxed text-ink/85">
                  Hi! Ask me anything about Orien: mining, the wallet, swaps, how it works. I answer from Orien&apos;s own
                  pages and can&apos;t see your wallet.
                </p>
                <div className="flex flex-wrap gap-2">
                  {SUGGESTIONS.map((s) => (
                    <button
                      key={s}
                      type="button"
                      onClick={() => ask(s)}
                      className="rounded-full border border-line bg-raised px-3 py-1.5 text-xs font-semibold text-ink/85 transition hover:border-pearl"
                    >
                      {s}
                    </button>
                  ))}
                </div>
              </>
            ) : (
              messages.map((m, i) =>
                m.role === 'user' ? (
                  <p key={i} className="max-w-[85%] self-end whitespace-pre-wrap rounded-2xl rounded-br-md bg-pearl/15 px-3 py-2 text-ink">
                    {m.content}
                  </p>
                ) : (
                  <div key={i} className="max-w-[92%] rounded-2xl rounded-bl-md bg-raised px-3 py-2 leading-relaxed text-ink/90">
                    {m.content ? (
                      <Answer text={m.content} />
                    ) : (
                      <span className="inline-flex gap-1 py-1" aria-label="Thinking">
                        {[0, 1, 2].map((d) => (
                          <span
                            key={d}
                            className="h-1.5 w-1.5 animate-pulse rounded-full bg-muted"
                            style={{ animationDelay: `${d * 150}ms` }}
                          />
                        ))}
                      </span>
                    )}
                  </div>
                ),
              )
            )}
          </div>

          <form
            onSubmit={(e) => {
              e.preventDefault()
              void ask(input)
            }}
            className="border-t border-line p-3 max-sm:pb-[max(12px,env(safe-area-inset-bottom))]"
          >
            <div className="flex items-center gap-2 rounded-xl border border-line bg-raised py-1.5 pl-3.5 pr-1.5 focus-within:border-pearl">
              <textarea
                ref={field}
                value={input}
                onChange={(e) => setInput(e.target.value)}
                onKeyDown={(e) => {
                  if (e.key === 'Enter' && !e.shiftKey) {
                    e.preventDefault()
                    void ask(input)
                  }
                }}
                rows={1}
                maxLength={1500}
                placeholder="Ask a question…"
                className="block max-h-28 flex-1 resize-none bg-transparent py-1 text-base leading-6 text-ink outline-none placeholder:text-muted sm:text-sm"
              />
              <button
                type="submit"
                disabled={busy || !input.trim()}
                aria-label="Send"
                className="pearl-fill grid h-8 w-8 shrink-0 place-items-center rounded-lg transition disabled:opacity-40"
              >
                <ArrowUp className="h-4 w-4" aria-hidden />
              </button>
            </div>
            <p className="mt-2 text-center text-[11px] leading-snug text-muted">
              AI answers can be wrong. Never share your recovery phrase.{' '}
              <a href={DISCORD} target="_blank" rel="noopener noreferrer" className="underline hover:text-ink">
                Ask people on Discord
              </a>
            </p>
          </form>
        </section>
      )}
    </>
  )
}
