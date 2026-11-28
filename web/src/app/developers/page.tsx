import type { Metadata } from 'next'
import { ArrowUpRight } from 'lucide-react'
import { CopyText } from '@/components/CopyText'
import { DISCORD, SOURCE_CODE, WALLET_SIGN_IN_DOCS } from '@/lib/brand'
import { pageMeta } from '@/lib/seo'

export const metadata: Metadata = pageMeta({
  title: 'Build with Orien: everything developers need',
  description:
    `Everything a developer needs to build with the Orien cryptocurrency: accepting and sending ORN, the explorer API,${WALLET_SIGN_IN_DOCS ? ' Sign in with Orien Wallet,' : ''} mining, running a validator, locked payments and swaps, and the code.`,
  path: '/developers/',
})

const SECTIONS = [
  { id: 'basics', title: 'The basics' },
  { id: 'orien', title: 'The orien command' },
  { id: 'accept', title: 'Accepting ORN' },
  { id: 'send', title: 'Sending ORN' },
  { id: 'read', title: 'Reading the network' },
  ...(WALLET_SIGN_IN_DOCS ? [{ id: 'sign-in', title: 'Sign in with Orien Wallet' }] : []),
  { id: 'locks', title: 'Locked payments and swaps' },
  { id: 'mining', title: 'Mining' },
  { id: 'validators', title: 'Running a validator' },
  { id: 'code', title: 'The code' },
  { id: 'safety', title: 'Rules to build by' },
]

const COMMANDS: [string, string][] = [
  ['orien wallet new', 'A new wallet, with a 24-word recovery phrase to write down. orien wallet restore brings it back.'],
  ['orien wallet address', 'The wallet’s address.'],
  ['orien balance [ADDRESS]', 'A balance: your wallet’s, or any address’s. --json answers for programs.'],
  ['orien activity [ADDRESS]', 'What came in and what was sent, from the validators themselves. --json answers for programs.'],
  ['orien send --to ADDRESS --amount 5', 'Pays, and waits until the payment is final. --receipt FILE keeps the proof.'],
  ['orien verify FILE', 'Checks a receipt offline, against the genesis file and the checkpoints.'],
  ['orien escrow / claim / escrow-status', 'Locked payments: lock ORN behind a secret, claim it, or look one up.'],
  ['orien mine [ADDRESS]', 'Mines for a wallet or any address. orien benchmark finds the best number of cores.'],
  ['orien status', 'Every validator’s state, and the next checkpoint.'],
  ['orien ping HOST:PORT', 'Whether a validator answers: exits with 0 if it does.'],
  ['orien keygen / node / stake / unstake', 'Running a validator, and staking for it.'],
  ['orien update', 'Installs the newest version, only if the Orien release key signed it.'],
  ['orien devnet init / run', 'A local test network with test coins, for building against.'],
]

const BASICS: [string, React.ReactNode][] = [
  ['Units', <>ORN has nine decimals. Everything below the surface counts in billionths of an ORN: 1 ORN is 1,000,000,000.</>],
  ['Addresses', <>64 characters, 0–9 and a–f.</>],
  [
    'Fees',
    <>
      The sender pays 0.1% on top of the amount, at least 0.000001 and at most 1 ORN. The receiver gets the full
      amount. Fees go to the staked validators.
    </>,
  ],
  [
    'Finality',
    <>
      A payment is final in a fraction of a second, once validators holding more than two thirds of the stake have
      signed for it. There are no blocks and no confirmations to wait for.
    </>,
  ],
  [
    'Checkpoints',
    <>
      Once an hour the validators agree on every balance. An hour is an epoch, and checkpoint N closes epoch N.
      Mining rewards and fees are paid at each checkpoint.
    </>,
  ],
  [
    'Validators',
    <>
      Reached at v1.orien.network:7101 to v4.orien.network:7104, plus any staked validator. The orien command and
      Orien Wallet find them from the genesis file.
    </>,
  ],
  [
    'Genesis file',
    <>
      The network’s founding document, which every program checks against:{' '}
      <a href="/network/genesis.json" className="font-semibold text-ink underline underline-offset-4">
        orien.network/network/genesis.json
      </a>
      . The installer puts it in ~/.orien.
    </>,
  ],
]

function Code({ children }: { children: string }) {
  return <code className="num break-all rounded-md border border-line bg-bg px-1.5 py-0.5 text-[0.85em] text-ink">{children}</code>
}

function Block({ children }: { children: string }) {
  return <pre className="num overflow-auto rounded-xl border border-line bg-bg p-4 text-xs leading-relaxed text-ink">{children}</pre>
}

