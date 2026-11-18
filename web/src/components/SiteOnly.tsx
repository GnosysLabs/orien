'use client'

import { usePathname } from 'next/navigation'

/** On an explorer page: under /explorer/, or anywhere on
 * explorer.orien.network, whose short addresses (/address/…) are explorer
 * pages too. */
const onExplorer = (path: string) =>
  path.startsWith('/explorer') || (typeof window !== 'undefined' && window.location.hostname.startsWith('explorer.'))

/** The site's own menus and footer: left off the explorer, which has its own. */
export function SiteOnly({ children }: { children: React.ReactNode }) {
  return onExplorer(usePathname()) ? null : children
}

/** The page's main area: the site's centred column, or the explorer's own
 * full-width frame. */
export function Main({ children }: { children: React.ReactNode }) {
  return onExplorer(usePathname()) ? (
    <main className="flex flex-1 flex-col">{children}</main>
  ) : (
    <main className="mx-auto w-full max-w-[1200px] flex-1 px-4 py-6 sm:py-8">{children}</main>
  )
}
