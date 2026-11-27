import type { Metadata } from 'next'
import { MovedTo } from '@/components/MovedTo'

export const metadata: Metadata = { title: 'Community', robots: { index: false } }

/** The Market has closed; old links go to the Community. */
export default function ClosedMarketPage() {
  return <MovedTo href="/community/" />
}
