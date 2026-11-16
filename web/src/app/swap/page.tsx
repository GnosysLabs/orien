import type { Metadata } from 'next'
import { SwapBoard } from '@/components/market/SwapBoard'
import { MarketBar } from '@/components/market/Shell'
import { pageMeta } from '@/lib/seo'

export const metadata: Metadata = pageMeta({
  title: 'Buy and sell ORN with SOL',
  description:
    'Buy ORN with SOL, or sell ORN for SOL, directly with another person through a trustless atomic swap that takes about a minute: either both sides get paid, or both get their coins back. Bitcoin works too. No exchange, no account holding your coins.',
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
