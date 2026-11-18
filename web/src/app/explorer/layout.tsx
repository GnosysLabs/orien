import { ExplorerBar } from '@/components/explorer/ExplorerBar'

/** The explorer has its own bar and footer instead of the site's. */
export default function ExplorerLayout({ children }: { children: React.ReactNode }) {
  return (
    <div className="flex flex-1 flex-col">
      <ExplorerBar />
      <div className="mx-auto w-full max-w-[1200px] flex-1 px-4 pb-16 pt-2">{children}</div>
      <footer className="border-t border-line px-4 py-6 text-center text-xs text-muted">
        Orien Explorer reads the live Orien network. ·{' '}
        <a href="https://orien.network/" className="hover:text-ink">
          orien.network
        </a>
      </footer>
    </div>
  )
}
