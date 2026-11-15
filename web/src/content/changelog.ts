// Every change people should know about, newest first. Add an entry with
// each release of orien or Orien Wallet, and for any change to the network.
// Dates are UTC.

export type Product = 'network' | 'orien' | 'validators' | 'wallet' | 'marketplace'

export type Entry = {
  date: string
  /** What changed. A release of orien that changes validators lists both. */
  products: Product[]
  /** The version, for a release. */
  version?: string
  title: string
  changes: string[]
}

export const PRODUCTS: Record<Product, string> = {
  network: 'Network',
  orien: 'orien',
  validators: 'Validators',
  wallet: 'Orien Wallet',
  marketplace: 'Marketplace',
}

export const CHANGELOG: Entry[] = [
  {
    date: '2026-11-15',
    products: ['wallet'],
    version: '1.4.3',
    title: 'Older activity fixed, and steadier pictures',
    changes: [
      'Mining rewards and payments recorded before the app read the network’s history for their hour now show what they were, once, when the app starts.',
      'Sellers’ pictures on the Swap screen show their initial until the picture has loaded, and no longer blink when offers refresh.',
    ],
  },
  {
    date: '2026-11-15',
    products: ['wallet'],
    version: '1.4.2',
    title: 'Signing up, said plainly',
    changes: [
      'The Swap screen’s sign-in is now Sign up or sign in, and says that a new email makes your account right there: no need to sign up on orien.network first.',
      'Swap with someone you know opens its own screen, with Sell ORN and Buy ORN.',
    ],
  },
  {
    date: '2026-11-15',
    products: ['wallet'],
    version: '1.4.1',
    title: 'Linking your wallet is free, and clearer activity',
    changes: [
      'Signing in links this wallet to your marketplace account at once, with no payment, even a brand-new wallet with no ORN.',
      'The balance fills its card evenly, however long it is.',
      'More room above the email field when signing in.',
      'The hour’s mining reward right after the app starts reading the network’s history shows as a mining reward, not as an incoming payment.',
      'Coins the app can only work out from your balance, with no record in the network’s history, say so, instead of calling them an incoming payment.',
    ],
  },
  {
    date: '2026-11-15',
    products: ['wallet'],
    version: '1.4.0',
    title: 'Buy and sell ORN in the app',
    changes: [
      'The Swap screen shows the open offers to buy or sell ORN for Bitcoin, cheapest first, with each seller, their swaps done, and the value in dollars. Take one, or post your own; you need no ORN to buy your first.',
      'After you take or post an offer, the two wallets carry the swap through on their own while they are open: nobody copies codes. Your swaps show each step, and what happens if the other side disappears.',
      'Sign in once with your email and a 6-digit code: the same account as on orien.network.',
      'Signing in also links this wallet to your account with one tiny payment (under 0.01 ORN), sent once when the wallet holds enough, so you can sell on the marketplace too.',
      'Swapping by passing codes yourself is still there, under Swap with someone you know.',
    ],
  },
  {
    date: '2026-11-15',
    products: ['marketplace'],
    title: 'Buy & sell ORN, and the marketplace',
    changes: [
      'orien.network/marketplace: people offer work and goods for ORN, with profiles, pictures, messages, and payment requests inside each conversation. Buyers pay sellers straight from their own wallets: the marketplace never holds anyone’s coins.',
      'Buy & sell ORN, at orien.network/swap: buy ORN with Bitcoin, or sell yours for Bitcoin, from offers with each one’s value in dollars. Take one in Orien Wallet and the two wallets carry the swap through on their own: either both sides get paid, or both get their coins back. You need no ORN to buy your first.',
      'Sign in with your email and a 6-digit code, the same account on the website and in Orien Wallet. Signing in through Orien Wallet adds your wallet to your account, so buyers’ payments reach you, with nothing to pay or prove.',
      'Everything posted is checked before anyone sees it, every listing and profile has a Report link, and you can delete your account at any time. orien.network/privacy says what is kept and why.',
    ],
  },
  {
    date: '2026-11-15',
    products: ['wallet'],
    version: '1.3.0',
    title: 'Swaps, step by step',
    changes: [
      'Each swap shows its steps in plain words: what has happened, what happens next, who has to act, and about how long it takes.',
      'At every step it says what happens if the other side disappears, and about when your ORN or Bitcoin would come back.',
      'The offer and reply codes each come with a line on what they are and whom to send them to.',
      'Before you sell or buy, one screen says what you need, the fees, and the smallest swap.',
      'The smallest swap is now 0.00005 BTC, more when Bitcoin is busy: the sell and buy screens show the smallest right now, and the wallet says so before anything is paid.',
    ],
  },
  {
    date: '2026-11-15',
    products: ['validators', 'orien'],
    version: '1.4.0',
    title: 'Validators keep themselves up to date',
    changes: [
      'About once an hour, a validator looks for a new release of orien. It installs only one signed with the Orien release key, whose download matches its signed checksum.',
      'It then restarts on the new version at a calm moment: well inside an hour, at a random point, and only while every other validator is answering, so the network never stops.',
      'Validators started with --no-auto-update stay as they are, for operators who update by hand with orien update.',
    ],
  },
  {
    date: '2026-11-15',
    products: ['wallet'],
    version: '1.2.1',
    title: 'Cleaner activity after mining rewards',
    changes: ['No stray "Received" line after each hour’s mining reward.'],
  },
  {
    date: '2026-11-15',
    products: ['wallet'],
    version: '1.2.0',
    title: 'Every payment and reward on its own line',
    changes: [
      'Activity shows one line for each payment you receive and each hour’s mining reward, read from the validators’ records, instead of one line for everything that came in while the app was closed.',
      'Orien Wallet for Mac, Windows and Linux, each updating itself.',
    ],
  },
  {
    date: '2026-11-15',
    products: ['validators', 'orien'],
    version: '1.3.1',
    title: 'Validators keep a record of incoming payments',
    changes: [
      'Each validator records what comes into every account: payments, claimed and refunded locked payments, fee shares, mining rewards and unstaked coins.',
      'Wallets can ask for it, to show each one separately.',
    ],
  },
  {
    date: '2026-11-15',
    products: ['wallet'],
    version: '1.1.0',
    title: 'Swap ORN for Bitcoin',
    changes: [
      'Sell ORN for Bitcoin, or buy ORN with Bitcoin, with anyone. Nobody sits in the middle, and nobody can take the other side’s coins.',
      'The app makes a one-time Bitcoin address for each swap, watches the Bitcoin network, and claims or refunds for you.',
    ],
  },
  {
    date: '2026-11-15',
    products: ['network', 'validators', 'orien'],
    version: '1.3.0',
    title: 'Locked payments and swaps',
    changes: [
      'A locked payment sends ORN that the receiver only gets by revealing a secret before a deadline. Otherwise it goes back to the sender at the deadline’s checkpoint.',
      'This lets two people swap ORN for Bitcoin with no one in the middle.',
      'New commands: orien escrow, orien claim and orien escrow-status.',
      'Open on the live network from epoch 15, 12:11 UTC on 15 November 2026.',
    ],
  },
  {
    date: '2026-11-14',
    products: ['orien'],
    version: '1.2.4',
    title: 'Restoring a wallet works the same everywhere',
    changes: ['Restoring from the 24-word recovery phrase gives the same wallet, address and coins on every computer.'],
  },
  {
    date: '2026-11-14',
    products: ['wallet'],
    version: '1.0.0',
    title: 'Orien Wallet',
    changes: [
      'A desktop app: make a wallet with its 24-word recovery phrase, see your balance, send and receive ORN, keep an address book, and see your payments.',
      'It uses the same wallet as orien, so you can use both. It updates itself with releases signed with the Orien update key.',
    ],
  },
  {
    date: '2026-11-14',
    products: ['validators', 'orien'],
    version: '1.2.3',
    title: 'Mining limits made plain',
    changes: [
      'Each validator counts up to 40,000 mining shares an hour. Shares already found still count, and the miner says so when validators have counted all they take.',
    ],
  },
  {
    date: '2026-11-14',
    products: ['orien'],
    version: '1.2.2',
    title: 'Mining asks how hard to work',
    changes: ['orien mine asks how much of your processor to use.', 'orien balance shows your balance plainly.'],
  },
  {
    date: '2026-11-14',
    products: ['orien'],
    version: '1.2.1',
    title: 'The skill for any AI assistant',
    changes: ['orien skill install puts the skill where any AI assistant finds it, as well as Claude Code’s.'],
  },
  {
    date: '2026-11-14',
    products: ['network'],
    title: 'The Orien network goes live',
    changes: [
      'The live network starts at 21:11 UTC, anchored to Bitcoin block 975,847, with four launch validators.',
      'There are no coins at the start: every ORN comes from mining.',
    ],
  },
  {
    date: '2026-11-14',
    products: ['validators', 'orien'],
    version: '1.2.0',
    title: 'Stricter validators',
    changes: [
      'A validator caught cheating is barred for good.',
      'Payments to the network’s own accounts are refused, so coins cannot be lost there by mistake.',
    ],
  },
  {
    date: '2026-11-14',
    products: ['orien'],
    version: '1.1.1',
    title: 'Safer signing',
    changes: ['A wallet’s key never signs two different payments, even after one is let go.'],
  },
  {
    date: '2026-11-14',
    products: ['orien'],
    version: '1.1.0',
    title: 'Recovery phrases, passwords and AI assistants',
    changes: [
      'Wallets have a 24-word recovery phrase, and can have a password.',
      'orien unlock lets programs you trust, such as an AI assistant, sign for a while and up to an amount, without learning your password. orien lock ends it.',
      'orien skill install teaches AI assistants to use orien for you.',
    ],
  },
  {
    date: '2026-11-14',
    products: ['orien'],
    version: '1.0.0',
    title: 'The first release',
    changes: [
      'orien for Mac, Linux and Windows, with an installer.',
      'Every release is signed with the Orien release key, and orien update installs only signed releases.',
    ],
  },
]
