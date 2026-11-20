'use client'

import { useRef } from 'react'
import { send } from './Analytics'

/** A video on the site, from /media/NAME.mp4 with NAME.jpg as its picture
 * until played. Nothing loads until then; the first press of play in a visit
 * is counted (see Analytics). */
export function SiteVideo({ name, label, className = '' }: { name: string; label: string; className?: string }) {
  const counted = useRef(false)
  return (
    <video
      className={`aspect-video w-full rounded-2xl border border-line bg-surface shadow-2xl shadow-black/40 ${className}`}
      src={`/media/${name}.mp4`}
      poster={`/media/${name}.jpg`}
      controls
      playsInline
      preload="none"
      aria-label={label}
      onPlay={() => {
        if (counted.current) return
        counted.current = true
        send({ t: 'play', p: `/media/${name}.mp4` })
      }}
    />
  )
}
