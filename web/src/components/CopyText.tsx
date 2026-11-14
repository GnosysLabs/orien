'use client'

import { useState } from 'react'
import { Check, Copy } from 'lucide-react'

export function CopyText({
  text,
  label,
  className = '',
}: {
  text: string
  label?: string
  className?: string
}) {
  const [copied, setCopied] = useState(false)
  return (
    <div className={`flex flex-col gap-1.5 ${className}`}>
      {label && <span className="text-xs font-medium text-muted">{label}</span>}
      <div className="flex items-center gap-2 rounded-xl border border-line bg-bg px-3 py-2">
        <code className="num min-w-0 flex-1 overflow-x-auto whitespace-nowrap text-xs sm:text-sm [scrollbar-width:none]">
          {text}
        </code>
        <button
          type="button"
          onClick={() => {
            void navigator.clipboard.writeText(text)
            setCopied(true)
            setTimeout(() => setCopied(false), 1500)
          }}
          className="flex h-8 shrink-0 items-center gap-1.5 rounded-lg bg-raised px-2.5 text-xs font-semibold text-ink transition hover:bg-line"
        >
          {copied ? (
            <Check className="h-3.5 w-3.5 text-go" strokeWidth={2.5} aria-hidden />
          ) : (
            <Copy className="h-3.5 w-3.5" aria-hidden />
          )}
          {copied ? 'Copied' : 'Copy'}
        </button>
      </div>
    </div>
  )
}
