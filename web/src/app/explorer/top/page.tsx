import type { Metadata } from 'next'
import { TopWallets } from '@/components/explorer/TopWallets'

export const metadata: Metadata = {
  title: 'Top wallets · Explorer',
  description: 'The 100 Orien wallets holding the most ORN, and their share of all ORN in wallets.',
}

/** /explorer/top/: the largest holders. */
export default function TopPage() {
  return (
    <div className="pb-16 pt-10 sm:pt-12">
      <h1 className="text-3xl font-bold tracking-tight text-ink">Top 100 wallets</h1>
      <p className="mb-8 mt-3 text-muted">The wallets holding the most ORN, and their share of all ORN in wallets.</p>
      <TopWallets />
    </div>
  )
}
