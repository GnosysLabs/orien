'use client'

import { useEffect } from 'react'

export type LangOption = {
  code: string
  native: string
  english: string
}

export const SITE_LANGS: LangOption[] = [
  { code: 'en', native: 'English', english: 'English' },
  { code: 'es', native: 'Español', english: 'Spanish' },
  { code: 'zh', native: '简体中文', english: 'Chinese (Simplified)' },
  { code: 'zh-TW', native: '繁體中文', english: 'Chinese (Traditional)' },
  { code: 'ja', native: '日本語', english: 'Japanese' },
  { code: 'ko', native: '한국어', english: 'Korean' },
  { code: 'pt', native: 'Português', english: 'Portuguese' },
  { code: 'fr', native: 'Français', english: 'French' },
  { code: 'de', native: 'Deutsch', english: 'German' },
  { code: 'id', native: 'Bahasa Indonesia', english: 'Indonesian' },
  { code: 'tr', native: 'Türkçe', english: 'Turkish' },
  { code: 'hi', native: 'हिन्दी', english: 'Hindi' },
  { code: 'vi', native: 'Tiếng Việt', english: 'Vietnamese' },
  { code: 'ru', native: 'Русский', english: 'Russian' },
  { code: 'nl', native: 'Nederlands', english: 'Dutch' },
]

const VALID_CODES = new Set(SITE_LANGS.map((l) => l.code))

const COUNTRY_TO_LANG: Record<string, string> = {
  // Spanish
  ES: 'es', MX: 'es', AR: 'es', CO: 'es', CL: 'es', PE: 'es', VE: 'es',
  EC: 'es', GT: 'es', CU: 'es', BO: 'es', DO: 'es', HN: 'es', PY: 'es',
  SV: 'es', NI: 'es', CR: 'es', PA: 'es', UY: 'es', PR: 'es',
  // Simplified Chinese
  CN: 'zh',
  // Traditional Chinese
  TW: 'zh-TW', HK: 'zh-TW', MO: 'zh-TW',
  // Japanese
  JP: 'ja',
  // Korean
  KR: 'ko',
  // Portuguese
  BR: 'pt', PT: 'pt', AO: 'pt', MZ: 'pt', CV: 'pt',
  // French
  FR: 'fr', SN: 'fr', CI: 'fr', CM: 'fr', MG: 'fr', ML: 'fr', BF: 'fr',
  NE: 'fr', TD: 'fr', GN: 'fr', BJ: 'fr', TG: 'fr', CF: 'fr', CG: 'fr',
  GA: 'fr', DJ: 'fr', KM: 'fr', HT: 'fr', MC: 'fr',
  // German
  DE: 'de', AT: 'de', LI: 'de',
  // Indonesian
  ID: 'id',
  // Turkish
  TR: 'tr', CY: 'tr',
  // Hindi
  IN: 'hi',
  // Vietnamese
  VN: 'vi',
  // Russian
  RU: 'ru', BY: 'ru', KZ: 'ru', KG: 'ru',
  // Dutch
  NL: 'nl', SR: 'nl',
}

// Multi-language countries where browser language breaks ties.
const MULTI_LANG_DEFAULT: Record<string, { allowed: string[]; fallback: string }> = {
  CA: { allowed: ['en', 'fr'], fallback: 'en' },
  BE: { allowed: ['nl', 'fr', 'de'], fallback: 'nl' },
  CH: { allowed: ['de', 'fr'], fallback: 'de' },
  SG: { allowed: ['en', 'zh', 'zh-TW'], fallback: 'en' },
  LU: { allowed: ['fr', 'de'], fallback: 'fr' },
}

const COOKIE_KEY = 'orien_lang'
const STORAGE_KEY = 'orien_lang'
const GEO_CACHE_KEY = 'orien_geo_country'

function matchBrowserLang(): string | null {
  const langs = typeof navigator !== 'undefined'
    ? (navigator.languages && navigator.languages.length ? navigator.languages : [navigator.language])
    : []
  for (const raw of langs) {
    if (!raw) continue
    const low = raw.toLowerCase()
    if (low === 'zh-tw' || low === 'zh-hk' || low === 'zh-mo' || low.startsWith('zh-hant')) return 'zh-TW'
    if (low.startsWith('zh')) return 'zh'
    const base = low.split('-')[0]
    if (VALID_CODES.has(base)) return base
  }
  return null
}

function readCookieLang(): string | null {
  if (typeof document === 'undefined') return null
  const m = document.cookie.match(/(?:^|;\s*)orien_lang=([^;]+)/)
  if (!m) return null
  const val = decodeURIComponent(m[1])
  return VALID_CODES.has(val) ? val : null
}

function readSavedLang(): string | null {
  const fromCookie = readCookieLang()
  if (fromCookie) return fromCookie
  try {
    const val = localStorage.getItem(STORAGE_KEY)
    if (val && VALID_CODES.has(val)) return val
  } catch {}
  return null
}

