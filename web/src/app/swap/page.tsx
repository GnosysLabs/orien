import type { Metadata } from 'next'
import { SwapBoard } from '@/components/market/SwapBoard'
import { MarketBar } from '@/components/market/Shell'
import { pageMeta } from '@/lib/seo'

export const metadata: Metadata = pageMeta({
  title: 'Buy and sell ORN with SOL',
  description:
    'Live ORN / SOL order book and price chart. Buy ORN with SOL, or sell ORN for SOL, directly with another person through a trustless swap in Orien Wallet that takes about a minute.',
  path: '/swap/',
})

export default function SwapPage() {
  return (
    <div className="mx-auto max-w-[1100px] pb-16">
      <MarketBar />
      <SwapBoard />
    </div>
  )
}
