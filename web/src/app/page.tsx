import Link from 'next/link'
import { ArrowLeftRight, ArrowRight, Gauge, Pickaxe, Scale, ServerCog, ShieldCheck, Sunrise, Wallet } from 'lucide-react'
import type { Metadata } from 'next'
import { SupplyCurve } from '@/components/SupplyCurve'
import { IPhoneApp } from '@/components/IPhoneApp'

export const metadata: Metadata = {
  title: { absolute: 'Orien: the quantum-proof cryptocurrency, final in a blink' },
  alternates: { canonical: '/' },
}

const BENEFITS = [
  {
    icon: Gauge,
    title: 'Final in a blink',
    body: 'A payment is final in about a sixth of a second across the internet. No blocks to wait for, no "confirmations", and never stuck behind anyone else’s payment.',
  },
  {
    icon: ShieldCheck,
    title: 'Ready for quantum computers',
    body: 'Quantum computers will break the signatures most cryptocurrencies rely on today. Orien’s signatures rest on one well-studied building block, the BLAKE3 hash, which they cannot break that way.',
  },
  {
    icon: Sunrise,
    title: 'A fair launch from nothing',
    body: 'No pre-mine, no sale, no founder’s share. The network started with zero coins, and every coin is earned. Its start was tied to a Bitcoin block announced in advance, so nobody could mine before everyone else.',
  },
  {
    icon: Scale,
    title: 'A small, fair fee',
    body: '0.1% of each payment, never more than 1 coin. The same share whatever a coin is worth. Fees go to the validators who keep the network running.',
  },
]

const WAYS_IN = [
  {
    icon: Pickaxe,
    title: 'Mine',
    body: 'Lend your computer’s spare power. Every hour, the new coins are shared out among everyone who did work in that hour.',
  },
  {
    icon: ArrowLeftRight,
    title: 'Swap',
    body: 'Buy ORN with SOL, or sell yours for SOL, directly with another person in about a minute. Either both of you get paid, or both get your coins back.',
  },
  {
    icon: ServerCog,
    title: 'Run a validator',
    body: 'Stake 10,000 coins behind a server that checks payments. Validators earn the network’s fees, shared by stake.',
  },
  {
    icon: Wallet,
    title: 'Pay and get paid',
    body: 'Make a wallet, send and receive. Each payment comes with a receipt anyone can check for themselves.',
  },
]

