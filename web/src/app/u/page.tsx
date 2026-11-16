import type { Metadata } from 'next'
import { Suspense } from 'react'
import { ProfileView } from '@/components/market/ProfileView'
import { MarketBar } from '@/components/market/Shell'

export const metadata: Metadata = { title: 'Profile', robots: { index: false } }

/** Every /u/NAME/ address opens this page (the web server sends them here),
 * which reads the name from the address. */
export default function UserPage() {
  return (
    <div className="mx-auto max-w-[1100px] pb-16">
      <MarketBar />
      <Suspense>
        <ProfileView />
      </Suspense>
    </div>
  )
}
