import { Mail, Send } from 'lucide-react'
import type { Contact } from '@/lib/market'
import { XIcon as XLogo } from '@/components/Header'

/** A seller's ways to be reached, as links. */
export function ContactLinks({ contact }: { contact: Contact }) {
  const ways = [
    contact.email && { href: `mailto:${contact.email}`, label: contact.email, icon: Mail },
    contact.telegram && { href: `https://t.me/${contact.telegram}`, label: `@${contact.telegram} on Telegram`, icon: Send },
    contact.x && { href: `https://x.com/${contact.x}`, label: `@${contact.x} on X`, icon: XLogo },
  ].filter(Boolean) as { href: string; label: string; icon: (props: { className?: string }) => React.ReactNode }[]
  if (ways.length === 0) return null
  return (
    <div className="flex flex-wrap gap-2">
      {ways.map(({ href, label, icon: Icon }) => (
        <a
          key={href}
          href={href}
          target={href.startsWith('mailto:') ? undefined : '_blank'}
          rel="noopener noreferrer nofollow"
          className="inline-flex h-10 items-center gap-2 rounded-xl border border-line bg-bg px-4 text-sm font-semibold text-ink transition hover:bg-raised"
        >
          <Icon className="h-4 w-4 text-pearl" /> {label}
        </a>
      ))}
    </div>
  )
}
