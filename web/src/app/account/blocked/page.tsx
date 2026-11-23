import type { Metadata } from 'next'
import { BlockedList } from '@/components/market/BlockedList'

export const metadata: Metadata = { title: 'Blocked people', robots: { index: false } }

export default function BlockedPage() {
  return (
    <div className="mx-auto max-w-[720px] pb-16 pt-5 sm:pt-7">
      <BlockedList />
    </div>
  )
}
