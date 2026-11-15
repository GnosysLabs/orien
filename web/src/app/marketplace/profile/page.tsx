import type { Metadata } from 'next'
import { Suspense } from 'react'
import { ProfileView } from '@/components/market/ProfileView'
import { MarketBar } from '@/components/market/Shell'

export const metadata: Metadata = { title: 'Seller', robots: { index: false } }

export default function ProfilePage() {
  return (
    <div className="mx-auto max-w-[900px] pb-16">
      <MarketBar />
      <Suspense>
        <ProfileView />
      </Suspense>
    </div>
  )
}
