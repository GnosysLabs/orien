import { CopySnippet } from '@/components/CopySnippet'
import { CopyText } from '@/components/CopyText'
import type { Metadata } from 'next'
import { ArrowUpRight } from 'lucide-react'
import { pageMeta } from '@/lib/seo'

export const metadata: Metadata = pageMeta({
  title: 'Sign in with Orien Wallet: for developers',
  description:
    'Let people sign in to your website with their Orien wallet: no email, no password, and no ORN moves. Your website checks the proof itself with Orien’s quantum-safe signatures, offline.',
  path: '/developers/sign-in/',
})

const SCRIPT = `<script src="https://orien.network/orien-sign-in.js"></script>`

const BUTTON = `<button type="button" class="orien-signin">
  <svg viewBox="0 0 512 512" width="28" height="28" aria-hidden="true"><g fill="currentColor"><rect x="239" y="74" width="36" height="66" rx="18"/><rect x="239" y="74" width="36" height="66" rx="18" transform="rotate(-44 256 298)"/><rect x="239" y="74" width="36" height="66" rx="18" transform="rotate(44 256 298)"/><path d="M138 298A118 118 0 0 1 374 298Z"/><rect x="76" y="338" width="360" height="40" rx="20"/><rect x="156" y="406" width="200" height="36" rx="18"/></g></svg>
  Sign in with Orien Wallet
</button>

<style>
  .orien-signin {
    display: inline-flex; align-items: center; justify-content: center; gap: 10px;
    height: 44px; padding: 0 24px; border: 0; border-radius: 12px; cursor: pointer;
    background: linear-gradient(115deg, #f4b6d4 0%, #b8c7ff 20%, #d3b6ff 38%, #a6ead6 56%, #f9dda4 74%, #f4b6d4 100%);
    background-size: 300% 300%; animation: orien-pearl 12s ease-in-out infinite;
    color: #0b0b0f; font: 700 14px/1 system-ui, -apple-system, sans-serif;
  }
  .orien-signin:hover { filter: brightness(1.05); }
  .orien-signin:disabled { opacity: 0.6; cursor: default; }
  @keyframes orien-pearl { 0%, 100% { background-position: 0% 50%; } 50% { background-position: 100% 50%; } }
  @media (prefers-reduced-motion: reduce) { .orien-signin { animation: none; } }
</style>`

const SUN = `<svg viewBox="0 0 512 512" width="28" height="28" aria-hidden="true"><g fill="currentColor"><rect x="239" y="74" width="36" height="66" rx="18"/><rect x="239" y="74" width="36" height="66" rx="18" transform="rotate(-44 256 298)"/><rect x="239" y="74" width="36" height="66" rx="18" transform="rotate(44 256 298)"/><path d="M138 298A118 118 0 0 1 374 298Z"/><rect x="76" y="338" width="360" height="40" rx="20"/><rect x="156" y="406" width="200" height="36" rx="18"/></g></svg>`

const PREVIEW_CSS = `  .orien-signin {
    display: inline-flex; align-items: center; justify-content: center; gap: 10px;
    height: 44px; padding: 0 24px; border: 0; border-radius: 12px; cursor: pointer; text-decoration: none;
    background: linear-gradient(115deg, #f4b6d4 0%, #b8c7ff 20%, #d3b6ff 38%, #a6ead6 56%, #f9dda4 74%, #f4b6d4 100%);
    background-size: 300% 300%; animation: orien-pearl 12s ease-in-out infinite;
    color: #0b0b0f; font: 700 14px/1 system-ui, -apple-system, sans-serif;
  }
  .orien-signin:hover { filter: brightness(1.05); }
  .orien-signin:disabled { opacity: 0.6; cursor: default; }
  @keyframes orien-pearl { 0%, 100% { background-position: 0% 50%; } 50% { background-position: 100% 50%; } }
  @media (prefers-reduced-motion: reduce) { .orien-signin { animation: none; } }`

