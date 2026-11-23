'use client'

import { useEffect, useState } from 'react'
import { ArrowUp } from 'lucide-react'

/** A round button at the bottom centre once you've scrolled down about a
 * screen: back to the top of the page, smoothly. Above the phone's bottom
 * bar; it fades out again near the top. */
export function BackToTop() {
  const [shown, setShown] = useState(false)
  useEffect(() => {
    const check = () => setShown(window.scrollY > window.innerHeight * 0.9)
    check()
    window.addEventListener('scroll', check, { passive: true })
    window.addEventListener('resize', check)
    return () => {
      window.removeEventListener('scroll', check)
      window.removeEventListener('resize', check)
    }
  }, [])
  return (
    <button
      type="button"
      aria-label="Back to top"
      title="Back to top"
      tabIndex={shown ? 0 : -1}
      aria-hidden={!shown}
      onClick={() => {
        const reduce = window.matchMedia('(prefers-reduced-motion: reduce)').matches
        window.scrollTo({ top: 0, behavior: reduce ? 'auto' : 'smooth' })
      }}
      className={`fixed bottom-[calc(5.5rem+env(safe-area-inset-bottom))] left-1/2 z-40 flex h-11 w-11 -translate-x-1/2 items-center justify-center rounded-full border border-line bg-surface/90 text-ink shadow-lg shadow-black/40 backdrop-blur transition duration-300 hover:bg-raised print:hidden lg:bottom-6 ${
        shown ? 'opacity-100' : 'pointer-events-none translate-y-2 opacity-0'
      }`}
    >
      <ArrowUp className="h-5 w-5" strokeWidth={2.25} aria-hidden />
    </button>
  )
}
