export function WalletNetworkGlyph({ network = 'ORN' }: { network?: 'ORN' | 'SOL' }) {
  const sol = network === 'SOL'
  const label = sol ? 'Solana Devnet' : 'Orien'
  const mask = `url("/brand/svg/${sol ? 'solana-mark' : 'orien-mark'}.svg") center / contain no-repeat`
  return (
    <span
      role="img" aria-label={label} title={label}
      style={{ mask, WebkitMask: mask }}
      className="me-1.5 inline-block h-[1em] w-[1em] shrink-0 bg-current align-[-0.12em]"
    />
  )
}
