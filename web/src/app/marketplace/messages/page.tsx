import type { Metadata } from 'next'
import { MovedToMessages } from './Moved'

export const metadata: Metadata = { title: 'Messages', robots: { index: false } }

/** Messages moved to /messages/; old links (emails, older wallets) land here.
 * The web server moves them too; this covers the dev server. */
export default function OldMessagesPage() {
  return <MovedToMessages />
}
