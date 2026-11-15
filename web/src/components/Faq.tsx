import { ChevronDown } from 'lucide-react'
import { jsonLd } from '@/lib/seo'

export type Question = { q: string; a: string }

/** Questions people ask, each opening to its answer, and the same questions
 * written out for search engines so they can show the answers directly. */
export function Faq({ questions }: { questions: Question[] }) {
  return (
    <>
      <script
        type="application/ld+json"
        dangerouslySetInnerHTML={jsonLd({
          '@type': 'FAQPage',
          mainEntity: questions.map(({ q, a }) => ({
            '@type': 'Question',
            name: q,
            acceptedAnswer: { '@type': 'Answer', text: a },
          })),
        })}
      />
      <div className="divide-y divide-line overflow-hidden rounded-2xl border border-line bg-surface">
        {questions.map(({ q, a }) => (
          <details key={q} className="group">
            <summary className="flex cursor-pointer list-none items-center justify-between gap-4 px-5 py-4 font-semibold text-ink transition hover:bg-raised [&::-webkit-details-marker]:hidden">
              <h3 className="text-base">{q}</h3>
              <ChevronDown className="h-4 w-4 shrink-0 text-muted transition group-open:rotate-180" aria-hidden />
            </summary>
            <p className="px-5 pb-5 leading-relaxed text-muted">{a}</p>
          </details>
        ))}
      </div>
    </>
  )
}
