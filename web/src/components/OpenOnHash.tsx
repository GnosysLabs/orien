'use client'

import { useEffect } from 'react'

/** Opens the <details> with this id when the address points at it, as a
 * menu link to /try/#cli does, and brings it into view. */
export function OpenOnHash({ id }: { id: string }) {
  useEffect(() => {
    const open = () => {
      if (window.location.hash !== `#${id}`) return
      const details = document.getElementById(id)
      if (!(details instanceof HTMLDetailsElement)) return
      details.open = true
      requestAnimationFrame(() => details.scrollIntoView({ behavior: 'smooth', block: 'start' }))
    }
    open()
    window.addEventListener('hashchange', open)
    // A link to the same page with the same #… fires no hashchange.
    const clicked = (event: MouseEvent) => {
      const link = (event.target as Element | null)?.closest?.('a[href]')
      if (link instanceof HTMLAnchorElement && link.pathname === window.location.pathname && link.hash === `#${id}`) {
        setTimeout(open, 0)
      }
    }
    document.addEventListener('click', clicked)
    return () => {
      window.removeEventListener('hashchange', open)
      document.removeEventListener('click', clicked)
    }
  }, [id])
  return null
}
