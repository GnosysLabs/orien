'use client'

import { useEffect, useState } from 'react'
import { ArrowUpRight } from 'lucide-react'
import { Wordmark } from '@/components/Wordmark'
import { homeHref } from '@/lib/explorer'
import { ExplorerSearch } from './Parts'

/** The explorer's top bar: its name, the search, and the way to the site. */
export function ExplorerBar() {
  const [home, setHome] = useState('/explorer/')
  useEffect(() => setHome(homeHref()), [])
  return (
    <header className="sticky top-0 z-30 border-b border-line/70 bg-bg/55 backdrop-blur-md">
      <div className="mx-auto max-w-[1200px] px-4">
        {/* The same row as the website's own bar, so the wordmark sits in
            exactly the same place and size when moving between them. */}
        <div className="flex h-16 items-center gap-3">
          <a href={home} aria-label="Orien Explorer home" className="flex items-center text-xl sm:text-2xl">
            <Wordmark />
          </a>
          <span className="ml-1 rounded-md border border-line px-2 py-0.5 text-xs font-semibold uppercase tracking-[0.14em] text-muted">
            Explorer
          </span>
          <div className="hidden min-w-0 flex-1 px-6 md:block">
            <div className="mx-auto max-w-xl">
              <ExplorerSearch />
            </div>
          </div>
          <a
            href="https://orien.network/"
            className="ml-auto flex h-10 shrink-0 items-center gap-1.5 rounded-[10px] border border-line bg-bg px-3.5 text-sm font-semibold text-ink transition hover:border-pearl md:ml-0"
          >
            <span className="max-sm:hidden">orien.network</span>
            <span className="sm:hidden">Site</span>
            <ArrowUpRight className="h-4 w-4" aria-hidden />
          </a>
        </div>
        <div className="pb-3 md:hidden">
          <ExplorerSearch />
        </div>
      </div>
    </header>
  )
}