function saveLang(code: string) {
  try {
    localStorage.setItem(STORAGE_KEY, code)
  } catch {}
  try {
    const host = typeof location !== 'undefined' ? location.hostname : ''
    const domain = host === 'orien.network' || host.endsWith('.orien.network') ? '; domain=.orien.network' : ''
    document.cookie = `${COOKIE_KEY}=${encodeURIComponent(code)}${domain}; path=/; max-age=31536000; SameSite=Lax`
  } catch {}
}

function langForCountry(country: string): string {
  const cc = (country || '').toUpperCase().trim()
  const multi = MULTI_LANG_DEFAULT[cc]
  if (multi) {
    const nav = matchBrowserLang()
    if (nav && multi.allowed.includes(nav)) return nav
    return multi.fallback
  }
  if (COUNTRY_TO_LANG[cc]) return COUNTRY_TO_LANG[cc]
  // English-speaking or unmapped countries: honor browser language if it's one of our 15, else English.
  return matchBrowserLang() || 'en'
}

type Pack = {
  _patterns?: [string, string][]
  [key: string]: unknown
}

declare global {
  interface Window {
    __orienLang?: string
    __orienSetLang?: (code: string, userPicked?: boolean) => Promise<void>
  }
}

const SKIP_TAGS = new Set(['SCRIPT', 'STYLE', 'CODE', 'PRE', 'TEXTAREA', 'INPUT', 'NOSCRIPT', 'SVG'])
const SKIP_CLOSEST = '[data-no-i18n], code, pre, script, style, svg, .font-mono'
const ATTRS = ['placeholder', 'title', 'aria-label'] as const

const origText = new WeakMap<Text, string>()
const lastAppliedText = new WeakMap<Text, string>()
const origAttr = new WeakMap<Element, Record<string, string>>()
const lastAppliedAttr = new WeakMap<Element, Record<string, string>>()
const packCache: Record<string, Pack> = {}

let currentLang = 'en'
let currentExact: Record<string, string> = {}
let currentPatterns: { re: RegExp; rep: string }[] = []
let origTitle: string | null = null
let lastAppliedTitle: string | null = null
let switchSeq = 0
let applying = false
let queued = false
let observer: MutationObserver | null = null

function compilePack(pack: Pack) {
  const exact: Record<string, string> = {}
  for (const [k, v] of Object.entries(pack)) {
    if (!k.startsWith('_') && typeof v === 'string') exact[k] = v
  }
  const patterns: { re: RegExp; rep: string }[] = []
  if (Array.isArray(pack._patterns)) {
    for (const [src, rep] of pack._patterns) {
      try {
        patterns.push({ re: new RegExp(src), rep })
      } catch {}
    }
  }
  return { exact, patterns }
}

function lookup(raw: string): string | null {
  if (!raw || currentLang === 'en') return null
  const lead = raw.match(/^\s*/)?.[0] || ''
  const trail = raw.match(/\s*$/)?.[0] || ''
  const trimmed = raw.slice(lead.length, raw.length - trail.length)
  if (!trimmed) return null

  // Preserve leading "· " separator if present.
  let prefix = ''
  let core = trimmed
  if (core.startsWith('· ')) {
    prefix = '· '
    core = core.slice(2)
  }

  let translated: string | null = null
  if (Object.prototype.hasOwnProperty.call(currentExact, core)) {
    translated = currentExact[core]
  } else {
    for (const p of currentPatterns) {
      if (p.re.test(core)) {
        translated = core.replace(p.re, p.rep)
        break
      }
    }
  }
  if (translated == null || translated === core) return null
  return lead + prefix + translated + trail
}

export function siteText(text: string): string {
  return lookup(text) ?? text
}

function shouldSkipElement(el: Element | null): boolean {
  if (!el) return true
  if (SKIP_TAGS.has(el.tagName)) return true
  if (el.closest && el.closest(SKIP_CLOSEST)) return true
  return false
}

function translateTextNode(node: Text) {
  const parent = node.parentElement
  if (shouldSkipElement(parent)) return
  const cur = node.nodeValue || ''
  if (!cur.trim()) return

  if (!origText.has(node)) {
    origText.set(node, cur)
  } else {
    // Only update saved original if React genuinely changed this node to a new string
    // (not if it still holds the last translation we wrote for a previous language).
    const saved = origText.get(node)!
    const applied = lastAppliedText.get(node)
    if (cur !== saved && cur !== applied) {
      origText.set(node, cur)
    }
  }

  const base = origText.get(node)!
  if (currentLang === 'en') {
    if (node.nodeValue !== base) node.nodeValue = base
    lastAppliedText.set(node, base)
    return
  }
  const next = lookup(base)
  const target = next !== null ? next : base
  if (node.nodeValue !== target) {
    node.nodeValue = target
  }
  lastAppliedText.set(node, target)
}

