const SIGN_GLYPHS = ['♈', '♉', '♊', '♋', '♌', '♍', '♎', '♏', '♐', '♑', '♒', '♓']

const PLANET_GLYPHS: Record<string, string> = {
  Sun: '☉', Moon: '☽', Mercury: '☿', Venus: '♀', Mars: '♂',
  Jupiter: '♃', Saturn: '♄', Uranus: '♅', Neptune: '♆', Pluto: '♇',
  Mc: 'MC', Ic: 'IC',
}

const ASPECT_COLORS: Record<string, string> = {
  conjunction: '#999',
  opposition: '#c55',
  square: '#c55',
  trine: '#55c',
  sextile: '#5a5',
  quincunx: '#c85',
  semisextile: '#999',
  semisquare: '#c55',
}

const SIZE = 520
const CX = SIZE / 2
const CY = SIZE / 2
const R_OUTER = 238
const R_SIGN_INNER = 200
const R_HOUSE_OUTER = 192
const R_PLANET = 158
const R_LABEL = 128
const R_INNER = 72

function toRad(deg: number) { return (deg * Math.PI) / 180 }

// Ascendant (cusps[0]) is placed at 9 o'clock (180° in SVG polar)
function eclToAngle(eclipticDeg: number, ascDeg: number): number {
  return 180 - (eclipticDeg - ascDeg)
}

function polar(angleDeg: number, r: number): { x: number; y: number } {
  const a = toRad(angleDeg)
  return { x: CX + r * Math.cos(a), y: CY + r * Math.sin(a) }
}

function formatArcMin(eclipticDeg: number): string {
  const whole = Math.floor(eclipticDeg)
  const inSign = whole % 30
  const mins = Math.round((eclipticDeg - whole) * 60)
  if (mins >= 60) return `${inSign + 1}°00′`
  return `${inSign}°${String(mins).padStart(2, '0')}′`
}

function houseMidAngle(cusps: number[], i: number, ascDeg: number): number {
  const cur = cusps[i]
  const next = cusps[(i + 1) % 12]
  const span = ((next - cur) + 360) % 360
  return eclToAngle(cur + span / 2, ascDeg)
}

type WheelData = { planets: Record<string, [number]>; cusps: number[]; aspects: any[] }

export function ChartWheel({ data }: { data: WheelData }) {
  const ascDeg = data.cusps[0] ?? 0

  return (
    <svg width={SIZE} height={SIZE} style={{ display: 'block', margin: '0 auto' }}>
      {/* Ring fills */}
      <circle cx={CX} cy={CY} r={R_OUTER} fill="#1a1a2e" />
      <circle cx={CX} cy={CY} r={R_SIGN_INNER} fill="#16213e" />
      <circle cx={CX} cy={CY} r={R_HOUSE_OUTER} fill="#0d1b2a" />
      <circle cx={CX} cy={CY} r={R_INNER} fill="#242424" />

      {/* Ring borders */}
      {[R_OUTER, R_SIGN_INNER, R_HOUSE_OUTER, R_INNER].map((r) => (
        <circle key={r} cx={CX} cy={CY} r={r} fill="none"
          stroke="rgba(255,255,255,0.2)" strokeWidth={1} />
      ))}

      {/* Zodiac sign divisions and glyphs */}
      {SIGN_GLYPHS.map((glyph, i) => {
        const divAngle = eclToAngle(i * 30, ascDeg)
        const midAngle = eclToAngle(i * 30 + 15, ascDeg)
        const p1 = polar(divAngle, R_SIGN_INNER)
        const p2 = polar(divAngle, R_OUTER)
        const pm = polar(midAngle, (R_SIGN_INNER + R_OUTER) / 2)
        return (
          <g key={i}>
            <line x1={p1.x} y1={p1.y} x2={p2.x} y2={p2.y}
              stroke="rgba(255,255,255,0.2)" strokeWidth={1} />
            <text x={pm.x} y={pm.y} textAnchor="middle" dominantBaseline="middle"
              fontSize={15} fill="rgba(255,255,255,0.6)">{glyph}</text>
          </g>
        )
      })}

      {/* House cusp lines */}
      {data.cusps.map((cuspDeg, i) => {
        const isAngle = i === 0 || i === 3 || i === 6 || i === 9
        const angle = eclToAngle(cuspDeg, ascDeg)
        const p1 = polar(angle, R_INNER)
        const p2 = polar(angle, R_HOUSE_OUTER)
        return (
          <line key={i} x1={p1.x} y1={p1.y} x2={p2.x} y2={p2.y}
            stroke={isAngle ? 'rgba(255,255,255,0.5)' : 'rgba(255,255,255,0.2)'}
            strokeWidth={isAngle ? 1.5 : 0.8} />
        )
      })}

      {/* House numbers */}
      {data.cusps.map((_cuspDeg, i) => {
        const mid = polar(houseMidAngle(data.cusps, i, ascDeg), (R_INNER + R_HOUSE_OUTER) / 2)
        return (
          <text key={i} x={mid.x} y={mid.y} textAnchor="middle" dominantBaseline="middle"
            fontSize={10} fill="rgba(255,255,255,0.45)">{i + 1}</text>
        )
      })}

      {/* ASC / IC / DSC / MC labels */}
      {([['ASC', 0], ['IC', 3], ['DSC', 6], ['MC', 9]] as [string, number][]).map(([label, idx]) => {
        const p = polar(eclToAngle(data.cusps[idx] ?? 0, ascDeg), R_HOUSE_OUTER + 10)
        return (
          <text key={label} x={p.x} y={p.y} textAnchor="middle" dominantBaseline="middle"
            fontSize={9} fill="rgba(255,255,255,0.7)" fontWeight="bold">{label}</text>
        )
      })}

      {/* Aspect lines */}
      {data.aspects.map((asp: any, i: number) => {
        const fromDeg = data.planets[asp.point?.name]?.[0]
        const toDeg = data.planets[asp.toPoint?.name]?.[0]
        if (fromDeg == null || toDeg == null) return null
        const p1 = polar(eclToAngle(fromDeg, ascDeg), R_INNER - 6)
        const p2 = polar(eclToAngle(toDeg, ascDeg), R_INNER - 6)
        const color = ASPECT_COLORS[asp.aspect?.name?.toLowerCase()] ?? 'rgba(255,255,255,0.2)'
        return (
          <line key={i} x1={p1.x} y1={p1.y} x2={p2.x} y2={p2.y}
            stroke={color} strokeWidth={0.9} opacity={0.65} />
        )
      })}

      {/* Planet glyphs and arc-minute labels */}
      {Object.entries(data.planets).map(([name, [deg]]) => {
        const angle = eclToAngle(deg, ascDeg)
        const pg = polar(angle, R_PLANET)
        const pl = polar(angle, R_LABEL)
        const glyph = PLANET_GLYPHS[name] ?? name.slice(0, 2)
        return (
          <g key={name}>
            <text x={pg.x} y={pg.y} textAnchor="middle" dominantBaseline="middle"
              fontSize={14} fill="rgba(255,255,255,0.9)">{glyph}</text>
            <text x={pl.x} y={pl.y} textAnchor="middle" dominantBaseline="middle"
              fontSize={9} fill="rgba(255,255,255,0.55)">{formatArcMin(deg)}</text>
          </g>
        )
      })}
    </svg>
  )
}
