import type { MetadataRoute } from 'next'
import { SITE_URL } from '@/lib/brand'

export const dynamic = 'force-static'

export default function robots(): MetadataRoute.Robots {
  return {
    rules: {
      userAgent: '*',
      allow: '/',
      // Private pages: your account, your messages, the admin dashboard.
      disallow: ['/admin/', '/api/', '/account/', '/marketplace/account/', '/marketplace/messages/', '/messages/', '/marketplace/sell/'],
    },
    sitemap: `${SITE_URL}/sitemap.xml`,
    host: SITE_URL,
  }
}