function Section({ id, title, children }: { id: string; title: string; children: React.ReactNode }) {
  return (
    <section id={id} className="scroll-mt-24 space-y-4 rounded-2xl border border-line bg-surface p-6 sm:p-8">
      <h2 className="text-2xl font-bold tracking-tight text-ink">{title}</h2>
      <div className="space-y-4 leading-relaxed text-muted [&_strong]:font-semibold [&_strong]:text-ink">{children}</div>
    </section>
  )
}

function Rows({ rows, wide = '10rem' }: { rows: [string, React.ReactNode][]; wide?: string }) {
  return (
    <dl className="divide-y divide-line rounded-xl border border-line">
      {rows.map(([name, meaning]) => (
        <div key={name} className="grid gap-1 px-4 py-3 sm:grid-cols-[var(--wide)_1fr] sm:gap-4" style={{ ['--wide' as string]: wide }}>
          <dt className="num break-words text-sm font-semibold text-ink">{name}</dt>
          <dd className="text-sm leading-relaxed text-muted">{meaning}</dd>
        </div>
      ))}
    </dl>
  )
}

function Link({ href, children }: { href: string; children: React.ReactNode }) {
  const outside = href.startsWith('http')
  return (
    <a
      href={href}
      {...(outside ? { target: '_blank', rel: 'noopener noreferrer' } : {})}
      className="font-semibold text-ink underline underline-offset-4"
    >
      {children}
    </a>
  )
}

const ACTIVITY = `{
  "address": "be5b7c…258b",
  "balance": "40193878403538",
  "activity": [
    {
      "epoch": 141,
      "time": null,
      "kind": "received",
      "amount": "25000000000",
      "counterparty": "c969e8…4352"
    }
  ],
  …
}`

