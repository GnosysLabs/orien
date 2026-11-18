import type { Metadata } from 'next'
import Link from 'next/link'
import { Geist, Geist_Mono, Jost } from 'next/font/google'
import { Analytics } from '@/components/Analytics'
import { BottomNav, Header } from '@/components/Header'
import { ScrollToTop } from '@/components/ScrollToTop'
import { Main, SiteOnly } from '@/components/SiteOnly'
import { Wordmark } from '@/components/Wordmark'
import { DISCORD, MARKETPLACE_LIVE, OPERATOR, OPERATOR_URL, SITE_DESCRIPTION, SITE_NAME, SITE_URL, TAGLINE, X_ACCOUNT } from '@/lib/brand'
import { SHARE_IMAGE } from '@/lib/seo'
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
    'ORN',
    'quantum-proof cryptocurrency',
    'quantum-resistant cryptocurrency',
    'post-quantum cryptocurrency',
    'post-quantum signatures',
    'hash-based signatures',
    'BLAKE3',
    'blockless cryptocurrency',
    'fast crypto payments',
    'CPU mining',
    'RandomX',
    'mine crypto on your computer',
    'fair launch',
    'no pre-mine',
    'atomic swap Bitcoin',
    'buy ORN with SOL',
    'buy ORN with Bitcoin',
    'crypto wallet for Mac, Windows and Linux',
    'Gnosys Labs',
  ],
  category: 'finance',
  creator: OPERATOR,
  publisher: OPERATOR,
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
    title: `${SITE_NAME}: the quantum-proof cryptocurrency, final in a blink`,
    description: SITE_DESCRIPTION,
    url: SITE_URL,
    images: [SHARE_IMAGE],
  },
  twitter: {
    card: 'summary_large_image',
    site: '@OrienNetwork',
    title: `${SITE_NAME}: the quantum-proof cryptocurrency, final in a blink`,
    description: SITE_DESCRIPTION,
    images: ['/og-nebula.png'],
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
      '@type': 'Organization',
      '@id': `${SITE_URL}/#orien`,
      name: SITE_NAME,
      url: SITE_URL,
      logo: { '@type': 'ImageObject', url: `${SITE_URL}/icon-rounded-512.png`, width: 512, height: 512 },
      description: SITE_DESCRIPTION,
      sameAs: [X_ACCOUNT, DISCORD],
      parentOrganization: { '@id': `${SITE_URL}/#org` },
    },
    {
      '@type': 'SoftwareApplication',
      '@id': `${SITE_URL}/#wallet`,
      name: 'Orien Wallet',
      description:
        'A wallet for the Orien cryptocurrency: send and receive ORN, mine ORN on your own computer, keep contacts, and unlock with Touch ID.',
      applicationCategory: 'FinanceApplication',
      operatingSystem: 'macOS, Windows, Linux',
      url: `${SITE_URL}/try/`,
      downloadUrl: `${SITE_URL}/try/`,
      offers: { '@type': 'Offer', price: '0', priceCurrency: 'USD' },
      publisher: { '@id': `${SITE_URL}/#org` },
    },
    {
      '@type': 'WebSite',
      '@id': `${SITE_URL}/#site`,
      name: SITE_NAME,
      url: SITE_URL,
      description: SITE_DESCRIPTION,
      slogan: TAGLINE,
      publisher: { '@id': `${SITE_URL}/#orien` },
      inLanguage: 'en',
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
          <ScrollToTop />
          <Analytics />
          <SiteOnly>
            <Header />
          </SiteOnly>
          <Main>{children}</Main>
          <SiteOnly>
          <footer className="border-t border-line px-4 print:hidden pb-24 pt-6 text-center text-xs text-muted lg:pb-8">
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
              <a href="https://explorer.orien.network/" className="hover:text-ink">
                Explorer
              </a>
              <Link href="/paper/" className="hover:text-ink">
                Technical paper
              </Link>
              <Link href="/changelog/" className="hover:text-ink">
                Changelog
              </Link>
              <Link href="/contact/" className="hover:text-ink">
                Contact
              </Link>
              {MARKETPLACE_LIVE && (
                <>
                  <Link href="/privacy/" className="hover:text-ink">
                    Privacy
                  </Link>
                  <Link href="/terms/" className="hover:text-ink">
                    Terms
                  </Link>
                </>
              )}
              <a href={OPERATOR_URL} target="_blank" rel="noreferrer" className="hover:text-ink">
                Gnosys Labs
              </a>
            </nav>
            <p className="mt-3">© 2026 {OPERATOR}</p>
          </footer>
          </SiteOnly>
        </div>
        <SiteOnly>
          <BottomNav />
        </SiteOnly>
      </body>
    </html>
  )
}
