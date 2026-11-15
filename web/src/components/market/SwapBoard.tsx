'use client'

import Link from 'next/link'
import { useEffect, useState } from 'react'
import { ArrowLeftRight, Bitcoin, Clock, ShieldCheck, Wallet } from 'lucide-react'
import { ask, formatBtc, formatOrn, me, parseBtc, parseOrn, sentence, type Account, type SwapOffer } from '@/lib/market'
import { Avatar } from './Avatar'
import { Notice } from './Shell'

const card = 'rounded-2xl border border-line bg-surface p-6'

/** Satoshis for one ORN, rounded for reading: "1,800". */
const perOrn = (offer: SwapOffer) => {
  const sats = offer.sats / (offer.orn / 1_000_000_000)
  return sats >= 100 ? Math.round(sats).toLocaleString('en') : sats.toPrecision(3)
}

export function SwapBoard() {
  const [offers, setOffers] = useState<SwapOffer[] | null>(null)
  const [account, setAccount] = useState<Account | null | undefined>(undefined)
  const [trouble, setTrouble] = useState('')
  const [taken, setTaken] = useState<SwapOffer | null>(null)

  const load = () =>
    ask<{ offers: SwapOffer[] }>('/swaps')
      .then((found) => setOffers(found.offers))
      .catch((error: Error) => setTrouble(sentence(error.message)))

  useEffect(() => {
    void load()
    me()
      .then((found) => setAccount(found?.account ?? null))
      .catch(() => setAccount(null))
    const timer = setInterval(load, 20000)
    return () => clearInterval(timer)
  }, [])

  const take = async (offer: SwapOffer) => {
    setTrouble('')
    try {
      await ask(`/swaps/${offer.id}/take`, 'POST')
      setTaken(offer)
      void load()
    } catch (error) {
      setTrouble(sentence((error as Error).message))
    }
  }

  return (
    <div className="space-y-10">
      <header className="space-y-4">
        <h1 className="text-balance text-4xl font-bold leading-[1.1] tracking-tight text-ink sm:text-5xl">
          Swap ORN and Bitcoin
        </h1>
        <p className="max-w-[680px] text-lg leading-relaxed text-muted">
          Trade directly with another person, with nobody in the middle. Either both of you get paid, or both of you get
          your coins back: nobody can take the other side&apos;s coins, and this site never touches them.
        </p>
      </header>

      <section className="grid gap-4 sm:grid-cols-3">
        {[
          { icon: ArrowLeftRight, title: 'Pick an offer', text: 'Sellers offer ORN for Bitcoin at their price. Take one in Orien Wallet, and it is held for you for 30 minutes.' },
          { icon: Wallet, title: 'Your wallets do the rest', text: 'The seller’s ORN are locked for you. You pay the Bitcoin into a lock only the seller can open, by revealing a secret that also opens the ORN.' },
          { icon: Clock, title: 'Done in under an hour', text: 'Usually 20 to 60 minutes, while Bitcoin confirms. If either side stops, the ORN and the Bitcoin go back to their owners on their own.' },
        ].map(({ icon: Icon, title, text }, n) => (
          <div key={title} className={card}>
            <div className="mb-3 flex items-center gap-3">
              <span className="flex h-8 w-8 items-center justify-center rounded-full bg-raised text-sm font-bold text-ink">{n + 1}</span>
              <Icon className="h-5 w-5 text-pearl" strokeWidth={1.75} aria-hidden />
            </div>
            <h2 className="mb-2 font-bold text-ink">{title}</h2>
            <p className="text-sm leading-relaxed text-muted">{text}</p>
          </div>
        ))}
      </section>

      {taken && (
        <Notice>
          You took {formatOrn(taken.orn)} ORN for {formatBtc(taken.sats)} BTC. Open Orien Wallet, signed in to the marketplace,
          within 30 minutes: it carries the swap through from here.
        </Notice>
      )}
      {trouble && <Notice tone="bad">{trouble}</Notice>}

      <section className="space-y-4">
        <div className="flex flex-wrap items-end justify-between gap-3">
          <h2 className="text-2xl font-bold tracking-tight text-ink">Offers</h2>
          <p className="text-sm text-muted">Cheapest Bitcoin per ORN first</p>
        </div>
        {offers === null ? (
          <p className="text-muted">Loading…</p>
        ) : offers.length === 0 ? (
          <Notice>No offers right now. Have ORN to sell? Post the first one below.</Notice>
        ) : (
          <ul className="overflow-hidden rounded-2xl border border-line bg-surface">
            {offers.map((offer) => {
              const mine = account?.id === offer.seller
              return (
                <li key={offer.id} className="flex flex-wrap items-center gap-4 border-b border-line px-5 py-4 last:border-b-0">
                  <Link href={`/marketplace/profile/?id=${offer.seller}`} className="flex min-w-0 items-center gap-3 sm:w-52">
                    <Avatar name={offer.seller_name} picture={offer.seller_picture} size={36} />
                    <span className="flex min-w-0 flex-col">
                      <span className="truncate text-sm font-semibold text-ink hover:underline">{offer.seller_name}</span>
                      <span className="text-xs text-muted">
                        {offer.seller_done === 0 ? 'No swaps yet' : `${offer.seller_done} swap${offer.seller_done === 1 ? '' : 's'} done`}
                      </span>
                    </span>
                  </Link>
                  <div className="flex-1">
                    <p className="num font-bold text-ink">
                      {formatOrn(offer.orn)} ORN <span className="font-normal text-muted">for</span> {formatBtc(offer.sats)} BTC
                    </p>
                    <p className="num text-xs text-muted">{perOrn(offer)} satoshis per ORN</p>
                  </div>
                  {mine ? (
                    <button
                      type="button"
                      onClick={() => ask(`/swaps/${offer.id}/close`, 'POST').then(load).catch((e: Error) => setTrouble(sentence(e.message)))}
                      className="h-10 rounded-xl border border-line px-4 text-sm font-semibold text-muted hover:bg-raised hover:text-ink"
                    >
                      Withdraw
                    </button>
                  ) : account ? (
                    <button type="button" onClick={() => take(offer)} className="pearl-fill h-10 rounded-xl px-5 text-sm font-bold">
                      Take
                    </button>
                  ) : (
                    <Link href="/marketplace/account/" className="h-10 rounded-xl border border-line px-4 text-sm font-semibold leading-10 text-ink hover:bg-raised">
                      Sign in to take
                    </Link>
                  )}
                </li>
              )
            })}
          </ul>
        )}
      </section>

      <PostOffer account={account} onPosted={load} />

      <section className={`${card} space-y-3`}>
        <div className="flex items-center gap-3">
          <ShieldCheck className="h-5 w-5 text-go" aria-hidden />
          <h2 className="text-lg font-bold tracking-tight text-ink">What keeps it safe</h2>
        </div>
        <ul className="list-disc space-y-1.5 pl-5 text-sm leading-relaxed text-muted marker:text-line">
          <li>The swap runs in Orien Wallet, on your computer, with your keys. This site only passes messages between the two wallets.</li>
          <li>The seller can only take the Bitcoin by revealing a secret, and that same secret is what lets the buyer take the ORN.</li>
          <li>If the buyer never pays, the seller&apos;s ORN come back after 48 hours. If the seller never takes the Bitcoin, the buyer&apos;s wallet takes it back after about 12 hours.</li>
          <li>Both sides have verified their Orien wallets, and profiles show how many swaps each has done.</li>
        </ul>
      </section>
    </div>
  )
}