const BROWSER = `document.querySelector(".orien-signin").onclick = async () => {
  const nonce = await fetch("/sign-in/nonce").then((r) => r.text())   // from your server
  try {
    const { address, proof } = await OrienSignIn.signIn({ nonce })
    await fetch("/sign-in", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ address, proof }),
    })
  } catch (error) {
    alert(error.message)   // plain sentences, fine to show people
  }
}`

const SERVER = `const { verifySignIn, newNonce } = require("./orien-sign-in.js")

// GET /sign-in/nonce: a one-time value, remembered until it is used.
const nonce = newNonce()

// POST /sign-in: throws with a plain reason if the proof isn't good.
const address = verifySignIn(proof, { origin: "https://yoursite.com", nonce })
// Accept each nonce once, forget it, then start the session for address.`

const PROFILE = `const who = await OrienSignIn.profile(address)
// null, or:
{
  name: "Satoshi",
  bio: "…",
  wallet: "Savings",
  picture: "https://orien.network/api/market/images/….jpg",
  banner: "https://orien.network/api/market/images/….jpg",
  profile: "https://orien.network/u/Satoshi/",
  links: [{ kind: "x", handle: "satoshi", url: "https://x.com/satoshi" }]
}`

function Code({ children }: { children: string }) {
  return <code className="num break-all rounded-md border border-line bg-bg px-1.5 py-0.5 text-[0.85em] text-ink">{children}</code>
}

function Section({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <section className="space-y-4 rounded-2xl border border-line bg-surface p-6 sm:p-8">
      <h2 className="text-xl font-bold tracking-tight text-ink">{title}</h2>
      <div className="space-y-4 leading-relaxed text-muted [&_strong]:font-semibold [&_strong]:text-ink">{children}</div>
    </section>
  )
}

function Rows({ rows }: { rows: [string, React.ReactNode][] }) {
  return (
    <dl className="divide-y divide-line rounded-xl border border-line">
      {rows.map(([name, meaning]) => (
        <div key={name} className="grid gap-1 px-4 py-3 sm:grid-cols-[11rem_1fr] sm:gap-4">
          <dt className="text-sm font-semibold text-ink">{name}</dt>
          <dd className="text-sm leading-relaxed text-muted">{meaning}</dd>
        </div>
      ))}
    </dl>
  )
}

const link = 'font-semibold text-ink underline underline-offset-4'

/** /developers/sign-in/: Sign in with Orien Wallet, for websites. The
 * checker is public/orien-sign-in.js, a copy of apps/wallet/signin-js. */
