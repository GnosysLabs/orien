import type { Metadata } from 'next'
import { Browse } from '@/components/market/Browse'
import { MarketBar } from '@/components/market/Shell'
import { pageMeta } from '@/lib/seo'

export const metadata: Metadata = pageMeta({
  title: 'Marketplace: work and goods for ORN',
  description:
    'The Orien marketplace: design, software, teaching and goods offered for ORN, paid straight from your own wallet, with messages and payment requests built in.',
  path: '/marketplace/',
})

export default function MarketplacePage() {
  return (
    <div className="mx-auto max-w-[1100px] pb-16">
      <MarketBar />
      <Browse />
    </div>
  )
}
