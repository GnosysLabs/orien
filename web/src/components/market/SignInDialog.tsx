'use client'

import { usePathname, useRouter } from 'next/navigation'
import { useCallback, useEffect, useState } from 'react'
import { X } from 'lucide-react'
import { takeWalletPass } from '@/lib/market'
import { SIGNED_IN_OR_OUT, useMe } from '@/lib/useMe'
import { SignIn } from './AccountView'

const OPEN = 'orien-sign-in'

/** Opens the sign-in dialog over the page, wherever the person is. */
export function openSignIn() {
  window.dispatchEvent(new Event(OPEN))
}

/** The one sign-in dialog, mounted once for the whole site: a frosted
 * window over whatever page the person is on. It opens from openSignIn(),
 * from any link to /account/ while signed out, and by itself on /account/,
 * where there is nothing else to show. It also takes a pass from Orien
 * Wallet's extension out of the address, on whatever page it landed. */
export function SignInGate() {
  const pathname = usePathname()
  const router = useRouter()
  const { account } = useMe()
  const [open, setOpen] = useState(false)
  const onAccountPage = pathname === '/account/' || pathname === '/account'
  const signedIn = () => window.dispatchEvent(new Event(SIGNED_IN_OR_OUT))

  // A pass from Orien Wallet signs this browser in without an email code.
  // The allow page handles its own, as part of letting a website in.
  useEffect(() => {
    if (pathname.startsWith('/oauth/')) return
    void takeWalletPass().then((ok) => {
      if (!ok) return
      signedIn()
      if (onAccountPage) router.push('/community/')
    })
    // Once, when the page opens.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [])

  useEffect(() => {
    const show = () => setOpen(true)
    window.addEventListener(OPEN, show)
    return () => window.removeEventListener(OPEN, show)
  }, [])

  // Signed out, every Sign in link opens the dialog here instead of leaving.
  useEffect(() => {
    if (account !== null) return
    const catchLink = (event: MouseEvent) => {
      if (event.defaultPrevented || event.button !== 0 || event.metaKey || event.ctrlKey || event.shiftKey || event.altKey) return
      const link = (event.target as Element | null)?.closest?.('a[href]')
      if (!(link instanceof HTMLAnchorElement)) return
      if (link.origin !== location.origin || link.pathname.replace(/\/$/, '') !== '/account') return
      event.preventDefault()
      setOpen(true)
    }
    document.addEventListener('click', catchLink, true)
    return () => document.removeEventListener('click', catchLink, true)
  }, [account])

  // On the account page itself, signed out, there is only the dialog.
  useEffect(() => {
    if (onAccountPage && account === null) setOpen(true)
  }, [onAccountPage, account])

  const close = useCallback(() => {
    if (onAccountPage && account === null) return
    setOpen(false)
  }, [onAccountPage, account])

  useEffect(() => {
    if (!open) return
    const key = (event: KeyboardEvent) => event.key === 'Escape' && close()
    document.addEventListener('keydown', key)
    return () => document.removeEventListener('keydown', key)
  }, [open, close])

  // Signed in, by this dialog or any other way: it goes away.
  useEffect(() => {
    if (account) setOpen(false)
  }, [account])

  if (!open || account) return null
  const dismissable = !(onAccountPage && account === null)
  return (
    <div
      className="fixed inset-0 z-[80] overflow-y-auto bg-bg/40 backdrop-blur-xl"
      onClick={(event) => event.target === event.currentTarget && close()}
    >
      <div className="flex min-h-full items-center justify-center p-4 sm:p-8" onClick={(event) => event.target === event.currentTarget && close()}>
        <div role="dialog" aria-modal="true" className="relative w-full max-w-[440px] rounded-2xl shadow-2xl shadow-black/60">
          {dismissable && (
            <button
              type="button"
              onClick={close}
              aria-label="Close"
              className="absolute right-4 top-4 z-10 flex h-8 w-8 items-center justify-center rounded-full text-muted transition hover:bg-raised hover:text-ink"
            >
              <X className="h-4 w-4" aria-hidden />
            </button>
          )}
          <SignIn
            wallet
            trouble=""
            onDone={() => {
              signedIn()
              setOpen(false)
              // From the account page there is nowhere else to be; everywhere
              // else, people stay where they were.
              if (onAccountPage) router.push('/community/')
            }}
          />
        </div>
      </div>
    </div>
  )
}
