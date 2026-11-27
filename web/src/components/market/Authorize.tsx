'use client'

import { WalletNetworkGlyph } from '@/components/WalletNetworkGlyph'
import { useEffect, useState } from 'react'
import { ShieldCheck, Wallet } from 'lucide-react'
import { ask, takeWalletPass } from '@/lib/market'
import { short } from '@/lib/explorer'
import { Wordmark } from '@/components/Wordmark'
import { SignIn } from './AccountView'
import { Avatar } from './Avatar'
import { Notice, SIGNED_IN_OR_OUT } from './Shell'

type Asked = {
  site: { name: string; host: string }
  you: { name: string; picture: string | null; wallets: { address: string; name: string | null }[]; allowed: boolean } | null
}

/** The request's parameters, as the website sent them. */
function params() {
  const query = new URLSearchParams(window.location.search)
  const get = (key: string) => query.get(key) ?? undefined
  return {
    client_id: get('client_id') ?? '',
    redirect_uri: get('redirect_uri') ?? '',
    response_type: get('response_type') ?? '',
    scope: get('scope') ?? '',
    state: get('state'),
    nonce: get('nonce'),
    code_challenge: get('code_challenge'),
    code_challenge_method: get('code_challenge_method'),
    prompt: get('prompt') ?? '',
  }
}

/** Back to the website with an answer it can read: an error, or a code. */
function back(redirect: string, answer: Record<string, string | undefined>) {
  const url = new URL(redirect)
  for (const [key, value] of Object.entries(answer)) if (value !== undefined) url.searchParams.set(key, value)
  window.location.replace(url.toString())
}

const button = 'flex h-11 flex-1 items-center justify-center rounded-xl px-6 text-sm font-bold transition disabled:opacity-60'

