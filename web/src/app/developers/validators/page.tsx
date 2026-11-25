import type { Metadata } from 'next'
import { CopyText } from '@/components/CopyText'
import { DISCORD } from '@/lib/brand'
import { pageMeta } from '@/lib/seo'

export const metadata: Metadata = pageMeta({
  title: 'Run a validator',
  description:
    'How to run an Orien validator: the stake it needs, installing and starting it, the one port that must be open to the internet, keeping its key safe, staying up to date, and how to reach the Orien team.',
  path: '/developers/validators/',
})

const FIGURES: [string, React.ReactNode][] = [
  ['Stake', <>At least 10,000 ORN, from your own wallet. Up to 33 validators take part: those with the largest stakes.</>],
  ['Joining', <>At the next checkpoint after staking, within the hour.</>],
  ['Earnings', <>A share of the hour’s fees at every checkpoint, in proportion to your stake, paid to the wallet that staked.</>],
  [
    'Leaving',
    <>
      Unstaking takes 168 checkpoints, a week. The stake stays locked meanwhile, and can still be taken for cheating.
    </>,
  ],
  [
    'Computer',
    <>
      A Linux or Mac computer that stays on and online, with an internet address others can reach and one open TCP
      port. It needs about 45,000 open files: each connection uses a thread and two.
    </>,
  ],
]

const SERVICE = `[Unit]
Description=Orien validator
After=network-online.target
Wants=network-online.target

[Service]
User=orien
ExecStart=/home/orien/.orien/bin/orien node -k /home/orien/validator.key -d /home/orien/data --listen 0.0.0.0:7100
Restart=always
RestartSec=5
LimitNOFILE=45000

[Install]
WantedBy=multi-user.target`

function Code({ children }: { children: string }) {
  return <code className="num [overflow-wrap:anywhere] rounded-md border border-line bg-bg px-1.5 py-0.5 text-[0.85em] text-ink">{children}</code>
}

function Section({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <section className="space-y-4 rounded-2xl border border-line bg-surface p-6 sm:p-8">
      <h2 className="text-2xl font-bold tracking-tight text-ink">{title}</h2>
      <div className="space-y-4 leading-relaxed text-muted [&_strong]:font-semibold [&_strong]:text-ink">{children}</div>
    </section>
  )
}

function Step({ n, title, children }: { n: number; title: string; children: React.ReactNode }) {
  return (
    <li className="grid grid-cols-[2rem_minmax(0,1fr)] gap-x-3">
      <span className="num pearl-fill flex h-8 w-8 items-center justify-center rounded-full text-sm font-bold">{n}</span>
      <div className="min-w-0 space-y-3 pt-1">
        <h3 className="font-semibold text-ink">{title}</h3>
        {children}
      </div>
    </li>
  )
}

function Link({ href, children }: { href: string; children: React.ReactNode }) {
  return (
    <a href={href} className="font-semibold text-ink underline underline-offset-4">
      {children}
    </a>
  )
}

/** /developers/validators/: everything an operator needs to run a
 * validator, on one page, from the orien command's own rules. */