export default function HomePage() {
  return (
    <div className="mx-auto max-w-[1040px]">
      {/* Hero */}
      <section className="pb-14 pt-10 text-center sm:pb-20 sm:pt-16">
        <p className="mb-5 text-xs font-semibold uppercase tracking-[0.2em] text-muted">Quantum-proof cryptocurrency</p>
        <h1 className="mx-auto max-w-3xl text-balance text-4xl font-bold leading-[1.1] tracking-tight text-ink sm:text-6xl">
          The cryptocurrency that settles in a blink, <span className="pearl-text bg-clip-text text-transparent">built for the quantum age.</span>
        </h1>
        <p className="mx-auto mt-6 max-w-2xl text-base leading-relaxed text-muted sm:text-lg">
          Orien is a new cryptocurrency. Payments are final in a fraction of a second, they never wait in line,
          and every signature is designed to stay safe when quantum computers arrive.
        </p>
        <IPhoneApp />
        <div className="mt-8 flex justify-center">
          <Link
            href="/try/"
            className="pearl-fill flex h-11 items-center gap-2 rounded-xl px-5 text-sm font-bold transition hover:brightness-105"
          >
            Get started <ArrowRight className="h-4 w-4" aria-hidden />
          </Link>
        </div>
      </section>

      {/* Benefits */}
      <section aria-label="Why Orien" className="grid gap-4 sm:grid-cols-2">
        {BENEFITS.map(({ icon: Icon, title, body }) => (
          <div key={title} className="rounded-2xl border border-line bg-surface p-6">
            <Icon className="mb-4 h-6 w-6 text-pearl" strokeWidth={1.75} aria-hidden />
            <h2 className="mb-2 text-lg font-semibold text-ink">{title}</h2>
            <p className="text-sm leading-relaxed text-muted">{body}</p>
          </div>
        ))}
      </section>

      {/* What makes it different */}
      <section className="mx-auto max-w-2xl py-20 text-center">
        <h2 className="text-2xl font-bold tracking-tight text-ink sm:text-3xl">No blocks. No waiting in line.</h2>
        <p className="mt-5 leading-relaxed text-muted">
          Most cryptocurrencies put every payment in the world into one queue and settle them a block at a time.
          Orien doesn’t. Each account numbers its own payments, and validators sign off on each one as it arrives. Once
          an hour they pause for a moment to agree on a snapshot of every balance. That is the only time they need to
          agree on anything.
        </p>
        <Link href="/how-it-works/" className="mt-6 inline-flex items-center gap-1.5 text-sm font-semibold text-pearl hover:underline">
          Read how it works <ArrowRight className="h-4 w-4" aria-hidden />
        </Link>
      </section>

      {/* The coin */}
      <section className="grid items-center gap-10 rounded-3xl border border-line bg-surface p-6 sm:p-10 md:grid-cols-[1fr_1.1fr]">
        <div>
          <p className="mb-3 text-xs font-semibold uppercase tracking-wider text-muted">The coin</p>
          <h2 className="text-2xl font-bold tracking-tight text-ink sm:text-3xl">100 million coins. Ever.</h2>
          <ul className="mt-5 space-y-3 text-sm leading-relaxed text-muted">
            <li>
              <span className="font-semibold text-ink">Zero at launch.</span> Nothing is set aside for anyone, the
              founders included.
            </li>
            <li>
              <span className="font-semibold text-ink">Half in the first four years.</span> Then half of what remains
              every four years after: about 1,977 coins in the very first hour.
            </li>
            <li>
              <span className="font-semibold text-ink">Earned by work.</span> Each hour’s new coins go to everyone who
              mined in that hour, in proportion to the work they did. Work only creates coins; it never decides payments.
            </li>
            <li>
              <span className="font-semibold text-ink">Nine decimal places.</span> Small payments stay easy however
              valuable a coin becomes.
            </li>
          </ul>
        </div>
        <SupplyCurve />
      </section>

      {/* Ways in */}
      <section className="py-20">
        <h2 className="text-center text-2xl font-bold tracking-tight text-ink sm:text-3xl">Four ways to take part</h2>
        <div className="mt-8 grid gap-4 sm:grid-cols-2">
          {WAYS_IN.map(({ icon: Icon, title, body }) => (
            <div key={title} className="rounded-2xl border border-line bg-surface p-6">
              <Icon className="mb-4 h-6 w-6 text-pearl" strokeWidth={1.75} aria-hidden />
              <h3 className="mb-2 font-semibold text-ink">{title}</h3>
              <p className="text-sm leading-relaxed text-muted">{body}</p>
            </div>
          ))}
        </div>
      </section>

      {/* Closing invitation */}
      <section className="mb-6 rounded-3xl border border-line bg-surface px-6 py-12 text-center sm:px-10">
        <h2 className="text-balance text-2xl font-bold tracking-tight text-ink sm:text-3xl">
          Every coin is earned. Start earning yours.
        </h2>
        <p className="mx-auto mt-4 max-w-xl text-sm leading-relaxed text-muted">
          Make a wallet and start mining in a few minutes, on any computer.
        </p>
        <div className="mt-7 flex flex-wrap items-center justify-center gap-3">
          <Link
            href="/try/"
            className="pearl-fill flex h-11 items-center gap-2 rounded-xl px-5 text-sm font-bold transition hover:brightness-105"
          >
            Get started <ArrowRight className="h-4 w-4" aria-hidden />
          </Link>
          <Link
            href="/how-it-works/"
            className="flex h-11 items-center rounded-xl border border-line bg-bg px-5 text-sm font-semibold text-ink transition hover:border-pearl"
          >
            How it works
          </Link>
        </div>
      </section>
    </div>
  )
}