/** /oauth/authorize/: where websites send people to Sign in with Orien. */
export function Authorize() {
  const [asked, setAsked] = useState<Asked | null>(null)
  const [problem, setProblem] = useState('')
  const [busy, setBusy] = useState(false)

  const load = async () => {
    const p = params()
    if (!p.client_id || !p.redirect_uri) {
      setProblem('This sign-in link is missing the website’s details. Go back to the website and try again.')
      return
    }
    const query = new URLSearchParams({ client_id: p.client_id, redirect_uri: p.redirect_uri })
    try {
      const found = await ask<Asked>(`/oidc/request?${query}`)
      // From here on the website is known and its return address registered.
      if (p.response_type !== 'code') {
        back(p.redirect_uri, { error: 'unsupported_response_type', state: p.state, iss: window.location.origin })
        return
      }
      if (p.prompt.split(' ').includes('none') && !(found.you && found.you.allowed)) {
        back(p.redirect_uri, {
          error: found.you ? 'consent_required' : 'login_required',
          state: p.state,
          iss: window.location.origin,
        })
        return
      }
      setAsked(found)
      if (found.you?.allowed && !p.prompt.split(' ').includes('consent')) void allow()
    } catch (error) {
      setProblem((error as Error).message)
    }
  }

  useEffect(() => {
    void (async () => {
      // A pass from Orien Wallet signs this browser in without an email code.
      await takeWalletPass()
      await load()
    })()
  }, [])

  const allow = async () => {
    setBusy(true)
    const p = params()
    try {
      const { redirect } = await ask<{ redirect: string }>('/oidc/allow', 'POST', {
        client_id: p.client_id,
        redirect_uri: p.redirect_uri,
        scope: p.scope,
        state: p.state,
        nonce: p.nonce,
        code_challenge: p.code_challenge,
        code_challenge_method: p.code_challenge_method,
      })
      window.location.replace(redirect)
    } catch (error) {
      setProblem((error as Error).message)
      setBusy(false)
    }
  }

  const cancel = () => {
    const p = params()
    back(p.redirect_uri, { error: 'access_denied', state: p.state, iss: window.location.origin })
  }

  const signOut = async () => {
    await ask('/signout', 'POST').catch(() => undefined)
    setAsked(null)
    await load()
  }

  if (problem) {
    return (
      <Frame>
        <h1 className="text-2xl font-bold tracking-tight text-ink">Can’t sign in</h1>
        <Notice tone="bad">{problem}</Notice>
      </Frame>
    )
  }
  if (!asked)
    return (
      <Overlay>
        <p className="text-center text-muted">Loading…</p>
      </Overlay>
    )

  if (!asked.you) {
    return (
      <Overlay>
        <SignIn
        onDone={() => {
          // The header follows the same news.
          window.dispatchEvent(new Event(SIGNED_IN_OR_OUT))
          void load()
        }}
        trouble=""
        wallet
        title={`Sign in to continue to ${asked.site.name}`}
        blurb="Use your Orien Network account."
        />
      </Overlay>
    )
  }

  const you = asked.you
  return (
    <Frame>
      <div className="space-y-2">
        <h1 className="text-2xl font-bold tracking-tight text-ink">
          Sign in to <span className="break-words">{asked.site.name}</span>
        </h1>
        <p className="text-muted">
          <span className="font-semibold text-ink">{asked.site.host}</span> is asking to see:
        </p>
      </div>

      <ul className="divide-y divide-line rounded-xl border border-line">
        <li className="flex items-center gap-3 px-4 py-3">
          <Avatar name={you.name} picture={you.picture} size={36} />
          <span className="min-w-0">
            <span className="block text-sm text-muted">Your Orien Network name, picture and banner</span>
            <span className="block truncate font-semibold text-ink">{you.name}</span>
          </span>
        </li>
        <li className="px-4 py-3">
          <span className="flex items-center gap-2 text-sm text-muted">
            <Wallet className="h-4 w-4" aria-hidden />
            Your verified wallets
          </span>
          {you.wallets.length > 0 ? (
            <ul className="mt-2 space-y-1.5">
              {you.wallets.map((wallet) => (
                <li key={wallet.address} className="flex items-center justify-between gap-3 text-sm">
                  <span className="truncate font-semibold text-ink"><WalletNetworkGlyph />{wallet.name ?? 'Wallet'}</span>
                  <span className="num shrink-0 text-muted">{short(wallet.address)}</span>
                </li>
              ))}
            </ul>
          ) : (
            <p className="mt-2 text-sm text-muted">None yet. You can verify a wallet in your account.</p>
          )}
        </li>
      </ul>

      <p className="flex items-start gap-2 text-sm leading-relaxed text-muted">
        <ShieldCheck className="mt-0.5 h-4 w-4 shrink-0 text-go" aria-hidden />
        It won’t see your email address, and it can’t move ORN or use your wallets.
      </p>

      <div className="flex gap-3">
        <button type="button" onClick={cancel} disabled={busy} className={`${button} border border-line bg-bg text-ink hover:border-pearl`}>
          Cancel
        </button>
        <button type="button" onClick={() => void allow()} disabled={busy} className={`${button} pearl-fill hover:brightness-105`}>
          {busy ? 'Signing in…' : 'Allow'}
        </button>
      </div>

      <p className="flex items-center justify-center gap-3 text-sm text-muted">
        Not {you.name}?
        <button
          type="button"
          onClick={() => void signOut()}
          className="flex h-8 items-center rounded-lg border border-line px-3 text-xs font-semibold text-ink transition hover:border-pearl"
        >
          Use another account
        </button>
      </p>
    </Frame>
  )
}

/** Over the whole page, the site frosted behind it, like a dialog. */
function Overlay({ children }: { children: React.ReactNode }) {
  return (
    <div className="fixed inset-0 z-[70] overflow-y-auto bg-bg/40 backdrop-blur-xl">
      <div className="flex min-h-full items-center justify-center p-4 sm:p-8">
        <div className="w-full max-w-[440px] rounded-2xl shadow-2xl shadow-black/60">{children}</div>
      </div>
    </div>
  )
}

function Frame({ children }: { children: React.ReactNode }) {
  return (
    <Overlay>
      <section role="dialog" aria-modal="true" className="space-y-6 rounded-2xl border border-line bg-surface p-6 sm:p-8">
        <div className="flex items-center gap-2 text-lg">
          <Wordmark />
        </div>
        {children}
      </section>
    </Overlay>
  )
}
