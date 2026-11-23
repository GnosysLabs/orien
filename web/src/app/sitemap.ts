import type { MetadataRoute } from 'next'
import { MARKETPLACE_LIVE, SITE_URL, WALLET_SIGN_IN_DOCS } from '@/lib/brand'

export const dynamic = 'force-static'

/** Every page search engines should know about, most important first. */
export default function sitemap(): MetadataRoute.Sitemap {
  const pages: (readonly [string, number, MetadataRoute.Sitemap[number]['changeFrequency']])[] = [
    ['/', 1, 'weekly'],
    ['/try/', 0.9, 'weekly'],
    ['/how-it-works/', 0.9, 'monthly'],
    ...(MARKETPLACE_LIVE
      ? ([
          ['/swap/', 0.8, 'daily'],
          ['/marketplace/', 0.8, 'daily'],
        ] as const)
      : []),
    ['/paper/', 0.7, 'monthly'],
    ['/community/', 0.8, 'hourly'],
    ['/developers/', 0.7, 'monthly'],
    ...(WALLET_SIGN_IN_DOCS ? ([['/developers/sign-in/', 0.6, 'monthly']] as const) : []),
    ['/developers/exchanges/', 0.6, 'monthly'],
    ['/news/randomx/', 0.6, 'yearly'],
    ['/changelog/', 0.5, 'weekly'],
    ['/contact/', 0.4, 'yearly'],
    ['/brand/', 0.3, 'yearly'],
    ...(MARKETPLACE_LIVE ? ([['/privacy/', 0.2, 'yearly'], ['/terms/', 0.2, 'yearly']] as const) : []),
  ]
  return pages.map(([path, priority, changeFrequency]) => ({ url: `${SITE_URL}${path}`, priority, changeFrequency }))
}
