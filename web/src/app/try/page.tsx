import type { Metadata } from 'next'
import { WindowsDownload } from '@/components/WindowsDownload'
import Link from 'next/link'
import { ArrowRight, Bot, Download, KeyRound, Monitor, Pickaxe, RefreshCw, Send, ServerCog, Wallet } from 'lucide-react'
import { CopyText } from '@/components/CopyText'
import { pageMeta } from '@/lib/seo'

export const metadata: Metadata = pageMeta({
  title: 'Get started: download Orien Wallet and mine ORN',
  description:
    'Download Orien Wallet for Mac, Windows or Linux, or install the orien command line. Make a quantum-proof wallet, mine ORN on your computer, send and receive, and run a validator.',
  path: '/try/',
})

function Step({
  number,
  icon: Icon,
  title,
  children,
}: {
  number: number
  icon: typeof Wallet
  title: string
  children: React.ReactNode
}) {
  return (
    <section className="relative rounded-2xl border border-line bg-surface p-6 sm:p-8">
      <div className="mb-4 flex items-center gap-3">
        <span className="flex h-9 w-9 shrink-0 items-center justify-center rounded-full bg-raised text-sm font-bold text-ink">
          {number}
        </span>
        <Icon className="h-5 w-5 text-pearl" strokeWidth={1.75} aria-hidden />
        <h2 className="text-xl font-bold tracking-tight text-ink">{title}</h2>
      </div>
      <div className="space-y-4 leading-relaxed text-muted [&_strong]:font-semibold [&_strong]:text-ink">{children}</div>
    </section>
  )
}

function Note({ children }: { children: React.ReactNode }) {
  return <p className="rounded-xl border border-line bg-bg px-4 py-3 text-sm">{children}</p>
}

