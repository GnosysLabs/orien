import type { Metadata } from 'next'
import Link from 'next/link'
import { Boxes, Network, Play, PlugZap, ShieldAlert, Terminal } from 'lucide-react'
import { CopyText } from '@/components/CopyText'
import { Wordmark } from '@/components/Wordmark'

export const metadata: Metadata = {
  title: 'Try the CLI & Architecture',
  description:
    'Run a four-validator Orien devnet, send sub-second quantum-proof payments, verify offline receipts, and test split double-spends and interrupted wallets.',
  alternates: { canonical: '/try/' },
}

const CRATES = [
  {
    name: 'crates/crypto',
    title: 'BLAKE3 primitives',
    body: 'Hash-based one-time owner signatures (1,248 B), validator Merkle key trees (2^height batches), and batch Merkle inclusion proofs.',
  },
  {
    name: 'crates/ledger',
    title: 'Pure protocol & agreement rules',
    body: 'Orders, votes, certificates, confirmations, Merkle state roots, Tendermint-style checkpoint close logic, spare-key cancellation, and adversarial simulators.',
  },
  {
    name: 'crates/net',
    title: 'Validator node, journal & wallet client',
    body: 'Validator TCP server, per-epoch atomic storage, fsynced close journal, peer log reconciliation, wallet key streams, and checkpoint following.',
  },
  {
    name: 'crates/cli',
    title: 'The orien command',
    body: 'Wallet management, sending and resuming payments, offline receipt verification, node runner, local devnet, and double-spend / lost-device fault injection.',
  },
]

