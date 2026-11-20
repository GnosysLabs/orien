import type { Metadata } from 'next'
import { CommunityView } from '@/components/market/CommunityView'
import { pageMeta } from '@/lib/seo'

export const metadata: Metadata = pageMeta({
  title: 'Community: the Orien network and its people',
  description:
    'What the Orien network is doing and what people say about it: new records, new miners and milestones, posts from the community, and the top miners and teams.',
  path: '/community/',
})

export default function CommunityPage() {
  return (
    <div className="mx-auto max-w-[1100px]">
      <CommunityView />
    </div>
  )
}