function PostOffer({ account, onPosted }: { account: Account | null | undefined; onPosted: () => void }) {
  const [orn, setOrn] = useState('')
  const [btc, setBtc] = useState('')
  const [trouble, setTrouble] = useState('')
  const [posted, setPosted] = useState(false)

  if (account === undefined) return null
  const field = 'num h-11 w-full rounded-xl border border-line bg-bg px-4 text-ink outline-none focus:border-pearl/60'

  const post = async (event: React.FormEvent) => {
    event.preventDefault()
    setTrouble('')
    setPosted(false)
    const units = parseOrn(orn)
    const sats = parseBtc(btc)
    if (units === null) return setTrouble('Write how many ORN, like 500.')
    if (sats === null) return setTrouble('Write the Bitcoin price in BTC, like 0.01.')
    try {
      await ask('/swaps', 'POST', { orn: units, sats })
      setOrn('')
      setBtc('')
      setPosted(true)
      onPosted()
    } catch (error) {
      setTrouble(sentence((error as Error).message))
    }
  }

  return (
    <form id="post" onSubmit={post} className={`${card} scroll-mt-24 space-y-4`}>
      <div className="flex items-center gap-3">
        <Bitcoin className="h-5 w-5 text-pearl" aria-hidden />
        <h2 className="text-lg font-bold tracking-tight text-ink">Sell ORN for Bitcoin</h2>
      </div>
      {!account ? (
        <p className="text-sm text-muted">
          <Link href="/marketplace/account/" className="font-semibold text-ink underline underline-offset-4">
            Sign in
          </Link>{' '}
          and verify your Orien wallet to post an offer.
        </p>
      ) : !account.wallet ? (
        <p className="text-sm text-muted">
          <Link href="/marketplace/account/" className="font-semibold text-ink underline underline-offset-4">
            Verify your Orien wallet
          </Link>{' '}
          first, so buyers know your ORN are yours.
        </p>
      ) : (
        <>
          <div className="grid gap-4 sm:grid-cols-2">
            <label className="block space-y-2">
              <span className="text-sm font-semibold text-ink">ORN you sell</span>
              <input value={orn} onChange={(e) => setOrn(e.target.value)} inputMode="decimal" placeholder="500" className={field} />
            </label>
            <label className="block space-y-2">
              <span className="text-sm font-semibold text-ink">Bitcoin you want, in BTC</span>
              <input value={btc} onChange={(e) => setBtc(e.target.value)} inputMode="decimal" placeholder="0.01" className={field} />
            </label>
          </div>
          <p className="text-sm text-muted">
            At least 0.00005 BTC, more when Bitcoin is busy. Nothing is locked until someone takes your offer, and Orien Wallet
            must be open then to start the swap: you get an email when it happens.
          </p>
          {trouble && <Notice tone="bad">{trouble}</Notice>}
          {posted && <p className="text-sm text-go">Your offer is on the board.</p>}
          <button type="submit" className="pearl-fill h-11 rounded-xl px-6 text-sm font-bold">
            Post offer
          </button>
        </>
      )}
    </form>
  )
}
