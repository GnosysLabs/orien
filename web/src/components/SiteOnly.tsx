'use client'

import { usePathname } from 'next/navigation'
import { SIDEBAR } from '@/lib/nav'

/** On an explorer page: under /explorer/, or an explorer page served at
 * another address, such as explorer.orien.network's short ones (/address/…).
 * Those are known by the explorer's own frame already in the page. */
const onExplorer = (path: string) =>
  path.startsWith('/explorer') || (typeof document !== 'undefined' && document.querySelector('[data-explorer]') !== null)

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

/** The column beside the sidebar on computers: everything but the sidebar. */
export function SiteColumn({ children }: { children: React.ReactNode }) {
  const pathname = usePathname()
  const beside = SIDEBAR && !onExplorer(pathname) && !pathname.startsWith('/admin')
  return <div className={`flex min-h-full flex-1 flex-col ${beside ? 'lg:pl-64' : ''}`}>{children}</div>
}
