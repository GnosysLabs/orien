import type { Metadata } from 'next'
import { OpenOnHash } from '@/components/OpenOnHash'
import Link from 'next/link'
import { ArrowRight, ChevronDown, Terminal } from 'lucide-react'
import { CopyText } from '@/components/CopyText'
import { WalletDownload } from '@/components/WalletDownload'
import { ReferralInvite } from '@/components/ReferralInvite'
import { SOURCE_CODE } from '@/lib/brand'
import { pageMeta } from '@/lib/seo'
import { SiteVideo } from '@/components/SiteVideo'

export const metadata: Metadata = pageMeta({
  title: 'Get started: download Orien Wallet and mine ORN',
  description:
    'Download Orien Wallet for Mac, Windows, Linux or iPhone. Make a quantum-proof wallet, mine ORN on your computer, and send and receive in a fraction of a second.',
  path: '/try/',
})

const code = 'rounded bg-raised px-1.5 py-0.5 text-sm text-ink'

/** The first steps, as they look in Orien Wallet. */
const STEPS: { title: string; body: React.ReactNode }[] = [
  {
    title: 'Make a wallet',
    body: (
      <>
        <p>
          Open Orien Wallet and choose <strong>Create a wallet</strong>. It shows you your <strong>recovery phrase</strong>
          : 24 words that bring your wallet back on any device. Write them on paper and keep them somewhere safe.
        </p>
        <p className="mt-3 rounded-xl border border-line bg-bg px-4 py-3 text-sm">
          Anyone who sees your 24 words can take your coins. Never type them into a website, and never give them to
          anyone.
        </p>
      </>
    ),
  },
  {
    title: 'Mine ORN',
    body: (
      <>
      <p>
        On your computer, press <strong>Mine</strong>, then <strong>Benchmark</strong> to find how many processor
        cores earn the most, or choose them yourself, and press <strong>Start mining</strong>. Every hour, that hour&apos;s new ORN are shared among everyone who mined, by the
        work each one did.
      </p>
      <SiteVideo name="how-to-mine" label="What is mining? Two minutes" className="mt-5" />
      </>
    ),
  },
  {
    title: 'Send and receive',
    body: (
      <p>
        <strong>Receive</strong> shows your address and a QR code to share. <strong>Send</strong> pays someone in a
        fraction of a second, for a fee of 0.1%, never more than 1 ORN. Payments can&apos;t be undone, so check the
        address first.
      </p>
    ),
  },
  {
    title: 'Add your other devices',
    body: (
      <p>
        Install Orien Wallet there too and choose <strong>Use my other device</strong> or{' '}
        <strong>Use my recovery phrase</strong>. It&apos;s the same wallet everywhere, with the same address and the
        same ORN.
      </p>
    ),
  },
]

