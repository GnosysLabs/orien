'use client'

import { usePathname, useRouter } from 'next/navigation'
import { createContext, Suspense, useCallback, useContext, useEffect, useMemo, useState } from 'react'
import { Messages } from './Messages'

const OPEN = 'orien-messages'

/** Opens Messages over the page, at `query` (like "?c=12" or "?to=name"). */
export function openMessages(query = '') {
  window.dispatchEvent(new CustomEvent(OPEN, { detail: query }))
}

type Where = { params: URLSearchParams; go: (query: string) => void; close: () => void }
const WhereContext = createContext<Where | null>(null)

/** Where Messages is: which conversation, and how to move. */
export function useMessagesWhere() {
  return useContext(WhereContext)
}

/** Moves Messages to `query`: inside the window, or opens it from a page. */
export function useGoMessages() {
  const where = useContext(WhereContext)
  return useCallback((query: string) => (where ? where.go(query) : openMessages(query)), [where])
}

const isMessages = (path: string) => /^\/(marketplace\/)?messages\/?$/.test(path)

/** Messages as one frosted window over whatever page the person is on, like
 * the sign-in dialog. Every link to /messages/ opens it, and links inside it
 * move between conversations without leaving the page. On /messages/ itself
 * (links in emails), it opens by itself. */
export function MessagesGate() {
  const pathname = usePathname()
  const router = useRouter()
  const [query, setQuery] = useState<string | null>(null)
  const onPage = isMessages(pathname)

  useEffect(() => {
    const show = (event: Event) => setQuery((event as CustomEvent<string>).detail ?? '')
    window.addEventListener(OPEN, show)
    return () => window.removeEventListener(OPEN, show)
  }, [])

  // Every link to Messages opens (or moves) the window instead of a page.
  useEffect(() => {
    const catchLink = (event: MouseEvent) => {
      if (event.defaultPrevented || event.button !== 0 || event.metaKey || event.ctrlKey || event.shiftKey || event.altKey) return
      const link = (event.target as Element | null)?.closest?.('a[href]')
      if (!(link instanceof HTMLAnchorElement)) return
      if (link.origin !== location.origin) return
      if (!isMessages(link.pathname)) {
        // A profile or a listing from inside the window: the page goes there
        // and the window steps aside, even when only the address's ?name= changes.
        if (link.closest('[role=dialog][aria-label=Messages]')) setQuery(null)
        return
      }
      event.preventDefault()
      setQuery(link.search)
    }
    document.addEventListener('click', catchLink, true)
    return () => document.removeEventListener('click', catchLink, true)
  }, [])

  // Landing on /messages/ opens it; going to another page (a profile, a
  // listing) closes it.
  useEffect(() => {
    setQuery(onPage ? window.location.search : null)
  }, [pathname, onPage])

  const close = useCallback(() => {
    setQuery(null)
    // On /messages/ there is nothing behind the window.
    if (onPage) router.push('/community/')
  }, [onPage, router])

  const open = query !== null
  useEffect(() => {
    if (!open) return
    const key = (event: KeyboardEvent) => {
      // Not while the sign-in dialog sits on top.
      if (event.key === 'Escape' && !document.querySelector('.z-\\[80\\]')) close()
    }
    document.addEventListener('keydown', key)
    const overflow = document.body.style.overflow
    document.body.style.overflow = 'hidden'
    return () => {
      document.removeEventListener('keydown', key)
      document.body.style.overflow = overflow
    }
  }, [open, close])

  const where = useMemo<Where>(
    () => ({ params: new URLSearchParams(query ?? ''), go: (q) => setQuery(q), close }),
    [query, close],
  )

  if (!open) return null
  return (
    <div
      className="fixed inset-0 z-[70] bg-bg/40 backdrop-blur-xl"
      onClick={(event) => event.target === event.currentTarget && close()}
    >
      <div
        className="flex h-full items-center justify-center sm:p-6 lg:p-10"
        onClick={(event) => event.target === event.currentTarget && close()}
      >
        <div
          role="dialog"
          aria-modal="true"
          aria-label="Messages"
          className="flex h-[100dvh] w-full max-w-[1100px] flex-col overflow-hidden bg-surface shadow-2xl shadow-black/60 sm:h-[min(820px,100%)] sm:rounded-2xl sm:border sm:border-line"
        >
          <WhereContext.Provider value={where}>
            <Suspense>
              <Messages />
            </Suspense>
          </WhereContext.Provider>
        </div>
      </div>
    </div>
  )
}
