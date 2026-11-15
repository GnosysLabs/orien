'use client'

import { usePathname } from 'next/navigation'
import { useEffect } from 'react'

/** Opens every new page at its top. Next.js is meant to, but here it kept
 * the scroll of the page before. A link to a part of a page (#buy) still
 * goes to that part. */
export function ScrollToTop() {
  const pathname = usePathname()
  useEffect(() => {
    if (!window.location.hash) window.scrollTo({ top: 0, left: 0, behavior: 'instant' })
  }, [pathname])
  return null
}
