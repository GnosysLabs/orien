'use client'

import { useEffect, useState } from 'react'
import { Download, X } from 'lucide-react'
import { CHROME_EXTENSION, WALLET_SIGN_IN_FOR_EVERYONE } from '@/lib/brand'
import { WindowsDownload } from '@/components/WindowsDownload'

/** What Orien Wallet's browser extension says about itself, on <html
 * data-orien-wallet>: "ready" (the wallet is running and signed in; the
 * extension takes the click itself), "app-closed", "signed-out", or nothing
 * when the extension isn't installed. */
type State = 'ready' | 'app-closed' | 'signed-out' | null

function walletState(): State {
  const state = document.documentElement.dataset.orienWallet
  return state === 'ready' || state === 'app-closed' || state === 'signed-out' ? state : null
}

/** Where this page is open: the browser, and whether on a Mac. Phones and
 * tablets first, since their browsers also say Safari or Chrome. */
type Where = { browser: 'safari' | 'chrome' | 'firefox' | 'other'; mac: boolean; phone: boolean; name: string }

/** The name of the browser that takes Chrome's extensions, for the button:
 * Brave says so itself; Edge, Opera and Vivaldi say so in the user agent. */
function chromeName(agent: string): string {
  if ('brave' in navigator) return 'Brave'
  if (/Edg\//.test(agent)) return 'Edge'
  if (/OPR\//.test(agent)) return 'Opera'
  if (/Vivaldi\//.test(agent)) return 'Vivaldi'
  return 'Chrome'
}

function where(): Where {
  const agent = navigator.userAgent
  const phone = /iPhone|iPad|iPod|Android/.test(agent) || (/Macintosh/.test(agent) && navigator.maxTouchPoints > 1)
  const mac = !phone && /Macintosh/.test(agent)
  const browser = /Firefox\/|FxiOS/.test(agent)
    ? 'firefox'
    : /Chrome\/|Chromium\/|CriOS|Edg\//.test(agent)
      ? 'chrome'
      : /Safari\//.test(agent)
        ? 'safari'
        : 'other'
  const name = browser === 'chrome' ? chromeName(agent) : browser === 'safari' ? 'Safari' : browser === 'firefox' ? 'Firefox' : 'your browser'
  return { browser, mac, phone, name }
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
  // Only where the extension works: Safari on a Mac, and Chrome (and the
  // browsers that take Chrome's extensions) once it is in the Chrome Web
  // Store. Never on phones and tablets. Decided once the page is open here,
  // so nobody else sees it flash up.
  const [supported, setSupported] = useState(false)
  useEffect(() => {
    const here = where()
    setSupported(!here.phone && ((here.browser === 'safari' && here.mac) || (here.browser === 'chrome' && !!CHROME_EXTENSION)))
  }, [])
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
  if (!supported || (!WALLET_SIGN_IN_FOR_EVERYONE && !present)) return null
  return (
    <>
      <button
        type="button"
        data-orien-wallet-signin
        className={`${className} gap-2.5`}
        onClick={() => {
          const state = walletState()
          if (state !== 'ready') setOpen(state ?? 'install')
        }}
      >
        <span className="orien-mark h-[1.6em] w-[1.6em] shrink-0 bg-bg" aria-hidden />
        Sign in with Orien Wallet
      </button>
      {after}
      {open && <WalletHelp state={open} onClose={() => setOpen(undefined)} />}
    </>
  )
}

function Steps({ children }: { children: React.ReactNode }) {
  return <ol className="list-decimal space-y-1.5 pl-5 text-sm marker:font-semibold marker:text-ink">{children}</ol>
}

const download = (
  <a href="/wallet/Orien-Wallet.dmg" className={primary}>
    <Download className="h-4 w-4" aria-hidden /> Get Orien Wallet for Mac
  </a>
)

function WalletHelp({ state, onClose }: { state: State | 'install'; onClose: () => void }) {
  const [here, setHere] = useState<Where>({ browser: 'other', mac: false, phone: false, name: 'your browser' })
  useEffect(() => {
    setHere(where())
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
    let advice: React.ReactNode
    if (here.phone) {
      advice = (
        <p>
          On a phone or tablet, sign in with your email for now. The extension is for Safari on a Mac and for Chrome
          on a computer.
        </p>
      )
    } else if (here.browser === 'safari' && here.mac) {
      advice = (
        <>
          <div className="space-y-2">
            <p className="font-semibold text-ink">Already have Orien Wallet?</p>
            <Steps>
              <li>Update it to the latest version (1.8.22 or later).</li>
              <li>
                In Safari, choose <span className="font-semibold text-ink">Safari → Settings → Extensions</span>.
              </li>
              <li>
                Tick <span className="font-semibold text-ink">Orien Wallet</span>, and allow it on orien.network.
              </li>
              <li>Reload this page.</li>
            </Steps>
          </div>
          <div className="space-y-3">
            <p className="font-semibold text-ink">New to Orien Wallet?</p>
            {download}
          </div>
        </>
      )
    } else if (here.browser === 'chrome' && CHROME_EXTENSION) {
      advice = (
        <>
          <div className="space-y-3">
            <p className="font-semibold text-ink">Already have Orien Wallet?</p>
            <Steps>
              <li>Update it to the latest version (1.8.22 or later).</li>
              <li>Add the extension to {here.name}, and reload this page.</li>
            </Steps>
            <a href={CHROME_EXTENSION} target="_blank" rel="noopener noreferrer" className={primary}>
              Add it to {here.name}
            </a>
          </div>
          <div className="space-y-3">
            <p className="font-semibold text-ink">New to Orien Wallet?</p>
            {here.mac ? (
              download
            ) : (
              <WindowsDownload className={secondary}>
                <Download className="h-4 w-4" aria-hidden /> Get Orien Wallet for Windows
              </WindowsDownload>
            )}
          </div>
        </>
      )
    } else {
      const which = here.browser === 'firefox' ? 'Firefox isn’t supported yet.' : here.browser === 'chrome' ? 'The Chrome extension is coming soon.' : 'Your browser isn’t supported yet.'
      advice = here.mac ? (
        <>
          <p>{which} On this Mac, it works today in Safari with the latest Orien Wallet.</p>
          {download}
        </>
      ) : (
        <p>{which} For now, sign in with your email.</p>
      )
    }
    body = (
      <>
        <p>
          With Orien Wallet’s browser extension, you sign in here with your wallet instead of an email code,
          whenever Orien Wallet is open and signed in.
        </p>
        {advice}
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
