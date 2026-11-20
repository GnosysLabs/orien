'use client'

import { useEffect, useState } from 'react'
import { Download, Smartphone } from 'lucide-react'
import { WindowsDownload } from '@/components/WindowsDownload'
import { IPHONE_BETA } from '@/lib/brand'

type Platform = 'mac' | 'windows' | 'linux' | 'iphone'
/** Android and other devices Orien Wallet doesn't run on: no main button. */
type Device = Platform | 'other'

const NAMES: Record<Platform, string> = { mac: 'Mac', windows: 'Windows', linux: 'Linux', iphone: 'iPhone' }

/** Which device the page is open on, so its download comes first. */
function detect(): Device {
  const agent = navigator.userAgent
  if (/Android/.test(agent)) return 'other'
  if (/iPhone|iPad|iPod/.test(agent)) return 'iphone'
  if (/Windows/.test(agent)) return 'windows'
  if (/Linux/.test(agent) && !/Android/.test(agent)) return 'linux'
  return 'mac'
}

const BIG = 'pearl-fill flex h-12 w-full items-center justify-center gap-2 rounded-xl px-6 font-bold transition hover:brightness-105 sm:w-auto'

function MainButton({ platform }: { platform: Platform }) {
  switch (platform) {
    case 'mac':
      return (
        <a href="/wallet/Orien-Wallet.dmg" className={BIG}>
          <Download className="h-4 w-4" aria-hidden /> Download for Mac
        </a>
      )
    case 'windows':
      return (
        <WindowsDownload className={BIG}>
          <Download className="h-4 w-4" aria-hidden /> Download for Windows
        </WindowsDownload>
      )
    case 'linux':
      return (
        <a href="/wallet/Orien-Wallet.AppImage" className={BIG}>
          <Download className="h-4 w-4" aria-hidden /> Download for Linux
        </a>
      )
    case 'iphone':
      return (
        <a href={IPHONE_BETA} target="_blank" rel="noopener noreferrer" className={BIG}>
          <Smartphone className="h-4 w-4" aria-hidden /> Get it for iPhone
        </a>
      )
  }
}

function Hint({ platform }: { platform: Platform }) {
  const code = 'whitespace-nowrap rounded bg-raised px-1.5 py-0.5 text-xs text-ink'
  switch (platform) {
    case 'mac':
      return <>For Macs with an Apple chip. Open the file and drag Orien Wallet into Applications.</>
    case 'windows':
      return <>Windows shows a warning for new apps. We show you what to click.</>
    case 'linux':
      return (
        <>
          Allow the file to run as a program, then open it. If it doesn&apos;t start, install FUSE:{' '}
          <code className={code}>sudo apt install libfuse2</code> (or <code className={code}>libfuse2t64</code> on
          Ubuntu 24.04 and newer).
        </>
      )
    case 'iphone':
      return <>A free public beta. The link shows you how to install it with Apple&apos;s TestFlight app.</>
  }
}

/** Orien Wallet's download: one big button for this device, the others
 * beside it. Picking another device shows its button and its install tip. */
export function WalletDownload() {
  const [platform, setPlatform] = useState<Device | null>(null)
  useEffect(() => setPlatform(detect()), [])

  return (
    <section className="rounded-2xl border border-line bg-surface p-6 text-center sm:p-10">
      {/* eslint-disable-next-line @next/next/no-img-element */}
      <img
        src="/iphone-app-icon.png"
        alt=""
        className="mx-auto h-16 w-16 rounded-[15px] ring-1 ring-white/15"
      />
      <h2 className="mt-5 text-2xl font-bold tracking-tight text-ink">Orien Wallet</h2>
      <p className="mx-auto mt-2 max-w-md leading-relaxed text-muted">
        Hold, send, mine and swap ORN.
      </p>

      {platform !== 'other' && (
        <div className="mt-8 flex min-h-12 justify-center">
          {platform ? <MainButton platform={platform} /> : <span className="h-12" aria-hidden />}
        </div>
      )}
      <p className="mx-auto mt-4 min-h-10 max-w-md text-sm leading-relaxed text-muted">
        {platform === 'other'
          ? 'Orien Wallet is for Mac, Windows, Linux and iPhone. Pick yours below.'
          : platform && <Hint platform={platform} />}
      </p>

      <div className="mt-8 border-t border-line pt-6">
        <p className="mb-3 text-xs font-semibold uppercase tracking-[0.2em] text-muted">
          {platform === 'other' ? 'Available for' : 'Also for'}
        </p>
        <div className="flex min-h-9 flex-wrap justify-center gap-2">
          {(platform ? (Object.keys(NAMES) as Platform[]) : [])
            .filter((other) => other !== platform)
            .map((other) => (
              <button
                key={other}
                type="button"
                onClick={() => setPlatform(other)}
                className="h-9 rounded-lg border border-line bg-bg px-4 text-sm font-semibold text-ink transition hover:border-pearl"
              >
                {NAMES[other]}
              </button>
            ))}
        </div>
      </div>
    </section>
  )
}
