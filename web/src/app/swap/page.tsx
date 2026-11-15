import type { Metadata } from 'next'
import { SwapBoard } from '@/components/market/SwapBoard'
import { MarketBar } from '@/components/market/Shell'

export const metadata: Metadata = {
  title: 'Buy and sell ORN',
  description: 'Buy ORN with Bitcoin, or sell ORN for Bitcoin, directly with another person: either both get paid, or both get their coins back.',
  alternates: { canonical: '/swap/' },
}

export default function SwapPage() {
  return (
    <div className="mx-auto max-w-[1100px] pb-16">
      <MarketBar />
      <SwapBoard />
    </div>
  )
}
