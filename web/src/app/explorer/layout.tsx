import { ExplorerFrame } from '@/components/explorer/ExplorerFrame'

/** The explorer has its own bar and footer instead of the site's. */
export default function ExplorerLayout({ children }: { children: React.ReactNode }) {
  return <ExplorerFrame>{children}</ExplorerFrame>
}
