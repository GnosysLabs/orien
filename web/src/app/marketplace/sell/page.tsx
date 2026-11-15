import type { Metadata } from 'next'
import { Suspense } from 'react'
import { SellForm } from '@/components/market/SellForm'
import { MarketBar } from '@/components/market/Shell'

export const metadata: Metadata = { title: 'Sell something', robots: { index: false } }

export default function SellPage() {
  return (
    <div className="mx-auto max-w-[1100px] pb-16">
      <MarketBar />
      <Suspense>
        <SellForm />
      </Suspense>
    </div>
  )
}
