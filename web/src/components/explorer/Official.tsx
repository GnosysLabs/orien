import { Check } from 'lucide-react'
import { Avatar as DiceBear, Style } from '@dicebear/core'
import identiconStyle from '@dicebear/styles/identicon.json'
import { Avatar } from '@/components/market/Avatar'
import { WalletNetworkGlyph } from '@/components/WalletNetworkGlyph'
import { NetworkAvatar } from '@/components/market/network'
import type { WalletProfile } from '@/lib/explorer'

/** "✓ Official": on Orien's own wallets only, which are named by address in
 * the code, so no profile can wear it. */
export function OfficialTag() {
  return (
    <span className="pearl-fill ml-1.5 inline-flex items-center gap-0.5 rounded-full py-px pl-1.5 pr-2 align-[1px] text-[11px] font-bold text-bg">
      <Check className="h-3 w-3" strokeWidth={3} aria-hidden />
      Official
    </span>
  )
}

/** DiceBear's Identicon (CC0), drawn here from the wallet's address, for
 * wallets no profile shows: Orien Wallet draws the same, with the same
 * versions (@dicebear/core 10.7.0, @dicebear/styles 10.6.0). */
const IDENTICONS = new Style(identiconStyle)
const drawn = new Map<string, string>()

function identicon(address: string): string {
  const seed = address.toLowerCase()
  let uri = drawn.get(seed)
  if (!uri) {
    uri = new DiceBear(IDENTICONS, { seed }).toDataUri()
    drawn.set(seed, uri)
  }
  return uri
}

export function Identicon({ address, size }: { address: string; size: number }) {
  // eslint-disable-next-line @next/next/no-img-element
  return (
    <img
      src={identicon(address)}
      alt=""
      aria-hidden
      style={{ width: size, height: size, padding: Math.round(size * 0.2) }}
      className="shrink-0 rounded-full border border-line bg-raised"
    />
  )
}

/** A wallet's picture: the Orien coin for Orien's own, its identicon for
 * one with a nickname, otherwise its owner's. */
export function WalletFace({ profile, size }: { profile: WalletProfile; size: number }) {
  if (profile.official) return <NetworkAvatar size={size} />
  if (profile.nickname && profile.address) return <Identicon address={profile.address} size={size} />
  return <Avatar name={profile.name} picture={profile.picture} size={size} />
}

/** The Orien Network mark before a name a profile showed; none before a
 * nickname, which no one chose. */
export function NameMark({ profile }: { profile: WalletProfile }) {
  return profile.nickname ? null : <WalletNetworkGlyph />
}

/** Names someone chose stand out more than nicknames. */
export const nameWeight = (profile: WalletProfile) => (profile.nickname ? 'font-medium' : 'font-semibold')
