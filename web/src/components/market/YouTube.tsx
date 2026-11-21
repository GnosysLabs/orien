'use client'

import { useState } from 'react'
import { Play } from 'lucide-react'

/** The id of the first YouTube video a text links to, if any: watch, short
 * or youtu.be addresses. */
export function youtubeId(text: string): string | null {
  const match = text.match(
    /https?:\/\/(?:www\.|m\.)?(?:youtube\.com\/(?:watch\?(?:[^\s]*&)?v=|shorts\/|live\/|embed\/)|youtu\.be\/)([A-Za-z0-9_-]{11})/,
  )
  return match ? match[1] : null
}

/** A YouTube video in a post: its picture and a play button until pressed,
 * so a feed stays quick and nothing is loaded from YouTube's player until
 * someone wants it; then the player, playing, without YouTube's cookies. */
export function YouTube({ id }: { id: string }) {
  const [playing, setPlaying] = useState(false)
  return (
    <div className="relative mt-3 aspect-video w-full overflow-hidden rounded-xl border border-line bg-black">
      {playing ? (
        <iframe
          src={`https://www.youtube-nocookie.com/embed/${id}?autoplay=1&rel=0`}
          title="YouTube video"
          allow="accelerometer; autoplay; clipboard-write; encrypted-media; gyroscope; picture-in-picture; web-share"
          allowFullScreen
          referrerPolicy="strict-origin-when-cross-origin"
          className="absolute inset-0 h-full w-full"
        />
      ) : (
        <button
          type="button"
          onClick={() => setPlaying(true)}
          aria-label="Play the video"
          className="group absolute inset-0 flex h-full w-full items-center justify-center"
        >
          {/* eslint-disable-next-line @next/next/no-img-element */}
          <img
            src={`https://i.ytimg.com/vi/${id}/hqdefault.jpg`}
            alt=""
            loading="lazy"
            className="absolute inset-0 h-full w-full object-cover transition group-hover:scale-[1.02]"
          />
          <span className="relative flex h-16 w-16 items-center justify-center rounded-full bg-bg/70 text-ink shadow-2xl shadow-black/50 backdrop-blur transition group-hover:bg-bg/85">
            <Play className="ml-1 h-7 w-7 fill-current" aria-hidden />
          </span>
        </button>
      )}
    </div>
  )
}
