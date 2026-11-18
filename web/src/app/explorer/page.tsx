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
      <header className="pb-10 pt-10 text-center sm:pt-12">
        <h1 className="text-balance text-3xl font-bold leading-[1.1] tracking-tight text-ink sm:text-4xl">
          The Orien network, live
        </h1>
        <p className="mx-auto mt-4 max-w-xl leading-relaxed text-muted">
          Every hourly checkpoint, the validators, and any address’s balance and payments.
        </p>
      </header>
      <ExplorerHome />
    </div>
  )
}
