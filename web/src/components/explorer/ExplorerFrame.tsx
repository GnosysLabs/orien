'use client'

import { usePathname } from 'next/navigation'
import { ExplorerApiLink, ExplorerBar } from './ExplorerBar'

/** The explorer's own bar and footer around a page. The network map is an
 * app that fills the window, so it gets the bar and all the room under it. */
export function ExplorerFrame({ children }: { children: React.ReactNode }) {
  // "/explorer/map/" here, "/map/" on explorer.orien.network.
  const fills = /(^|\/)map\/?$/.test(usePathname())
  if (fills) {
    return (
      <div data-explorer className="flex h-dvh flex-col overflow-hidden">
        <ExplorerBar />
        <div className="relative min-h-0 flex-1">{children}</div>
      </div>
    )
  }
  return (
    <div data-explorer className="flex flex-1 flex-col">
      <ExplorerBar />
      <div className="mx-auto w-full max-w-[1200px] flex-1 px-4 pb-16 pt-2">{children}</div>
      <footer className="border-t border-line px-4 py-6 text-center text-xs text-muted">
        Orien Explorer reads the live Orien network. ·{' '}
        <a href="https://orien.network/" className="hover:text-ink">
          orien.network
        </a>{' '}
        · <ExplorerApiLink />
      </footer>
    </div>
  )
}
