import type { Metadata } from 'next'
import { Suspense } from 'react'
import { AddressView } from '@/components/explorer/AddressView'

export const metadata: Metadata = { title: 'Address · Explorer', robots: { index: false } }

/** /explorer/address/?a=ADDRESS: one address's balance and activity. */
export default function AddressPage() {
  return (
    <div className="pb-16 pt-10 sm:pt-12">
      <Suspense>
        <AddressView />
      </Suspense>
    </div>
  )
}
