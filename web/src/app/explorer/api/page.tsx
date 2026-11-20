import type { Metadata } from 'next'
import { Card } from '@/components/explorer/Parts'
import { EXAMPLES } from '@/content/explorer-api-examples'

export const metadata: Metadata = {
  title: 'API · Explorer',
  description:
    'The figures behind the Orien Explorer, free to read as JSON: the network now, every hourly checkpoint, any address, and the largest wallets.',
}

const BASE = 'https://explorer.orien.network/api/explorer'

type Field = [name: string, meaning: string]

type Endpoint = {
  id: keyof typeof EXAMPLES
  title: string
  path: string
  about: string
  fields: Field[]
  /** A note under the example. */
  shortened: string
}

const ENDPOINTS: Endpoint[] = [
  {
    id: 'overview',
    title: 'The network now',
    path: '/overview',
    about: 'Validators, how many ORN exist, mining, and the latest 24 checkpoints. New figures every 15 seconds.',
    fields: [
      ['epoch', 'The hour under way. Checkpoint N closes hour N.'],
      ['epoch_started, epoch_seconds', 'When this hour began, and how long an hour is: 3600 seconds.'],
      ['mining', 'The mining math this hour, such as "RandomX".'],
      ['validators', 'Each validator: seat (from 0), up, the hour it is on, how many of the latest checkpoints it agreed (agreed of of), stake, and the account that staked it (null for launch validators).'],
      ['supply.cap', 'The most ORN there will ever be.'],
      ['supply.released', 'ORN released so far, by mining and at launch.'],
      ['supply.per_hour', 'New ORN released with each checkpoint now.'],
      ['supply.in_wallets, staked, burned', 'ORN held in wallets, staked by validators, and destroyed.'],
      ['supply.holding, holding_one', 'Wallets holding any ORN, and holding at least 1 ORN.'],
      ['miners', 'Wallets paid for mining at checkpoint epoch (last_hour), and at least once over the hours before it (last_day, over hours checkpoints).'],
      ['hashrate.per_second', 'The network’s estimated mining speed in tries a second, averaged over hours hours up to checkpoint epoch.'],
      ['checkpoints', 'The latest 24 checkpoints, newest first, each as described below.'],
    ],
    shortened: 'Shortened: one validator and one checkpoint shown.',
  },
  {
    id: 'checkpoints',
    title: 'Every checkpoint',
    path: '/checkpoints?before=NUMBER',
    about:
      'Checkpoints 50 at a time, newest first. Leave out before for the newest; to go back, ask again with before set to the oldest number you have. more says whether there are older ones.',
    fields: [
      ['epoch', 'The checkpoint’s number, which is the hour it closes.'],
      ['time', 'When it was made.'],
      ['payments', 'Payments made final with it.'],
      ['released', 'New ORN released with it, to miners.'],
      ['agreed, of', 'The seats of the validators holding this checkpoint, and how many validators there were.'],
      ['round', 'The voting round it was agreed in. 0 is the first try.'],
      ['state', 'A fingerprint of every account’s balance after it.'],
      ['digest', 'This checkpoint’s own fingerprint.'],
    ],
    shortened: 'Shortened: one of 50 shown.',
  },
  {
    id: 'checkpoint',
    title: 'One checkpoint',
    path: '/checkpoints/NUMBER',
    about: 'One checkpoint, who was paid for mining that hour, and each validator’s part in it.',
    fields: [
      ['checkpoint', 'The checkpoint, as in the list above.'],
      ['latest', 'The newest checkpoint’s number.'],
      ['previous', 'The fingerprint of the checkpoint before it.'],
      ['rewards.known', 'Whether the explorer has every miner’s reward for that hour. Validators keep these records from checkpoint since on.'],
      ['rewards.miners', 'Each wallet paid for mining: its address, amount, and part of all the ORN mined that hour (0 to 1). Largest first.'],
      ['rewards.mined, fees', 'ORN paid to miners, and payment fees shared among stakers.'],
      ['rewards.stakers', 'Each staker paid a share of fees: address and amount.'],
      ['rewards.math', 'The mining math that hour.'],
      ['rewards.harder', 'How much harder mining is the next hour: 0.05 is 5% harder, -0.05 is 5% easier. null when the math changes.'],
      ['validators', 'Each validator that hour: seat, whether it agreed, stake, and the account that staked it.'],
    ],
    shortened: 'Shortened: two of 85 miners and one validator shown.',
  },
  {
    id: 'address',
    title: 'One address',
    path: '/address/ADDRESS',
    about: 'An address’s balance and its latest activity, newest first, up to 500 entries.',
    fields: [
      ['balance', 'ORN it can spend now.'],
      ['staked, returning', 'ORN it has staked, and ORN unstaked and on its way back.'],
      ['payments_made', 'How many payments it has sent.'],
      ['activity', 'Each entry: the hour (epoch), when its checkpoint was made (time, null for the hour under way), kind, amount, and the other address (counterparty, where there is one).'],
      ['kind', '"received", "sent", "mined", "fees", "claimed", "refunded" or "unstaked".'],
      ['more', 'Whether older activity was left out.'],
      ['since, since_time', 'Activity starts at this checkpoint, made at this time.'],
    ],
    shortened: 'Shortened: two of 97 entries shown.',
  },
  {
    id: 'top',
    title: 'The largest wallets',
    path: '/top',
    about: 'The 100 wallets holding the most ORN, largest first. New figures every minute.',
    fields: [
      ['holders', 'Each wallet: address, the ORN it holds and has staked, how many payments it has sent, and its share of all ORN in wallets (0 to 1).'],
      ['in_wallets', 'All ORN held in wallets.'],
      ['holding', 'Wallets holding any ORN.'],
      ['epoch', 'The hour these figures are from.'],
    ],
    shortened: 'Shortened: one of 100 shown.',
  },
]

