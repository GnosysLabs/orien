'use client'

import { useEffect, useState } from 'react'
import { Download, X } from 'lucide-react'
import { CHROME_EXTENSION, WALLET_SIGN_IN_FOR_EVERYONE } from '@/lib/brand'

/** What Orien Wallet's browser extension says about itself, on <html
 * data-orien-wallet>: "ready" (the wallet is running and signed in; the
 * extension takes the click itself), "app-closed", "signed-out", or nothing
 * when the extension isn't installed. */
type State = 'ready' | 'app-closed' | 'signed-out' | null

function walletState(): State {
  const state = document.documentElement.dataset.orienWallet
  return state === 'ready' || state === 'app-closed' || state === 'signed-out' ? state : null
}

/** Safari on a Mac, Chrome (or another browser that takes Chrome's
 * extensions), or anything else. */
function browser(): 'safari-mac' | 'chrome' | 'other' {
  const agent = navigator.userAgent
  const mac = /Macintosh/.test(agent) && navigator.maxTouchPoints < 2
  if (/Chrome\/|Chromium\/|Edg\//.test(agent)) return 'chrome'
  if (mac && /Safari\//.test(agent)) return 'safari-mac'
  return 'other'
}

const primary =
  'pearl-fill flex h-11 w-full items-center justify-center gap-2 rounded-xl px-6 text-sm font-bold transition hover:brightness-105'
const secondary =
  'flex h-11 w-full items-center justify-center rounded-xl border border-line bg-bg px-6 text-sm font-semibold text-ink transition hover:border-pearl'

/** The Sign in with Orien Wallet button, always shown. With the wallet
 * running and signed in, its extension handles the click before this does;
 * otherwise this explains what's missing. */
export function WalletSignIn({ className, after }: { className: string; after?: React.ReactNode }) {
  const [open, setOpen] = useState<State | 'install' | undefined>(undefined)
  // Before the release, shown only where the extension has said it is there.
  const [present, setPresent] = useState(false)
  useEffect(() => {
    if (WALLET_SIGN_IN_FOR_EVERYONE) return
    const look = () => setPresent(walletState() !== null)
    look()
    const watching = new MutationObserver(look)
    watching.observe(document.documentElement, { attributes: true, attributeFilter: ['data-orien-wallet'] })
    return () => watching.disconnect()
  }, [])
  if (!WALLET_SIGN_IN_FOR_EVERYONE && !present) return null
  return (
    <>
      <button
        type="button"
        data-orien-wallet-signin
        className={className}
        onClick={() => {
          const state = walletState()
          if (state !== 'ready') setOpen(state ?? 'install')
        }}
      >
        Sign in with Orien Wallet
      </button>
      {after}
      {open && <WalletHelp state={open} onClose={() => setOpen(undefined)} />}
    </>
  )
}

function WalletHelp({ state, onClose }: { state: State | 'install'; onClose: () => void }) {
  const [where, setWhere] = useState<ReturnType<typeof browser>>('other')
  useEffect(() => {
    setWhere(browser())
    const close = (event: KeyboardEvent) => event.key === 'Escape' && onClose()
    document.addEventListener('keydown', close)
    return () => document.removeEventListener('keydown', close)
  }, [onClose])

  let title: string
  let body: React.ReactNode
  if (state === 'app-closed') {
    title = 'Open Orien Wallet'
    body = <p>Open Orien Wallet on this computer, then try again.</p>
  } else if (state === 'signed-out') {
    title = 'Sign in to Orien Wallet'
    body = <p>Open Orien Wallet and sign in to Orien Network (in Swap or Market), then try again.</p>
  } else {
    title = 'Sign in with Orien Wallet'
    body = (
      <>
        <p>
          With Orien Wallet’s browser extension, you sign in here with Touch ID instead of an email code, whenever
          Orien Wallet is open and signed in.
        </p>
        <div className="space-y-3 pt-1">
          {(where === 'safari-mac' || where === 'other' || !CHROME_EXTENSION) && (
            <a href="/wallet/Orien-Wallet.dmg" className={primary}>
              <Download className="h-4 w-4" aria-hidden /> Get Orien Wallet for Mac
            </a>
          )}
          {CHROME_EXTENSION && (where === 'chrome' || where === 'other') && (
            <a href={CHROME_EXTENSION} target="_blank" rel="noopener noreferrer" className={where === 'chrome' ? primary : secondary}>
              Add it to Chrome
            </a>
          )}
        </div>
        <p className="text-sm">
          {where === 'safari-mac' || !CHROME_EXTENSION
            ? 'The Safari extension comes with Orien Wallet for Mac: turn it on in Safari’s settings, under Extensions.'
            : 'On a Mac, the Safari extension comes with Orien Wallet for Mac.'}
        </p>
      </>
    )
  }

  return (
    <div
      className="fixed inset-0 z-[80] overflow-y-auto bg-bg/40 backdrop-blur-xl"
      onClick={(event) => event.target === event.currentTarget && onClose()}
    >
      <div className="flex min-h-full items-center justify-center p-4 sm:p-8" onClick={(event) => event.target === event.currentTarget && onClose()}>
        <section
          role="dialog"
          aria-modal="true"
          aria-label={title}
          className="w-full max-w-[420px] space-y-5 rounded-2xl border border-line bg-surface p-6 shadow-2xl shadow-black/60 sm:p-8"
        >
          <div className="flex items-start gap-3">
            <h2 className="min-w-0 flex-1 text-xl font-bold tracking-tight text-ink">{title}</h2>
            <button
              type="button"
              onClick={onClose}
              aria-label="Close"
              className="flex h-9 w-9 shrink-0 items-center justify-center rounded-lg border border-line text-muted transition hover:border-pearl hover:text-ink"
            >
              <X className="h-4 w-4" aria-hidden />
            </button>
          </div>
          <div className="space-y-4 leading-relaxed text-muted">{body}</div>
          <button type="button" onClick={onClose} className={secondary}>
            Use my email instead
          </button>
        </section>
      </div>
    </div>
  )
}
