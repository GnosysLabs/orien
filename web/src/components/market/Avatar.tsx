import { Avatar as DiceBear, Style } from '@dicebear/core'
import planets from '@dicebear/styles/planets.json'
import { pictureUrl } from '@/lib/market'

/** DiceBear's Planets (CC0 1.0), drawn here rather than fetched, so no one
 * else learns whose face is shown. Orien Wallet draws the same: the same
 * versions (@dicebear/core 10.7.0, @dicebear/styles 10.6.0), no options
 * but the seed, and the seed is the account's name, trimmed and in lower
 * case ("orien" when there's none). */
const PLANETS = new Style(planets)
const drawn = new Map<string, string>()

function planet(name: string): string {
  const seed = name.trim().toLowerCase() || 'orien'
  let uri = drawn.get(seed)
  if (!uri) {
    uri = new DiceBear(PLANETS, { seed }).toDataUri()
    drawn.set(seed, uri)
  }
  return uri
}

/** Someone's picture, or, when they haven't put one up, their planet. */
export function Avatar({ name, picture, size = 40 }: { name: string; picture: string | null; size?: number }) {
  const style = { width: size, height: size }
  // eslint-disable-next-line @next/next/no-img-element
  return (
    <img
      src={picture ? pictureUrl(picture) : planet(name)}
      alt=""
      style={style}
      className="shrink-0 rounded-full border border-line bg-raised object-cover"
      aria-hidden={picture ? undefined : true}
    />
  )
}
