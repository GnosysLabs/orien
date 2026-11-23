'use client'

import { useEffect, useRef, useState } from 'react'
import { ChevronRight } from 'lucide-react'

const BUTTON =
  'inline-flex h-11 items-center gap-1 rounded-xl border border-line bg-bg px-5 text-sm font-semibold text-ink transition hover:border-pearl'

/** The network map's details view, on the Explorer. */
const LIVE = { details: 'https://explorer.orien.network/map/?view=details' }

/** The home page's live picture of the network: the validators on the
 * globe, and the miners and largest wallets on rings around it. It is the
 * network map's globe, shown in a frame (/netviz/embed.html) so it starts
 * afresh each time the page shows it and stays apart from the page. It loads
 * with the page, so it is ready before the visitor scrolls to it. */
export function NetworkGlobe() {
  const frame = useRef<HTMLIFrameElement>(null)
  const [links, setLinks] = useState(LIVE)
  // The frame stays invisible until the globe inside is drawn: before that
  // it is an empty white frame, then the map's panels before its program
  // hides them, which flash as the page scrolls past.
  const [ready, setReady] = useState(false)

  useEffect(() => {
    // Anywhere but the live site, such as on this computer, the map is the site's own page.
    if (!/(^|\.)orien\.network$/.test(window.location.hostname)) {
      setLinks({ details: '/explorer/map/?view=details' })
    }
  }, [])

  // Watches the frame rather than waiting for its load event, which may come
  // before this page's program is running: dresses its page once it has
  // loaded, and shows the frame once the globe inside has its pictures and
  // has been drawn, or a few seconds after its page arrived whatever happens.
  useEffect(() => {
    let dressed: Document | null = null
    let arrived = 0
    let timer = 0
    let watcher: IntersectionObserver | null = null
    // Safari does not draw a frame that is off the screen, so the globe is
    // drawn only once it scrolls into view: it is shown after it has drawn
    // twice there, never before.
    const showWhenDrawnInView = (inside: Window) => {
      watcher = new IntersectionObserver((seen) => {
        if (!seen.some((entry) => entry.isIntersecting)) return
        watcher?.disconnect()
        inside.requestAnimationFrame(() => inside.requestAnimationFrame(() => setReady(true)))
      })
      if (frame.current) watcher.observe(frame.current)
    }
    const check = () => {
      const doc = frame.current?.contentDocument
      const inside = frame.current?.contentWindow
      if (doc && inside && doc.location.pathname.endsWith('/embed.html')) {
        arrived ||= Date.now()
        if (doc.readyState === 'complete' && dressed !== doc) {
          dressed = doc
          dress()
        }
        const loaded = (name: string) =>
          inside.performance
            .getEntriesByType('resource')
            .some((e) => e.name.includes(name) && (e as PerformanceResourceTiming).responseEnd > 0)
        const drawn =
          doc.getElementById('app')?.classList.contains('is-embed') &&
          doc.querySelector('#globe-container canvas') &&
          loaded('earth-night') &&
          loaded('earth-topology')
        if (drawn || Date.now() - arrived > 6000) {
          showWhenDrawnInView(inside)
          return
        }
      }
      timer = window.setTimeout(check, 150)
    }
    check()
    return () => {
      clearTimeout(timer)
      watcher?.disconnect()
    }
    // dress reads only the frame and the page's own styles.
  }, [])

  /** Gives the frame the site's own type, so its labels match the page. */
  const dress = () => {
    const inside = frame.current?.contentDocument
    if (!inside) return
    const faces: string[] = []
    for (const sheet of Array.from(document.styleSheets)) {
      try {
        for (const rule of Array.from(sheet.cssRules)) {
          if (!(rule instanceof CSSFontFaceRule)) continue
          // The font files are named from the stylesheet's own place; give their full addresses.
          const from = sheet.href ?? window.location.href
          faces.push(rule.cssText.replace(/url\((["']?)([^"')]+)\1\)/g, (_, quote, address) => `url("${new URL(address, from).href}")`))
        }
      } catch {
        // A stylesheet from elsewhere cannot be read; it has none of the site's type.
      }
    }
    const page = getComputedStyle(document.documentElement)
    const style = inside.createElement('style')
    style.textContent = `${faces.join('\n')}
      :root {
        --font-geist-sans: ${page.getPropertyValue('--font-geist-sans')};
        --font-geist-mono: ${page.getPropertyValue('--font-geist-mono')};
      }`
    inside.head.appendChild(style)
  }

  return (
    <section id="network" className="scroll-mt-20 pt-20">
      <h2 className="text-center text-2xl font-bold tracking-tight text-ink sm:text-3xl">The network, live</h2>
      <p className="mx-auto mt-3 max-w-xl text-center leading-relaxed text-muted">
        Four validators keep Orien running. Around the globe, every dot is a miner or a wallet on the network right
        now.
      </p>
      <div className="relative mx-auto mt-4 aspect-square w-full max-w-[720px]">
        <iframe
          ref={frame}
          src="/netviz/embed.html"
          title="The Orien network, live: validators on the globe, miners and wallets around it"
          className={`absolute inset-0 h-full w-full border-0 transition-opacity duration-500 ${ready ? 'opacity-100' : 'opacity-0'}`}
        />
      </div>
      <div className="mt-5 flex flex-wrap justify-center gap-3">
        <a href={links.details} className={BUTTON}>
          Network details <ChevronRight className="h-4 w-4" aria-hidden />
        </a>
      </div>
    </section>
  )
}
