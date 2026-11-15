import type { Metadata } from 'next'
import { Browse } from '@/components/market/Browse'
import { MarketBar } from '@/components/market/Shell'

export const metadata: Metadata = {
  title: 'Marketplace',
  description: 'Work and things offered for ORN, paid straight from your own wallet.',
  alternates: { canonical: '/marketplace/' },
}

export default function MarketplacePage() {
  return (
    <div className="mx-auto max-w-[900px] pb-16">
      <MarketBar />
      <Browse />
    </div>
  )
}
