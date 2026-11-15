'use client'

import Link from 'next/link'
import { usePathname } from 'next/navigation'
import { useEffect, useState } from 'react'
import { Pickaxe, X } from 'lucide-react'

// Remembered per notice, so a later notice shows again.
const DISMISSED = 'orien-notice-randomx'

/** A strip above the menu telling miners to update for RandomX. */
export function MinerNotice() {
  const pathname = usePathname()
  // Shown until this browser closes it; read after loading, so the page
  // the server made and the first one drawn here agree.
  const [hidden, setHidden] = useState(true)
  useEffect(() => {
    setHidden(localStorage.getItem(DISMISSED) === '1')
  }, [])
  if (hidden || pathname.startsWith('/admin')) return null
  return (
    <div className="print:hidden border-b border-pearl/30 bg-pearl/10 text-ink">
      <div className="mx-auto flex max-w-[1200px] items-center gap-3 px-4 py-2 text-sm">
        <Pickaxe className="h-4 w-4 shrink-0 text-pearl" strokeWidth={2} aria-hidden />
        <p className="flex-1 leading-snug">
          <strong className="font-bold">Attention miners:</strong> run <code className="num font-semibold">orien update</code>{' '}
          and restart <code className="num font-semibold">orien mine</code> to keep earning ORN for your shares.{' '}
          <Link href="/news/randomx/" className="whitespace-nowrap font-semibold underline underline-offset-4 hover:text-pearl">
            Read more
          </Link>
        </p>
        <button
          type="button"
          aria-label="Close this notice"
          onClick={() => {
            localStorage.setItem(DISMISSED, '1')
            setHidden(true)
          }}
          className="flex h-7 w-7 shrink-0 items-center justify-center rounded-lg text-muted transition hover:bg-raised hover:text-ink"
        >
          <X className="h-4 w-4" strokeWidth={2} aria-hidden />
        </button>
      </div>
    </div>
  )
}
