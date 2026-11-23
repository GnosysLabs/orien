import type { Metadata } from 'next'
import { Download } from 'lucide-react'
import { CopyText } from '@/components/CopyText'
import { pageMeta } from '@/lib/seo'

export const metadata: Metadata = pageMeta({
  title: 'Logos: the ORN coin icon and the Orien sun',
  description:
    'Download the Orien (ORN) coin icon and the Orien sun, as SVG and PNG, for exchanges, wallets, price trackers and anyone writing about Orien.',
  path: '/brand/',
})

const SIZES = [1024, 512, 256, 200, 128, 64, 32]

const LOGOS: { file: string; name: string; use: string }[] = [
  {
    file: 'orien-coin-round',
    name: 'Coin icon',
    use: 'The one to use wherever a coin is shown: in a list, on a chart or next to a trading pair. Works on light and dark pages.',
  },
  {
    file: 'orien-coin-square',
    name: 'Square coin icon',
    use: 'The same icon on a dark square, for places that crop the picture themselves, to a circle or a rounded square.',
  },
  {
    file: 'orien-mark',
    name: 'The Orien sun',
    use: 'The rising sun on its own, in the pearl colours, with no circle behind it. Works on light and dark pages.',
  },
]

const COLOURS: [string, string][] = [
  ['Dark background', '#0b0b0f'],
  ['Pearl pink', '#f4b6d4'],
  ['Pearl blue', '#b8c7ff'],
  ['Pearl violet', '#d3b6ff'],
  ['Pearl mint', '#a6ead6'],
  ['Pearl gold', '#f9dda4'],
]

const chip =
  'inline-flex h-9 items-center justify-center rounded-lg border border-line bg-raised px-3 text-sm font-semibold text-ink transition hover:border-pearl'

function Section({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <section className="space-y-5 rounded-2xl border border-line bg-surface p-6 sm:p-8">
      <h2 className="text-2xl font-bold tracking-tight text-ink">{title}</h2>
      {children}
    </section>
  )
}

/** /brand/: the coin icon and the sun, to download, for exchanges, trackers
 * and the press. The files are in public/brand/, drawn by the brand tools. */
export default function BrandPage() {
  return (
    <div className="mx-auto max-w-[900px] space-y-8 pb-16 pt-5 sm:pt-7">
      <header className="space-y-5">
        <p className="text-xs font-semibold uppercase tracking-[0.14em] text-muted">Brand</p>
        <h1 className="text-balance text-4xl font-bold tracking-tight text-ink sm:text-5xl">Logos</h1>
        <p className="max-w-2xl text-lg leading-relaxed text-muted">
          The ORN coin icon and the Orien sun, for exchanges, wallets, price trackers and anyone writing about Orien.
          Each comes as SVG, which stays sharp at any size, and as PNG with a transparent background.
        </p>
        <a
          href="/brand/orien-logo-pack.zip"
          download
          className="pearl-fill pearl-sheen inline-flex h-11 items-center gap-2 rounded-[10px] px-4 text-sm font-bold transition hover:brightness-105"
        >
          <Download className="h-4 w-4" strokeWidth={2.25} aria-hidden />
          Download all logos (zip, 1 MB)
        </a>
      </header>

      {LOGOS.map((logo) => (
        <Section key={logo.file} title={logo.name}>
          <div className="grid gap-6 sm:grid-cols-[15rem_1fr] sm:items-center">
            <div className="grid h-40 grid-cols-2 overflow-hidden rounded-xl border border-line">
              <div className="flex items-center justify-center bg-white">
                {/* eslint-disable-next-line @next/next/no-img-element */}
                <img src={`/brand/svg/${logo.file}.svg`} alt={`${logo.name} on white`} className="h-24 w-24" />
              </div>
              <div className="flex items-center justify-center bg-[#0b0b0f]">
                {/* eslint-disable-next-line @next/next/no-img-element */}
                <img src={`/brand/svg/${logo.file}.svg`} alt={`${logo.name} on dark`} className="h-24 w-24" />
              </div>
            </div>
            <div className="space-y-5">
              <p className="leading-relaxed text-muted">{logo.use}</p>
              <div className="flex flex-wrap items-center gap-2">
                <a href={`/brand/svg/${logo.file}.svg`} download className={`${chip} gap-1.5`}>
                  <Download className="h-3.5 w-3.5" strokeWidth={2.25} aria-hidden />
                  SVG
                </a>
                <span className="ml-2 mr-1 text-sm font-semibold text-muted">PNG</span>
                {SIZES.map((size) => (
                  <a key={size} href={`/brand/png/${logo.file}-${size}.png`} download className={chip}>
                    <span className="num">{size}</span>
                  </a>
                ))}
              </div>
              <p className="text-xs text-muted">Each PNG is square, its size in pixels.</p>
            </div>
          </div>
        </Section>
      ))}

      <Section title="Colours">
        <div className="grid gap-4 sm:grid-cols-2">
          {COLOURS.map(([name, hex]) => (
            <div key={hex} className="flex items-end gap-4">
              <span className="h-[50px] w-[50px] shrink-0 rounded-xl border border-line" style={{ background: hex }} />
              <CopyText text={hex} label={name} className="min-w-0 flex-1" />
            </div>
          ))}
        </div>
      </Section>

      <Section title="Please">
        <p className="leading-relaxed text-muted">
          Keep the icon’s proportions, colours and space around it as they are. Don’t stretch it, recolour it or add
          effects. For anything else, write to{' '}
          <a href="mailto:hello@orien.network" className="font-semibold text-ink underline underline-offset-4">
            hello@orien.network
          </a>
          .
        </p>
      </Section>
    </div>
  )
}
