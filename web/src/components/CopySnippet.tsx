'use client'

import { useState } from 'react'
import { Check, Copy } from 'lucide-react'

/** A few lines of code to copy, with a button that copies them all. */
export function CopySnippet({ text }: { text: string }) {
  const [copied, setCopied] = useState(false)
  return (
    <div className="relative">
      <pre className="num overflow-auto rounded-xl border border-line bg-bg p-4 pr-24 text-xs leading-relaxed text-ink">{text}</pre>
      <button
        type="button"
        onClick={() => {
          void navigator.clipboard.writeText(text)
          setCopied(true)
          setTimeout(() => setCopied(false), 1500)
        }}
        className="absolute right-3 top-3 flex h-8 items-center gap-1.5 rounded-lg bg-raised px-2.5 text-xs font-semibold text-ink transition hover:bg-line"
      >
        {copied ? <Check className="h-3.5 w-3.5" aria-hidden /> : <Copy className="h-3.5 w-3.5" aria-hidden />}
        {copied ? 'Copied' : 'Copy'}
      </button>
    </div>
  )
}
