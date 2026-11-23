import type { Metadata } from 'next'
import { NetworkApp } from '@/components/explorer/NetworkApp'

export const metadata: Metadata = {
  title: 'Network map · Explorer',
  description:
    'The Orien network as a live map: the validators on the globe, every miner and every wallet that holds ORN around it, and the health of the links between validators.',
}

/** /explorer/map/: the network as a live map, filling the window under the bar. */
export default function MapPage() {
  return <NetworkApp />
}
