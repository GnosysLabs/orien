import type { Metadata } from 'next'
import { notFound } from 'next/navigation'
import { CopyText } from '@/components/CopyText'
import { EXCHANGES_LIVE } from '@/lib/brand'
import { pageMeta } from '@/lib/seo'

export const metadata: Metadata = pageMeta({
  title: 'Listing ORN: for exchanges',
  description:
    'What an exchange needs to list the Orien cryptocurrency (ORN): the figures to enter, how deposits and withdrawals work, updates and backups, on one page.',
  path: '/developers/exchanges/',
})

const FIGURES: [string, React.ReactNode][] = [
  ['Name', <>Orien</>],
  ['Ticker', <>ORN</>],
  ['Decimals', <>9. The smallest unit is a billionth of an ORN.</>],
  ['Maximum supply', <>100,000,000 ORN, never more.</>],
  [
    'Fee',
    <>
      0.1% of the amount, at least 0.000001 and at most 1 ORN. The sender pays it on top; the receiver gets the full
      amount.
    </>,
  ],
  ['Addresses', <>64 characters, 0–9 and a–f. There is no memo or tag.</>],
  ['Confirmations', <>None to count. A payment is final in a fraction of a second.</>],
  [
    'Explorer',
    <>
      <span className="num break-all">https://explorer.orien.network/address/ADDRESS/</span> shows any address.
    </>,
  ],
]

function Code({ children }: { children: string }) {
  return <code className="num break-all rounded-md border border-line bg-bg px-1.5 py-0.5 text-[0.85em] text-ink">{children}</code>
}

function Section({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <section className="space-y-4 rounded-2xl border border-line bg-surface p-6 sm:p-8">
      <h2 className="text-2xl font-bold tracking-tight text-ink">{title}</h2>
      <div className="space-y-4 leading-relaxed text-muted [&_strong]:font-semibold [&_strong]:text-ink">{children}</div>
    </section>
  )
}

function Link({ href, children }: { href: string; children: React.ReactNode }) {
  return (
    <a href={href} className="font-semibold text-ink underline underline-offset-4">
      {children}
    </a>
  )
}

/** /developers/exchanges/: what an exchange's integration needs, on one
 * page. Everything else is in Build with Orien, so nothing is written twice. */
export default function ExchangesPage() {
  if (!EXCHANGES_LIVE) notFound()
  return (
    <div className="mx-auto max-w-[900px] space-y-8 pb-16 pt-5 sm:pt-7">
      <header className="space-y-4">
        <p className="text-xs font-semibold uppercase tracking-[0.14em] text-muted">For exchanges</p>
        <h1 className="text-balance text-4xl font-bold tracking-tight text-ink sm:text-5xl">Listing ORN</h1>
        <p className="max-w-2xl text-lg leading-relaxed text-muted">
          What an exchange needs to take deposits and send withdrawals in ORN, on one page. It uses the orien command,
          the network’s own program. <Link href="/developers/">Build with Orien</Link> covers installing it and
          everything else.
        </p>
      </header>

      <Section title="The figures">
        <dl className="divide-y divide-line rounded-xl border border-line">
          {FIGURES.map(([name, meaning]) => (
            <div key={name} className="grid gap-1 px-4 py-3 sm:grid-cols-[11rem_1fr] sm:gap-4">
              <dt className="text-sm font-semibold text-ink">{name}</dt>
              <dd className="text-sm leading-relaxed text-muted">{meaning}</dd>
            </div>
          ))}
        </dl>
      </Section>

      <Section title="Deposits">
        <p>
          A payment can’t carry a note, so <strong>give each customer their own address.</strong> One recovery phrase
          makes as many separate accounts as you need, each with its own address and its own wallet file:
        </p>
        <CopyText text="orien wallet restore customer-42.wallet --account 42" />
        <p>
          It asks for the phrase, typed or piped in, and prints the account’s address. Account 1 is the wallet the
          phrase was made with.
        </p>
        <p>
          <strong>A deposit counts as soon as it shows.</strong> It is final then, with nothing to wait for. To check
          an address, ask the validators themselves:
        </p>
        <CopyText text="orien activity ADDRESS --json" />
        <p>
          That lists each payment that came in, with its sender, its amount and a <Code>payment</Code> id that tells
          it from every other. <Code>orien balance ADDRESS --json</Code> gives the balance that validators holding
          more than two thirds of the stake agree on. Both depend on nothing of ours but the network. The{' '}
          <Link href="https://explorer.orien.network/api/">explorer’s API</Link> answers the same questions over the
          web.
        </p>
        <p>
          Once an hour the validators sign a checkpoint of every balance. A deposit is final before that; the
          checkpoint is what proves it to anyone who checks later.
        </p>
      </Section>

      <Section title="Withdrawals">
        <p>Send from one wallet, on one computer. The command returns once the payment is final:</p>
        <CopyText text="orien send --to ADDRESS --amount 25 --receipt withdrawal-1041.receipt" />
        <ul className="list-disc space-y-2 pl-5">
          <li>
            <strong>One at a time.</strong> Every payment is signed with a one-time key, so queue withdrawals and send
            the next when the last has returned. Never run two copies of the same wallet.
          </li>
          <li>
            <strong>The receipt is proof of payment.</strong> It holds the validators’ signatures, and anyone can check
            it offline with <Code>orien verify withdrawal-1041.receipt</Code>. Keep it with the withdrawal.
          </li>
          <li>
            <strong>If sending stops halfway,</strong> <Code>orien resume</Code> finishes the payment as it was signed.
            Do that before sending anything else from the wallet.
          </li>
          <li>
            <strong>The fee is yours.</strong> A withdrawal of 25 ORN takes 25.025 from your wallet, and the customer
            receives 25.
          </li>
        </ul>
        <p>
          Moving a customer’s deposit to your main wallet is a payment from that customer’s account, by the same
          rules.
        </p>
      </Section>

      <Section title="Updates">
        <p>
          <strong>Nothing you run here updates itself.</strong> Sending, checking balances and checking receipts never
          do. Only <Code>orien mine</Code> and <Code>orien node</Code> keep themselves current, and each takes{' '}
          <Code>--no-auto-update</Code>.
        </p>
        <p>
          Update when you choose, with <Code>orien update</Code>. It installs a release only if the Orien release key
          signed it. The <Link href="/changelog/">changelog</Link> lists every release and every change to the network.
        </p>
      </Section>

      <Section title="Backup">
        <ul className="list-disc space-y-2 pl-5">
          <li>
            <strong>The 24-word recovery phrase, kept offline.</strong> It brings back every account made from it, on
            any computer. Anyone who has it can spend them all.
          </li>
          <li>
            <strong>Wallet files stay on one computer,</strong> sealed with <Code>orien wallet password</Code>. Don’t
            copy a wallet file to a second computer that also sends: restore from the phrase only when the first is
            gone for good.
          </li>
        </ul>
      </Section>

      <Section title="Everything else">
        <p>
          <Link href="/developers/#orien">Installing the orien command</Link>, the{' '}
          <Link href="/developers/#safety">rules to build by</Link>, the <Link href="/paper/">technical paper</Link> and
          the <Link href="/network/genesis.json">genesis file</Link> are all one step away. To talk to a person, write
          to <Link href="mailto:hello@orien.network">hello@orien.network</Link>.
        </p>
      </Section>
    </div>
  )
}