export default function ValidatorsPage() {
  return (
    <div className="mx-auto max-w-[900px] space-y-8 pb-16 pt-5 sm:pt-7">
      <header className="space-y-4">
        <p className="text-xs font-semibold uppercase tracking-[0.14em] text-muted">For validator operators</p>
        <h1 className="text-balance text-4xl font-bold tracking-tight text-ink sm:text-5xl">Run a validator</h1>
        <p className="max-w-2xl text-lg leading-relaxed text-muted">
          Validators keep the Orien network running. They vote on every payment, agree on every balance once an hour,
          and share the fees. Anyone with enough ORN can run one, with the orien command, the network’s own program.
        </p>
      </header>

      <Section title="What it takes">
        <dl className="divide-y divide-line rounded-xl border border-line">
          {FIGURES.map(([name, meaning]) => (
            <div key={name} className="grid gap-1 px-4 py-3 sm:grid-cols-[9rem_1fr] sm:gap-4">
              <dt className="text-sm font-semibold text-ink">{name}</dt>
              <dd className="text-sm leading-relaxed text-muted">{meaning}</dd>
            </div>
          ))}
        </dl>
      </Section>

      <Section title="Set it up">
        <ol className="space-y-7">
          <Step n={1} title="Install orien">
            <p>On the computer that will run the validator:</p>
            <CopyText text="curl -fsSL https://orien.network/install.sh | sh" />
            <p>
              It installs the orien command and the network’s genesis file in <Code>~/.orien</Code>, after checking
              the download against its published checksum.
            </p>
          </Step>

          <Step n={2} title="Make the validator’s key">
            <CopyText text="orien keygen validator.key" />
            <p>
              This key signs the validator’s votes. It is not a wallet: it holds no ORN, and your stake stays yours.
            </p>
          </Step>

          <Step n={3} title="Open its port to the internet">
            <p>
              A validator uses <strong>one TCP port</strong>, 7100 unless you choose another. Both other validators and
              wallets reach it there: wallets ask it for balances and status and send it payments to vote on. Allow
              incoming connections to that port <strong>from every address</strong>, in the computer’s firewall and, at
              home, in your router’s port forwarding.
            </p>
            <p>
              A validator that turns wallets away still signs checkpoints, but every wallet counts it as unreachable.
            </p>
          </Step>

          <Step n={4} title="Start the validator">
            <CopyText text="orien node -k validator.key -d data --listen 0.0.0.0:7100" />
            <p>
              <Code>data</Code> is the folder where it keeps its records. Leave it running: to keep it going through
              restarts, see <Link href="#service">Keep it running</Link> below.
            </p>
          </Step>

          <Step n={5} title="Check it from outside">
            <p>From another computer, on another network, such as a phone’s hotspot:</p>
            <CopyText text="orien ping HOST:7100" />
            <p>
              <Code>HOST</Code> is the validator’s public internet address or a domain name pointing to it. It must say
              the validator answers before you stake.
            </p>
            <div className="space-y-2 rounded-xl border border-line bg-bg/60 px-4 py-3 text-sm">
              <p className="font-semibold text-ink">If it doesn’t answer</p>
              <ul className="list-disc space-y-1.5 pl-5">
                <li>
                  <strong>Timed out, or connection refused:</strong> the port isn’t open yet. Check the firewall, the
                  router’s port forwarding, and that the validator is running.
                </li>
                <li>
                  <strong>Connection reset, or connection closed:</strong> the port is reachable, but something cuts the
                  connection. Look for a firewall, proxy or security service in front of the validator that turns
                  connections away, and make sure orien is up to date with <Code>orien update</Code>. Then ping again
                  from another network.
                </li>
              </ul>
            </div>
          </Step>

          <Step n={6} title="Stake">
            <p>From the wallet holding your ORN, on any computer:</p>
            <CopyText text="orien stake --amount 10000 --validator-key validator.key --address HOST:7100" />
            <p>
              <Code>--address</Code> is where everyone will reach your validator, so use the same public address as in
              step 5, never <Code>0.0.0.0</Code> or a home network address like <Code>192.168.1.20</Code>. If the stake
              is among the 33 largest, the validator joins at the next checkpoint.
            </p>
          </Step>

          <Step n={7} title="See it join">
            <CopyText text="orien status" />
            <p>
              After the next checkpoint it lists your validator with the others. It shows on the{' '}
              <Link href="https://explorer.orien.network/">explorer</Link> too.
            </p>
          </Step>
        </ol>
      </Section>

      <section id="service" className="scroll-mt-24">
        <Section title="Keep it running">
          <p>
            On Linux, a systemd service starts the validator with the computer and brings it back if it stops. Save
            this as <Code>/etc/systemd/system/orien-validator.service</Code>, with your own user and paths:
          </p>
          <pre className="num overflow-auto rounded-xl border border-line bg-bg p-4 text-xs leading-relaxed text-ink">{SERVICE}</pre>
          <CopyText text="sudo systemctl enable --now orien-validator" />
          <p>
            <Code>LimitNOFILE</Code> gives it the open files it needs. <Code>journalctl -u orien-validator -f</Code>{' '}
            shows what it is doing.
          </p>
        </Section>
      </section>

      <Section title="Keep the key safe">
        <p>
          A validator that signs two different things with the same one-time key, or votes for two conflicting
          payments, <strong>loses its whole stake</strong> at the next checkpoint. Honest validators never do, as long
          as these rules hold:
        </p>
        <ul className="list-disc space-y-2 pl-5">
          <li>
            <strong>Run one copy, ever.</strong> Never start the same key on two computers, even for a moment.
          </li>
          <li>
            <strong>The key file and the data folder belong together.</strong> The data folder records which one-time
            keys are used. Never go back to an older copy of it: the validator would use those keys again.
          </li>
          <li>
            <strong>Moving to a new computer:</strong> stop the old one for good first, then move the key file and the
            data folder together, and start it on the new one.
          </li>
          <li>
            <strong>Keep the key private</strong>, readable only by the user that runs the validator (
            <Code>chmod 600 validator.key</Code>).
          </li>
          <li>
            <strong>It renews its key by itself.</strong> Once half of its one-time keys are used, it makes the next key
            in its data folder and announces it at a checkpoint.
          </li>
        </ul>
        <p>
          Your stake belongs to the wallet that staked it, not to the key. If the validator is gone, release the stake
          from that wallet with <Code>orien unstake --amount 10000</Code>.
        </p>
      </Section>

      <Section title="Stay up to date">
        <p>
          <strong>A validator updates itself.</strong> When a new release signed with the Orien release key comes out,
          it installs it and restarts at a calm moment, in the minutes after a checkpoint, one validator at a time.
          Leave this on.
        </p>
        <p>
          <Code>--no-auto-update</Code> turns it off. Then follow the <Link href="/changelog/">changelog</Link> and
          update promptly with <Code>orien update</Code>, because a validator on an old release can fall behind the
          network’s rules.
        </p>
      </Section>

      <Section title="Talk to us">
        <p>
          Please say hello on the <Link href={DISCORD}>Orien Discord</Link> once your validator runs, with its address.
          Then we can tell you if something looks wrong with it, and you can ask anything. You can also write to{' '}
          <Link href="mailto:hello@orien.network">hello@orien.network</Link>.
        </p>
        <p>
          The <Link href="/paper/">technical paper</Link> states every rule validators follow, and{' '}
          <Link href="/developers/">Build with Orien</Link> covers the rest of the orien command.
        </p>
      </Section>
    </div>
  )
}
