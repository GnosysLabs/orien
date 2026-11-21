'use client'

import { useEffect, useState } from 'react'
import { me, takeWalletPass } from '@/lib/market'
import { useMe } from '@/lib/useMe'
import { SignIn } from './AccountView'
import { YourWebsites } from './YourWebsites'

/** /developers/websites/: the websites this account registered for Sign in
 * with Orien, after signing in. */
export function WebsitesView() {
  const [signedIn, setSignedIn] = useState<boolean | undefined>(undefined)
  const reload = () =>
    me()
      .then((found) => setSignedIn(found !== null))
      .catch(() => setSignedIn(false))
  useEffect(() => {
    void takeWalletPass().then(reload)
  }, [])
  // Signed out elsewhere, from the menu or another tab.
  const live = useMe()
  useEffect(() => {
    if (live.account === null) setSignedIn(false)
    else if (live.account) setSignedIn(true)
  }, [live.account])

  if (signedIn === undefined) return <p className="pt-10 text-muted sm:pt-14">Loading…</p>
  if (!signedIn)
    return (
      <div className="pb-16 pt-10 sm:pt-14 lg:flex lg:min-h-[calc(100dvh-4rem)] lg:items-center lg:justify-center lg:py-0">
        <div className="w-full">
          <SignIn
            onDone={reload}
            trouble=""
            wallet
            title="Sign in to manage your websites"
            blurb="Use your Orien Network account."
          />
        </div>
      </div>
    )
  return (
    <div className="space-y-6 pb-16 pt-10 sm:pt-14">
      <header className="space-y-3">
        <p className="text-xs font-semibold uppercase tracking-[0.14em] text-muted">Build</p>
        <h1 className="text-3xl font-bold tracking-tight text-ink">Sign in with Orien</h1>
        <p className="max-w-2xl leading-relaxed text-muted">
          Register the websites and apps that let people sign in with their Orien Network account.{' '}
          <a href="/developers/sign-in/" className="font-semibold text-ink underline underline-offset-4">
            How it works for developers
          </a>
          .
        </p>
      </header>
      <YourWebsites />
    </div>
  )
}