export default function DevelopersPage() {
  return (
    <div className="mx-auto max-w-[900px] space-y-8 pb-16 pt-5 sm:pt-7">
      <header className="space-y-4">
        <p className="text-xs font-semibold uppercase tracking-[0.14em] text-muted">For developers</p>
        <h1 className="text-balance text-4xl font-bold tracking-tight text-ink sm:text-5xl">Build with Orien</h1>
        <p className="max-w-2xl text-lg leading-relaxed text-muted">
          Orien is a quantum-proof cryptocurrency whose payments are final in a fraction of a second. Everything here is
          free and open: the network, its code{WALLET_SIGN_IN_DOCS ? ', the explorer’s API and Sign in with Orien Wallet' : ' and the explorer’s API'}. This page is everything you
          need to build with it.
        </p>
        <div className="flex flex-wrap gap-2 pt-1">
          {[
            ['Technical paper', '/paper/'],
            ['Explorer', 'https://explorer.orien.network/'],
            ['Source code', SOURCE_CODE],
            ['Changelog', '/changelog/'],
            ['Discord', DISCORD],
          ].map(([label, href]) => (
            <a
              key={label}
              href={href}
              {...(href.startsWith('http') ? { target: '_blank', rel: 'noopener noreferrer' } : {})}
              className="flex h-9 items-center gap-1.5 rounded-lg border border-line bg-bg px-3 text-sm font-semibold text-ink transition hover:border-pearl"
            >
              {label}
              {href.startsWith('http') && <ArrowUpRight className="h-3.5 w-3.5" aria-hidden />}
            </a>
          ))}
        </div>
      </header>

      <nav aria-label="On this page" className="rounded-2xl border border-line bg-surface p-5 sm:p-6">
        <p className="mb-3 text-xs font-semibold uppercase tracking-[0.14em] text-muted">On this page</p>
        <ol className="grid gap-x-6 gap-y-2 text-sm sm:grid-cols-2">
          {SECTIONS.map((section, index) => (
            <li key={section.id}>
              <a href={`#${section.id}`} className="text-muted transition hover:text-ink">
                <span className="num mr-2 text-ink">{index + 1}.</span>
                {section.title}
              </a>
            </li>
          ))}
        </ol>
      </nav>

      <Section id="basics" title="The basics">
        <Rows rows={BASICS} />
      </Section>

      <Section id="orien" title="The orien command">
        <p>
          <strong>orien</strong> is the network’s own program: a wallet, a miner and a validator in one, for scripts and
          servers. Install it on a Mac or Linux:
        </p>
        <CopyText text="curl -fsSL https://orien.network/install.sh | sh" />
        <p>On Windows, in PowerShell:</p>
        <CopyText text="irm https://orien.network/install.ps1 | iex" />
        <p>
          The installer checks the download against its signed checksum, and puts orien and the genesis file in
          ~/.orien. The wallet is ~/.orien/wallet unless you pass <Code>--wallet</Code> or set{' '}
          <Code>ORIEN_WALLET</Code>.
        </p>
        <Rows rows={COMMANDS} wide="17rem" />
        <p>
          Every command explains itself with <Code>--help</Code>.
        </p>
      </Section>

      <Section id="accept" title="Accepting ORN">
        <p>
          A payment can’t carry a note, so tell payments apart by who sent them or where they went. Either way you know
          the moment one is final.
        </p>
        <ul className="list-disc space-y-2 pl-5">
          <li>
            <strong>An address for each customer or order.</strong> Make a wallet for each with{' '}
            <Code>orien wallet new</Code> and watch its address.
          </li>
          {WALLET_SIGN_IN_DOCS && <li>
            <strong>Know who your customers are.</strong> With{' '}
            <Link href="#sign-in">Sign in with Orien Wallet</Link>, people sign in with their wallet, so you know its
            address and a payment from it is theirs.
          </li>}
        </ul>
        <p>
          To see payments arrive, ask the explorer’s API about your address. Final payments appear within seconds, with
          <Code>time</Code> empty until the hour’s checkpoint:
        </p>
        <CopyText text="https://explorer.orien.network/api/explorer/address/ADDRESS" />
        <Block>{ACTIVITY}</Block>
        <p>
          Entries of kind <Code>received</Code> are payments to you, from <Code>counterparty</Code>, of{' '}
          <Code>amount</Code> billionths of an ORN. Or ask the validators themselves, with nothing of ours in between:{' '}
          <Code>orien activity ADDRESS --json</Code> lists the same payments, and{' '}
          <Code>orien balance ADDRESS --json</Code> gives the balance.
        </p>
      </Section>

      <Section id="send" title="Sending ORN">
        <p>From a server, send with the orien command. It returns once the payment is final:</p>
        <CopyText text="orien send --to ADDRESS --amount 25 --receipt payment.receipt" />
        <p>
          The receipt holds the validators’ signatures for the payment. Anyone can check it offline with{' '}
          <Code>orien verify payment.receipt</Code>, which makes it proof of payment to anyone.
        </p>
        <p>
          Listing ORN on an exchange? <Link href="/developers/exchanges/">Listing ORN</Link> puts what you need on one
          page.
        </p>
        <p>
          <strong>Send one payment at a time from one wallet,</strong> and keep each wallet on one computer. See{' '}
          <Link href="#safety">Rules to build by</Link>.
        </p>
      </Section>

      <Section id="read" title="Reading the network">
        <p>
          The explorer’s API answers in JSON, free and with no key: the network now, every checkpoint, any address and
          the largest wallets. Up to 2 requests a second, with short bursts of 10.
        </p>
        <p>
          <Link href="https://explorer.orien.network/api/">The explorer’s API</Link> describes each address and field,
          with real answers.
        </p>
      </Section>

      {WALLET_SIGN_IN_DOCS && <Section id="sign-in" title="Sign in with Orien Wallet">
        <p>
          Let people sign in to your website with their Orien wallet, as with Phantom or MetaMask: no email, no
          password, and no ORN moves. They approve in the Orien Wallet browser extension, and your website gets their
          wallet address with a proof it checks itself, offline, with Orien’s quantum-safe signatures.
        </p>
        <p>
          <Link href="/developers/sign-in/">Sign in with Orien Wallet for developers</Link> has the code, and a demo to
          try.
        </p>
      </Section>}

      <Section id="locks" title="Locked payments and swaps">
        <p>
          A locked payment holds ORN for someone who can take it by revealing a secret before a deadline. After the
          deadline it returns to the sender by itself. Locks are what let people swap ORN for other cryptocurrencies
          with no one in the middle.
        </p>
        <CopyText text="orien escrow --to ADDRESS --amount 500 --hours 48" />
        <ul className="list-disc space-y-2 pl-5">
          <li>
            The secret is 32 bytes; a lock names its SHA-256. Pass <Code>--hash</Code> to lock behind someone
            else’s secret. Locks last 2 to 720 hours.
          </li>
          <li>
            The receiver takes it with <Code>orien claim ADDRESS:NUMBER</Code>, revealing the secret, and{' '}
            <Code>orien escrow-status</Code> shows any lock and, once claimed, its secret.
          </li>
          <li>
            On Solana the other side locks in the orien-lock program,{' '}
            <Code>DAeXj31NAoSgno7SiFX7t4tWoMDBtpN9rn8c2X7WNtYE</Code>, which is final and can never be changed. On
            Bitcoin it is a script.
          </li>
        </ul>
        <p>
          The design, step by step, is in the code’s{' '}
          <Link href={`${SOURCE_CODE}/blob/main/docs/locked-payments.md`}>locked payments</Link> and{' '}
          <Link href={`${SOURCE_CODE}/blob/main/docs/sol-swaps.md`}>Solana swaps</Link> notes.
        </p>
      </Section>

      <Section id="mining" title="Mining">
        <p>
          Every checkpoint releases new ORN, shared among the miners of that hour in proportion to the work shares each
          found. Mining uses RandomX, which suits ordinary computers’ processors. Work only shares out new ORN: it never
          decides payments.
        </p>
        <CopyText text="orien mine ADDRESS --threads 4" />
        <ul className="list-disc space-y-2 pl-5">
          <li>Mine for any address; the miner needs no wallet of its own.</li>
          <li>
            The miner keeps itself on the newest signed release, so a change to the network’s rules never leaves it
            doing work nobody counts.
          </li>
          <li>
            The explorer shows who was paid at each checkpoint, and the network’s hashrate. Half of what remains of the
            100 million cap is released every four years.
          </li>
        </ul>
        <h3 id="hiveos" className="pt-2 text-lg font-semibold text-ink">Mining rigs on HiveOS</h3>
        <p>
          Orien is a HiveOS custom miner: one flight sheet sets many rigs mining, each with its hashrate in the HiveOS
          dashboard. Its scripts are in the code, in hiveos/orien.
        </p>
        <p>
          <Link href="/try/#hiveos">How to set it up</Link>
        </p>
      </Section>

      <Section id="validators" title="Running a validator">
        <p>
          Anyone can run a validator by staking at least 10,000 ORN behind a validator key. It joins at the next
          checkpoint, earns a share of the fees in proportion to its stake, and votes on every payment.{' '}
          <Link href="/developers/validators/">Run a validator</Link> walks through it step by step.
        </p>
        <Block>{`orien keygen validator.key
orien node -k validator.key -d data --listen 0.0.0.0:7100
orien stake --amount 10000 --validator-key validator.key --address HOST:7100`}</Block>
        <ul className="list-disc space-y-2 pl-5">
          <li>
            A validator that votes for two conflicting payments, or signs twice with one key, loses its stake at the
            next checkpoint.
          </li>
          <li>
            <Code>orien unstake</Code> releases stake after 168 checkpoints, a week, during which it can still be taken
            for cheating.
          </li>
          <li>
            Validators install new signed releases by themselves, one at a time, in the minutes after a checkpoint.
            Allow about 45,000 open files: each connection uses a thread and two.
          </li>
        </ul>
      </Section>

      <Section id="code" title="The code">
        <p>
          Everything is open source, under the MIT or Apache 2.0 license, written in Rust:{' '}
          <Link href={SOURCE_CODE}>github.com/GnosysLabs/orien</Link>.
        </p>
        <Rows
          rows={[
            ['orien-ledger', 'Accounts, payments and their rules, checkpoints and mining work.'],
            ['orien-crypto', 'BLAKE3 and the one-time hash-based signatures.'],
            ['orien-net', 'Validators, and the client wallets use to pay and to read the network.'],
            ['orien-cli', 'The orien command.'],
          ]}
          wide="9rem"
        />
        <p>
          Use the crates from the repository as a git dependency. Build against a local test network first, with test
          coins: <Code>orien devnet init</Code> then <Code>orien devnet run</Code>. The{' '}
          <Link href="/paper/">technical paper</Link> states every rule exactly.
        </p>
      </Section>

      <Section id="safety" title="Rules to build by">
        <ul className="list-disc space-y-2 pl-5">
          <li>
            <strong>Never sign two payments at once from one wallet.</strong> Every payment is signed with a one-time
            key. Two copies of a wallet signing different payments at the same moment use one key twice, which can let
            someone forge a payment from it. Keep each wallet on one computer, and send from it one payment at a time.
          </li>
          <li>
            <strong>A signed payment can’t be cancelled.</strong> If sending stops halfway, <Code>orien resume</Code>{' '}
            finishes it as it was signed.
          </li>
          <li>
            <strong>Keep the recovery phrase offline.</strong> Anyone with it can spend the wallet’s ORN. Seal wallet
            files with <Code>orien wallet password</Code>.
          </li>
          <li>
            <strong>Install only signed releases.</strong> The installer and <Code>orien update</Code> check the
            Orien release key’s signature.
          </li>
        </ul>
        <p>
          Found a security problem? Please write to <Link href="mailto:hello@orien.network">hello@orien.network</Link>{' '}
          before publishing it. Questions are welcome on <Link href={DISCORD}>Discord</Link>.
        </p>
      </Section>
    </div>
  )
}
