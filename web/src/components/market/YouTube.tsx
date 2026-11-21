/** The id of the first YouTube video a text links to, if any: watch, short
 * or youtu.be addresses. */
export function youtubeId(text: string): string | null {
  const match = text.match(
    /https?:\/\/(?:www\.|m\.)?(?:youtube\.com\/(?:watch\?(?:[^\s]*&)?v=|shorts\/|live\/|embed\/)|youtu\.be\/)([A-Za-z0-9_-]{11})/,
  )
  return match ? match[1] : null
}

/** A YouTube video in a post: YouTube's own player, without its cookies,
 * loaded when the post comes into view. */
export function YouTube({ id }: { id: string }) {
  return (
    <div className="relative mt-3 aspect-video w-full overflow-hidden rounded-xl border border-line bg-black">
      <iframe
        src={`https://www.youtube-nocookie.com/embed/${id}?rel=0&hl=en&cc_lang_pref=en`}
        title="YouTube video"
        loading="lazy"
        allow="accelerometer; clipboard-write; encrypted-media; gyroscope; picture-in-picture; web-share"
        allowFullScreen
        referrerPolicy="strict-origin-when-cross-origin"
        className="absolute inset-0 h-full w-full"
      />
    </div>
  )
}
