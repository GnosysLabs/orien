import type { Metadata } from 'next'
import Link from 'next/link'
import { ArrowRight } from 'lucide-react'
import { SupplyCurve } from '@/components/SupplyCurve'
import { pageMeta } from '@/lib/seo'
import { Faq, type Question } from '@/components/Faq'
import { MARKETPLACE_LIVE } from '@/lib/brand'

export const metadata: Metadata = pageMeta({
  title: 'How Orien works: quantum-proof payments, final in a blink',
  description:
    'How the Orien cryptocurrency works: payments final in a fraction of a second with no blocks, quantum-proof hash-based signatures, hourly checkpoints, staking, low fees, and ORN mined on ordinary computers with RandomX.',
  path: '/how-it-works/',
})

const SECTIONS = [
  { id: 'payments', title: 'Payments without blocks' },
  { id: 'signatures', title: 'Quantum-proof signatures' },
  { id: 'checkpoints', title: 'Hourly checkpoints' },
  { id: 'validators', title: 'Validators and staking' },
  { id: 'fees', title: 'Fees' },
  { id: 'coins', title: 'Coins and mining' },
  { id: 'swaps', title: 'Swapping with Bitcoin' },
  { id: 'safety', title: 'What keeps it safe' },
  { id: 'measured', title: 'Measured' },
  { id: 'questions', title: 'Questions' },
]

const QUESTIONS: Question[] = [
  {
    q: 'Is Orien really quantum-proof?',
    a: 'Orien’s signatures rest on one building block, the BLAKE3 hash. Quantum computers are expected to break the signatures Bitcoin, Ethereum and most other cryptocurrencies use today, because those rest on the mathematics of elliptic curves. Against a hash, a quantum computer only gets a much smaller speed-up, and Orien’s 256-bit hashes leave a wide margin.',
  },
  {
    q: 'How fast is an Orien payment?',
    a: 'About 160 milliseconds across the internet, measured on real validators. There are no blocks and no confirmations to wait for: once a payment is final, it is final.',
  },
  {
    q: 'Is Orien a blockchain?',
    a: 'Not in the usual sense. Orien has no blocks. Each payment is settled on its own by the validators, and they agree on the whole state only once an hour, at a checkpoint.',
  },
  {
    q: 'How do I get ORN?',
    a: MARKETPLACE_LIVE
      ? 'Mine it on your own computer with Orien Wallet or the orien command line, buy it with Bitcoin from another person through a trustless swap, or sell your work or goods for ORN on the Market.'
      : 'Mine it on your own computer with Orien Wallet or the orien command line, or be paid in ORN by someone who has it.',
  },
  {
    q: 'Can I mine ORN on my own computer?',
    a: 'Yes. Orien mines with RandomX, which suits an ordinary processor, so graphics cards and custom mining chips gain little. Orien Wallet mines on Macs with Apple chips, Windows and Linux, and you choose how much of the processor it uses.',
  },
  {
    q: 'Was there a pre-mine?',
    a: 'No. Orien started with zero coins: no pre-mine, no sale, no founder’s share. Its first hour was tied to a Bitcoin block announced in advance, so nobody could mine before everyone else.',
  },
  {
    q: 'How many ORN will ever exist?',
    a: 'At most 100 million. Half are released in the first four years, half of what remains in the next four, and so on, paid out every hour to the people mining.',
  },
  {
    q: 'What does a payment cost?',
    a: '0.1% of the amount, never more than 1 ORN. The fee goes to the validators who keep the network running.',
  },
]

function Section({ id, title, children }: { id: string; title: string; children: React.ReactNode }) {
  return (
    <section id={id} className="scroll-mt-24 border-t border-line py-12">
      <h2 className="mb-5 text-2xl font-bold tracking-tight text-ink">{title}</h2>
      <div className="space-y-4 leading-relaxed text-muted [&_strong]:font-semibold [&_strong]:text-ink">
        {children}
      </div>
    </section>
  )
}

