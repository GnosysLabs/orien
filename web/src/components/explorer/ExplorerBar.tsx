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
    <header className="sticky top-0 z-30 border-b border-line/70 bg-bg/70 backdrop-blur-md">
      <div className="mx-auto flex max-w-[1200px] flex-wrap items-center gap-x-6 gap-y-3 px-4 py-3 md:h-16 md:flex-nowrap md:py-0">
        <a href={home} aria-label="Orien Explorer home" className="flex shrink-0 items-center gap-2.5 sm:gap-3">
          <Wordmark className="text-lg sm:text-xl" />
          <span className="rounded-md border border-line px-1.5 py-1 text-[10px] font-semibold uppercase tracking-[0.14em] text-muted sm:px-2 sm:text-xs">
            Explorer
          </span>
        </a>
        <a
          href="https://orien.network/"
          className="ml-auto flex h-9 shrink-0 items-center gap-1 rounded-xl border border-line bg-bg px-3 text-xs font-semibold sm:h-10 sm:gap-1.5 sm:px-4 sm:text-sm text-ink transition hover:border-pearl md:order-last md:ml-0"
        >
          orien.network <ArrowUpRight className="h-4 w-4" aria-hidden />
        </a>
        <div className="w-full md:mx-auto md:max-w-xl md:flex-1">
          <ExplorerSearch />
        </div>
      </div>
    </header>
  )
}
