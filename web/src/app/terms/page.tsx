import type { Metadata } from 'next'
import Link from 'next/link'
import { OPERATOR } from '@/lib/brand'
import { pageMeta } from '@/lib/seo'

export const metadata: Metadata = pageMeta({
  title: 'Terms of Service',
  description:
    'The terms for using orien.network, Orien Wallet and an Orien Network account: what is yours, the Market rules, and what we do about abuse.',
  path: '/terms/',
})

function Section({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <section className="space-y-3">
      <h2 className="text-xl font-bold tracking-tight text-ink">{title}</h2>
      <div className="space-y-3 leading-relaxed text-muted [&_a]:font-semibold [&_a]:text-ink [&_a]:underline [&_a]:underline-offset-4 [&_strong]:font-semibold [&_strong]:text-ink [&_ul]:list-disc [&_ul]:space-y-1.5 [&_ul]:pl-5">
        {children}
      </div>
    </section>
  )
}

export default function TermsPage() {
  return (
    <div className="mx-auto max-w-[760px] space-y-10 pb-16">
      <header className="pt-5 sm:pt-7">
        <p className="mb-4 text-xs font-semibold uppercase tracking-[0.2em] text-muted">Terms of Service</p>
        <h1 className="text-balance text-4xl font-bold leading-[1.1] tracking-tight text-ink sm:text-5xl">
          The terms, in plain words
        </h1>
        <p className="mt-6 text-lg leading-relaxed text-muted">
          These terms cover orien.network, Orien Wallet, the orien program and your Orien Network account. They are run
          by {OPERATOR} (&ldquo;we&rdquo;). By using any of them you agree to these terms and to our{' '}
          <Link href="/privacy/" className="font-semibold text-ink underline underline-offset-4">
            Privacy Policy
          </Link>
          .
        </p>
      </header>

      <Section title="Who can use it">
        <p>
          You must be at least 18, and allowed to use cryptocurrency where you live. You are responsible for following
          the laws that apply to you, including on taxes.
        </p>
      </Section>

      <Section title="Your wallet is yours alone">
        <p>
          Orien Wallet and the orien program are self-custodial: your keys and your 24-word recovery phrase are made and
          kept on your own devices, and never sent to us. That means only you can move your ORN, and also that{' '}
          <strong>nobody, including us, can recover a lost recovery phrase, undo a payment or restore coins sent to the
          wrong address.</strong> Keep your recovery phrase written down somewhere safe, and never share it with anyone.
          We will never ask for it.
        </p>
      </Section>

      <Section title="Your Orien Network account">
        <p>
          An Orien Network account, made by signing in with a code sent to your email, gives you a profile, the Market,
          messages and the swap board. You can have it without holding ORN, and hold ORN without it. Keep your email
          secure: anyone who can read it can sign in as you. You can delete your account at any time from Your account;
          that deletes your profile, listings, pictures and messages, and never touches your wallet or coins.
        </p>
        <p>
          Your username must not pretend to be someone else, and we may change or take back a name that misleads,
          offends or infringes someone&apos;s rights.
        </p>
      </Section>

      <Section title="Rules for the Market and messages">
        <p>
          We have <strong>zero tolerance</strong> for objectionable content and abusive people. You must not post,
          offer, send or ask for:
        </p>
        <ul>
          <li>anything illegal, or goods and services that are illegal where you or the other person are;</li>
          <li>scams, fake listings, or attempts to take payment without delivering;</li>
          <li>harassment, threats, bullying or hate against anyone;</li>
          <li>sexual content involving minors, of any kind, which we also report to the authorities;</li>
          <li>sexually explicit or violent material;</li>
          <li>pretending to be another person, business or Orien itself;</li>
          <li>spam, or someone else&apos;s private information or work without their permission.</li>
        </ul>
        <p>
          Everything posted, including listings, names, bios, pictures and messages, is checked automatically before
          anyone else sees it, and is refused if it breaks these rules.
        </p>
      </Section>

      <Section title="Reporting and blocking">
        <p>
          Every listing and profile has a Report button, and you can block anyone from their profile or a conversation:
          they then cannot message you, and you no longer see their listings. We read every report and
          act on it, usually within a day, by removing the content, and by suspending or closing the accounts of people
          who break these rules. Serious or repeated abuse means a permanent ban.
        </p>
      </Section>

      <Section title="Payments and trades are between you and the other person">
        <p>
          Payments in ORN go straight from one wallet to another on the Orien network. A payment is final the moment
          the network confirms it: it cannot be reversed or refunded by us or anyone else. When you buy or sell on the
          Market, or swap ORN for SOL or Bitcoin on the swap board, the deal is between you and the other person.{' '}
          {OPERATOR} is not a party to it, does not hold your coins or theirs, and cannot settle disputes or get money
          back. Agree the details in messages first, and only pay people you are willing to trust.
        </p>
        <p>
          Swaps are carried out by the two wallets themselves, and either both sides are paid or both get their coins
          back. Fees charged by other networks, such as Bitcoin or Solana, are yours to pay.
        </p>
      </Section>

      <Section title="Cryptocurrency carries risk">
        <p>
          ORN is a new cryptocurrency. Its value can fall to nothing, and nothing on this website is financial advice. Only
          use what you can afford to lose.
        </p>
      </Section>

      <Section title="No warranty">
        <p>
          The website, Orien Wallet, the orien program and the Orien network are provided &ldquo;as is&rdquo; and
          &ldquo;as available&rdquo;, without warranties of any kind. The software is open source and may have bugs, and
          the network or any of our services may be unavailable at times.
        </p>
      </Section>

      <Section title="Limitation of liability">
        <p>
          As far as the law allows, {OPERATOR} is not liable for any loss of coins, profits or data, or for any indirect
          or consequential loss, arising from your use of any of these services, including losses caused by lost
          recovery phrases, mistaken payments, other users, or bugs. Where the law doesn&apos;t let liability be excluded
          completely, it is limited to what you paid {OPERATOR} in the twelve months before the claim, which for our
          free services is nothing.
        </p>
      </Section>

      <Section title="Which laws apply">
        <p>
          {OPERATOR} is registered in the United States, in California. These terms are governed by the laws of the
          State of California and of the United States, and any dispute about them is for the state or federal courts
          in California, unless the law where you live gives you the right to bring it there.
        </p>
      </Section>

      <Section title="Changes to these terms">
        <p>
          We may change these terms as Orien grows. The latest version is always on this page, and important changes are
          also noted in the{' '}
          <Link href="/changelog/">changelog</Link>. If you keep using the services after a change, you accept the new
          terms; if you don&apos;t, you can delete your account at any time.
        </p>
      </Section>

      <Section title="Contact">
        <p>
          Questions, reports or anything else: <Link href="/contact/">write to us</Link>.
        </p>
      </Section>
    </div>
  )
}
