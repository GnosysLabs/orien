'use client'

import { useEffect, useState } from 'react'
import { LogIn } from 'lucide-react'
import { me } from '@/lib/market'
import { useMe } from '@/lib/useMe'
import { openSignIn } from './SignInDialog'
import { YourWebsites } from './YourWebsites'

/** /developers/websites/: the websites this account registered for Sign in
 * with Orien. Signed out, the page shows through a frosted sign-in dialog. */
export function WebsitesView() {
  const [signedIn, setSignedIn] = useState<boolean | undefined>(undefined)
  const reload = () =>
    me()
      .then((found) => setSignedIn(found !== null))
      .catch(() => setSignedIn(false))
  useEffect(() => {
    void reload()
  }, [])
  // Signed in or out anywhere: the dialog over the site, the menu, another tab.
  const live = useMe()
  useEffect(() => {
    if (live.account === null) setSignedIn(false)
    else if (live.account) setSignedIn(true)
  }, [live.account])
  // Signed out, the sign-in dialog opens over this page by itself.
  useEffect(() => {
    if (signedIn === false) openSignIn()
  }, [signedIn])

  return (
    <>
      <div className="space-y-6 pb-16 pt-5 sm:pt-7">
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
        {signedIn === false && (
          <button
            type="button"
            onClick={openSignIn}
            className="flex h-11 items-center gap-2 rounded-xl border border-line px-5 text-sm font-semibold text-ink transition hover:bg-raised"
          >
            <LogIn className="h-4 w-4" strokeWidth={2.25} aria-hidden /> Sign in to manage your websites
          </button>
        )}
        {signedIn && <YourWebsites />}
      </div>
    </>
  )
}
