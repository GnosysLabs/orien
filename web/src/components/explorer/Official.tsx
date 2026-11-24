import { Check } from 'lucide-react'
import { Avatar } from '@/components/market/Avatar'
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

/** A wallet's picture: the Orien coin for Orien's own, otherwise its
 * owner's. */
export function WalletFace({ profile, size }: { profile: WalletProfile; size: number }) {
  return profile.official ? <NetworkAvatar size={size} /> : <Avatar name={profile.name} picture={profile.picture} size={size} />
}
