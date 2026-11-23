'use client'

import { useEffect } from 'react'
import built from '@/lib/netviz.json'

declare global {
  interface Window {
    __NETVIZ__?: { api: string; assets: string; readOnly: boolean }
  }
}

/** The network map: an app of its own (the globe, the validators, every
 * miner and wallet, and the engineers' view of the links between
 * validators), running in the room this page gives it. Its files are built
 * apart from the site and copied to /netviz/; `built` names them. */
export function NetworkApp() {
  useEffect(() => {
    // Read-only here: visitors look, the server measures on its own clock.
    window.__NETVIZ__ = { api: '/api/netviz', assets: '/netviz/', readOnly: true }
    const script = document.createElement('script')
    script.type = 'module'
    script.src = built.js
    document.body.appendChild(script)
    return () => script.remove()
  }, [])
  return (
    <>
      <link rel="stylesheet" href={built.css} precedence="default" />
      <div
        id="app"
        className="app-root"
        style={{ position: 'absolute', inset: 0 }}
        // The app's own elements, exactly as its own page has them; it fills them in.
        dangerouslySetInnerHTML={{ __html: built.markup }}
        suppressHydrationWarning
      />
    </>
  )
}
