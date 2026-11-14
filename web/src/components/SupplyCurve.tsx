export function SupplyCurve() {
  // Supply released at year t with 100M cap and 4-year half-life: 100 * (1 - 2^(-t/4))
  const W = 560
  const H = 200
  const padL = 44
  const padR = 16
  const padT = 18
  const padB = 30
  const plotW = W - padL - padR
  const plotH = H - padT - padB
  const maxYear = 16

  const xAt = (yr: number) => padL + (yr / maxYear) * plotW
  const yAt = (millions: number) => padT + (1 - millions / 100) * plotH

  const samples = Array.from({ length: 65 }, (_, i) => {
    const yr = (i / 64) * maxYear
    const supply = 100 * (1 - Math.pow(2, -yr / 4))
    return { yr, supply, x: xAt(yr), y: yAt(supply) }
  })

  const linePath = samples.map((p, i) => `${i === 0 ? 'M' : 'L'}${p.x.toFixed(1)},${p.y.toFixed(1)}`).join(' ')
  const areaPath = `${linePath} L${xAt(maxYear).toFixed(1)},${yAt(0).toFixed(1)} L${xAt(0).toFixed(1)},${yAt(0).toFixed(1)} Z`

  const milestones = [
    { yr: 0, supply: 0, label: '0M' },
    { yr: 4, supply: 50, label: '50M' },
    { yr: 8, supply: 75, label: '75M' },
    { yr: 12, supply: 87.5, label: '87.5M' },
    { yr: 16, supply: 93.75, label: '93.8M' },
  ]

  return (
    <div className="flex flex-col gap-3 rounded-xl border border-line bg-bg/60 p-4">
      <div className="flex flex-wrap items-baseline justify-between gap-2">
        <div>
          <span className="text-xs font-bold uppercase tracking-wider text-muted">Release curve</span>
          <p className="text-sm font-semibold text-ink">100M cap · 4-year half-life · 0 at genesis</p>
        </div>
        <span className="num text-xs text-pearl">~1,977 coins in hour 1</span>
      </div>

      <svg
        viewBox={`0 0 ${W} ${H}`}
        className="h-auto w-full select-none overflow-visible"
        role="img"
        aria-label="Orien coin emission curve from year 0 to year 16: 0 at genesis, 50 million at year 4, 75 million at year 8, approaching the 100 million cap"
      >
        <defs>
          <linearGradient id="supplyStroke" x1="0" y1="0" x2="1" y2="0">
            <stop offset="0%" stopColor="#f4b6d4" />
            <stop offset="30%" stopColor="#b8c7ff" />
            <stop offset="60%" stopColor="#a6ead6" />
            <stop offset="100%" stopColor="#f9dda4" />
          </linearGradient>
          <linearGradient id="supplyFill" x1="0" y1="0" x2="0" y2="1">
            <stop offset="0%" stopColor="#d3b6ff" stopOpacity="0.22" />
            <stop offset="100%" stopColor="#d3b6ff" stopOpacity="0.0" />
          </linearGradient>
        </defs>

        {/* 100M cap dashed line */}
        <line
          x1={padL}
          y1={yAt(100)}
          x2={W - padR}
          y2={yAt(100)}
          stroke="#262633"
          strokeWidth="1"
          strokeDasharray="4 4"
        />
        <text x={padL - 8} y={yAt(100) + 4} textAnchor="end" className="fill-muted text-[10px]" fontFamily="monospace">
          100M
        </text>
        <line x1={padL} y1={yAt(50)} x2={W - padR} y2={yAt(50)} stroke="#262633" strokeWidth="1" />
        <text x={padL - 8} y={yAt(50) + 4} textAnchor="end" className="fill-muted text-[10px]" fontFamily="monospace">
          50M
        </text>
        <line x1={padL} y1={yAt(0)} x2={W - padR} y2={yAt(0)} stroke="#262633" strokeWidth="1" />
        <text x={padL - 8} y={yAt(0) + 4} textAnchor="end" className="fill-muted text-[10px]" fontFamily="monospace">
          0
        </text>

        <path d={areaPath} fill="url(#supplyFill)" />
        <path d={linePath} fill="none" stroke="url(#supplyStroke)" strokeWidth="2.5" strokeLinecap="round" />

        {milestones.map((m) => {
          const cx = xAt(m.yr)
          const cy = yAt(m.supply)
          return (
            <g key={m.yr}>
              <circle cx={cx} cy={cy} r="4" fill="#0b0b0f" stroke="#d3c4ff" strokeWidth="2" />
              {m.yr > 0 && (
                <text
                  x={cx}
                  y={cy - 9}
                  textAnchor={m.yr === maxYear ? 'end' : 'middle'}
                  className="fill-ink text-[10px] font-semibold"
                  fontFamily="monospace"
                >
                  {m.label}
                </text>
              )}
              <text x={cx} y={H - 8} textAnchor="middle" className="fill-muted text-[10px]" fontFamily="monospace">
                Y{m.yr}
              </text>
            </g>
          )
        })}
      </svg>

      <p className="text-xs leading-relaxed text-muted">
        Coins release by elapsed wall-clock time between checkpoints rather than checkpoint count, so closing an epoch
        early or late never changes total issuance.
      </p>
    </div>
  )
}
