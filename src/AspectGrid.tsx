const ASPECT_SYMBOLS: Record<string, string> = {
  conjunction: '☌',
  opposition: '☍',
  trine: '△',
  square: '□',
  sextile: '⚹',
  quincunx: '⚻',
}

const ASPECT_COLORS: Record<string, string> = {
  conjunction: '#aaa',
  opposition: '#c55',
  square: '#c55',
  trine: '#55c',
  sextile: '#5a5',
  quincunx: '#c85',
}

type Aspect = { from: string; to: string; type: string; orb: string; applying: boolean | null }

export function AspectGrid({ planets, aspects }: { planets: string[]; aspects: Aspect[] }) {
  const aspectMap = new Map<string, Aspect>()
  for (const a of aspects) {
    aspectMap.set(`${a.from}|${a.to}`, a)
    aspectMap.set(`${a.to}|${a.from}`, a)
  }

  return (
    <div style={{ overflowX: 'auto', marginTop: 8 }}>
      <table style={{ borderCollapse: 'collapse', fontSize: '0.85em' }}>
        <thead>
          <tr>
            <th />
            {planets.map(p => (
              <th key={p} style={{ padding: '4px 2px', opacity: 0.7, fontWeight: 'normal', border: '1px solid #666', textAlign: 'center' }}>{p}</th>
            ))}
          </tr>
        </thead>
        <tbody>
          {planets.map((row, rowIdx) => (
            <tr key={row}>
              <td style={{ padding: '4px 2px', opacity: 0.7, border: '1px solid #666' }}>{row}</td>
              {planets.map((col, colIdx) => {
                if (colIdx === rowIdx) {
                  return <td key={col} style={{ padding: '4px 2px', border: '1px solid #666', minWidth: 32 }} />
                }
                const asp = aspectMap.get(`${row}|${col}`)
                return (
                  <td key={col} style={{ padding: '4px 2px', textAlign: 'center', border: '1px solid #666', minWidth: 32 }}>
                    {asp ? (
                      <span
                        title={`${asp.type.charAt(0).toUpperCase() + asp.type.slice(1)} ${asp.orb} — ${asp.applying == null ? '' : asp.applying ? 'applying' : 'separating'}`}
                        style={{ color: ASPECT_COLORS[asp.type] ?? '#aaa', fontSize: '1.1em', cursor: 'default' }}
                      >
                        {ASPECT_SYMBOLS[asp.type] ?? asp.type.slice(0, 3)}
                      </span>
                    ) : null}
                  </td>
                )
              })}
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  )
}
