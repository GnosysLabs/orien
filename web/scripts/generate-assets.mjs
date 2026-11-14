import { mkdir, writeFile } from 'node:fs/promises'
import path from 'node:path'
import { fileURLToPath } from 'node:url'
import sharp from 'sharp'

const __dirname = path.dirname(fileURLToPath(import.meta.url))
const PUBLIC = path.join(__dirname, '..', 'public')

const PEARL_STOPS = `
  <stop offset="0" stop-color="#f4b6d4"/>
  <stop offset="0.25" stop-color="#b8c7ff"/>
  <stop offset="0.48" stop-color="#d3b6ff"/>
  <stop offset="0.72" stop-color="#a6ead6"/>
  <stop offset="1" stop-color="#f9dda4"/>
`

// Geometric rising sun over horizon (oriens = rising), built to stay crisp down to 16px.
const MARK_ELEMENTS = (fill = '#0b0b0f') => `
  <g fill="${fill}">
    <rect x="239" y="92" width="34" height="58" rx="17" />
    <rect x="239" y="92" width="34" height="58" rx="17" transform="rotate(-44 256 298)" />
    <rect x="239" y="92" width="34" height="58" rx="17" transform="rotate(44 256 298)" />
    <path d="M150 298 A106 106 0 0 1 362 298 Z" />
    <rect x="92" y="332" width="328" height="36" rx="18" />
    <rect x="166" y="394" width="180" height="32" rx="16" />
  </g>
`

// The sunrise mark as the header shows it, in the pearl gradient on a
// transparent background, cropped close so it fills a browser tab.
// In a light browser theme the pastel pearl is faint on a pale tab bar, so a
// deeper version of the same colours takes over there.
const DEEP_STOPS = `
  <stop offset="0" stop-color="#d9468f"/>
  <stop offset="0.25" stop-color="#4f6bf0"/>
  <stop offset="0.48" stop-color="#8b4ff0"/>
  <stop offset="0.72" stop-color="#13a37e"/>
  <stop offset="1" stop-color="#d99a14"/>
`

const faviconSvg = `<svg xmlns="http://www.w3.org/2000/svg" viewBox="56 58 400 400">
  <defs>
    <linearGradient id="p" x1="76" y1="74" x2="436" y2="442" gradientUnits="userSpaceOnUse">${PEARL_STOPS}</linearGradient>
    <linearGradient id="d" x1="76" y1="74" x2="436" y2="442" gradientUnits="userSpaceOnUse">${DEEP_STOPS}</linearGradient>
  </defs>
  <style>
    .mark { fill: url(#p); }
    @media (prefers-color-scheme: light) { .mark { fill: url(#d); } }
  </style>
  <g class="mark">
    <rect x="239" y="74" width="36" height="66" rx="18" />
    <rect x="239" y="74" width="36" height="66" rx="18" transform="rotate(-44 256 298)" />
    <rect x="239" y="74" width="36" height="66" rx="18" transform="rotate(44 256 298)" />
    <path d="M138 298 A118 118 0 0 1 374 298 Z" />
    <rect x="76" y="338" width="360" height="40" rx="20" />
    <rect x="156" y="406" width="200" height="36" rx="18" />
  </g>
</svg>`

const iconRoundedSvg = `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 512 512">
  <defs>
    <linearGradient id="p" x1="0" y1="0" x2="1" y2="1">${PEARL_STOPS}</linearGradient>
  </defs>
  <rect width="512" height="512" rx="112" fill="url(#p)"/>
  ${MARK_ELEMENTS('#0b0b0f')}
</svg>`

const iconSquareSvg = `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 512 512">
  <defs>
    <linearGradient id="p" x1="0" y1="0" x2="1" y2="1">${PEARL_STOPS}</linearGradient>
  </defs>
  <rect width="512" height="512" fill="url(#p)"/>
  ${MARK_ELEMENTS('#0b0b0f')}
</svg>`

const rings = Array.from(
  { length: 28 },
  (_, i) => `<circle cx="600" cy="-60" r="${70 + i * 32}" />`,
).join('')

