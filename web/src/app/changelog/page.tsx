import type { Metadata } from 'next'
import { Changelog } from '@/components/Changelog'

export const metadata: Metadata = {
  title: 'Changelog',
  description: 'Every change to the Orien network, its validators, orien and Orien Wallet, as it happens.',
  alternates: { canonical: '/changelog/' },
}

export default function ChangelogPage() {
  return (
    <div className="mx-auto max-w-[760px] pb-16">
      <header className="pb-8 pt-10 sm:pt-14">
        <p className="mb-4 text-xs font-semibold uppercase tracking-[0.2em] text-muted">Changelog</p>
        <h1 className="text-balance text-4xl font-bold leading-[1.1] tracking-tight text-ink sm:text-5xl">What changed, and when</h1>
        <p className="mt-6 text-lg leading-relaxed text-muted">
          Every change to the network, its validators, the <strong className="font-semibold text-ink">orien</strong> program
          and Orien Wallet, newest first. Dates are UTC.
        </p>
      </header>
      <Changelog />
    </div>
  )
}
