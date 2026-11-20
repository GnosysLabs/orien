'use client'

import { useRef } from 'react'
import { send } from './Analytics'

/** The introduction video on the home page. Nothing loads until it is
 * played; the first press of play in a visit is counted (see Analytics). */
export function IntroductionVideo() {
  const counted = useRef(false)
  return (
    <video
      className="aspect-video w-full rounded-2xl border border-line bg-surface shadow-2xl shadow-black/40"
      src="/media/orien-introduction.mp4"
      poster="/media/orien-introduction.jpg"
      controls
      playsInline
      preload="none"
      aria-label="Introduction to Orien, two minutes"
      onPlay={() => {
        if (counted.current) return
        counted.current = true
        send({ t: 'play', p: '/media/orien-introduction.mp4' })
      }}
    />
  )
}
