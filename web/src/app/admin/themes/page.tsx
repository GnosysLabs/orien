'use client'

import Link from 'next/link'
import { ago, pictureUrl } from '@/lib/market'
import { Action, count, usePoll } from '../admin-client'

type StoreTheme = {
  id: number
  name: string
  base: string
  colors: string[]
  picture: string | null
  thumb: string | null
  created: number
  author: { id: number | null; name: string; official?: boolean }
  builtin?: string
  downloads: number
  tips: { count: number; orn: number }
}

export default function AdminThemesPage() {
  const found = usePoll<{ themes: StoreTheme[] }>('/themes', 30_000)
  const reload = () => void found.reload()
  const themes = found.data?.themes ?? []
  return (
    <div className="mx-auto flex max-w-[1200px] flex-col gap-5 pb-16 pt-10">
      <header className="flex flex-wrap items-center justify-between gap-3">
        <div>
          <h1 className="text-2xl font-bold tracking-tight text-ink">Theme Store</h1>
          {found.data && <p className="mt-1 text-xs text-muted">{count(themes.length)} {themes.length === 1 ? 'theme' : 'themes'}</p>}
        </div>
        <Link
          href="/admin/"
          className="flex h-10 items-center rounded-xl border border-line bg-surface px-4 text-sm font-semibold text-ink transition hover:border-pearl"
        >
          Back to dashboard
        </Link>
      </header>
      {found.error && <p role="alert" className="rounded-xl border border-stop/40 bg-stop/10 px-4 py-2 text-sm text-ink">{found.error}</p>}
      {!found.data && !found.error && <p role="status" className="text-sm text-muted">Loading themes…</p>}
      {found.data && themes.length === 0 && (
        <p className="text-sm text-muted">No themes shared yet. Each new one also shows in #new-themes on Discord.</p>
      )}
      {themes.length > 0 && (
        <ul className="grid gap-4 sm:grid-cols-2 lg:grid-cols-3">
          {themes.map((t) => (
            <li key={t.id} className="overflow-hidden rounded-xl border border-line bg-bg">
              {t.picture && t.thumb ? (
                <a href={pictureUrl(t.picture)} target="_blank" rel="noopener noreferrer" className="block">
                  {/* eslint-disable-next-line @next/next/no-img-element */}
                  <img src={pictureUrl(t.thumb)} alt={t.name} className="aspect-video w-full object-cover" />
                </a>
              ) : (
                <div className="flex aspect-video w-full items-center justify-center bg-raised text-xs text-muted">
                  Orien’s own theme: its picture is in the app
                </div>
              )}
              <div className="space-y-3 p-3">
                <div className="flex items-center justify-between gap-2">
                  <p className="min-w-0 truncate font-semibold text-ink">{t.name}</p>
                  <span className="flex shrink-0 gap-1">
                    {t.colors.map((c) => (
                      <svg key={c} viewBox="0 0 16 16" className="h-4 w-4" aria-label={c}>
                        <circle cx="8" cy="8" r="7" fill={c} stroke="rgba(255,255,255,0.25)" />
                      </svg>
                    ))}
                  </span>
                </div>
                <p className="text-xs text-muted">
                  {t.base} · by{' '}
                  {t.author.id === null ? (
                    <span className="font-semibold text-ink">{t.author.name}</span>
                  ) : (
                    <Link href={`/marketplace/profile/?id=${t.author.id}`} className="font-semibold text-ink hover:text-pearl">
                      {t.author.name}
                    </Link>
                  )}{' '}
                  · {ago(t.created)} · {count(t.downloads)} downloads{t.builtin ? '' : ` · ${count(t.tips.count)} tips`}
                </p>
                {!t.builtin && (
                  <Action
                    label="Remove"
                    sure={`Take down ${t.name} by ${t.author.name}? It leaves the theme store for everyone.`}
                    path={`/themes/${t.id}/remove`}
                    onDone={reload}
                    danger
                  />
                )}
              </div>
            </li>
          ))}
        </ul>
      )}
    </div>
  )
}
