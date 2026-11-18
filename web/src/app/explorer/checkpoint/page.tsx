import type { Metadata } from 'next'
import { Suspense } from 'react'
import { CheckpointView } from '@/components/explorer/CheckpointView'

export const metadata: Metadata = { title: 'Checkpoint · Explorer', robots: { index: false } }

/** /explorer/checkpoint/?n=NUMBER: one hourly checkpoint. */
export default function CheckpointPage() {
  return (
    <div className="pb-16 pt-10 sm:pt-12">
      <Suspense>
        <CheckpointView />
      </Suspense>
    </div>
  )
}
