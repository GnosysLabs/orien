import type { Metadata } from 'next'
import { Suspense } from 'react'
import { Messages } from '@/components/market/Messages'
import { MarketBar } from '@/components/market/Shell'

export const metadata: Metadata = { title: 'Messages', robots: { index: false } }

export default function MessagesPage() {
  return (
    <div className="mx-auto max-w-[900px] pb-16">
      <MarketBar />
      <Suspense>
        <Messages />
      </Suspense>
    </div>
  )
}
