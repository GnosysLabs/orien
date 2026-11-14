// Building blocks of the technical paper. Each part reads on screen in the
// site's dark style and prints (or saves as PDF) as black on white.

export function PaperSection({
  id,
  number,
  title,
  children,
}: {
  id: string
  number: number
  title: string
  children: React.ReactNode
}) {
  return (
    <section id={id} className="scroll-mt-24 border-t border-line py-10 print:break-before-auto print:border-neutral-300 print:py-6">
      <h2 className="mb-5 text-2xl font-bold tracking-tight text-ink print:text-xl print:text-black">
        <span className="mr-3 text-muted print:text-neutral-500">{number}</span>
        {title}
      </h2>
      <div className="space-y-4 leading-relaxed text-muted print:text-[10.5pt] print:leading-snug print:text-neutral-800 [&_code]:rounded [&_code]:bg-raised [&_code]:px-1 [&_code]:py-0.5 [&_code]:font-mono [&_code]:text-[0.9em] [&_code]:text-ink print:[&_code]:bg-neutral-100 print:[&_code]:text-black [&_strong]:font-semibold [&_strong]:text-ink print:[&_strong]:text-black">
        {children}
      </div>
    </section>
  )
}

export function Sub({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <div className="space-y-3 pt-2 print:break-inside-avoid-page">
      <h3 className="font-semibold text-ink print:text-black">{title}</h3>
      {children}
    </div>
  )
}

/** A formula or data layout, set apart in monospace. */
export function Formula({ children }: { children: string }) {
  return (
    <pre className="overflow-x-auto rounded-xl border border-line bg-surface px-4 py-3 font-mono text-[13px] leading-relaxed text-ink print:whitespace-pre-wrap print:border-neutral-300 print:bg-neutral-50 print:text-[9pt] print:text-black">
      {children.trim()}
    </pre>
  )
}

/** A table of names and values. */
export function Table({ head, rows }: { head: string[]; rows: React.ReactNode[][] }) {
  return (
    <div className="overflow-x-auto rounded-xl border border-line print:border-neutral-300">
      <table className="w-full border-collapse text-left text-sm print:text-[9pt]">
        <thead>
          <tr className="bg-surface print:bg-neutral-100">
            {head.map((cell, i) => (
              <th key={i} className="px-3 py-2 font-semibold text-ink print:text-black">
                {cell}
              </th>
            ))}
          </tr>
        </thead>
        <tbody>
          {rows.map((row, i) => (
            <tr key={i} className="border-t border-line align-top print:border-neutral-200">
              {row.map((cell, j) => (
                <td key={j} className="px-3 py-2 print:text-neutral-800">
                  {cell}
                </td>
              ))}
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  )
}
