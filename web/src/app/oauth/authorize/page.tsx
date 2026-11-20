import type { Metadata } from 'next'
import { Authorize } from '@/components/market/Authorize'

export const metadata: Metadata = { title: 'Sign in with Orien', robots: { index: false } }

/** Where websites send people to Sign in with Orien. */
export default function AuthorizePage() {
  return (
    <div className="mx-auto max-w-[1100px] pb-16 pt-10 sm:pt-14">
      <Authorize />
    </div>
  )
}