export default function SignInDevelopersPage() {
  return (
    <div className="mx-auto max-w-[860px] space-y-8 pb-16 pt-5 sm:pt-7">
      <header className="space-y-4">
        <p className="text-xs font-semibold uppercase tracking-[0.14em] text-muted">For developers</p>
        <h1 className="text-balance text-3xl font-bold tracking-tight text-ink sm:text-4xl">Sign in with Orien Wallet</h1>
        <p className="max-w-2xl text-lg leading-relaxed text-muted">
          A person’s Orien wallet is their login: no email, no password, and no ORN moves. Your website checks the
          proof itself, offline, with Orien’s quantum-safe signatures. Free, with nothing to register.
        </p>
        <div className="flex flex-wrap gap-2 pt-1">
          <a
            href="/developers/sign-in/demo/"
            className="pearl-fill pearl-sheen inline-flex h-11 items-center gap-2 rounded-[10px] px-4 text-sm font-bold transition hover:brightness-105"
          >
            Try the demo
            <ArrowUpRight className="h-4 w-4" strokeWidth={2.25} aria-hidden />
          </a>
          <a
            href="/orien-sign-in.js"
            download
            className="inline-flex h-11 items-center rounded-[10px] border border-line bg-raised px-4 text-sm font-semibold text-ink transition hover:border-pearl"
          >
            Download orien-sign-in.js
          </a>
        </div>
      </header>

      <Section title="How it works">
        <p>
          It works the way Phantom and MetaMask sign-ins do. People click your sign-in button and approve in the Orien
          Wallet browser extension, for Chrome, Edge, Brave, Firefox and Safari. Your website receives their wallet
          address and a proof that the wallet signed in to your website, just now.
        </p>
        <p>
          Your server checks the proof by itself, with no network and no server to ask, orien.network included. Nothing
          a website receives can move ORN.
        </p>
        <p>
          People need Orien Wallet running on their computer. Phones aren’t supported yet.
        </p>
      </Section>

      <Section title="1. Add the script to your page">
        <CopyText text={SCRIPT} />
        <p>
          It has no dependencies and is MIT licensed. The same file checks proofs on your server.
        </p>
      </Section>

      <Section title="2. Make a nonce on your server">
        <p>
          A nonce is a one-time value: any string up to 200 characters. Your server makes one for each sign-in and
          remembers it. <Code>OrienSignIn.newNonce()</Code> makes a good one.
        </p>
      </Section>

      <Section title="3. Add the button">
        <p>
          The same button as on orien.network, so people recognise it. Copy it as it is: it needs no images or fonts.
        </p>
        <div className="flex justify-center rounded-xl border border-line bg-bg px-4 py-8">
          <style>{PREVIEW_CSS}</style>
          <a href="/developers/sign-in/demo/" className="orien-signin" aria-label="Sign in with Orien Wallet: try the demo">
            <span dangerouslySetInnerHTML={{ __html: SUN }} className="flex" />
            Sign in with Orien Wallet
          </a>
        </div>
        <CopySnippet text={BUTTON} />
      </Section>

      <Section title="4. Sign in when it’s clicked">
        <p>
          <Code>OrienSignIn.signIn(&#123; nonce &#125;)</Code> asks the extension, and resolves once the person
          approves. Send the address and proof to your server.
        </p>
        <CopySnippet text={BROWSER} />
      </Section>

      <Section title="5. Check the proof on your server">
        <p>
          Download <a href="/orien-sign-in.js" download className={link}>orien-sign-in.js</a> next to your server’s
          code. It runs in Node 18 and later, Deno and Bun.
        </p>
        <CopySnippet text={SERVER} />
        <p>
          <Code>verifySignIn</Code> returns the wallet’s address, or throws with a plain reason. Accept each nonce once,
          then start the session.
        </p>
      </Section>

      <Section title="6. Show who signed in (optional)">
        <p>
          People who verified their wallet on orien.network have a public profile. <Code>OrienSignIn.profile(address)</Code>{' '}
          returns it, or null for a wallet that isn’t verified. Links hold only what the person chose to show.
        </p>
        <CopySnippet text={PROFILE} />
      </Section>

      <Section title="Details">
        <Rows
          rows={[
            [
              'Proofs',
              <>
                Good for 5 minutes, and only for the exact origin the browser reports: scheme, host and port, such as{' '}
                <Code>https://yoursite.com</Code>.
              </>,
            ],
            [
              'The raw API',
              <>
                The extension adds <Code>window.orien</Code>, with <Code>window.orien.signIn(&#123; nonce &#125;)</Code>,
                and fires <Code>orien#initialized</Code> on the page when it does.
              </>,
            ],
            ['Errors', <>Plain sentences, fine to show to people.</>],
            [
              'Without the extension',
              <>
                <Code>signIn()</Code> throws “Orien Wallet isn’t set up for this website yet…”. Send people to{' '}
                <a href="/" className={link}>
                  orien.network
                </a>{' '}
                for Orien Wallet and its extension.
              </>,
            ],
          ]}
        />
      </Section>

      <Section title="See it working">
        <p>
          <a href="/developers/sign-in/demo/" className={link}>
            The demo
          </a>{' '}
          is a whole website on one page: it makes the nonce, signs in, checks the proof and shows the profile. View its
          source for a complete example.
        </p>
      </Section>
    </div>
  )
}
