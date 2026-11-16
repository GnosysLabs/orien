'use client'

import Link from 'next/link'
import { useSearchParams } from 'next/navigation'
import { useEffect, useState } from 'react'
import { ArrowLeft, BadgeCheck } from 'lucide-react'
import { ask, me, memberSince, pictureUrl, profileHref, type Account, type Listing, type VerifiedWallet } from '@/lib/market'
import { Linked } from './Linked'
import { Avatar } from './Avatar'
import { ContactLinks } from './ContactLinks'
import { Card } from './Browse'
import { ReportButton } from './ReportButton'
import { Notice } from './Shell'

export function ProfileView() {
  // /u/NAME/, or the old /marketplace/profile/?id=, which moves to it.
  // Read from the browser's own address: the web server sends every /u/
  // address to one page, which only the browser knows the rest of.
  const params = useSearchParams()
  const [fromPath, setFromPath] = useState<string | null>(null)
  useEffect(() => {
    const path = window.location.pathname
    setFromPath(path.startsWith('/u/') ? decodeURIComponent(path.slice(3).replace(/\/$/, '')) : '')
  }, [])
  const name = fromPath || params.get('name')
  const id = name ? null : params.get('id')
  const [found, setFound] = useState<{ account: Account; listings: Listing[]; wallets?: VerifiedWallet[] } | null>(null)
  const [trouble, setTrouble] = useState('')
  const [viewer, setViewer] = useState<number | null>(null)

  useEffect(() => {
    me()
      .then((found) => setViewer(found?.account.id ?? null))
      .catch(() => {})
  }, [])

  useEffect(() => {
    if (fromPath === null || (!name && !id)) return
    const path = name ? `/users/${encodeURIComponent(name)}` : `/profiles/${encodeURIComponent(id!)}`
    ask<{ account: Account; listings: Listing[]; wallets?: VerifiedWallet[] }>(path)
      .then((profile) => {
        setFound(profile)
        // An old link shows the profile's own address from now on.
        if (!fromPath) window.history.replaceState(null, '', profileHref(profile.account.name))
      })
      .catch((error: Error) => setTrouble(error.message === 'not found' ? 'There is nobody with that name.' : error.message))
  }, [name, id, fromPath])

  if (fromPath === null) return <p className="text-muted">Loading…</p>
  if (!name && !id) return <Notice tone="bad">No one was named.</Notice>
  if (trouble) return <Notice tone="bad">{trouble}</Notice>
  if (!found) return <p className="text-muted">Loading…</p>
  const { account, listings } = found
  const wallets = found.wallets ?? []

  return (
    <div className="space-y-8">
      <Link href="/marketplace/" className="inline-flex items-center gap-2 text-sm text-muted hover:text-ink">
        <ArrowLeft className="h-4 w-4" aria-hidden /> All listings
      </Link>
      <section
        className={`relative isolate flex flex-col items-center gap-4 rounded-2xl border border-line bg-surface p-8 text-center sm:flex-row sm:items-start sm:text-left ${
          account.banner ? 'pt-28 sm:min-h-56 sm:pt-8' : ''
        }`}
      >
        {account.banner && <ProfileBanner banner={account.banner} />}
        {viewer !== account.id && (
          <div className="absolute right-3 top-3 z-10">
            <ReportButton kind="account" id={account.id} what={account.name} signedIn={viewer !== null} icon />
          </div>
        )}
        <div className="relative shrink-0 rounded-full ring-4 ring-surface">
          <Avatar name={account.name} picture={account.picture} size={112} />
        </div>
        {/* relative: drawn above the banner behind it. */}
        <div className={`relative min-w-0 space-y-2 ${account.banner ? 'on-banner' : ''}`}>
          <h1 className="text-3xl font-bold tracking-tight text-ink">{account.name}</h1>
          {wallets.length > 0 && (
            <p
              className="inline-flex items-center gap-1.5 rounded-full border border-line bg-bg px-3 py-1 text-xs font-semibold text-go"
              title="This person proved at least one wallet is theirs"
            >
              <BadgeCheck className="h-3.5 w-3.5" strokeWidth={2.25} aria-hidden /> Verified
            </p>
          )}
          <p className="text-sm text-muted">{memberSince(account.created)}</p>
          {account.bio && (
            <p className="whitespace-pre-line pt-2 leading-relaxed text-muted">
              <Linked text={account.bio} />
            </p>
          )}
          <div className="pt-3">
            <ContactLinks contact={account.contact} />
          </div>
        </div>
      </section>
      <section className="space-y-4">
        <h2 className="text-xl font-bold tracking-tight text-ink">What {account.name} offers</h2>
        {listings.length === 0 ? (
          <Notice>Nothing on offer right now.</Notice>
        ) : (
          <div className="grid gap-5 sm:grid-cols-2 lg:grid-cols-3">
            {listings.map((listing) => (
              <Card key={listing.id} listing={listing} />
            ))}
          </div>
        )}
      </section>
    </div>
  )
}

/** The profile's banner, fading into the card: down to the content on a
 * phone, leftwards into the name on a wider screen. */
export function ProfileBanner({ banner }: { banner: string }) {
  return (
    // Clipped to the card's corners here, not by the card, so a menu can
    // open over the card's edge.
    <div className="pointer-events-none absolute inset-0 -z-10 overflow-hidden rounded-2xl">
      {/* eslint-disable-next-line @next/next/no-img-element */}
      <img
        src={pictureUrl(banner)}
        alt=""
        className="absolute right-0 top-0 h-40 w-full object-cover opacity-90 [mask-image:linear-gradient(to_bottom,black_35%,transparent)] sm:h-full sm:w-3/4 sm:[mask-image:linear-gradient(to_left,black_30%,transparent)]"
      />
    </div>
  )
}
