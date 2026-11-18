'use client'

import { useEffect, useState } from 'react'
import { ArrowRight } from 'lucide-react'
import { IPHONE_BETA } from '@/lib/brand'

/** On an iPhone only: a card offering Orien Wallet for iPhone. Hidden on every
 * other device, and until the page knows which device it is on. */
export function IPhoneApp() {
  const [iphone, setIphone] = useState(false)
  useEffect(() => {
    setIphone(/iPhone|iPod/.test(navigator.userAgent))
  }, [])
  if (!iphone) return null
  return (
    <a
      href={IPHONE_BETA}
      target="_blank"
      rel="noopener noreferrer"
      className="mx-auto mt-8 flex max-w-md items-center gap-3 rounded-2xl border border-pearl/40 bg-surface p-4 text-left transition hover:border-pearl"
    >
      {/* eslint-disable-next-line @next/next/no-img-element */}
      <img src="/iphone-app-icon.png" alt="" className="h-12 w-12 shrink-0 rounded-[11px] ring-1 ring-white/15" />
      <span className="min-w-0 flex-1">
        <span className="block font-bold text-ink">Orien Wallet for iPhone</span>
        <span className="block text-sm text-muted">Free public beta on TestFlight</span>
      </span>
      <span className="pearl-fill flex h-10 shrink-0 items-center gap-1.5 rounded-xl px-4 text-sm font-bold">
        Get it <ArrowRight className="h-4 w-4" aria-hidden />
      </span>
    </a>
  )
}
