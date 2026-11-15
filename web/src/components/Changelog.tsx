'use client'

import { useState } from 'react'
import { CHANGELOG, PRODUCTS, type Product } from '@/content/changelog'

const date = (text: string) =>
  new Date(`${text}T00:00:00Z`).toLocaleDateString('en-GB', { day: 'numeric', month: 'long', year: 'numeric', timeZone: 'UTC' })

export function Changelog() {
  const [only, setOnly] = useState<Product | null>(null)
  const entries = CHANGELOG.filter((entry) => !entry.unreleased && (!only || entry.products.includes(only)))
  return (
    <div className="space-y-8">
      <div className="flex flex-wrap gap-2">
        {([null, ...Object.keys(PRODUCTS)] as (Product | null)[]).map((product) => (
          <button
            key={product ?? 'all'}
            type="button"
            onClick={() => setOnly(product)}
            className={`h-9 rounded-full border px-4 text-sm font-semibold transition ${
              only === product ? 'border-pearl/60 bg-raised text-ink' : 'border-line text-muted hover:text-ink'
            }`}
          >
            {product ? PRODUCTS[product] : 'Everything'}
          </button>
        ))}
      </div>
      <ol className="relative space-y-6 border-l border-line pl-6">
        {entries.map((entry) => (
          <li key={`${entry.date}-${entry.title}`} className="relative">
            <span className="absolute -left-[29px] top-2 h-2.5 w-2.5 rounded-full bg-pearl" aria-hidden />
            <article className="rounded-2xl border border-line bg-surface p-6">
              <div className="mb-3 flex flex-wrap items-center gap-2">
                <time dateTime={entry.date} className="text-sm text-muted">
                  {date(entry.date)}
                </time>
                {entry.products.map((product) => (
                  <span key={product} className="rounded-full bg-raised px-2.5 py-0.5 text-xs font-semibold text-ink">
                    {PRODUCTS[product]}
                  </span>
                ))}
                {entry.version && <span className="num ml-auto text-sm font-semibold text-pearl">{entry.version}</span>}
              </div>
              <h2 className="mb-3 text-xl font-bold tracking-tight text-ink">{entry.title}</h2>
              <ul className="list-disc space-y-1.5 pl-5 leading-relaxed text-muted marker:text-line">
                {entry.changes.map((change) => (
                  <li key={change}>{change}</li>
                ))}
              </ul>
            </article>
          </li>
        ))}
      </ol>
    </div>
  )
}
