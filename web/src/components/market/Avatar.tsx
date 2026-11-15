import { pictureUrl } from '@/lib/market'

/** A seller's picture, or the first letter of their name on a soft circle. */
export function Avatar({ name, picture, size = 40 }: { name: string; picture: string | null; size?: number }) {
  const style = { width: size, height: size }
  if (picture) {
    // eslint-disable-next-line @next/next/no-img-element
    return <img src={pictureUrl(picture)} alt="" style={style} className="shrink-0 rounded-full border border-line object-cover" />
  }
  return (
    <span
      style={{ ...style, fontSize: size * 0.42 }}
      className="flex shrink-0 items-center justify-center rounded-full border border-line bg-raised font-bold text-pearl"
      aria-hidden
    >
      {(name.trim()[0] ?? '?').toUpperCase()}
    </span>
  )
}
