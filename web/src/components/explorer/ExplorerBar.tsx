'use client'

import { useEffect, useState } from 'react'
import { ArrowUpRight } from 'lucide-react'
import { Wordmark } from '@/components/Wordmark'
import { LanguagePicker } from '@/components/LanguagePicker'
import { apiHref, homeHref } from '@/lib/explorer'
import { ExplorerSearch } from './Parts'

/** The explorer's top bar: its name, the search, and the way to the site. */
export function ExplorerBar() {
  const [home, setHome] = useState('/explorer/')
  const [api, setApi] = useState('/explorer/api/')
  useEffect(() => {
    setHome(homeHref())
    setApi(apiHref())
  }, [])
  return (
    <header className="sticky top-0 z-30 border-b border-line/70 bg-bg/55 backdrop-blur-md">
      <div className="mx-auto max-w-[1200px] px-4">
        {/* The same row as the website's own bar, so the wordmark sits in
            exactly the same place and size when moving between them. */}
        <div className="flex h-16 items-center gap-2 sm:gap-3">
          <a href={home} aria-label="Orien Explorer home" className="flex items-center text-xl sm:text-2xl">
            <Wordmark />
          </a>
          <a
            href={home}
            className="ml-1 rounded-md border border-line px-2 py-0.5 max-[359px]:hidden text-xs font-semibold uppercase tracking-[0.14em] text-muted transition hover:border-pearl hover:text-ink"
          >
            Explorer
          </a>
          <div className="hidden min-w-0 flex-1 px-6 md:block">
            <div className="mx-auto max-w-xl">
              <ExplorerSearch />
            </div>
          </div>
          <a
            href={api}
            className="ml-auto flex h-10 shrink-0 items-center rounded-[10px] border border-line bg-bg px-3.5 text-sm font-semibold text-ink transition hover:border-pearl md:ml-0"
          >
            API
          </a>
          <LanguagePicker />
          <a
            href="https://orien.network/"
            className="flex h-10 shrink-0 items-center gap-1.5 rounded-[10px] border border-line bg-bg px-3.5 text-sm font-semibold text-ink transition hover:border-pearl"
          >
            <span className="max-sm:hidden">orien.network</span>
            <span className="sm:hidden">Site</span>
            <ArrowUpRight className="h-4 w-4 max-sm:hidden" aria-hidden />
          </a>
        </div>
        <div className="pb-3 md:hidden">
          <ExplorerSearch />
        </div>
      </div>
    </header>
  )
}

/** The footer's link to the API page, at this host's own address for it. */
export function ExplorerApiLink() {
  const [href, setHref] = useState('/explorer/api/')
  useEffect(() => setHref(apiHref()), [])
  return (
    <a href={href} className="hover:text-ink">
      API
    </a>
  )
}
