import type { Metadata } from 'next'
import { CommunityView } from '@/components/market/CommunityView'
import { pageMeta } from '@/lib/seo'
import { COMMUNITY_LIVE } from '@/lib/brand'

export const metadata: Metadata = pageMeta({
  title: 'Community: the Orien network and its people',
  description:
    'What the Orien network is doing and what people say about it: new records, new miners and milestones, posts from the community, and the top miners.',
  path: '/community/',
})

export default function CommunityPage() {
  if (!COMMUNITY_LIVE) {
    return (
      <div className="mx-auto max-w-[640px] py-24 text-center">
        <h1 className="text-3xl font-bold tracking-tight text-ink">Community</h1>
        <p className="mt-3 text-muted">The Orien community opens soon.</p>
      </div>
    )
  }
  return (
    <div className="mx-auto max-w-[1100px]">
      <CommunityView />
    </div>
  )
}
