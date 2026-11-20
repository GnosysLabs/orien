import type { Metadata } from 'next'
import { WebsitesView } from '@/components/market/WebsitesView'

export const metadata: Metadata = { title: 'Sign in with Orien: your websites', robots: { index: false } }

export default function WebsitesPage() {
  return (
    <div className="mx-auto max-w-[900px]">
      <WebsitesView />
    </div>
  )
}
