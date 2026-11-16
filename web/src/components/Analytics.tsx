'use client'

import { useEffect, useState } from 'react'
import { usePathname } from 'next/navigation'

// The website's own traffic pings, for the admin dashboard: one per page
// viewed, and one per wallet download. No cookies; the server keeps counts
// only (see crates/market/src/traffic.rs). The referrer goes only with the
// first page of a visit. The dashboard and the tailnet are not counted, nor
// a browser opted out with ?notrack=1 (the dashboard links to it);
// ?notrack=0 counts it again.
let firstView = true
const OPT_OUT = 'orien_no_analytics'

const optedOut = () => {
  try {
    return localStorage.getItem(OPT_OUT) === '1'
  } catch {
    return false
  }
}

function send(body: object) {
  if (location.hostname.endsWith('.ts.net') || location.hostname === 'localhost' || optedOut()) return
  const data = JSON.stringify(body)
  if (!navigator.sendBeacon?.('/api/market/t', new Blob([data], { type: 'application/json' })))
    fetch('/api/market/t', { method: 'POST', body: data, keepalive: true }).catch(() => {})
}

/** Applies ?notrack=1 or ?notrack=0 and takes it out of the address bar;
 * returns what to tell the visitor, if anything. */
function applyOptOut(): string | null {
  const url = new URL(location.href)
  const flag = url.searchParams.get('notrack')
  if (flag === null) return null
  try {
    if (flag === '1') localStorage.setItem(OPT_OUT, '1')
    else localStorage.removeItem(OPT_OUT)
  } catch {
    return null
  }
  url.searchParams.delete('notrack')
  history.replaceState(null, '', url.pathname + url.search + url.hash)
  return flag === '1' ? 'Your visits in this browser are no longer counted.' : 'Your visits in this browser are counted again.'
}

/** A wallet download, from a link's address: the file's name. */
function download(href: string): string | null {
  try {
    const url = new URL(href, location.href)
    if (url.origin !== location.origin) return null
    return /^\/(wallet|downloads)\/.+\.[a-z]+$/i.test(url.pathname) ? url.pathname : null
  } catch {
    return null
  }
}

export function Analytics() {
  const pathname = usePathname()
  const [notice, setNotice] = useState<string | null>(null)

  useEffect(() => {
    const message = applyOptOut()
    if (message) {
      setTimeout(() => setNotice(message), 0)
      setTimeout(() => setNotice(null), 5000)
    }
    if (pathname.startsWith('/admin')) return
    send({ t: 'view', p: pathname, r: firstView ? document.referrer : '' })
    firstView = false
  }, [pathname])

  useEffect(() => {
    const onClick = (event: MouseEvent) => {
      const link = (event.target as Element | null)?.closest?.('a[href]') as HTMLAnchorElement | null
      const file = link && download(link.getAttribute('href') ?? '')
      if (file) send({ t: 'download', p: file })
    }
    document.addEventListener('click', onClick, true)
    return () => document.removeEventListener('click', onClick, true)
  }, [])

  return notice ? (
    <div
      role="status"
      className="fixed bottom-24 left-1/2 z-50 -translate-x-1/2 rounded-xl border border-line bg-surface px-4 py-2.5 text-sm shadow-2xl lg:bottom-4"
    >
      {notice}
    </div>
  ) : null
}
