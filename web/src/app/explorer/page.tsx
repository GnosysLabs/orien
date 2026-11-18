import type { Metadata } from 'next'
import { ExplorerHome } from '@/components/explorer/ExplorerHome'
import { pageMeta } from '@/lib/seo'

export const metadata: Metadata = pageMeta({
  title: 'Explorer: the Orien network, live',
  description:
    'See the Orien network as it runs: every hourly checkpoint, the validators, how many ORN exist, and any address’s balance and payments.',
  path: '/explorer/',
})

export default function ExplorerPage() {
  return (
    <div className="mx-auto max-w-[1100px] pb-16">
      <header className="pb-10 pt-10 text-center sm:pt-14">
        <p className="mb-4 text-xs font-semibold uppercase tracking-[0.2em] text-muted">Explorer</p>
        <h1 className="text-balance text-4xl font-bold leading-[1.1] tracking-tight text-ink sm:text-5xl">
          The Orien network, live
        </h1>
        <p className="mx-auto mt-6 max-w-xl text-lg leading-relaxed text-muted">
          Every hourly checkpoint, the validators, and any address’s balance and payments.
        </p>
      </header>
      <ExplorerHome />
    </div>
  )
}
