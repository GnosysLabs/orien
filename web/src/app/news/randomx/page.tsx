import type { Metadata } from 'next'
import Link from 'next/link'
import { CopyText } from '@/components/CopyText'
import { RANDOMX_FROM } from '@/lib/brand'
import { jsonLd, pageMeta } from '@/lib/seo'
import { SITE_URL } from '@/lib/brand'

export const metadata: Metadata = pageMeta({
  title: 'Mining moves to RandomX',
  description:
    'From epoch 21, Orien mining uses RandomX, the CPU-friendly proof-of-work Monero uses, so graphics cards and custom chips gain little and any computer can mine. What changed, why, and what miners do.',
  path: '/news/randomx/',
  type: 'article',
  published: '2026-11-15',
})

function Section({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <section className="space-y-3">
      <h2 className="text-xl font-bold tracking-tight text-ink">{title}</h2>
      <div className="space-y-3 leading-relaxed text-muted [&_strong]:font-semibold [&_strong]:text-ink [&_ul]:list-disc [&_ul]:space-y-1.5 [&_ul]:pl-5 [&_ol]:list-decimal [&_ol]:space-y-1.5 [&_ol]:pl-5">
        {children}
      </div>
    </section>
  )
}

export default function RandomXPost() {
  return (
    <article className="mx-auto max-w-[760px] space-y-10 pb-16">
      <script
        type="application/ld+json"
        dangerouslySetInnerHTML={jsonLd({
          '@type': 'NewsArticle',
          headline: 'Mining moves to RandomX',
          description:
            'From epoch 21, Orien mining uses RandomX, the CPU-friendly proof-of-work Monero uses, so any computer can mine.',
          datePublished: '2026-11-15',
          image: `${SITE_URL}/og-nebula.png`,
          mainEntityOfPage: `${SITE_URL}/news/randomx/`,
          author: { '@id': `${SITE_URL}/#orien` },
          publisher: { '@id': `${SITE_URL}/#orien` },
        })}
      />
      <header className="pt-10 sm:pt-14">
        <p className="mb-4 text-xs font-semibold uppercase tracking-[0.2em] text-muted">15 November 2026 · Network</p>
        <h1 className="text-balance text-4xl font-bold leading-[1.1] tracking-tight text-ink sm:text-5xl">
          Mining moves to RandomX
        </h1>
        <p className="mt-6 text-lg leading-relaxed text-muted">
          From epoch {RANDOMX_FROM}, starting at 18:11 UTC on 15 November 2026, Orien mining uses RandomX, the
          proof-of-work built for ordinary computers. If you mine, run <code className="num text-ink">orien update</code>{' '}
          and restart <code className="num text-ink">orien mine</code>: shares from older versions no longer count.
        </p>
      </header>

      <Section title="What miners need to do">
        <ol>
          <li>Stop mining: press Ctrl-C in the window where <code className="num text-ink">orien mine</code> runs.</li>
          <li>Update orien:</li>
        </ol>
        <CopyText label="Mac, Linux and Windows" text="orien update" />
        <ol start={3}>
          <li>Start mining again, the way you started it before:</li>
        </ol>
        <CopyText label="For example, on half the processor" text="orien mine --cpu 50" />
        <p>
          That’s the last time you have to do it by hand. From version 1.5.0, <strong>orien mine keeps itself up to
          date</strong>: about once an hour it looks for a new release, installs it and carries on mining on it.
        </p>
        <p>
          <strong>Mining in Orien Wallet?</strong> Mining arrives in the app with its next version, already using
          RandomX. Nothing to do. The wallet itself, your balance, sending and swaps, works as before.
        </p>
      </Section>

      <Section title="What changed">
        <p>
          Miners look for numbers that, hashed with their account and the hour’s seed, come out below a target. Until
          epoch {RANDOMX_FROM}, that hash was BLAKE3. Now it is <strong>RandomX</strong>, the mining math Monero has used
          since 2019.
        </p>
        <p>
          Each RandomX try runs a small random program over 2 GB of memory, so mining now needs about 2 GB of free
          memory. At each hourly checkpoint the miner spends up to a minute getting ready for the new hour; the log says{' '}
          <em>getting RandomX ready</em>. A computer with less memory mines in a slower light mode, and says so.
        </p>
        <p>
          You’ll see far fewer tries a second: hundreds or thousands, not millions. That’s expected. Each RandomX try is
          far more work, the difficulty started again at the switch to match, and it adjusts each hour as before. Your
          share of each hour’s ORN still depends on your share of all the work done.
        </p>
      </Section>

      <Section title="Why">
        <p>
          BLAKE3 is a superb hash, which is why Orien’s signatures rely on it. But it is the kind of work graphics cards
          do thousands of times at once. Anyone who wrote a graphics-card miner could have out-mined every ordinary
          computer many times over, and taken almost every hour’s new ORN.
        </p>
        <p>
          RandomX was designed to stop exactly that. An ordinary processor is close to the best hardware there is for
          it, and graphics cards and custom chips gain little. Monero has relied on it for years, and mining there has
          stayed open to people with ordinary computers.
        </p>
        <p>
          It’s far easier to change this now, while the network is young and its miners are few, than later. We would
          rather ask a handful of early miners to update once than let mining drift beyond ordinary people’s reach.
        </p>
      </Section>

      <Section title="What stays the same">
        <ul>
          <li>Every balance, payment and piece of history. Only how new shares are judged changed.</li>
          <li>How many ORN exist and arrive each hour: the release schedule follows the clock, not the mining math.</li>
          <li>Signatures and payments, which still rest on BLAKE3 alone.</li>
          <li>The hourly split: each hour’s new ORN go to that hour’s miners, in proportion to their shares.</li>
        </ul>
      </Section>

      <Section title="More">
        <p>
          The exact rules are in the <Link href="/paper/#coins" className="font-semibold text-ink underline underline-offset-4">technical paper</Link>, and
          every release is in the <Link href="/changelog/" className="font-semibold text-ink underline underline-offset-4">changelog</Link>. Questions are
          welcome on <a href="https://discord.gg/5FFRZAw3u9" target="_blank" rel="noopener noreferrer" className="font-semibold text-ink underline underline-offset-4">Discord</a>.
        </p>
      </Section>
    </article>
  )
}
