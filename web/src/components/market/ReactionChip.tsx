'use client'

export type ReactionCount = { emoji: string; count: number; me: boolean; names?: string[] }

/** "Sam", "Sam and Maya", "Sam, Maya and 3 others". */
export function whoReacted({ count, names = [] }: ReactionCount) {
  if (names.length === 0) return `${count} ${count === 1 ? 'person' : 'people'}`
  const others = count - names.length
  const parts = others > 0 ? [...names, `${others} ${others === 1 ? 'other' : 'others'}`] : names
  return parts.length === 1 ? parts[0] : `${parts.slice(0, -1).join(', ')} and ${parts[parts.length - 1]}`
}

/** A reaction with its count; hovering over it (or focusing it) says who
 * left it, as in Discord. Pressing it adds or takes back yours. */
export function ReactionChip({
  r,
  onClick,
  small = false,
  below = false,
}: {
  r: ReactionCount
  onClick: () => void
  small?: boolean
  /** Under the chip, from its left edge: in a chat, so it never covers the message. */
  below?: boolean
}) {
  const who = whoReacted(r)
  return (
    <span className="group/reaction relative inline-flex">
      <button
        type="button"
        onClick={onClick}
        aria-pressed={r.me}
        aria-label={`${r.emoji} ${r.count}: ${who}`}
        className={`flex items-center gap-1 rounded-full border px-2 transition ${small ? 'h-[26px] text-[13px]' : 'h-7 text-sm'} ${
          r.me ? 'border-pearl/70 bg-raised text-ink' : 'border-line text-muted hover:border-pearl/60 hover:text-ink'
        }`}
      >
        <span aria-hidden>{r.emoji}</span>
        <span className="num text-xs font-semibold">{r.count}</span>
      </button>
      <span
        role="tooltip"
        className={`pointer-events-none absolute z-30 flex w-max max-w-[240px] items-center ${below ? 'left-0 top-full mt-2' : 'bottom-full left-1/2 mb-2 -translate-x-1/2'} gap-2.5 rounded-xl border border-line bg-surface px-3 py-2 text-left text-xs leading-snug text-ink opacity-0 shadow-2xl shadow-black/60 transition-opacity duration-150 group-focus-within/reaction:opacity-100 group-hover/reaction:opacity-100 group-hover/reaction:delay-200`}
      >
        <span className="text-xl leading-none" aria-hidden>
          {r.emoji}
        </span>
        <span>
          <span className="font-semibold">{who}</span> <span className="text-muted">reacted</span>
        </span>
      </span>
    </span>
  )
}
