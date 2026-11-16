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
  /** Written ahead of its release: left off the page until then. */
  unreleased?: boolean
}

export const PRODUCTS: Record<Product, string> = {
  network: 'Network',
  orien: 'orien',
  validators: 'Validators',
  wallet: 'Orien Wallet',
  marketplace: 'Market',
}

export const CHANGELOG: Entry[] = [
  {
    date: '2026-11-16',
    products: ['wallet'],
    version: '1.8.5',
    title: 'Orien Market, right in your wallet',
    changes: [
      'Browse Orien Market in the app: search, categories, listings with their pictures, and sellers\' pages.',
      'Message sellers and buyers in the app. When a seller asks for payment, pay it right in the chat with one tap, no copying addresses.',
      'Sell from the app: post and edit listings with up to four pictures, and close or reopen them.',
      'Contacts with an Orien profile show their own name and picture, and stay up to date when they change them.',
      'Your profile saves as you type, and each wallet can get its icon when you add it.',
    ],
  },
  {
    date: '2026-11-16',
    products: ['wallet'],
    version: '1.8.4',
    title: 'Names, pictures and a fresh look',
    changes: [
      'People with an Orien Market profile show by name and picture in Send, Contacts and Activity, and you can find them by typing their username in Send.',
      'Your profile now lives on the home screen: tap your picture to change your picture, banner, bio and links, and to verify which of your wallets are yours.',
      'Receive shows a QR code of your address, with Share on computers that have a share panel.',
      'Give each wallet an emoji, and a cleaner wallet list.',
      'On Windows, the window has its own sleek top bar. Everywhere, softer scrolling edges, no scroll bars, and buttons that never squeeze.',
      'Prices under a cent show properly, and amounts show their dollar value as you type.',
    ],
  },
  {
    date: '2026-11-16',
    products: ['wallet'],
    version: '1.8.3',
    title: 'Swap ORN for SOL',
    changes: [
      'Swap ORN and SOL with someone directly, as with Bitcoin but in minutes. SOL is now the first choice when you post an offer; Bitcoin is one tap away.',
      'Buyers pay from the SOL wallet they already have, such as Phantom, by scanning a code. The smallest swap is 0.01 SOL.',
      'If you type an address that can never receive SOL, the app tells you before you post.',
    ],
  },
  {
    date: '2026-11-16',
    products: ['network', 'marketplace'],
    title: 'SOL swaps are open',
    changes: [
      'Orien’s lock program is live on Solana: a 4,016-byte program that holds a swap’s SOL until the seller shows the secret or the deadline passes, and that can only pay the two people named in the lock. It was reviewed, tried with real SOL, then made permanent, so nobody, including us, can ever change it.',
      'Buy & sell lists SOL offers first, with Bitcoin offers below, and How it works and the technical paper describe swaps with SOL.',
    ],
  },
  {
    date: '2026-11-16',
    products: ['wallet'],
    version: '1.8.2',
    title: 'A clear next step after taking an offer',
    changes: [
      'After you take a swap offer, a screen shows what you get and what you pay, and waits for the seller to accept. The pay screen then opens by itself.',
    ],
  },
  {
    date: '2026-11-16',
    products: ['wallet'],
    version: '1.8.1',
    title: 'Clearer when a seller has been paid',
    changes: [
      'Once your wallet sends a buyer’s Bitcoin on to you, the swap moves to its last step: “Paid · on its way to your Bitcoin address”, pending in your Bitcoin wallet until Bitcoin confirms it.',
      'Each swap step also makes the Orien Wallet icon in the Dock or taskbar ask for attention.',
      'The smallest swap is now 0.00015 BTC, so any Bitcoin wallet can send it and its fees leave enough.',
    ],
  },
  {
    date: '2026-11-16',
    products: ['wallet'],
    version: '1.8.0',
    title: 'Approve a sale once, shorter swaps, and steadier Bitcoin checks',
    changes: [
      'Posting a swap offer asks for Touch ID (or your password) once. When someone takes the offer, your wallet accepts it within seconds, even with the window closed. The approval covers only that offer and is erased when it ends.',
      'Swaps hold coins for less time: if a buyer never pays, the seller’s ORN come back within 12 hours (was 48), and an unclaimed payment comes back to the buyer after about 6 hours (was 12). Taking an offer needs Orien Wallet 1.8.0 or later.',
      'The app checks Bitcoin through three independent services and needs two of them to agree, so one that is busy or down no longer holds a swap up. It also asks them far less often.',
      'The seller sees the buyer’s payment the moment it’s sent, and both sides count its confirmations (1 of 3, 2 of 3…).',
      'The time to pay is the real one, a few hours, shown as “Pay by 6:15 AM, in about 9 hours”.',
      'More space between the swap card and your balance on the home screen.',
    ],
  },
  {
    date: '2026-11-16',
    products: ['wallet'],
    version: '1.7.1',
    title: 'Swaps you can’t miss',
    changes: [
      'When a swap needs you, an amber card at the top of the home screen says exactly what to do, with a countdown and one button, and the Dock or taskbar icon shows a number.',
      'A notification for every step the other side completes: your offer taken, your swap accepted, payment seen, swap done. “Pay now” repeats every 10 minutes until you’ve paid.',
      'Paying for a swap is one box: the exact amount, a QR code for any Bitcoin wallet, Copy address and Copy amount buttons, and how long you have. The app sees the payment by itself.',
      'Swaps from the board show no codes, and their steps are simply Accepted, Pay, Confirming and Done.',
      'Before you take an offer, the app makes clear that the seller has 30 minutes to accept.',
      'On a Mac, Orien Wallet opens at login the Mac’s own way, so System Settings and the login message name Orien Wallet.',
    ],
  },
  {
    date: '2026-11-16',
    products: ['wallet'],
    version: '1.7.0',
    title: 'Runs in the background, notifies you of payments, and swaps from any device',
    changes: [
      'Closing the window keeps Orien Wallet running, with an Orien icon in the menu bar (Mac) or system tray (Windows, Linux), and it starts by itself when you log in. Choose Quit from the icon’s menu to stop it.',
      'A notification tells you when someone sends you ORN.',
      'Swap offers can be finished by any of your devices that has the offer’s wallet, and only one of them ever runs a swap. When someone takes your offer, you’re notified and asked for Touch ID (or your password) once, and the rest happens by itself.',
      'When you make a wallet, you can save the recovery phrase as a text file.',
      'The Orien sunrise shows while the app opens, until your balance and activity are ready.',
      'The marketplace is called the Market throughout the app.',
    ],
  },
  {
    date: '2026-11-15',
    products: ['orien'],
    version: '1.8.1',
    title: 'The AI assistant skill is gone',
    changes: [
      'orien no longer installs a skill for AI assistants: Orien Wallet does everything it did, more simply. Updating removes the copies earlier versions installed.',
    ],
    unreleased: true,
  },
  {
    date: '2026-11-16',
    products: ['marketplace'],
    title: 'The smallest swap is now 0.00015 BTC',
    changes: [
      'Swap offers start at 0.00015 BTC (about $10), up from 0.00005 BTC: an amount ordinary Bitcoin wallets can send, and enough that Bitcoin’s fees on a busy day still leave the swap worth finishing.',
    ],
  },
  {
    date: '2026-11-16',
    products: ['marketplace'],
    title: 'Shorter swaps need Orien Wallet 1.8.0',
    changes: [
      'Taking a swap offer needs Orien Wallet 1.8.0 or later, whose shorter locks keep a seller’s ORN for at most 12 hours and a buyer’s Bitcoin for about 6 if the other side stops. An older wallet is asked to update before anything is locked.',
    ],
  },
  {
    date: '2026-11-16',
    products: ['marketplace'],
    title: 'Buy and sell ORN in Orien Wallet',
    changes: [
      'Buying and selling ORN now happen only in Orien Wallet, which holds your keys and runs each swap. The website’s Buy & sell ORN page shows the live offers and prices and explains how it works.',
    ],
  },
  {
    date: '2026-11-16',
    products: ['marketplace'],
    title: 'Fairer swap holds',
    changes: [
      'When a seller’s wallet doesn’t start a taken swap in time, the offer goes back on the board and the buyer is no longer counted as having let it lapse.',
      'Swap offers can be posted on the website too: choose which of your wallets sells from the Sell from menu, and any of your devices running Orien Wallet 1.7.0 or later finishes the swap.',
    ],
  },
  {
    date: '2026-11-15',
    products: ['marketplace'],
    title: 'A contact page',
    changes: ['orien.network/contact: write to the Orien team from the website, and we reply by email.'],
  },
  {
    date: '2026-11-15',
    products: ['marketplace'],
    title: 'Pictures of any size',
    changes: [
      'Big pictures straight from a phone or camera upload fine: your browser shrinks them before sending, for listings, profile pictures and banners.',
    ],
  },
  {
    date: '2026-11-15',
    products: ['wallet'],
    version: '1.6.1',
    title: 'Made for iPhone, Face ID and Touch ID by default, and smoother sending',
    changes: [
      'The iPhone app looks and feels like an iPhone app: Apple’s text sizes, a Back button with the screen’s title, rounded lists, and its own app icon.',
      'On iPhones and Macs, new and restored wallets are locked with Face ID or Touch ID from the start, with no password to choose. Wallets that had no lock get one. If you use orien in Terminal on a Mac, update it to 1.8.0 first.',
      'Your recent activity scrolls in its own box on the home screen, and All activity is one box with each day labelled.',
      'Send: type a contact’s or one of your wallets’ names and pick it from the suggestions. A chosen recipient shows by name, or by the two ends of the address, and Paste becomes Clear. Send all is a button under the balance, and long amounts shrink to fit.',
      'Switching wallets shows the Orien sunrise until the wallet is ready, and the balance no longer jumps in size.',
      'A payment sent from your other device shows once, not three times, and wallets first opened before 1.6.0 now show their whole past.',
      'On a Mac, the app reopens after an update in the Dock icon you keep.',
    ],
  },
  {
    date: '2026-11-15',
    products: ['orien'],
    version: '1.8.0',
    title: 'Touch ID in orien on a Mac',
    changes: [
      'When Orien Wallet locks a wallet with Touch ID, orien uses Touch ID too: sending asks for your fingerprint, or your Mac’s password, instead of a typed wallet password. Checking a balance and mining never ask.',
      'If you use Orien Wallet on a Mac, run orien update before its next version locks your wallet with Touch ID.',
    ],
  },
  {
    date: '2026-11-15',
    products: ['wallet'],
    version: '1.6.0',
    title: 'Contacts on every device, Touch ID, and your full history',
    changes: [
      'Your contacts follow your wallet: add, rename or remove one on one device and the change shows on your others. They are sealed with a key from your recovery phrase, so only your wallet can read them, and no account is needed.',
      'On a Mac with Touch ID or an iPhone with Face ID, unlock your wallet with your finger or face instead of typing its password. On Windows and Linux, restoring suggests the same password as on your other devices.',
      'A restored wallet shows its history from the start: what it received and, from today, what it sent.',
      'On a Mac, the app reopens after an update in the Dock icon you keep, instead of in a second icon. (This starts with the next update after 1.6.0.)',
    ],
  },
  {
    date: '2026-11-15',
    products: ['wallet'],
    version: '1.5.2',
    title: 'Mining fixed on Macs, and a smoother slider',
    changes: [
      'Mining on a Mac no longer closes the app once RandomX starts.',
      'Moving the processor slider while mining changes the speed within a few seconds, without getting ready again.',
      'Swaps: once the seller’s wallet takes the Bitcoin, it delivers the buyer’s ORN too, so the buyer gets them even with their app closed.',
    ],
  },
  {
    date: '2026-11-15',
    products: ['orien', 'validators', 'marketplace'],
    version: '1.7.0',
    title: 'History of what you sent, and a home for your contacts',
    changes: [
      'Validators now also record each payment an account sends, so a wallet restored on another device can show what it sent as well as what it received, from this release on.',
      'Your wallet’s contacts can now follow it from device to device with no account: they are sealed with a key from your recovery phrase and kept on orien.network, which cannot read them. Orien Wallet uses it from its next version.',
    ],
  },
  {
    date: '2026-11-15',
    products: ['network', 'orien', 'validators'],
    version: '1.6.0',
    title: 'Swaps finish for the buyer even when their wallet is closed',
    changes: [
      'From epoch 23, at 20:11 UTC on 15 November 2026, anyone who knows a locked payment’s secret can claim it for its payee: the coins can only ever go to the payee. In a swap, the seller’s wallet now claims the buyer’s ORN for them as soon as it has taken the Bitcoin, so a buyer who pays and then closes their wallet still gets their ORN.',
      'If two claims for one locked payment arrive at once, the payee is paid once; the other claim pays nothing and costs nothing.',
      'Validators let a new mining share wait a moment for its turn to be checked, instead of turning it away when busy, and check each share only once however many times it arrives.',
      'orien mine and Orien Wallet can change how much of the processor mining uses without getting RandomX ready again.',
    ],
  },
  {
    date: '2026-11-15',
    products: ['wallet'],
    version: '1.5.1',
    title: 'Room for your latest activity, and quicker updates',
    changes: [
      'The window opens tall enough to show your three latest payments in full, and can’t be made shorter than that.',
      'While open, the app looks for a new version every hour instead of every six, so it’s ready soon after a release.',
    ],
  },
  {
    date: '2026-11-15',
    products: ['network', 'orien', 'validators'],
    version: '1.5.0',
    title: 'Mining moves to RandomX, so any computer can take part',
    changes: [
      'From epoch 21, at 18:11 UTC on 15 November 2026, mining uses RandomX, the mining math Monero has used since 2019. Each try runs a small random program over 2 GB of memory: work an ordinary processor is built for, where graphics cards and custom chips gain little. Mining stays open to anyone with a computer.',
      'Miners: run orien update once. From this version on, orien mine keeps itself up to date: it installs each new release and carries on mining on it. Mining needs about 2 GB of memory and spends up to a minute getting ready at each hourly checkpoint; with less memory it mines in a slower light mode.',
      'The mining difficulty starts again at the switch, at about one share in 400,000 tries, and adjusts each hour as before. Balances, payments and history are unchanged.',
      'orien is no longer built for Macs with Intel chips.',
    ],
  },
  {
    date: '2026-11-15',
    products: ['wallet'],
    version: '1.5.0',
    title: 'Several wallets, and mining in the app',
    changes: [
      'Keep several wallets in one app: tap the wallet’s name above the balance to switch, rename them (Main, Mining…), add a new one or restore one from its recovery phrase. Each has its own address, recovery phrase and activity. Removing one only takes it off the list.',
      'Mine ORN from the app, with RandomX: Start and Stop, a processor slider, and live speed, shares this hour and your last mining reward. It mines to the open wallet while the app is open, and the Mine button on Home shows when it is on.',
      'The Swap screen has two tabs, Offers and Your swaps. Swaps keep the wallet they started with, and the screen says which wallet marketplace payments go to, with a button to change it.',
      'Sellers’ photos show on the Swap screen, with their initial while a photo loads.',
    ],
  },
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
