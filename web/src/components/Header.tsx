'use client'

import Link from 'next/link'
import { usePathname } from 'next/navigation'
import { useEffect, useRef, useState } from 'react'
import { BookOpen, ChevronDown, Compass, Download, Monitor, Terminal, type LucideIcon } from 'lucide-react'
import { Wordmark } from './Wordmark'

type NavItem = {
  href: string
  label: string
  icon: LucideIcon
  match?: (pathname: string) => boolean
}

const ITEMS: NavItem[] = [
  {
    href: '/',
    label: 'Home',
    icon: Compass,
    match: (p) => p === '/',
  },
  {
    href: '/how-it-works/',
    label: 'How it works',
    icon: BookOpen,
    match: (p) => p.startsWith('/how-it-works'),
  },
  {
    href: '/try/',
    label: 'Get started',
    icon: Terminal,
    match: (p) => p.startsWith('/try'),
  },
]

/** What can be downloaded. Add each app here as it is published. */
const DOWNLOADS: { href: string; label: string; detail: string; icon: LucideIcon }[] = [
  { href: '/wallet/Orien-setup.exe', label: 'Orien app for Windows', detail: 'Desktop wallet', icon: Monitor },
  { href: '/try/', label: 'orien command line', detail: 'Mac, Linux and Windows', icon: Terminal },
]

function DownloadsMenu() {
  const [open, setOpen] = useState(false)
  const box = useRef<HTMLDivElement>(null)
  useEffect(() => {
    if (!open) return
    const close = (event: MouseEvent | KeyboardEvent) => {
      if (event instanceof KeyboardEvent ? event.key === 'Escape' : !box.current?.contains(event.target as Node)) {
        setOpen(false)
      }
    }
    document.addEventListener('mousedown', close)
    document.addEventListener('keydown', close)
    return () => {
      document.removeEventListener('mousedown', close)
      document.removeEventListener('keydown', close)
    }
  }, [open])

  return (
    <div ref={box} className="relative">
      <button
        type="button"
        aria-expanded={open}
        aria-haspopup="menu"
        onClick={() => setOpen(!open)}
        className={`flex h-9 items-center gap-2 rounded-lg px-3 text-sm font-semibold transition ${
          open ? 'bg-raised text-ink' : 'text-muted hover:bg-raised/60 hover:text-ink'
        }`}
      >
        <Download className="h-4 w-4 shrink-0" strokeWidth={2} aria-hidden />
        <span>Downloads</span>
        <ChevronDown className={`h-3.5 w-3.5 transition ${open ? 'rotate-180' : ''}`} strokeWidth={2.25} aria-hidden />
      </button>
      {open && (
        <div
          role="menu"
          className="absolute left-0 top-full mt-2 w-64 overflow-hidden rounded-xl border border-line bg-surface p-1.5 shadow-2xl shadow-black/40"
        >
          {DOWNLOADS.map((item) => {
            const Icon = item.icon
            return (
              <a
                key={item.href}
                href={item.href}
                role="menuitem"
                onClick={() => setOpen(false)}
                className="flex items-center gap-3 rounded-lg px-3 py-2.5 transition hover:bg-raised"
              >
                <Icon className="h-4 w-4 shrink-0 text-pearl" strokeWidth={2} aria-hidden />
                <span className="flex flex-col">
                  <span className="text-sm font-semibold text-ink">{item.label}</span>
                  <span className="text-xs text-muted">{item.detail}</span>
                </span>
              </a>
            )
          })}
        </div>
      )}
    </div>
  )
}

const isActive = (item: NavItem, pathname: string) =>
  item.match ? item.match(pathname) : pathname === item.href

export function Header() {
  const pathname = usePathname()
  const onTry = pathname.startsWith('/try')

  return (
    <header className="sticky top-0 z-30 print:hidden border-b border-line/70 bg-bg/55 backdrop-blur-md">
      <div className="mx-auto flex h-16 max-w-[1200px] items-center gap-3 px-4">
        <Link href="/" aria-label="Orien home" className="flex items-center text-xl sm:text-2xl">
          <Wordmark />
        </Link>

        <nav aria-label="Pages" className="ml-4 hidden items-center gap-1 md:flex">
          {ITEMS.slice(0, 2).map((item) => {
            const on = isActive(item, pathname)
            const Icon = item.icon
            return (
              <Link
                key={item.href}
                href={item.href}
                aria-current={on ? 'page' : undefined}
                className={`flex h-9 items-center gap-2 rounded-lg px-3 text-sm font-semibold transition ${
                  on ? 'bg-raised text-ink' : 'text-muted hover:bg-raised/60 hover:text-ink'
                }`}
              >
                <Icon className={`h-4 w-4 shrink-0 ${on ? 'text-pearl' : ''}`} strokeWidth={2} aria-hidden />
                <span>{item.label}</span>
              </Link>
            )
          })}
          <DownloadsMenu />
        </nav>

        <div className="ml-auto flex items-center gap-3">
          <Link
            href="/try/"
            aria-current={onTry ? 'page' : undefined}
            className="pearl-fill pearl-sheen flex h-9 items-center gap-2 whitespace-nowrap rounded-[10px] px-3.5 text-sm font-bold transition hover:brightness-105 sm:h-10"
          >
            <Terminal className="h-4 w-4" strokeWidth={2.25} aria-hidden />
            <span>Get started</span>
          </Link>
        </div>
      </div>
    </header>
  )
}

export function BottomNav() {
  const pathname = usePathname()
  const mobileItems = ITEMS

  return (
    <nav
      aria-label="Mobile navigation"
      className="fixed inset-x-0 bottom-0 z-40 grid grid-cols-3 print:hidden border-t border-line/70 bg-bg/85 px-1 pb-[env(safe-area-inset-bottom)] backdrop-blur-md md:hidden"
    >
      {mobileItems.map((item) => {
        const on = isActive(item, pathname)
        const Icon = item.icon
        const isPrimary = item.href === '/try/'
        return (
          <Link
            key={item.href}
            href={item.href}
            aria-current={on ? 'page' : undefined}
            className={`flex h-16 flex-col items-center justify-center gap-1 text-[11px] font-semibold ${
              on ? 'text-ink' : 'text-muted'
            }`}
          >
            {isPrimary ? (
              <span className="pearl-fill flex h-8 w-8 items-center justify-center rounded-xl">
                <Icon className="h-4 w-4" strokeWidth={2.5} aria-hidden />
              </span>
            ) : (
              <Icon className={`h-5 w-5 ${on ? 'text-pearl' : ''}`} strokeWidth={2} aria-hidden />
            )}
            {item.label}
          </Link>
        )
      })}
    </nav>
  )
}
