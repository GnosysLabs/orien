import type { Metadata } from 'next'
import { MovedTo } from '@/components/MovedTo'

export const metadata: Metadata = { title: 'Your account', robots: { index: false } }

/** Your account moved to /account/. */
export default function OldAccountPage() {
  return <MovedTo href="/account/" />
}
