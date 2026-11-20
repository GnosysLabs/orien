import type { Metadata } from 'next'
import { AccountView } from '@/components/market/AccountView'

export const metadata: Metadata = { title: 'Your account', robots: { index: false } }

export default function AccountPage() {
  return (
    <div className="mx-auto max-w-[1100px]">
      <AccountView />
    </div>
  )
}
