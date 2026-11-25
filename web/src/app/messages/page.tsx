import type { Metadata } from 'next'

export const metadata: Metadata = { title: 'Messages', robots: { index: false } }

/** Messages opens as a window over the page (MessagesGate, in the layout);
 * landing here from an email link, it opens by itself. */
export default function MessagesPage() {
  return <div className="min-h-[60dvh]" />
}
