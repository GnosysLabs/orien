import type { Metadata } from 'next'
import { SITE_NAME, SITE_URL } from './brand'

/** The picture shown when a page is shared. */
export const SHARE_IMAGE = { url: '/og.png?v=2', width: 1200, height: 630, alt: 'Orien: the quantum-proof cryptocurrency' }

/** A page's metadata: its title and description for search results, and
 * the same for links shared on X, Discord, Telegram and the like, with the
 * page's own address. */
export function pageMeta({
  title,
  description,
  path,
  type = 'website',
  published,
}: {
  title: string
  description: string
  path: string
  type?: 'website' | 'article'
  /** For articles: when it was published, as 2026-11-15. */
  published?: string
}): Metadata {
  const shared = `${title} · ${SITE_NAME}`
  return {
    title,
    description,
    alternates: { canonical: path },
    openGraph: {
      type,
      siteName: SITE_NAME,
      title: shared,
      description,
      url: `${SITE_URL}${path}`,
      images: [SHARE_IMAGE],
      ...(published ? { publishedTime: published } : {}),
    },
    twitter: { card: 'summary_large_image', site: '@OrienNetwork', title: shared, description, images: [SHARE_IMAGE.url] },
  }
}

/** Structured data for search engines, as a script tag's contents. */
export function jsonLd(data: object): { __html: string } {
  return { __html: JSON.stringify({ '@context': 'https://schema.org', ...data }).replace(/</g, '\\u003c') }
}