export default function GetStartedPage() {
  return (
    <div className="mx-auto max-w-[760px]">
      <header className="pb-6 pt-10 sm:pt-14">
        <p className="mb-4 text-xs font-semibold uppercase tracking-[0.2em] text-muted">Get started</p>
        <h1 className="text-balance text-4xl font-bold leading-[1.1] tracking-tight text-ink sm:text-5xl">
          From nothing to your first ORN
        </h1>
        <p className="mt-6 text-lg leading-relaxed text-muted">
          Everything happens in <strong className="font-semibold text-ink">orien</strong>, one small program for Mac,
          Linux and Windows. Install it, make a wallet, and start mining in a few minutes.
        </p>
      </header>

      <section className="mb-6 rounded-2xl border border-line bg-surface p-6 sm:p-8">
        <div className="mb-4 flex items-center gap-3">
          <Monitor className="h-5 w-5 text-pearl" strokeWidth={1.75} aria-hidden />
          <h2 className="text-xl font-bold tracking-tight text-ink">Or use Orien Wallet</h2>
        </div>
        <div className="space-y-4 leading-relaxed text-muted">
          <p>
            Orien Wallet is a desktop app: make a wallet with its 24-word recovery phrase, see your balance, send and
            receive ORN, keep an address book, and swap ORN for Bitcoin with anyone, with no one in the middle. It uses
            the same wallet as orien, so you can use both. It updates itself.
          </p>
          <div className="flex flex-wrap items-center gap-4">
            <a
              href="/wallet/Orien-Wallet.dmg"
              className="pearl-fill flex h-11 items-center gap-2 rounded-xl px-5 text-sm font-bold transition hover:brightness-105"
            >
              <Download className="h-4 w-4" aria-hidden /> Download for Mac
            </a>
            <WindowsDownload className="pearl-fill flex h-11 items-center gap-2 rounded-xl px-5 text-sm font-bold transition hover:brightness-105">
              <Download className="h-4 w-4" aria-hidden /> Download for Windows
            </WindowsDownload>
            <a
              href="/wallet/Orien-Wallet.AppImage"
              className="pearl-fill flex h-11 items-center gap-2 rounded-xl px-5 text-sm font-bold transition hover:brightness-105"
            >
              <Download className="h-4 w-4" aria-hidden /> Download for Linux
            </a>
            <a href="/wallet/SHA256SUMS" className="text-sm underline decoration-line underline-offset-4 hover:text-ink">
              Download fingerprints (SHA-256)
            </a>
          </div>
          <p className="text-sm">
            On a Mac, open the downloaded file and drag Orien Wallet into Applications. It needs a Mac with an Apple
            chip.
          </p>
          <p className="text-sm">
            Windows may say it protected your PC from an unrecognised app. Choose <strong className="font-semibold text-ink">More info</strong>,
            then <strong className="font-semibold text-ink">Run anyway</strong>.
          </p>
          <p className="text-sm">
            On Linux, allow the file to run (right-click it, open Properties, and turn on running it as a program), then
            double-click it. Afterwards Orien Wallet is in your app menu. If it does not open, your system needs FUSE:{' '}
            <code className="rounded bg-raised px-1.5 py-0.5 text-ink">sudo apt install libfuse2</code>, or{' '}
            <code className="rounded bg-raised px-1.5 py-0.5 text-ink">libfuse2t64</code> on Ubuntu 24.04 and newer.
          </p>
        </div>
      </section>

      <div className="space-y-6">
        <Step number={1} icon={Download} title="Install orien">
          <p>Open a terminal and paste one line. It picks the right version for your computer and checks it before installing.</p>
          <CopyText label="Mac and Linux" text="curl -fsSL https://orien.network/install.sh | sh" />
          <CopyText label="Windows (PowerShell)" text="irm https://orien.network/install.ps1 | iex" />
          <p className="text-sm">Then open a new terminal window, so it can find orien.</p>
        </Step>

        <Step number={2} icon={Wallet} title="Make a wallet">
          <CopyText text="orien wallet new" />
          <p>
            You get an address to be paid at, and a <strong>recovery phrase</strong>: 24 words that can rebuild your
            wallet on any computer. Write them on paper and keep them safe. You can also choose a password, so that a
            copy of your wallet file is useless to anyone else.
          </p>
          <Note>
            Anyone who sees your 24 words can take your coins. Never type them into a website, and never give them to
            anyone, an AI assistant included.
          </Note>
        </Step>

        <Step number={3} icon={Pickaxe} title="Mine ORN">
          <CopyText text="orien mine" />
          <p>
            It first asks how much of your processor to use. Choose less on a computer you also work on, so it stays
            quick, and 100% on one that does nothing else. Every hour, the new ORN released in that hour are shared out
            among everyone who mined, in proportion to the work they did. It keeps going until you press Ctrl-C.
          </p>
        </Step>

        <Step number={4} icon={Send} title="Check your balance, send and receive">
          <CopyText label="Your balance" text="orien balance" />
          <CopyText label="Your address, to share with whoever pays you" text="orien wallet address" />
          <CopyText label="Pay someone" text="orien send --to THEIR_ADDRESS --amount 5" />
          <p>
            A payment is final in a fraction of a second. It pays a fee of 0.1% of the amount, never more than 1 ORN.
            Payments cannot be undone, so check the address before you send.
          </p>
        </Step>

        <Step number={5} icon={Bot} title="Let your AI assistant do it">
          <CopyText text="orien skill install" />
          <p>
            This teaches <strong>Claude Code</strong>, <strong>Codex</strong> and any other AI assistant that reads
            skills from <code className="rounded bg-raised px-1.5 py-0.5 text-ink">~/.agents/skills</code> to use orien
            for you. Start a new conversation and ask in plain words: “what’s my ORN balance?”, “send 5 ORN to Bob”, “start mining on this
            computer”. It asks before every payment, and never sees your 24 words or your password.
          </p>
          <Note>
            The assistant has to run on your own computer, as Claude Code and Codex do, because it works by running
            orien there. Chats on claude.ai, chatgpt.com and their apps run on the provider’s servers, so they cannot use
            orien or reach your wallet.
          </Note>
          <p>
            If your wallet has a password, let the assistant send for a while, up to an amount you choose. It can then
            send without ever learning the password:
          </p>
          <CopyText label="For the next hour, up to 50 ORN" text="orien unlock --for 1h --up-to 50" />
          <p className="text-sm">
            End it early with <code className="rounded bg-raised px-1.5 py-0.5 text-ink">orien lock</code>. For an
            assistant that keeps its skills somewhere else,{' '}
            <code className="rounded bg-raised px-1.5 py-0.5 text-ink">orien skill install --dir FOLDER</code> puts the
            skill in that folder.
          </p>
        </Step>

        <Step number={6} icon={KeyRound} title="Use a second computer">
          <CopyText text="orien wallet restore" />
          <p>
            Type your 24 words when asked. It’s the same wallet on both computers, with the same address and the same
            ORN, so you can send from either one.
          </p>
        </Step>

        <Step number={7} icon={RefreshCw} title="Stay up to date">
          <CopyText text="orien update" />
          <p>It installs a newer version only if it carries the Orien release signature, so nobody can slip in a fake one. It updates the AI skill too.</p>
        </Step>

        <Step number={8} icon={ServerCog} title="Run a validator">
          <p>
            Validators check payments and earn the network’s fees, shared by stake. You need at least{' '}
            <strong>10,000 ORN</strong> to stake, and a server that stays on, ideally running Linux.
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
            Staking needs your wallet on the server too (step 6 shows how), holding the ORN you
            stake.
          </p>
          <p className="text-sm">
            The validator keeps itself up to date: it installs each new release signed with the Orien release key, and
            restarts on it at a calm moment, so it always follows the network&apos;s rules. To update it yourself instead,
            add <code className="rounded bg-raised px-1.5 py-0.5 text-ink">--no-auto-update</code>.
          </p>
          <p>
            At the next hourly checkpoint, if its stake is among the largest, it joins the validators. To stop, unstake
            with <code className="rounded bg-raised px-1.5 py-0.5 text-ink">orien unstake --amount 10000</code>: the ORN
            come back after a week.
          </p>
        </Step>
      </div>

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
