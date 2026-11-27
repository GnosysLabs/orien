import type { Metadata } from 'next'
import { Suspense } from 'react'
import { ProfileView } from '@/components/market/ProfileView'

export const metadata: Metadata = { title: 'Seller', robots: { index: false } }

export default function ProfilePage() {
  return (
    <div className="mx-auto max-w-[1100px] pb-16">
      <Suspense>
        <ProfileView />
      </Suspense>
    </div>
  )
}
