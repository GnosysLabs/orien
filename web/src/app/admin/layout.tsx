import type { Metadata } from 'next'

// Never indexed; it only opens on the tailnet anyway.
export const metadata: Metadata = { title: 'Admin', robots: { index: false, follow: false } }

export default function AdminLayout({ children }: { children: React.ReactNode }) {
  return children
}
