'use client'

import { useEffect, useRef, useState } from 'react'
import { ChevronRight } from 'lucide-react'

const BUTTON =
  'inline-flex h-11 items-center gap-1 rounded-xl border border-line bg-bg px-5 text-sm font-semibold text-ink transition hover:border-pearl'

/** The network map's own page on the Explorer, and its details view. */
const LIVE = { map: 'https://explorer.orien.network/map/?view=simple', details: 'https://explorer.orien.network/map/?view=details' }

/** The home page's live picture of the network: the validators on the
 * globe, and the miners and largest wallets on rings around it. It is the
 * network map's globe, shown in a frame (/netviz/embed.html) so it starts
 * afresh each time the page shows it and stays apart from the page. */
export function NetworkGlobe() {
  const frame = useRef<HTMLIFrameElement>(null)
  const [links, setLinks] = useState(LIVE)

  useEffect(() => {
    // Anywhere but the live site, such as on this computer, the map is the site's own page.
    if (!/(^|\.)orien\.network$/.test(window.location.hostname)) {
      setLinks({ map: '/explorer/map/?view=simple', details: '/explorer/map/?view=details' })
    }
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
          loading="lazy"
          onLoad={dress}
          className="absolute inset-0 h-full w-full border-0"
        />
      </div>
      <div className="mt-5 flex flex-wrap justify-center gap-3">
        <a href={links.map} className={BUTTON}>
          Network map <ChevronRight className="h-4 w-4" aria-hidden />
        </a>
        <a href={links.details} className={BUTTON}>
          Network details <ChevronRight className="h-4 w-4" aria-hidden />
        </a>
      </div>
    </section>
  )
}
