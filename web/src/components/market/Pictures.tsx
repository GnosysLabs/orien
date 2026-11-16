'use client'

import { useState } from 'react'
import { Brush, Code2, GraduationCap, Package, Sparkles } from 'lucide-react'
import { pictureUrl } from '@/lib/market'

/** A listing's pictures: a large one, and the others to switch to. */
export function Pictures({ pictures, title }: { pictures: string[]; title: string }) {
  const [shown, setShown] = useState(0)
  if (pictures.length === 0) return null
  const current = pictures[Math.min(shown, pictures.length - 1)]
  return (
    <div className="space-y-3">
      {/* eslint-disable-next-line @next/next/no-img-element */}
      <img src={pictureUrl(current)} alt={title} className="aspect-video w-full rounded-2xl border border-line object-cover" />
      {pictures.length > 1 && (
        <div className="flex gap-3">
          {pictures.map((picture, n) => (
            <button
              key={picture}
              type="button"
              onClick={() => setShown(n)}
              aria-label={`Picture ${n + 1}`}
              className={`overflow-hidden rounded-xl border-2 transition ${picture === current ? 'border-pearl' : 'border-transparent opacity-70 hover:opacity-100'}`}
            >
              {/* eslint-disable-next-line @next/next/no-img-element */}
              <img src={pictureUrl(picture)} alt="" className="aspect-video w-24 object-cover" />
            </button>
          ))}
        </div>
      )}
    </div>
  )
}

const ICONS: Record<string, typeof Brush> = { design: Brush, software: Code2, teaching: GraduationCap, goods: Package }

/** The top of a listing's card: its cover, or a quiet stand-in. */
export function Cover({ pictures, category }: { pictures: string[]; category: string }) {
  if (pictures[0]) {
    // eslint-disable-next-line @next/next/no-img-element
    return <img src={pictureUrl(pictures[0])} alt="" className="aspect-video w-full object-cover" />
  }
  const Icon = ICONS[category] ?? Sparkles
  return (
    <div className="flex aspect-video w-full items-center justify-center bg-gradient-to-br from-raised to-bg">
      <Icon className="h-10 w-10 text-pearl/40" strokeWidth={1.5} aria-hidden />
    </div>
  )
}
