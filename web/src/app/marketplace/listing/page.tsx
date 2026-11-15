import type { Metadata } from 'next'
import { Suspense } from 'react'
import { ListingView } from '@/components/market/ListingView'
import { MarketBar } from '@/components/market/Shell'

export const metadata: Metadata = { title: 'Listing', robots: { index: false } }

export default function ListingPage() {
  return (
    <div className="mx-auto max-w-[900px] pb-16">
      <MarketBar />
      <Suspense>
        <ListingView />
      </Suspense>
    </div>
  )
}