function Code({ children }: { children: string }) {
  return <code className="num break-all rounded-md border border-line bg-bg px-1.5 py-0.5 text-[0.85em] text-ink">{children}</code>
}

/** /explorer/api/: how other programs read the explorer's figures. */
export default function ApiPage() {
  return (
    <div className="space-y-10 pb-16 pt-10 sm:pt-12">
      <header>
        <h1 className="text-3xl font-bold tracking-tight text-ink">API</h1>
        <p className="mt-3 max-w-2xl leading-relaxed text-muted">
          The figures behind this explorer, free for any website or program to read as JSON. No key or sign-up.
        </p>
      </header>

      <Card title="The basics">
        <ul className="space-y-4 px-5 py-5 leading-relaxed text-muted sm:px-6">
          <li>
            Every address below starts with <Code>{BASE}</Code>, and is read with a plain GET.
          </li>
          <li>
            <span className="font-semibold text-ink">Amounts of ORN are in billionths of an ORN</span>, as whole numbers
            in text so they stay exact: <Code>&quot;1000000000&quot;</Code> is 1 ORN. Divide by 1,000,000,000.{' '}
            <Code>supply.released</Code>, <Code>balance</Code> and every other amount is text this way. Counts and shares
            are plain numbers.
          </li>
          <li>Times are seconds since 1 January 1970, UTC.</li>
          <li>
            The network makes one checkpoint an hour. An hour is called an epoch, and checkpoint 136 closes epoch 136.
          </li>
          <li>An Orien address is 64 characters, 0–9 and a–f.</li>
          <li>
            <span className="font-semibold text-ink">How often:</span> up to 2 requests a second from one computer,
            with short bursts of up to 10. Past that the answer is 429; wait a second and ask again. Figures change at
            most every 15 seconds and checkpoints once an hour, so asking once a minute is plenty for most uses.
          </li>
          <li>
            Something that doesn’t exist answers 404, and a malformed address 400, each with a reason:{' '}
            <Code>{'{"error": "not found"}'}</Code>.
          </li>
          <li>
            New fields may be added. Nothing here is renamed or removed without a note in the{' '}
            <a href="https://orien.network/changelog/" className="font-semibold text-ink underline underline-offset-4">
              changelog
            </a>
            .
          </li>
        </ul>
      </Card>

      {ENDPOINTS.map((endpoint) => (
        <Card key={endpoint.id} title={endpoint.title}>
          <div className="space-y-5 px-5 py-5 sm:px-6">
            <p className="num break-all rounded-xl border border-line bg-bg px-4 py-3 text-sm text-ink">
              <span className="font-semibold text-go">GET</span> {BASE}
              {endpoint.path}
            </p>
            <p className="leading-relaxed text-muted">{endpoint.about}</p>
            <dl className="divide-y divide-line rounded-xl border border-line">
              {endpoint.fields.map(([name, meaning]) => (
                <div key={name} className="grid gap-1 px-4 py-3 sm:grid-cols-[14rem_1fr] sm:gap-4">
                  <dt className="num break-words text-sm font-semibold text-ink">{name}</dt>
                  <dd className="text-sm leading-relaxed text-muted">{meaning}</dd>
                </div>
              ))}
            </dl>
            <div>
              <p className="mb-2 text-sm text-muted">Example answer. {endpoint.shortened}</p>
              <pre className="num max-h-[28rem] overflow-auto rounded-xl border border-line bg-bg p-4 text-xs leading-relaxed text-ink">
                {EXAMPLES[endpoint.id]}
              </pre>
            </div>
          </div>
        </Card>
      ))}
    </div>
  )
}
