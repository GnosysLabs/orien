import type { Metadata } from 'next'
import Link from 'next/link'
import { Geist, Geist_Mono, Jost } from 'next/font/google'
import { BottomNav, Header } from '@/components/Header'
import { Wordmark } from '@/components/Wordmark'
import { MARKETPLACE_LIVE, OPERATOR, OPERATOR_URL, SITE_DESCRIPTION, SITE_NAME, SITE_URL, TAGLINE } from '@/lib/brand'
import './globals.css'

const geistSans = Geist({ variable: '--font-geist-sans', subsets: ['latin'] })
const geistMono = Geist_Mono({ variable: '--font-geist-mono', subsets: ['latin'] })
const jost = Jost({
  variable: '--font-jost',
  weight: ['600', '700', '800'],
  subsets: ['latin'],
})

export const metadata: Metadata = {
  metadataBase: new URL(SITE_URL),
  title: {
    default: `${SITE_NAME}: ${TAGLINE.replace(/^A /, 'a ')}`,
    template: `%s · ${SITE_NAME}`,
  },
  applicationName: SITE_NAME,
  description: SITE_DESCRIPTION,
  keywords: [
    'Orien',
    'post-quantum payment network',
    'quantum-proof cryptocurrency',
    'hash-based signatures',
    'BLAKE3',
    'consensus-free payments',
    'GnosysLabs',
  ],
  icons: {
    icon: [
      { url: '/icon.svg', type: 'image/svg+xml' },
      { url: '/favicon-32.png', sizes: '32x32', type: 'image/png' },
      { url: '/icon-rounded-192.png', sizes: '192x192', type: 'image/png' },
    ],
    apple: [{ url: '/apple-icon.png', sizes: '180x180', type: 'image/png' }],
  },
  openGraph: {
    type: 'website',
    siteName: SITE_NAME,
    title: `${SITE_NAME}: quantum-proof payments, final in milliseconds`,
    description: SITE_DESCRIPTION,
    url: SITE_URL,
    images: [{ url: '/og.png', width: 1200, height: 630, alt: 'Orien: quantum-proof payment network' }],
  },
  twitter: {
    card: 'summary_large_image',
    title: `${SITE_NAME}: quantum-proof payments, final in milliseconds`,
    description: SITE_DESCRIPTION,
    images: ['/og.png'],
  },
}

const SITE_DATA = {
  '@context': 'https://schema.org',
  '@graph': [
    {
      '@type': 'Organization',
      '@id': `${SITE_URL}/#org`,
      name: OPERATOR,
      url: OPERATOR_URL,
    },
    {
      '@type': 'WebSite',
      '@id': `${SITE_URL}/#site`,
      name: SITE_NAME,
      url: SITE_URL,
      description: SITE_DESCRIPTION,
      slogan: TAGLINE,
      publisher: { '@id': `${SITE_URL}/#org` },
    },
  ],
}

export default function RootLayout({ children }: { children: React.ReactNode }) {
  return (
    <html lang="en" className={`${geistSans.variable} ${geistMono.variable} ${jost.variable} h-full antialiased`}>
      <body className="flex min-h-full flex-col print:bg-white">
        <script
          type="application/ld+json"
          dangerouslySetInnerHTML={{ __html: JSON.stringify(SITE_DATA) }}
        />
        <div className="flex min-h-full flex-1 flex-col">
          <Header />
          <main className="mx-auto w-full max-w-[1200px] flex-1 px-4 py-6 sm:py-8">{children}</main>
          <footer className="border-t border-line px-4 print:hidden pb-24 pt-6 text-center text-xs text-muted md:pb-8">
            <p className="mb-2 flex items-center justify-center gap-2 font-semibold text-ink/85">
              <Wordmark className="text-sm" />
              <span className="text-muted">·</span>
              <span>From the Latin <em>oriens</em>, rising</span>
            </p>
            <nav aria-label="Footer" className="mt-3 flex flex-wrap justify-center gap-x-5 gap-y-1">
              <Link href="/" className="hover:text-ink">
                Home
              </Link>
              <Link href="/how-it-works/" className="hover:text-ink">
                How it works
              </Link>
              <Link href="/try/" className="hover:text-ink">
                Get started
              </Link>
              <Link href="/paper/" className="hover:text-ink">
                Technical paper
              </Link>
              <Link href="/changelog/" className="hover:text-ink">
                Changelog
              </Link>
              {MARKETPLACE_LIVE && (
                <Link href="/privacy/" className="hover:text-ink">
                  Privacy
                </Link>
              )}
              <a href={OPERATOR_URL} target="_blank" rel="noreferrer" className="hover:text-ink">
                GnosysLabs
              </a>
            </nav>
            <p className="mt-3">© 2026 {OPERATOR}</p>
          </footer>
        </div>
        <BottomNav />
      </body>
    </html>
  )
}
