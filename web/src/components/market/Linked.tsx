// Text people wrote, with its web addresses made into links. Only http and
// https addresses become links; they open in a new tab, carry no referrer,
// and are marked as user content so search engines don't follow them.

const ADDRESS = /(https?:\/\/[^\s<>"']+)/g

/** Trailing punctuation that ends a sentence, not the address. */
const trim = (url: string) => {
  const match = url.match(/[.,;:!?)\]]+$/)
  return match ? [url.slice(0, -match[0].length), match[0]] : [url, '']
}

export function Linked({ text }: { text: string }) {
  const parts = text.split(ADDRESS)
  return (
    <>
      {parts.map((part, i) => {
        if (i % 2 === 0) return part
        const [url, after] = trim(part)
        let shown = url
        try {
          const parsed = new URL(url)
          if (parsed.protocol !== 'http:' && parsed.protocol !== 'https:') return part
          shown = parsed.host + (parsed.pathname === '/' ? '' : parsed.pathname) + parsed.search
        } catch {
          return part
        }
        return (
          <span key={i}>
            <a
              href={url}
              target="_blank"
              rel="noopener noreferrer nofollow ugc"
              className="font-semibold text-ink underline decoration-line underline-offset-4 [overflow-wrap:anywhere] hover:decoration-pearl"
            >
              {shown}
            </a>
            {after}
          </span>
        )
      })}
    </>
  )
}
