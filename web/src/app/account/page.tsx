import type { Metadata } from 'next'
import { AccountView } from '@/components/market/AccountView'
import { MarketBar } from '@/components/market/Shell'

export const metadata: Metadata = { title: 'Your account', robots: { index: false } }

export default function AccountPage() {
  return (
    <div className="mx-auto max-w-[1100px] pb-16">
      <MarketBar />
      <AccountView />
    </div>
  )
}
