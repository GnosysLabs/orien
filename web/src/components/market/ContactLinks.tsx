import { Mail, Send } from 'lucide-react'
import type { Contact } from '@/lib/market'

/** X's logo, which its brand guidelines let anyone use to link to an X account. */
function XLogo({ className = '' }: { className?: string }) {
  return (
    <svg viewBox="0 0 24 24" fill="currentColor" aria-hidden className={className}>
      <path d="M18.244 2.25h3.308l-7.227 8.26 8.502 11.24H16.17l-5.214-6.817L4.99 21.75H1.68l7.73-8.835L1.254 2.25H8.08l4.713 6.231zm-1.161 17.52h1.833L7.084 4.126H5.117z" />
    </svg>
  )
}

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
