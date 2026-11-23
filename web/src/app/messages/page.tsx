import type { Metadata } from 'next'
import { Suspense } from 'react'
import { Messages } from '@/components/market/Messages'

export const metadata: Metadata = { title: 'Messages', robots: { index: false } }

/** Your conversations: with anyone directly, or about a listing on the Market. */
export default function MessagesPage() {
  return (
    <div className="mx-auto max-w-[1100px] pb-16 pt-5 sm:pt-7">
      <Suspense>
        <Messages />
      </Suspense>
    </div>
  )
}
