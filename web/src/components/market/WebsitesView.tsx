'use client'

import { useEffect, useState } from 'react'
import { me, takeWalletPass } from '@/lib/market'
import { SIGNED_IN_OR_OUT, useMe } from '@/lib/useMe'
import { SignIn } from './AccountView'
import { YourWebsites } from './YourWebsites'

/** /developers/websites/: the websites this account registered for Sign in
 * with Orien. Signed out, the page shows through a frosted sign-in dialog. */
export function WebsitesView() {
  const [signedIn, setSignedIn] = useState<boolean | undefined>(undefined)
  const reload = () =>
    me()
      .then((found) => setSignedIn(found !== null))
      .catch(() => setSignedIn(false))
  // Signing in here: the header follows the same news.
  const arrived = () => {
    window.dispatchEvent(new Event(SIGNED_IN_OR_OUT))
    void reload()
  }
  useEffect(() => {
    void takeWalletPass().then((signedIn) => (signedIn ? arrived() : reload()))
  }, [])
  // Signed out elsewhere, from the menu or another tab.
  const live = useMe()
  useEffect(() => {
    if (live.account === null) setSignedIn(false)
    else if (live.account) setSignedIn(true)
  }, [live.account])

  return (
    <>
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
        {signedIn === undefined && <p className="text-muted">Loading…</p>}
        {signedIn && <YourWebsites />}
      </div>
      {signedIn === false && (
        <div className="fixed inset-0 z-[70] overflow-y-auto bg-bg/40 backdrop-blur-xl">
          <div className="flex min-h-full items-center justify-center p-4 sm:p-8">
            <div role="dialog" aria-modal="true" className="w-full max-w-[440px] rounded-2xl shadow-2xl shadow-black/60">
              <SignIn onDone={arrived} trouble="" wallet title="Sign in to manage your websites" blurb="Use your Orien Network account." />
            </div>
          </div>
        </div>
      )}
    </>
  )
}
