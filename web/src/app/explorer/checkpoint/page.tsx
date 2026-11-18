import type { Metadata } from 'next'
import { Suspense } from 'react'
import { CheckpointView } from '@/components/explorer/CheckpointView'

export const metadata: Metadata = { title: 'Checkpoint · Explorer', robots: { index: false } }

/** /explorer/checkpoint/?n=NUMBER: one hourly checkpoint. */
export default function CheckpointPage() {
  return (
    <div className="mx-auto max-w-[1100px] pb-16 pt-10 sm:pt-14">
      <Suspense>
        <CheckpointView />
      </Suspense>
    </div>
  )
}