export default function GetStartedPage() {
  return (
    <div className="mx-auto max-w-[760px]">
      <header className="pb-10 pt-5 text-center sm:pt-7">
        <p className="mb-4 text-xs font-semibold uppercase tracking-[0.2em] text-muted">Get started</p>
        <h1 className="text-balance text-4xl font-bold leading-[1.1] tracking-tight text-ink sm:text-5xl">
          From nothing to your first ORN
        </h1>
        <p className="mx-auto mt-6 max-w-xl text-lg leading-relaxed text-muted">
          Download Orien Wallet, make a wallet, and start mining. It takes a few minutes.
        </p>
      </header>

      <ReferralInvite />
      <WalletDownload />
      <p className="mt-4 text-center text-sm text-muted">
        <a href="/wallet/SHA256SUMS" className="underline decoration-line underline-offset-4 hover:text-ink">
          Download fingerprints (SHA-256)
        </a>
        <span className="mx-3 text-muted/50" aria-hidden>
          ·
        </span>
        <a href={SOURCE_CODE} target="_blank" rel="noreferrer" className="underline decoration-line underline-offset-4 hover:text-ink">
          Source code
        </a>
      </p>

      <h2 className="mb-6 mt-16 text-2xl font-bold tracking-tight text-ink">Your first few minutes</h2>
      <ol className="divide-y divide-line rounded-2xl border border-line bg-surface">
        {STEPS.map(({ title, body }, index) => (
          <li key={title} className="flex gap-5 p-6 sm:p-8">
            <span className="flex h-9 w-9 shrink-0 items-center justify-center rounded-full bg-raised text-sm font-bold text-ink">
              {index + 1}
            </span>
            <div className="min-w-0 flex-1 pt-1">
              <h3 className="text-lg font-bold tracking-tight text-ink">{title}</h3>
              <div className="mt-2 leading-relaxed text-muted [&_strong]:font-semibold [&_strong]:text-ink">{body}</div>
            </div>
          </li>
        ))}
      </ol>

      <OpenOnHash id="cli" />
      <details id="cli" className="group mt-16 scroll-mt-24 rounded-2xl border border-line bg-surface">
        <summary className="flex cursor-pointer list-none items-center gap-4 p-6 transition hover:bg-raised sm:px-8 [&::-webkit-details-marker]:hidden group-open:rounded-t-2xl rounded-2xl">
          <Terminal className="h-5 w-5 shrink-0 text-pearl" strokeWidth={1.75} aria-hidden />
          <span className="min-w-0 flex-1">
            <span className="block font-bold text-ink">Prefer the command line?</span>
            <span className="block text-sm text-muted">
              The orien program does everything in a terminal, and runs validators.
            </span>
          </span>
          <ChevronDown className="h-5 w-5 shrink-0 text-muted transition group-open:rotate-180" aria-hidden />
        </summary>

        <div className="space-y-10 border-t border-line p-6 leading-relaxed text-muted sm:p-8 [&_strong]:font-semibold [&_strong]:text-ink">
          <div className="space-y-4">
            <h3 className="text-lg font-bold tracking-tight text-ink">Install orien</h3>
            <p>Paste one line into a terminal. It picks the right version for your computer and checks it first.</p>
            <CopyText label="Mac and Linux" text="curl -fsSL https://orien.network/install.sh | sh" />
            <CopyText label="Windows (PowerShell)" text="irm https://orien.network/install.ps1 | iex" />
            <p className="text-sm">
              Then open a new terminal window. orien uses the same wallet as Orien Wallet, so you can use both.
            </p>
          </div>

          <div className="space-y-4">
            <h3 className="text-lg font-bold tracking-tight text-ink">Everyday commands</h3>
            <CopyText label="Make a wallet" text="orien wallet new" />
            <CopyText label="Restore one with its 24 words" text="orien wallet restore" />
            <CopyText label="Find how many cores earn the most, once" text="orien benchmark" />
            <CopyText label="Mine (press Ctrl-C to stop)" text="orien mine" />
            <CopyText label="Your balance" text="orien balance" />
            <CopyText label="Your address, to share with whoever pays you" text="orien wallet address" />
            <CopyText label="Pay someone" text="orien send --to THEIR_ADDRESS --amount 5" />
            <CopyText label="Update, only to releases signed by Orien" text="orien update" />
          </div>

          <div className="space-y-4">
            <h3 className="text-lg font-bold tracking-tight text-ink">Run a validator</h3>
            <p>
              Validators check payments and earn the network&apos;s fees, shared by stake. You need at least{' '}
              <strong>10,000 ORN</strong> to stake, and a server that stays on, ideally running Linux, with your wallet
              on it.
            </p>
            <CopyText label="Make the validator’s key, on the server" text="orien keygen ~/.orien/validator.key" />
            <CopyText
              label="Start it, at an address other validators can reach"
              text="orien node -k ~/.orien/validator.key -d ~/.orien/validator --listen 0.0.0.0:7101"
            />
            <CopyText
              label="Stake behind it, on the same server"
              text="orien stake --amount 10000 --validator-key ~/.orien/validator.key --address YOUR_SERVER_IP:7101"
            />
            <p className="text-sm">
              It keeps itself up to date with each signed release, restarting at a calm moment. To update it yourself
              instead, add <code className={code}>--no-auto-update</code>. At the next hourly checkpoint, if its stake is
              among the largest, it joins the validators. To stop, run{' '}
              <code className={code}>orien unstake --amount 10000</code>: the ORN come back after a week.
            </p>
          </div>
        </div>
      </details>

      <div className="mb-8 mt-6 flex flex-wrap items-center justify-between gap-4 rounded-2xl border border-line bg-surface p-6">
        <p className="font-semibold text-ink">Curious what happens under the hood?</p>
        <Link
          href="/how-it-works/"
          className="flex h-11 items-center gap-2 rounded-xl border border-line bg-bg px-5 text-sm font-semibold text-ink transition hover:border-pearl"
        >
          How it works <ArrowRight className="h-4 w-4" aria-hidden />
        </Link>
      </div>
    </div>
  )
}
