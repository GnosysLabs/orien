import { CopySnippet } from '@/components/CopySnippet'
import type { Metadata } from 'next'
import { pageMeta } from '@/lib/seo'

export const metadata: Metadata = pageMeta({
  title: 'Sign in with Orien: for developers',
  description:
    'Let people sign in to your website or app with their Orien Network account and see the wallets they proved are theirs. Standard OpenID Connect, free, with no key to apply for.',
  path: '/developers/sign-in/',
})

const ISSUER = 'https://orien.network'

const EXAMPLE = `{
  "iss": "https://orien.network",
  "aud": "orn_9e7ea2879fb3f326a68270de",
  "sub": "15ab8d389f0a3a6f9c7eed22331c0e66",
  "name": "Satoshi",
  "preferred_username": "Satoshi",
  "picture": "https://orien.network/api/market/images/….jpg",
  "orien_banner": "https://orien.network/api/market/images/….jpg",
  "profile": "https://orien.network/u/Satoshi/",
  "orien_wallets": [
    {
      "address": "be5b7cc81cbe475fe002bee2597c30b0ae4f030434e93f12ade7f1a266c6258b",
      "name": "Savings"
    }
  ],
  "nonce": "…",
  "iat": 1795198269,
  "exp": 1795201869
}`

const CLAIMS: [string, string][] = [
  ['sub', 'The account’s id. It never changes, even if the person renames their account: use it to recognise them.'],
  ['name, preferred_username', 'Their Orien Network username, which they can change.'],
  ['picture', 'Their profile picture, when they have one.'],
  ['orien_banner', 'Their profile’s wide banner (about 3:1), when they have one.'],
  ['profile', 'Their public profile page on orien.network.'],
  [
    'orien_wallets',
    'The wallets the account proved are its own, by sending a small payment from each. Each has its 64-character address and the name the person gave it in Orien Wallet (null if none). The list can be empty.',
  ],
]

const ENDPOINTS: [string, string][] = [
  ['Discovery', `${ISSUER}/.well-known/openid-configuration`],
  ['Authorize', `${ISSUER}/oauth/authorize/`],
  ['Token', `${ISSUER}/api/market/oidc/token`],
  ['Userinfo', `${ISSUER}/api/market/oidc/userinfo`],
  ['Keys (JWKS)', `${ISSUER}/api/market/oidc/jwks`],
]

/** The button developers paste in: the sun and the words on the pearl.
 * Its address is wherever their sign-in library starts the flow. */
const BUTTON = `<a href="/auth/orien" class="orien-signin">
  <svg viewBox="0 0 512 512" width="28" height="28" aria-hidden="true"><g fill="currentColor"><rect x="239" y="74" width="36" height="66" rx="18"/><rect x="239" y="74" width="36" height="66" rx="18" transform="rotate(-44 256 298)"/><rect x="239" y="74" width="36" height="66" rx="18" transform="rotate(44 256 298)"/><path d="M138 298A118 118 0 0 1 374 298Z"/><rect x="76" y="338" width="360" height="40" rx="20"/><rect x="156" y="406" width="200" height="36" rx="18"/></g></svg>
  Sign in with Orien Network
</a>

<style>
  .orien-signin {
    display: inline-flex; align-items: center; justify-content: center; gap: 10px;
    height: 44px; padding: 0 24px; border-radius: 12px;
    background: linear-gradient(115deg, #f4b6d4 0%, #b8c7ff 20%, #d3b6ff 38%, #a6ead6 56%, #f9dda4 74%, #f4b6d4 100%);
    color: #0b0b0f; font: 700 14px/1 system-ui, sans-serif; text-decoration: none;
  }
  .orien-signin:hover { filter: brightness(1.05); }
</style>`

function Code({ children }: { children: string }) {
  return <code className="num break-all rounded-md border border-line bg-bg px-1.5 py-0.5 text-[0.85em] text-ink">{children}</code>
}

function Section({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <section className="space-y-4 rounded-2xl border border-line bg-surface p-6 sm:p-8">
      <h2 className="text-xl font-bold tracking-tight text-ink">{title}</h2>
      <div className="space-y-4 leading-relaxed text-muted">{children}</div>
    </section>
  )
}

