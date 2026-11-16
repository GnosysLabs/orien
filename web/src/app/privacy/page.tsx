import type { Metadata } from 'next'
import { OPERATOR } from '@/lib/brand'
import { pageMeta } from '@/lib/seo'

export const metadata: Metadata = pageMeta({
  title: 'Privacy',
  description:
    'What orien.network keeps about you, why, and how to delete it.',
  path: '/privacy/',
})

function Section({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <section className="space-y-3">
      <h2 className="text-xl font-bold tracking-tight text-ink">{title}</h2>
      <div className="space-y-3 leading-relaxed text-muted [&_strong]:font-semibold [&_strong]:text-ink [&_ul]:list-disc [&_ul]:space-y-1.5 [&_ul]:pl-5">
        {children}
      </div>
    </section>
  )
}

export default function PrivacyPage() {
  return (
    <div className="mx-auto max-w-[760px] space-y-10 pb-16">
      <header className="pt-10 sm:pt-14">
        <p className="mb-4 text-xs font-semibold uppercase tracking-[0.2em] text-muted">Privacy</p>
        <h1 className="text-balance text-4xl font-bold leading-[1.1] tracking-tight text-ink sm:text-5xl">
          What we keep, and why
        </h1>
        <p className="mt-6 text-lg leading-relaxed text-muted">
          orien.network is run by {OPERATOR}. We keep as little as we can, never sell it, and show no ads.
        </p>
      </header>

      <Section title="Reading the website">
        <p>
          Reading these pages needs no account and sets no cookies. The website does not record who you are or follow you
          elsewhere.
        </p>
        <p>
          To see how many people visit, each page you open is counted: which page, the site that sent you, your country
          and whether you are on a phone or a computer. To count each visitor once a day, your internet address and
          browser are scrambled with a code that is thrown away at the end of the day; the address itself is never kept.
          Only the day&apos;s totals remain. Nobody else sees them.
        </p>
      </Section>

      <Section title="Writing to us">
        <p>
          The contact form emails your name, your email address and your message to us, so we can reply. The website
          keeps no copy.
        </p>
      </Section>

      <Section title="Notifications on iPhone">
        <p>
          If you turn on notifications in Orien Wallet on iPhone, the app sends us the code Apple gave your phone for
          notifications and the addresses of the wallets in the app, so we can tell you when one receives ORN. What an
          address receives is public on the Orien network anyway. Turning notifications off deletes both; if you
          remove the app instead, they are deleted once Apple tells us the phone no longer has it.
        </p>
      </Section>

      <Section title="Your Market account">
        <p>When you use the Market or the swap board, we keep:</p>
        <ul>
          <li>
            <strong>Your email address,</strong> to send you sign-in codes and tell you about new messages and taken swap
            offers. Nobody else sees it.
          </li>
          <li>
            <strong>What you choose to show:</strong> your name, picture, bio, the ways to reach you that you add, your
            listings and swap offers, and the Orien address you are paid at. Everyone can see these.
          </li>
          <li>
            <strong>Whether your wallet is verified.</strong> If you verify the wallet you are paid at, by sending a tiny
            amount from it to Orien Market, your name and picture show beside that address in other people&apos;s Orien
            Wallet, for example when they pay you. Use a different wallet to stop it.
          </li>
          <li>
            <strong>Your messages,</strong> so both of you can read them. We do not read them as a matter of course, but we
            can, for example to look into a report of a scam.
          </li>
          <li>
            <strong>The names and addresses of the wallets in your Orien Wallet apps,</strong> when you sign in to the
            Market from them, so the swap board can let you choose which wallet sells. Only you see them.
          </li>
          <li>
            <strong>Reports you make,</strong> and what we did about them.
          </li>
          <li>
            <strong>One cookie,</strong> which keeps you signed in. Its contents mean nothing to anyone but our server.
          </li>
        </ul>
      </Section>

      <Section title="Who else sees some of it">
        <ul>
          <li>
            <strong>Resend</strong> delivers our emails, so it handles your email address and the emails we send you.
          </li>
          <li>
            <strong>OpenRouter</strong> and the AI model it runs check everything posted before anyone else sees it:
            listings, names, bios, pictures and messages. They see what is checked, not who you are.
          </li>
          <li>
            Payments in ORN are on the Orien network, which is public by design: anyone can see an address&apos;s balance
            and payments. That is true of every cryptocurrency, and not something this website controls.
          </li>
        </ul>
      </Section>

      <Section title="Deleting it">
        <p>
          You can delete your account at any time, from the bottom of Your account. That deletes your profile, listings,
          pictures, conversations, messages and swap offers. Your Orien wallet and coins are yours alone and are never
          touched. Payments already made on the Orien network cannot be undone by anyone.
        </p>
      </Section>
    </div>
  )
}
