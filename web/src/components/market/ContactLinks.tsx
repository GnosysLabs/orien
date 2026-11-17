'use client'

import { useState } from 'react'
import { Check, Send } from 'lucide-react'
import type { Contact } from '@/lib/market'
import { DiscordIcon, XIcon as XLogo } from '@/components/Header'

const chip =
  'inline-flex h-10 items-center gap-2 rounded-xl border border-line bg-bg px-4 text-sm font-semibold text-ink transition hover:bg-raised'

/** A seller's ways to be reached besides messages: Telegram and X as links,
 * Discord as a username to copy, since Discord has no page for one. */
export function ContactLinks({ contact }: { contact: Contact }) {
  const [copied, setCopied] = useState(false)
  const links = [
    contact.telegram && { href: `https://t.me/${contact.telegram}`, label: `@${contact.telegram} on Telegram`, icon: Send },
    contact.x && { href: `https://x.com/${contact.x}`, label: `@${contact.x} on X`, icon: XLogo },
  ].filter(Boolean) as { href: string; label: string; icon: (props: { className?: string }) => React.ReactNode }[]
  if (links.length === 0 && !contact.discord) return null
  return (
    <div className="flex flex-wrap gap-2">
      {links.map(({ href, label, icon: Icon }) => (
        <a key={href} href={href} target="_blank" rel="noopener noreferrer nofollow" className={chip}>
          <Icon className="h-4 w-4 text-pearl" /> {label}
        </a>
      ))}
      {contact.discord && (
        <button
          type="button"
          title="Copy this Discord username"
          onClick={() => {
            void navigator.clipboard.writeText(contact.discord)
            setCopied(true)
            setTimeout(() => setCopied(false), 1500)
          }}
          className={chip}
        >
          {copied ? <Check className="h-4 w-4 text-go" /> : <DiscordIcon className="h-4 w-4 text-pearl" />}
          {copied ? 'Copied' : `${contact.discord} on Discord`}
        </button>
      )}
    </div>
  )
}
