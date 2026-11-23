'use client'

import { useEffect } from 'react'

export function MovedToMessages() {
  useEffect(() => window.location.replace(`/messages/${window.location.search}`), [])
  return <p className="pt-7 text-muted">Loading…</p>
}
