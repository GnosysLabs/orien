'use client'

import { useEffect, useState } from 'react'
import { me, type Account } from './market'

/** Sent when this browser signs in or out, or reads messages, so every part
 * of the page catches up. */
export const SIGNED_IN_OR_OUT = 'orien-market-account'

/** Who is signed in to Orien Network and how many messages they haven't read:
 * undefined until known, null when nobody is. */
export type Me = { account: Account | null | undefined; unread: number }

let current: Me = { account: undefined, unread: 0 }
const listeners = new Set<(me: Me) => void>()
let started = false

async function refresh() {
  try {
    const found = await me()
    current = { account: found?.account ?? null, unread: found?.unread ?? 0 }
  } catch {
    current = { account: null, unread: 0 }
  }
  listeners.forEach((listener) => listener(current))
}

/** One check for the whole page, every 30 seconds, when the tab comes back
 * into view, and whenever something signals a change. */
function start() {
  if (started) return
  started = true
  void refresh()
  setInterval(refresh, 30_000)
  window.addEventListener(SIGNED_IN_OR_OUT, () => void refresh())
  document.addEventListener('visibilitychange', () => {
    if (document.visibilityState === 'visible') void refresh()
  })
}

export function useMe(): Me {
  const [state, setState] = useState<Me>(current)
  useEffect(() => {
    start()
    listeners.add(setState)
    setState(current)
    return () => {
      listeners.delete(setState)
    }
  }, [])
  return state
}
