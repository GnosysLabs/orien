'use client'

import { useEffect, useRef, useState } from 'react'
import { Check, Globe } from 'lucide-react'
import { SITE_LANGS, setSiteLang } from './SiteI18n'

export function LanguagePicker({ align = 'right', compact = false }: { align?: 'left' | 'right'; compact?: boolean }) {
  const [open, setOpen] = useState(false)
  const [lang, setLang] = useState('en')
  const box = useRef<HTMLDivElement>(null)

  useEffect(() => {
    if (typeof window !== 'undefined' && window.__orienLang) {
      setLang(window.__orienLang)
    }
    const onChange = (e: Event) => {
      const detail = (e as CustomEvent<string>).detail
      if (detail) setLang(detail)
    }
    window.addEventListener('orien-lang-change', onChange)
    return () => window.removeEventListener('orien-lang-change', onChange)
  }, [])

  useEffect(() => {
    if (!open) return
    const close = (event: MouseEvent | KeyboardEvent) => {
      if (
        event instanceof KeyboardEvent
          ? event.key === 'Escape'
          : !box.current?.contains(event.target as Node)
      ) {
        setOpen(false)
      }
    }
    document.addEventListener('mousedown', close)
    document.addEventListener('keydown', close)
    return () => {
      document.removeEventListener('mousedown', close)
      document.removeEventListener('keydown', close)
    }
  }, [open])

  const active = SITE_LANGS.find((l) => l.code === lang) || SITE_LANGS[0]

  return (
    <div ref={box} data-no-i18n className="relative">
      <button
        type="button"
        aria-label="Language"
        title={`Language: ${active.native}`}
        aria-expanded={open}
        aria-haspopup="menu"
        onClick={() => setOpen((o) => !o)}
        className={`flex h-9 items-center justify-center gap-1.5 rounded-lg text-muted transition hover:bg-raised/60 hover:text-ink ${
          compact ? 'w-9' : 'px-2.5 text-xs font-semibold'
        } ${open ? 'bg-raised text-ink' : ''}`}
      >
        <Globe className="h-[18px] w-[18px] shrink-0" strokeWidth={2} aria-hidden />
        {!compact && <span className="hidden uppercase sm:inline">{active.code}</span>}
      </button>

      {open && (
        <div
          role="menu"
          className={`absolute top-full z-50 mt-2 max-h-[70vh] w-52 overflow-y-auto rounded-xl border border-line bg-surface p-1.5 shadow-2xl shadow-black/40 ${
            align === 'left' ? 'left-0' : 'right-0'
          }`}
        >
          {SITE_LANGS.map((item) => {
            const isSelected = item.code === lang
            return (
              <button
                key={item.code}
                type="button"
                role="menuitem"
                onClick={() => {
                  setOpen(false)
                  setLang(item.code)
                  void setSiteLang(item.code, true)
                }}
                className={`flex w-full items-center justify-between gap-2 rounded-lg px-3 py-2 text-left text-sm transition ${
                  isSelected
                    ? 'bg-raised font-semibold text-ink'
                    : 'text-ink/90 hover:bg-raised/70'
                }`}
              >
                <span className="truncate">{item.native}</span>
                {isSelected && <Check className="h-4 w-4 shrink-0 text-pearl" strokeWidth={2.25} aria-hidden />}
              </button>
            )
          })}
        </div>
      )}
    </div>
  )
}
