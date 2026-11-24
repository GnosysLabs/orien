import { Avatar as DiceBear, Style } from '@dicebear/core'
import botttsNeutral from '@dicebear/styles/bottts-neutral.json'
import { pictureUrl } from '@/lib/market'

/** DiceBear's Bottts Neutral (a remix of Pablo Stanley's Bottts, free for
 * personal and commercial use) with its "Full Cast" preset: every robot
 * gets the surface texture. Drawn here rather than fetched, so no one else
 * learns whose face is shown. Orien Wallet draws the same: the same
 * versions (@dicebear/core 10.7.0, @dicebear/styles 10.6.0), the same
 * options, and the seed is the account's name, trimmed and in lower case
 * ("orien" when there's none). */
const ROBOTS = new Style(botttsNeutral)
const drawn = new Map<string, string>()

function robot(name: string): string {
  const seed = name.trim().toLowerCase() || 'orien'
  let uri = drawn.get(seed)
  if (!uri) {
    uri = new DiceBear(ROBOTS, { seed, textureProbability: 100 }).toDataUri()
    drawn.set(seed, uri)
  }
  return uri
}

/** Someone's picture, or, when they haven't put one up, their robot. */
export function Avatar({ name, picture, size = 40 }: { name: string; picture: string | null; size?: number }) {
  const style = { width: size, height: size }
  // eslint-disable-next-line @next/next/no-img-element
  return (
    <img
      src={picture ? pictureUrl(picture) : robot(name)}
      alt=""
      style={style}
      className="shrink-0 rounded-full border border-line bg-raised object-cover"
      aria-hidden={picture ? undefined : true}
    />
  )
}