function Facts({ rows }: { rows: [string, string][] }) {
  return (
    <dl className="mt-6 grid gap-px overflow-hidden rounded-2xl border border-line bg-line sm:grid-cols-2">
      {rows.map(([label, value]) => (
        <div key={label} className="bg-surface p-4">
          <dt className="text-xs font-semibold uppercase tracking-wider text-muted">{label}</dt>
          <dd className="mt-1 font-semibold text-ink">{value}</dd>
        </div>
      ))}
    </dl>
  )
}

export default function HowItWorksPage() {
  return (
    <div className="mx-auto max-w-[760px]">
      <header className="pb-10 pt-10 sm:pt-14">
        <p className="mb-4 text-xs font-semibold uppercase tracking-[0.2em] text-muted">How it works</p>
        <h1 className="text-balance text-4xl font-bold leading-[1.1] tracking-tight text-ink sm:text-5xl">
          A cryptocurrency without blocks
        </h1>
        <p className="mt-6 text-lg leading-relaxed text-muted">
          Orien settles each payment on its own, in a fraction of a second, and asks its validators to agree on
          anything only once an hour. Every signature it uses rests on a single, well-studied building block: the
          BLAKE3 hash function.
        </p>
        <nav aria-label="On this page" className="mt-8 rounded-2xl border border-line bg-surface p-5">
          <p className="mb-3 text-xs font-semibold uppercase tracking-wider text-muted">On this page</p>
          <ol className="grid gap-x-6 gap-y-2 text-sm sm:grid-cols-2">
            {SECTIONS.map((section, i) => (
              <li key={section.id}>
                <a href={`#${section.id}`} className="text-ink/85 hover:text-pearl">
                  <span className="mr-2 text-muted">{i + 1}.</span>
                  {section.title}
                </a>
              </li>
            ))}
          </ol>
        </nav>
      </header>

      <Section id="payments" title="Payments without blocks">
        <p>
          Most cryptocurrencies gather the world’s payments into blocks and settle them one block at a time, so every
          payment waits its turn. Orien has no blocks and no global line.
        </p>
        <p>
          <strong>Each account numbers its own payments.</strong> A payment says, in effect, “account A, payment #7:
          send 5 coins to B”. A validator votes for it if it has never seen a different payment #7 from A, and A can
          afford it. When validators holding more than two thirds of the stake have voted, the votes together form a{' '}
          <strong>certificate</strong>.
        </p>
        <p>
          Two conflicting payments can never both get one: any two groups holding more than two thirds of the stake
          share at least one honest validator, and an honest validator votes for only one payment #7.
        </p>
        <p>
          <strong>Final in two steps.</strong> The wallet hands the certificate back to the validators, and the
          payment is final once more than two thirds of the stake confirm they have recorded it. The wallet keeps a{' '}
          <strong>receipt</strong> holding both sets of signatures, which anyone can check for themselves, without
          asking a validator.
        </p>
      </Section>

      <Section id="signatures" title="Quantum-proof signatures">
        <p>
          The signatures most cryptocurrencies use today will be broken by large quantum computers. Orien uses{' '}
          <strong>hash-based signatures</strong> instead, whose safety rests only on the BLAKE3 hash function, which
          quantum computers cannot break that way.
        </p>
        <p>
          <strong>One key per payment.</strong> A hash-based key may safely sign only once, so every payment is signed
          with a fresh one-time key and names the key for the account’s next payment. “Never sign two different
          payment #7s” and “never use a key twice” become the same rule, and the validators enforce it.
        </p>
        <p>
          <strong>Spare keys.</strong> Every address also commits in advance to sixteen spare keys. If an owner ever
          does sign two different payments with one key, the next checkpoint cancels both and the account carries on
          with a spare key. Nobody who learns the burned key can touch the spares.
        </p>
        <p>
          <strong>Validators sign in batches.</strong> A validator gathers every vote and confirmation asked of it in
          the last few milliseconds and signs them all at once, so one signature covers many payments.
        </p>
      </Section>

      <Section id="checkpoints" title="Hourly checkpoints">
        <p>
          Once an hour the validators pause for a moment and agree on a <strong>checkpoint</strong>: a short summary
          that fixes every balance at that moment. It is the only time they need to agree on anything, and no payment
          waits for it: a payment sent during the pause simply goes through a moment later.
        </p>
        <p>At each checkpoint the network also:</p>
        <ul className="list-disc space-y-2 pl-5">
          <li>frees any account whose owner signed twice, onto its next spare key;</li>
          <li>chooses the next hour’s validators from the coins staked behind them;</li>
          <li>pays the hour’s fees to the validators;</li>
          <li>releases the hour’s new coins and pays them for the work done in that hour;</li>
          <li>takes the stake of any validator caught cheating.</li>
        </ul>
        <p>
          Each checkpoint names the one before it, back to the network’s starting file. A wallet or a new validator
          checks them in order and so learns who the validators are, without trusting anyone’s word. A validator that
          has been away catches up the same way, and needs to keep only its last few hours of data.
        </p>
      </Section>

      <Section id="validators" title="Validators and staking">
        <p>
          <strong>Anyone can become a validator</strong> by staking at least 10,000 coins behind a server. At each
          checkpoint, the largest stakes are chosen, up to 33 validators, and a newcomer’s server is found by the
          address it names when it stakes.
        </p>
        <p>
          <strong>Launch seats.</strong> The validators that start the network hold seats that share 3 million coins
          of voting weight. Their weight shrinks as real stake arrives, and once real stake reaches 3 million coins the
          seats step away for good, leaving the network to those with coins at stake. Seats earn no fees and no new
          coins.
        </p>
        <p>
          <strong>Unstaking</strong> releases coins after a week (168 hours), so a validator cannot cheat and leave
          with its stake before the proof of it is acted on.
        </p>
        <p>
          <strong>Cheating costs everything staked.</strong> A validator that votes for two conflicting payments, or
          signs two things with one key, leaves proof in its own signatures. Other validators pass that proof on, and
          at the next checkpoint the cheater’s whole stake, including coins waiting to be released, is burned and its key
          is barred from the list for good.
        </p>
        <Facts
          rows={[
            ['Minimum stake', '10,000 coins'],
            ['Most validators', '33'],
            ['Unstaking takes', '1 week'],
            ['Launch seats', '3 million coins of weight, fading'],
          ]}
        />
      </Section>

      <Section id="fees" title="Fees">
        <p>
          Every payment pays <strong>0.1% of its amount</strong>, never less than 0.000001 coin and never more than 1
          coin. A share of the amount keeps the fee about the same in real terms however much a coin is worth; the
          minimum stops floods of tiny payments, and the maximum keeps large payments from paying far more than the
          work they cost. Staking and unstaking pay the minimum.
        </p>
        <p>
          Fees gather over the hour and are paid at the checkpoint to the validators with real stake, in proportion to
          it. Fees gathered while no validator has real stake wait for the first ones that do.
        </p>
      </Section>

      <Section id="coins" title="Coins and mining">
        <p>
          <strong>No coins exist at the start.</strong> There is no pre-mine, no sale and no share for anyone. At most
          100 million coins will ever exist, each divisible to nine decimal places.
        </p>
        <p>
          <strong>Released smoothly, halving every four years.</strong> Half of the 100 million is released in the
          first four years, half of what remains in the next four, and so on: about 1,977 coins in the very first
          hour. Coins follow the clock rather than the number of checkpoints, so an hour that closes early or late
          changes nothing about how many exist.
        </p>
        <p>
          <strong>Earned by work.</strong> Miners look for numbers that, hashed with RandomX together with their
          account and the hour’s seed, come out below a target. Each such number is a <strong>work share</strong>.
          RandomX, the mining math Monero uses, needs 2 GB of memory and suits an ordinary processor, so graphics cards
          and custom chips gain little and any computer can take part.
          Validators collect the shares, and at each checkpoint the hour’s new coins are paid out in proportion to
          them. The target moves each hour to keep about a thousand shares an hour, however many people mine.
        </p>
        <p>
          The seed of each hour is the checkpoint before it, which nobody can know in advance. The very first seed
          comes from a Bitcoin block announced ahead of time, so nobody can mine before everyone else. Work only
          creates coins; it never decides which payments go through.
        </p>
        <div className="mt-6">
          <SupplyCurve />
        </div>
      </Section>

      <Section id="swaps" title="Swapping with Bitcoin">
        <p>
          Two people who don’t know each other can swap ORN for Bitcoin directly, with nobody in the middle and no way
          for either to cheat. It works with a <strong>locked payment</strong>: ORN that the receiver can only take by
          revealing a secret before a deadline, and that otherwise go back to the sender on their own.
        </p>
        <ol className="list-decimal space-y-2 pl-5">
          <li>The seller locks the ORN for the buyer behind a secret only the seller knows.</li>
          <li>The buyer locks the Bitcoin for the seller behind the same secret, on Bitcoin.</li>
          <li>The seller takes the Bitcoin, which reveals the secret to everyone.</li>
          <li>The buyer uses the secret to take the ORN.</li>
        </ol>
        <p>
          Either both are paid, or both get their coins back. Orien Wallet does every step for you; you only send the
          other person a code. The lock uses SHA-256, which Bitcoin can check, so the same secret opens both sides.
        </p>
      </Section>

      <Section id="safety" title="What keeps it safe">
        <ul className="list-disc space-y-2 pl-5">
          <li>
            <strong>One assumption.</strong> Every signature and every proof rests on the BLAKE3 hash function
            alone.
          </li>
          <li>
            <strong>Up to a third may misbehave.</strong> As long as validators holding more than two thirds of the
            stake are honest, no two conflicting payments can both become final, and nothing final is ever lost.
          </li>
          <li>
            <strong>Nobody needs to be trusted on their word.</strong> Votes, confirmations, checkpoints and proofs of
            cheating are all signed and checked on arrival. Wallets and miners take balances and the work target only
            from checkpoints they check themselves.
          </li>
          <li>
            <strong>Open to the internet.</strong> Validators limit how many connections any one address may hold and
            close connections that stay silent or send junk.
          </li>
        </ul>
      </Section>

      <Section id="measured" title="Measured">
        <p>
          These figures come from real validators that write every vote to disk before answering, with no in-memory
          shortcuts.
        </p>
        <Facts
          rows={[
            ['Final, across the internet', 'about 160 ms'],
            ['Payments per second, one laptop', '950 to 1,500'],
            ['One signature', '1,248 bytes'],
            ['Signatures checked per second, one core', 'about 40,000'],
          ]}
        />
      </Section>

      <section id="questions" className="scroll-mt-24 border-t border-line py-12">
        <h2 className="mb-5 text-2xl font-bold tracking-tight text-ink">Questions</h2>
        <Faq questions={QUESTIONS} />
      </section>

      <div className="mt-4 flex flex-wrap items-center justify-between gap-4 rounded-2xl border border-line bg-surface p-6">
        <p className="font-semibold text-ink">Every rule, in full detail</p>
        <Link
          href="/paper/"
          className="flex h-11 items-center gap-2 rounded-xl border border-line bg-bg px-5 text-sm font-semibold text-ink transition hover:border-pearl"
        >
          Technical paper <ArrowRight className="h-4 w-4" aria-hidden />
        </Link>
      </div>

      <div className="mb-8 mt-6 flex flex-wrap items-center justify-between gap-4 rounded-2xl border border-line bg-surface p-6">
        <p className="font-semibold text-ink">Ready to try it?</p>
        <Link
          href="/try/"
          className="pearl-fill flex h-11 items-center gap-2 rounded-xl px-5 text-sm font-bold transition hover:brightness-105"
        >
          Get started <ArrowRight className="h-4 w-4" aria-hidden />
        </Link>
      </div>
    </div>
  )
}