export default function TryPage() {
  return (
    <div className="mx-auto flex max-w-5xl flex-col gap-6">
      <header className="flex flex-col gap-2">
        <span className="text-xs font-bold uppercase tracking-widest text-pearl">Hands-on walkthrough</span>
        <h1 className="text-3xl font-black tracking-tight sm:text-4xl">
          Try <Wordmark showMark={false} className="text-[0.92em]" /> on a local devnet
        </h1>
        <p className="max-w-3xl text-muted">
          Everything builds into a single <code className="num text-ink">orien</code> Rust binary — validator node,
          wallet, offline receipt verifier, and fault-injection scenarios.
        </p>
      </header>

      {/* Step 1: Build & start */}
      <section className="rounded-2xl border border-line bg-surface p-5 sm:p-6">
        <h2 className="mb-3 flex items-center gap-2.5 text-xl font-bold">
          <span className="pearl-fill grid h-8 w-8 shrink-0 place-items-center rounded-lg">
            <Play className="h-4 w-4" aria-hidden />
          </span>
          1. Build and start four local validators
        </h2>
        <p className="mb-4 text-sm leading-relaxed text-ink/90">
          Hashing dominates everything Orien does, so build with <code className="num">--release</code>. Initializing
          creates four validator key trees, test wallets (<code className="num">alice</code>,{' '}
          <code className="num">bob</code>, <code className="num">carol</code>) and{' '}
          <code className="num">devnet/genesis.json</code>:
        </p>
        <div className="flex flex-col gap-3">
          <CopyText label="Build the Rust workspace:" text="cargo build --release" />
          <CopyText label="Create a 4-validator local network:" text="./target/release/orien devnet init" />
          <CopyText label="Run all four validators in this terminal:" text="./target/release/orien devnet run" />
        </div>
      </section>

      {/* Step 2: Send & verify */}
      <section className="rounded-2xl border border-line bg-surface p-5 sm:p-6">
        <h2 className="mb-3 flex items-center gap-2.5 text-xl font-bold">
          <span className="pearl-fill grid h-8 w-8 shrink-0 place-items-center rounded-lg">
            <Terminal className="h-4 w-4" aria-hidden />
          </span>
          2. Send a payment and verify its receipt offline
        </h2>
        <p className="mb-4 text-sm leading-relaxed text-ink/90">
          In a second terminal, point <code className="num">ORIEN_GENESIS</code> at the local genesis file, send coins
          from Alice to Carol, and verify the resulting 12 KB receipt completely offline:
        </p>
        <div className="flex flex-col gap-3">
          <CopyText text="export ORIEN_GENESIS=devnet/genesis.json" />
          <CopyText text="./target/release/orien balance devnet/wallets/alice.wallet" />
          <CopyText text="./target/release/orien send -w devnet/wallets/alice.wallet --to devnet/wallets/carol.wallet --amount 250 --receipt payment.receipt" />
          <CopyText
            label="Verify using only genesis.json — no network calls:"
            text="./target/release/orien verify payment.receipt"
          />
          <CopyText
            label="Inspect every validator's ledger fingerprint:"
            text="./target/release/orien status"
          />
        </div>
        <p className="mt-3 text-xs leading-relaxed text-muted">
          <code className="num text-ink">orien status</code> ends each validator&apos;s line with a BLAKE3 fingerprint
          of every balance it holds. Matching fingerprints prove identical ledgers.
        </p>
      </section>

      {/* Step 3: Fault injection */}
      <section className="rounded-2xl border border-line bg-surface p-5 sm:p-6">
        <h2 className="mb-3 flex items-center gap-2.5 text-xl font-bold">
          <span className="pearl-fill grid h-8 w-8 shrink-0 place-items-center rounded-lg">
            <PlugZap className="h-4 w-4" aria-hidden />
          </span>
          3. Test a double-spend and an interrupted device
        </h2>
        <div className="grid gap-4 lg:grid-cols-2">
          <div className="flex flex-col justify-between gap-3 rounded-xl border border-line bg-bg/60 p-4">
            <div className="flex flex-col gap-2">
              <h3 className="font-bold text-ink">Split double-spend</h3>
              <p className="text-xs leading-relaxed text-muted sm:text-sm">
                Sends two conflicting payments from Bob to half the validators each at the same instant. Neither gets
                &gt; 2/3 votes, zero coins move, and at the next checkpoint (60s by default on devnet) Bob&apos;s burned
                key is cancelled onto <code className="num text-ink">spare[0]</code>.
              </p>
            </div>
            <CopyText text="./target/release/orien devnet double-spend --wallet bob --split" />
          </div>

          <div className="flex flex-col justify-between gap-3 rounded-xl border border-line bg-bg/60 p-4">
            <div className="flex flex-col gap-2">
              <h3 className="font-bold text-ink">Lost device mid-payment</h3>
              <p className="text-xs leading-relaxed text-muted sm:text-sm">
                Signs a payment from Alice and drops it halfway. Because validators retain every promised signed order
                until final, another copy of Alice&apos;s wallet can pick it up from the network and finish it cleanly:
              </p>
            </div>
            <div className="flex flex-col gap-2">
              <CopyText text="./target/release/orien devnet interrupt" />
              <CopyText text="./target/release/orien resume -w devnet/wallets/alice.wallet" />
            </div>
          </div>
        </div>
      </section>

      {/* Step 4: Across machines */}
      <section className="rounded-2xl border border-line bg-surface p-5 sm:p-6">
        <h2 className="mb-3 flex items-center gap-2.5 text-xl font-bold">
          <span className="pearl-fill grid h-8 w-8 shrink-0 place-items-center rounded-lg">
            <Network className="h-4 w-4" aria-hidden />
          </span>
          4. Spread validators across machines
        </h2>
        <p className="mb-4 text-sm leading-relaxed text-ink/90">
          Pass one <code className="num">--address</code> per validator, copy <code className="num">genesis.json</code>{' '}
          to every machine, and place each <code className="num">vN.key</code> only on the machine that runs that
          validator:
        </p>
        <div className="flex flex-col gap-3">
          <CopyText text="orien devnet init --dir net --name my-network --address 100.64.0.1:7101 --address 100.64.0.2:7101 --address 100.64.0.3:7101 --address 100.64.0.3:7102" />
          <CopyText text="orien node -g genesis.json -k v1.key -d data --listen 100.64.0.1:7101" />
        </div>
        <p className="mt-4 flex items-start gap-2 rounded-xl border border-warn/35 bg-warn/10 p-3 text-xs sm:text-sm text-warn">
          <ShieldAlert className="mt-0.5 h-4 w-4 shrink-0" aria-hidden />
          <span>
            Wire traffic is not encrypted or rate-limited yet. Forged votes and receipts are rejected by signature, but
            always bind rehearsal validators to private Tailscale or WireGuard addresses — never a public IP.
          </span>
        </p>
      </section>

      {/* Workspace layout */}
      <section className="rounded-2xl border border-line bg-surface p-5 sm:p-6">
        <h2 className="mb-4 flex items-center gap-2.5 text-xl font-bold">
          <span className="pearl-fill grid h-8 w-8 shrink-0 place-items-center rounded-lg">
            <Boxes className="h-4 w-4" aria-hidden />
          </span>
          Rust workspace layout
        </h2>
        <div className="grid gap-3 sm:grid-cols-2">
          {CRATES.map((c) => (
            <div key={c.name} className="flex flex-col gap-1.5 rounded-xl border border-line bg-bg/60 p-4">
              <code className="num text-xs font-bold text-pearl">{c.name}</code>
              <h3 className="text-sm font-bold text-ink">{c.title}</h3>
              <p className="text-xs leading-relaxed text-muted sm:text-sm">{c.body}</p>
            </div>
          ))}
        </div>
      </section>

      <div className="flex flex-wrap items-center gap-3 rounded-2xl border border-line bg-surface p-5">
        <p className="flex-1 font-semibold">Read how hourly checkpoints reconcile logs and unlock stuck accounts</p>
        <Link href="/how-it-works/" className="pearl-fill pearl-sheen rounded-xl px-5 py-2.5 text-sm font-bold">
          Epoch design
        </Link>
        <Link href="/" className="rounded-xl border border-line px-5 py-2.5 text-sm font-semibold hover:border-pearl">
          Overview
        </Link>
      </div>
    </div>
  )
}