function translateAttrs(el: Element) {
  if (shouldSkipElement(el)) return
  for (const attr of ATTRS) {
    if (!el.hasAttribute(attr)) continue
    const val = el.getAttribute(attr) || ''
    if (!val.trim()) continue
    let map = origAttr.get(el)
    if (!map) {
      map = {}
      origAttr.set(el, map)
    }
    let appliedMap = lastAppliedAttr.get(el)
    if (!appliedMap) {
      appliedMap = {}
      lastAppliedAttr.set(el, appliedMap)
    }
    if (!(attr in map)) {
      map[attr] = val
    } else {
      const saved = map[attr]
      const applied = appliedMap[attr]
      if (val !== saved && val !== applied) {
        map[attr] = val
      }
    }
    const base = map[attr]
    if (currentLang === 'en') {
      if (val !== base) el.setAttribute(attr, base)
      appliedMap[attr] = base
    } else {
      const next = lookup(base)
      const target = next !== null ? next : base
      if (val !== target) el.setAttribute(attr, target)
      appliedMap[attr] = target
    }
  }
}

function walkTree(root: Node) {
  if (root.nodeType === Node.TEXT_NODE) {
    translateTextNode(root as Text)
    return
  }
  if (root.nodeType !== Node.ELEMENT_NODE) return
  const el = root as Element
  if (shouldSkipElement(el)) return
  translateAttrs(el)
  const walker = document.createTreeWalker(el, NodeFilter.SHOW_TEXT | NodeFilter.SHOW_ELEMENT)
  let n: Node | null = walker.nextNode()
  while (n) {
    if (n.nodeType === Node.TEXT_NODE) {
      translateTextNode(n as Text)
    } else if (n.nodeType === Node.ELEMENT_NODE) {
      translateAttrs(n as Element)
    }
    n = walker.nextNode()
  }
}

function applyAll() {
  if (typeof document === 'undefined' || !document.body) return
  applying = true
  try {
    if (origTitle === null) {
      origTitle = document.title
    } else if (document.title !== origTitle && document.title !== lastAppliedTitle) {
      origTitle = document.title
    }
    if (currentLang === 'en') {
      if (origTitle !== null) {
        if (document.title !== origTitle) document.title = origTitle
        lastAppliedTitle = origTitle
      }
    } else if (origTitle !== null) {
      const nextTitle = lookup(origTitle)
      const targetTitle = nextTitle !== null ? nextTitle : origTitle
      if (document.title !== targetTitle) document.title = targetTitle
      lastAppliedTitle = targetTitle
    }
    walkTree(document.body)
  } finally {
    applying = false
  }
}

function scheduleApply() {
  if (queued || currentLang === 'en') return
  queued = true
  queueMicrotask(() => {
    queued = false
    applyAll()
  })
}

async function loadPack(code: string): Promise<Pack | null> {
  if (packCache[code]) return packCache[code]
  try {
    const res = await fetch(`/lang/${encodeURIComponent(code)}.json`, { cache: 'default' })
    if (!res.ok) return null
    const data = (await res.json()) as Pack
    packCache[code] = data
    return data
  } catch {
    return null
  }
}

export async function setSiteLang(code: string, userPicked = true) {
  const next = VALID_CODES.has(code) ? code : 'en'
  if (userPicked) saveLang(next)
  const seq = ++switchSeq

  if (next === 'en') {
    currentLang = 'en'
    currentExact = {}
    currentPatterns = []
    document.documentElement.lang = 'en'
    window.__orienLang = 'en'
    applyAll()
    window.dispatchEvent(new CustomEvent('orien-lang-change', { detail: 'en' }))
    return
  }

  const pack = await loadPack(next)
  if (!pack || seq !== switchSeq) return
  const compiled = compilePack(pack)
  currentLang = next
  currentExact = compiled.exact
  currentPatterns = compiled.patterns
  document.documentElement.lang = next
  window.__orienLang = next
  applyAll()
  window.dispatchEvent(new CustomEvent('orien-lang-change', { detail: next }))
}

async function detectInitialLang(): Promise<string> {
  const saved = readSavedLang()
  if (saved) return saved

  try {
    const cachedCountry = sessionStorage.getItem(GEO_CACHE_KEY)
    if (cachedCountry !== null) {
      return langForCountry(cachedCountry)
    }
    const res = await fetch('/geo', { cache: 'no-store' })
    if (res.ok) {
      const data = (await res.json()) as { country?: string }
      const country = (data.country || '').toUpperCase()
      try {
        sessionStorage.setItem(GEO_CACHE_KEY, country)
      } catch {}
      if (country && country !== 'XX' && country !== 'T1') {
        return langForCountry(country)
      }
    }
  } catch {}

  return matchBrowserLang() || 'en'
}

export function SiteI18n() {
  useEffect(() => {
    window.__orienLang = currentLang
    window.__orienSetLang = setSiteLang

    if (!observer && typeof MutationObserver !== 'undefined') {
      observer = new MutationObserver(() => {
        if (!applying) scheduleApply()
      })
      observer.observe(document.body, {
        childList: true,
        subtree: true,
        characterData: true,
      })
    }

    let cancelled = false
    detectInitialLang().then((lang) => {
      if (!cancelled && lang !== 'en') {
        void setSiteLang(lang, false)
      }
    })

    return () => {
      cancelled = true
    }
  }, [])

  return null
}