const ogSvg = `<svg xmlns="http://www.w3.org/2000/svg" width="1200" height="630" viewBox="0 0 1200 630">
  <defs>
    <linearGradient id="pearl" x1="0" y1="0" x2="1" y2="1">${PEARL_STOPS}</linearGradient>
    <linearGradient id="ringGrad" x1="0" y1="0" x2="1" y2="1">
      <stop offset="0" stop-color="#ff8ec6"/>
      <stop offset="0.2" stop-color="#8aa6ff"/>
      <stop offset="0.38" stop-color="#b483ff"/>
      <stop offset="0.56" stop-color="#5fe0bd"/>
      <stop offset="0.74" stop-color="#ffcf70"/>
      <stop offset="1" stop-color="#ff8ec6"/>
    </linearGradient>
    <radialGradient id="topGlow" cx="50%" cy="0%" r="65%">
      <stop offset="0%" stop-color="#d3b6ff" stop-opacity="0.20"/>
      <stop offset="100%" stop-color="#0b0b0f" stop-opacity="0"/>
    </radialGradient>
    <linearGradient id="fadeDown" x1="0" y1="0" x2="0" y2="1">
      <stop offset="0.08" stop-color="#ffffff" stop-opacity="1"/>
      <stop offset="0.92" stop-color="#ffffff" stop-opacity="0"/>
    </linearGradient>
    <mask id="ringMask">
      <rect width="1200" height="630" fill="url(#fadeDown)"/>
    </mask>
  </defs>

  <rect width="1200" height="630" fill="#0b0b0f"/>
  <rect width="1200" height="630" fill="url(#topGlow)"/>
  <g mask="url(#ringMask)" fill="none" stroke="url(#ringGrad)" stroke-width="1.6" opacity="0.26">
    ${rings}
  </g>

  <!-- Centered lockup: rising-horizon badge + geometric ORIEN wordmark -->
  <g transform="translate(425, 118)">
    <g transform="scale(0.145)">
      <rect width="512" height="512" rx="116" fill="url(#pearl)"/>
      ${MARK_ELEMENTS('#0b0b0f')}
    </g>
    <text x="94" y="57" font-family="Jost, Futura, Avenir Next, Helvetica Neue, sans-serif" font-size="60" font-weight="800" letter-spacing="8" fill="url(#pearl)">ORIEN</text>
  </g>

  <text x="600" y="288" text-anchor="middle" font-family="Geist, Inter, -apple-system, Helvetica Neue, sans-serif" font-size="47" font-weight="800" letter-spacing="-1" fill="#ececf1">
    Quantum-proof payments, final in milliseconds.
  </text>
  <text x="600" y="344" text-anchor="middle" font-family="Geist, Inter, -apple-system, Helvetica Neue, sans-serif" font-size="27" font-weight="500" fill="#8d8d9e">
    No blocks, no miners and no waiting in line behind other payments.
  </text>

  <!-- Feature pills -->
  <g transform="translate(142, 416)" font-family="Geist, Inter, -apple-system, Helvetica Neue, sans-serif" font-size="22" font-weight="600" fill="#ececf1">
    <rect x="0" y="0" width="272" height="56" rx="28" fill="#1c1c26" stroke="#262633" stroke-width="2"/>
    <circle cx="32" cy="28" r="6" fill="#a6ead6"/>
    <text x="50" y="36">25–162 ms finality</text>

    <rect x="292" y="0" width="286" height="56" rx="28" fill="#1c1c26" stroke="#262633" stroke-width="2"/>
    <circle cx="324" cy="28" r="6" fill="#d3b6ff"/>
    <text x="342" y="36">BLAKE3 hash-only</text>

    <rect x="598" y="0" width="318" height="56" rx="28" fill="#1c1c26" stroke="#262633" stroke-width="2"/>
    <circle cx="630" cy="28" r="6" fill="#f9dda4"/>
    <text x="648" y="36">100M cap · 0 pre-mine</text>
  </g>

  <text x="600" y="562" text-anchor="middle" font-family="Geist Mono, SFMono-Regular, Menlo, monospace" font-size="20" fill="#8d8d9e">
    orien.network · GnosysLabs LLC
  </text>
</svg>`

await mkdir(PUBLIC, { recursive: true })
await writeFile(path.join(PUBLIC, 'icon.svg'), faviconSvg)
await sharp(Buffer.from(iconRoundedSvg)).resize(192, 192).png().toFile(path.join(PUBLIC, 'icon-rounded-192.png'))
await sharp(Buffer.from(iconRoundedSvg)).resize(512, 512).png().toFile(path.join(PUBLIC, 'icon-rounded-512.png'))
await sharp(Buffer.from(iconSquareSvg)).resize(180, 180).png().toFile(path.join(PUBLIC, 'apple-icon.png'))
await sharp(Buffer.from(faviconSvg), { density: 384 }).resize(32, 32).png().toFile(path.join(PUBLIC, 'favicon-32.png'))
await sharp(Buffer.from(ogSvg)).resize(1200, 630).png().toFile(path.join(PUBLIC, 'og.png'))

await writeFile(
  path.join(PUBLIC, 'robots.txt'),
  `User-agent: *\nAllow: /\n\nSitemap: https://orien.network/sitemap.xml\n`,
)

await writeFile(
  path.join(PUBLIC, 'sitemap.xml'),
  `<?xml version="1.0" encoding="UTF-8"?>
<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">
  <url><loc>https://orien.network/</loc></url>
  <url><loc>https://orien.network/how-it-works/</loc></url>
  <url><loc>https://orien.network/try/</loc></url>
</urlset>
`,
)

console.log('Generated Orien icons, OG card, robots.txt and sitemap.xml')
