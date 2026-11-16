'use client'

import Link from 'next/link'
import { useEffect } from 'react'

/** A page that moved: goes to its new address, keeping any ?query. */
export function MovedTo({ href }: { href: string }) {
  useEffect(() => {
    window.location.replace(href + window.location.search + window.location.hash)
  }, [href])
  return (
    <p className="py-16 text-center text-muted">
      This page moved to{' '}
      <Link href={href} className="font-semibold text-ink underline underline-offset-4">
        {href}
      </Link>
      .
    </p>
  )
}
