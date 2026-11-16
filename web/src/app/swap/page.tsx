import type { Metadata } from 'next'
import { SwapBoard } from '@/components/market/SwapBoard'
import { MarketBar } from '@/components/market/Shell'
import { pageMeta } from '@/lib/seo'

export const metadata: Metadata = pageMeta({
  title: 'Buy and sell ORN with Bitcoin',
  description:
    'Buy ORN with Bitcoin, or sell ORN for Bitcoin, directly with another person through a trustless atomic swap: either both sides get paid, or both get their coins back. No exchange, no account holding your coins.',
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
