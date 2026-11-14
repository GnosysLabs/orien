export function Wordmark({
  className = '',
  showMark = true,
}: {
  className?: string
  showMark?: boolean
}) {
  return (
    <span className={`inline-flex items-center gap-[0.26em] leading-none ${className}`}>
      {showMark && <span className="orien-mark pearl-fill shrink-0" aria-hidden />}
      {/* Trimmed to the capitals' height, so that centring the word centres
          the letters you see rather than room kept for descenders. The
          padding, taken back by the margin, leaves room for the round O to
          rise above and dip below the flat capitals: the gradient fill is
          only painted inside the box. */}
      <span className="-my-[0.1em] py-[0.1em] font-brand font-extrabold uppercase tracking-[0.14em] pearl-text [text-box:trim-both_cap_alphabetic]">
        ORIEN
      </span>
    </span>
  )
}