export default function SignInDevelopersPage() {
  return (
    <div className="mx-auto max-w-[860px] space-y-8 pb-16 pt-10 sm:pt-14">
      <header className="space-y-3">
        <p className="text-xs font-semibold uppercase tracking-[0.14em] text-muted">For developers</p>
        <h1 className="text-3xl font-bold tracking-tight text-ink sm:text-4xl">Sign in with Orien</h1>
        <p className="max-w-2xl text-lg leading-relaxed text-muted">
          Let people sign in to your website or app with their Orien Network account, and see the wallets they
          proved are theirs. It’s standard OpenID Connect, so the sign-in library you already use works. Free, with
          nothing to apply for.
        </p>
      </header>

      <Section title="What people see">
        <p>
          They press your Sign in with Orien Network button and come to orien.network. If they aren’t signed in there
          yet, they sign in with a code sent to their email, or with Orien Wallet’s browser extension. Then they see your website’s
          name and address, and exactly what you’ll receive: their Orien Network name, picture and banner, and their
          verified wallets. They choose Allow or Cancel, and come back to you.
        </p>
        <p>
          You never receive their email address, and nothing you get can move ORN or use their wallets. The next
          time, people who allowed you before come straight back.
        </p>
      </Section>

      <Section title="1. Register your website">
        <p>
          Sign in at{' '}
          <a href="/developers/websites/" className="font-semibold text-ink underline underline-offset-4">
            orien.network/developers/websites
          </a>{' '}
          and choose Add a website. Give its name, shown to people when they sign in, and
          its return addresses: where people come back to after signing in. They must start with https://, or
          http://localhost while you build.
        </p>
        <p>
          Choose <span className="font-semibold text-ink">from its server</span> if your website has a server that
          can keep a secret: you get a client ID and a client secret, shown once. Choose{' '}
          <span className="font-semibold text-ink">from its pages only</span> for websites and apps with no server:
          you get a client ID and use PKCE instead of a secret.
        </p>
      </Section>

      <Section title="2. Point your sign-in library at Orien">
        <p>
          Most libraries need only the issuer, <Code>{ISSUER}</Code>, with your client ID, secret (if you have one)
          and return address. They find the rest in the discovery document. For reference:
        </p>
        <dl className="divide-y divide-line rounded-xl border border-line">
          {ENDPOINTS.map(([name, url]) => (
            <div key={name} className="grid gap-1 px-4 py-3 sm:grid-cols-[9rem_1fr] sm:gap-4">
              <dt className="text-sm font-semibold text-ink">{name}</dt>
              <dd className="num break-all text-sm text-ink">{url}</dd>
            </div>
          ))}
        </dl>
        <ul className="list-disc space-y-2 pl-5">
          <li>The authorization code flow: response_type=code.</li>
          <li>
            Scopes: <Code>openid</Code> is required. <Code>profile</Code> and <Code>wallets</Code> are accepted; the
            name, picture, banner and wallets come either way, as people are shown.
          </li>
          <li>
            PKCE with S256 is required for websites without a secret, and welcome for the rest. Send a nonce and
            state; both come back.
          </li>
          <li>
            A secret goes in an Authorization: Basic header, or as client_secret in the token request’s form.
          </li>
          <li>
            prompt=none answers at once, with login_required or consent_required if the person must act;
            prompt=consent always asks.
          </li>
          <li>ID tokens are signed with RS256; check them with the published keys. Tokens last an hour.</li>
        </ul>
      </Section>

      <Section title="3. What you receive">
        <p>In the ID token, and from the userinfo endpoint with the access token:</p>
        <pre className="num overflow-auto rounded-xl border border-line bg-bg p-4 text-xs leading-relaxed text-ink">
          {EXAMPLE}
        </pre>
        <dl className="divide-y divide-line rounded-xl border border-line">
          {CLAIMS.map(([name, meaning]) => (
            <div key={name} className="grid gap-1 px-4 py-3 sm:grid-cols-[11rem_1fr] sm:gap-4">
              <dt className="num text-sm font-semibold text-ink">{name}</dt>
              <dd className="text-sm leading-relaxed text-muted">{meaning}</dd>
            </div>
          ))}
        </dl>
        <p>
          To check what a wallet holds or has received, read it from the{' '}
          <a href="https://explorer.orien.network/api/" className="font-semibold text-ink underline underline-offset-4">
            explorer’s API
          </a>
          .
        </p>
      </Section>

      <Section title="The button">
        <p>
          Use Orien’s own button so people recognise it: the sun and the words on the pearl. Copy it as it is, and
          point its address at wherever your sign-in library starts the flow.
        </p>
        <div className="flex justify-center rounded-xl border border-line bg-bg px-6 py-8">
          <span className="pearl-fill inline-flex h-11 items-center justify-center gap-2.5 rounded-xl px-6 text-sm font-bold">
            <span className="orien-mark shrink-0 bg-bg" style={{ width: 28, height: 28 }} aria-hidden />
            Sign in with Orien Network
          </span>
        </div>
        <CopySnippet text={BUTTON} />
        <p>
          The words are “Sign in with Orien Network”. Keep the button on a plain background with a little room around
          it, and don’t recolour it. Orien Wallet is the app; Orien Network is the account people sign in with.
        </p>
      </Section>

      <Section title="Good to know">
        <ul className="list-disc space-y-2 pl-5">
          <li>Label your button “Sign in with Orien”.</li>
          <li>
            Wallets in orien_wallets are proven by a payment from each, so they belong to that account. People can
            add and remove them at any time, so read them again at each sign-in.
          </li>
          <li>
            Websites that mislead people can be switched off, after which they sign nobody in and their tokens stop
            working.
          </li>
        </ul>
      </Section>
    </div>
  )
}
