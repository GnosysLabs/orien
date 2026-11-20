'use client'

import { useEffect, useState } from 'react'

/** base64url of bytes, as PKCE and JWTs use. */
function b64(bytes: Uint8Array) {
  return btoa(String.fromCharCode(...bytes)).replace(/\+/g, '-').replace(/\//g, '_').replace(/=+$/, '')
}

function random() {
  return b64(crypto.getRandomValues(new Uint8Array(32)))
}

const KEPT = 'orien-sign-in-test'
const button =
  'pearl-fill flex h-11 items-center justify-center rounded-xl px-6 text-sm font-bold transition hover:brightness-105 disabled:opacity-60'

/** /oauth/test/: a website of our own that signs in with Orien, the way
 * any website's pages would, to try the whole flow. */
export function SignInTest() {
  const [clientId, setClientId] = useState('')
  const [result, setResult] = useState<{ claims?: unknown; userinfo?: unknown; error?: string } | null>(null)
  const here = () => `${window.location.origin}${window.location.pathname}`

  useEffect(() => {
    setClientId(localStorage.getItem(`${KEPT}-client`) ?? '')
    const query = new URLSearchParams(window.location.search)
    if (!query.has('code') && !query.has('error')) return
    const kept = JSON.parse(sessionStorage.getItem(KEPT) ?? 'null') as { verifier: string; state: string; client: string } | null
    history.replaceState(null, '', window.location.pathname)
    if (query.has('error')) {
      setResult({ error: `${query.get('error')}${query.get('error_description') ? `: ${query.get('error_description')}` : ''}` })
      return
    }
    if (!kept || query.get('state') !== kept.state) {
      setResult({ error: 'The state that came back is not the one sent.' })
      return
    }
    void (async () => {
      const response = await fetch('/api/market/oidc/token', {
        method: 'POST',
        headers: { 'Content-Type': 'application/x-www-form-urlencoded' },
        body: new URLSearchParams({
          grant_type: 'authorization_code',
          code: query.get('code') ?? '',
          redirect_uri: here(),
          client_id: kept.client,
          code_verifier: kept.verifier,
        }),
      })
      const tokens = await response.json()
      if (!response.ok) {
        setResult({ error: `${tokens.error}: ${tokens.error_description}` })
        return
      }
      const body = tokens.id_token.split('.')[1].replace(/-/g, '+').replace(/_/g, '/')
      const claims = JSON.parse(decodeURIComponent(escape(atob(body))))
      const userinfo = await fetch('/api/market/oidc/userinfo', {
        headers: { Authorization: `Bearer ${tokens.access_token}` },
      }).then((r) => r.json())
      setResult({ claims, userinfo })
    })()
  }, [])

  const start = async () => {
    const verifier = random()
    const state = random()
    const challenge = b64(new Uint8Array(await crypto.subtle.digest('SHA-256', new TextEncoder().encode(verifier))))
    localStorage.setItem(`${KEPT}-client`, clientId.trim())
    sessionStorage.setItem(KEPT, JSON.stringify({ verifier, state, client: clientId.trim() }))
    const query = new URLSearchParams({
      response_type: 'code',
      client_id: clientId.trim(),
      redirect_uri: here(),
      scope: 'openid profile wallets',
      state,
      nonce: random(),
      code_challenge: challenge,
      code_challenge_method: 'S256',
    })
    window.location.assign(`/oauth/authorize/?${query}`)
  }

  return (
    <section className="mx-auto max-w-[640px] space-y-6 rounded-2xl border border-line bg-surface p-6 sm:p-8">
      <div className="space-y-2">
        <h1 className="text-2xl font-bold tracking-tight text-ink">Sign in with Orien: test</h1>
        <p className="leading-relaxed text-muted">
          This page acts as a website. Register it in your account as a website that signs in from its pages only,
          with the return address <span className="num break-all text-ink">https://orien.network/oauth/test/</span>, then
          paste its client ID here.
        </p>
      </div>
      <input
        value={clientId}
        onChange={(e) => setClientId(e.target.value)}
        placeholder="Client ID, orn_…"
        spellCheck={false}
        className="num h-11 w-full rounded-xl border border-line bg-bg px-4 text-sm text-ink outline-none focus:border-pearl/60"
      />
      <button type="button" onClick={() => void start()} disabled={!clientId.trim()} className={button}>
        Sign in with Orien
      </button>
      {result?.error && <p className="rounded-xl border border-stop/50 p-4 text-sm text-stop">{result.error}</p>}
      {result?.claims !== undefined && (
        <div className="space-y-3">
          <p className="font-semibold text-go">Signed in. What the website received:</p>
          <p className="text-sm text-muted">The ID token’s contents</p>
          <pre className="num overflow-auto rounded-xl border border-line bg-bg p-4 text-xs text-ink">
            {JSON.stringify(result.claims, null, 2)}
          </pre>
          <p className="text-sm text-muted">The userinfo answer</p>
          <pre className="num overflow-auto rounded-xl border border-line bg-bg p-4 text-xs text-ink">
            {JSON.stringify(result.userinfo, null, 2)}
          </pre>
        </div>
      )}
    </section>
  )
}
