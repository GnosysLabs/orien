import type { Metadata } from 'next'
import { ExplorerHome } from '@/components/explorer/ExplorerHome'
import { EXPLORER_URL } from '@/lib/explorer'
import { pageMeta } from '@/lib/seo'

export const metadata: Metadata = {
  ...pageMeta({
  title: 'Explorer: the Orien network, live',
  description:
    'See the Orien network as it runs: every hourly checkpoint, the validators, how many ORN exist, and any address’s balance and payments.',
  path: '/explorer/',
  }),
  alternates: { canonical: EXPLORER_URL },
}

export default function ExplorerPage() {
  return (
    <div className="pb-16">
      <ExplorerHome />
    </div>
  )
}
