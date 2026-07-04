const SIGN_GLYPHS = ['♈︎', '♉︎', '♊︎', '♋︎', '♌︎', '♍︎', '♎︎', '♏︎', '♐︎', '♑︎', '♒︎', '♓︎']

const PLANET_GLYPHS: Record<string, string> = {
  Sun: '☉', Moon: '☽', Mercury: '☿', Venus: '♀', Mars: '♂',
  Jupiter: '♃', Saturn: '♄', Uranus: '♅', Neptune: '♆', Pluto: '⯓',
  Mc: 'MC', Ic: 'IC',
}

const PLANET_FONT_SIZES: Record<string, number> = {
  Pluto: 14, Neptune: 26,
  Sun: 16, Moon: 16, Uranus: 22,
}


const ASPECT_COLORS: Record<string, string> = {
  conjunction: '#999',
  opposition: '#c55',
  square: '#c55',
  trine: '#5c5',
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
const R_PLANET = 178
const R_LABEL = 162
const R_INNER = 95

function toRad(deg: number) { return (deg * Math.PI) / 180 }

// Ascendant (cusps[0]) is placed at 9 o'clock (180° in SVG polar)
function eclToAngle(eclipticDeg: number, ascDeg: number): number {
  return 180 - (eclipticDeg - ascDeg)
}

function polar(angleDeg: number, r: number): { x: number; y: number } {
  const a = toRad(angleDeg)
  return { x: CX + r * Math.cos(a), y: CY + r * Math.sin(a) }
}

function labelCenter(angle: number, inset: number): { x: number; y: number } {
  const g = polar(angle, R_PLANET - inset)
  const a = toRad(angle)
  const gap = R_PLANET - R_LABEL
  return { x: g.x - gap * Math.cos(a), y: g.y - gap * 0.75 * Math.sin(a) }
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

type Box = { minX: number; maxX: number; minY: number; maxY: number }

function layoutPlanets(nameAnglePairs: [string, number][], obstacles: Box[] = []): Map<string, { angle: number; inset: number }> {
  const LANE_STEP = 18
  const MAX_INSET = R_LABEL - R_INNER - 30
  const names = nameAnglePairs.map(([n]) => n)

  function angSep(a: number, b: number) {
    const d = Math.abs(((a - b) + 360) % 360)
    return Math.min(d, 360 - d)
  }

  function getBounds(angle: number, inset: number) {
    const g = polar(angle, R_PLANET - inset)
    const l = labelCenter(angle, inset)
    return [
      { minX: g.x - 7,  maxX: g.x + 7,  minY: g.y - 7, maxY: g.y + 7 },
      { minX: l.x - 13, maxX: l.x + 13, minY: l.y - 4, maxY: l.y + 4 },
    ]
  }

  function overlaps(a: ReturnType<typeof getBounds>, b: ReturnType<typeof getBounds>) {
    return a.some(ba => b.some(bb =>
      ba.minX < bb.maxX && ba.maxX > bb.minX && ba.minY < bb.maxY && ba.maxY > bb.minY
    ))
  }

  // Phase 1: assign radial lanes — planets within 10° get different rings
  const placed: { angle: number; inset: number }[] = []
  const insets = new Map<string, number>()
  for (const [name, angle] of nameAnglePairs) {
    let lane = 0
    while (true) {
      const testInset = Math.min(lane * LANE_STEP, MAX_INSET)
      const conflict = placed.some(p => p.inset === testInset && angSep(angle, p.angle) < 10)
      if (!conflict || testInset >= MAX_INSET) {
        insets.set(name, testInset)
        placed.push({ angle, inset: testInset })
        break
      }
      lane++
    }
  }

  function boxOverlap(a: Box[], b: Box) {
    return a.some(ba => ba.minX < b.maxX && ba.maxX > b.minX && ba.minY < b.maxY && ba.maxY > b.minY)
  }

  // Phase 2: angular spreading for any remaining bounding-box overlaps, including obstacles
  const state = new Map(nameAnglePairs.map(([name, angle]) => [name, { angle, inset: insets.get(name) ?? 0 }]))
  for (let iter = 0; iter < 300; iter++) {
    let moved = false

    // Avoid fixed obstacles (house numbers)
    for (const name of names) {
      const p = state.get(name)!
      const bounds = getBounds(p.angle, p.inset)
      for (const obs of obstacles) {
        if (boxOverlap(bounds, obs)) {
          const obsCX = (obs.minX + obs.maxX) / 2
          const obsCY = (obs.minY + obs.maxY) / 2
          const obsAngle = Math.atan2(obsCY - CY, obsCX - CX) * (180 / Math.PI)
          let sep = ((p.angle - obsAngle) + 360) % 360
          if (sep > 180) sep -= 360
          p.angle += (Math.sign(sep) || 1) * 0.5
          moved = true
        }
      }
    }

    for (let i = 0; i < names.length; i++) {
      for (let j = i + 1; j < names.length; j++) {
        const a = state.get(names[i])!
        const b = state.get(names[j])!
        if (overlaps(getBounds(a.angle, a.inset), getBounds(b.angle, b.inset))) {
          let sep = ((b.angle - a.angle) + 360) % 360
          if (sep > 180) sep -= 360
          const dir = Math.sign(sep) || 1
          a.angle -= dir * 0.3
          b.angle += dir * 0.3
          moved = true
        }
      }
    }
    if (!moved) break
  }

  return state
}

const DARK_THEME = {
  ringOuter: '#1a1a2e', ringSign: '#16213e', ringHouse: '#0d1b2a', ringInner: '#242424',
  stroke: 'rgba(255,255,255,0.2)', strokeAngle: 'rgba(255,255,255,0.5)',
  signGlyph: 'rgba(255,255,255,0.6)',
  houseNum: 'rgba(255,255,255,0.45)',
  angleLabel: 'rgba(255,255,255,0.7)',
  notch: 'rgba(255,255,255,0.7)',
  aspectFallback: 'rgba(255,255,255,0.2)',
  planetGlyph: 'rgba(255,255,255,0.9)',
  planetLabel: 'rgba(255,255,255,0.55)',
}

const LIGHT_THEME = {
  ringOuter: '#c8cce0', ringSign: '#d8dcee', ringHouse: '#e4e8f4', ringInner: '#f0f0f8',
  stroke: 'rgba(0,0,0,0.15)', strokeAngle: 'rgba(0,0,0,0.35)',
  signGlyph: 'rgba(0,0,0,0.65)',
  houseNum: 'rgba(0,0,0,0.45)',
  angleLabel: 'rgba(0,0,0,0.65)',
  notch: 'rgba(0,0,0,0.45)',
  aspectFallback: 'rgba(0,0,0,0.15)',
  planetGlyph: 'rgba(0,0,0,0.85)',
  planetLabel: 'rgba(0,0,0,0.55)',
}

type WheelData = { planets: Record<string, number[]>; cusps: number[]; aspects: any[] }

export function ChartWheel({ data, darkMode = true }: { data: WheelData; darkMode?: boolean }) {
  const t = darkMode ? DARK_THEME : LIGHT_THEME
  const ascDeg = data.cusps[0] ?? 0
  const houseObstacles: Box[] = data.cusps.map((_, i) => {
    const pt = polar(houseMidAngle(data.cusps, i, ascDeg), R_INNER + 10)
    return { minX: pt.x - 6, maxX: pt.x + 6, minY: pt.y - 6, maxY: pt.y + 6 }
  })
  const planetLayout = layoutPlanets(
    Object.entries(data.planets).map(([name, [deg]]) => [name, eclToAngle(deg, ascDeg)]),
    houseObstacles
  )

  return (
    <svg width={SIZE} height={SIZE} style={{ display: 'block', margin: '0 auto' }}>
      {/* Ring fills */}
      <circle cx={CX} cy={CY} r={R_OUTER} fill={t.ringOuter} />
      <circle cx={CX} cy={CY} r={R_SIGN_INNER} fill={t.ringSign} />
      <circle cx={CX} cy={CY} r={R_HOUSE_OUTER} fill={t.ringHouse} />
      <circle cx={CX} cy={CY} r={R_INNER} fill={t.ringInner} />

      {/* Ring borders */}
      {[R_OUTER, R_SIGN_INNER, R_HOUSE_OUTER, R_INNER].map((r) => (
        <circle key={r} cx={CX} cy={CY} r={r} fill="none"
          stroke={t.stroke} strokeWidth={1} />
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
              stroke={t.stroke} strokeWidth={1} />
            <text x={pm.x} y={pm.y} textAnchor="middle" dominantBaseline="middle"
              fontSize={15} fill={t.signGlyph}>{glyph}</text>
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
            stroke={isAngle ? t.strokeAngle : t.stroke}
            strokeWidth={isAngle ? 1.5 : 0.8} />
        )
      })}

      {/* House numbers */}
      {data.cusps.map((_cuspDeg, i) => {
        const mid = polar(houseMidAngle(data.cusps, i, ascDeg), R_INNER + 10)
        return (
          <text key={i} x={mid.x} y={mid.y} textAnchor="middle" dominantBaseline="middle"
            fontSize={10} fill={t.houseNum}>{i + 1}</text>
        )
      })}

      {/* ASC / IC / DSC / MC labels */}
      {([['ASC', 0], ['IC', 3], ['DSC', 6], ['MC', 9]] as [string, number][]).map(([label, idx]) => {
        const p = polar(eclToAngle(data.cusps[idx] ?? 0, ascDeg), R_HOUSE_OUTER + 10)
        return (
          <text key={label} x={p.x} y={p.y} textAnchor="middle" dominantBaseline="middle"
            fontSize={9} fill={t.angleLabel} fontWeight="bold">{label}</text>
        )
      })}

      {/* Planet notches on inner ring and inside of house ring */}
      {Object.entries(data.planets).map(([name, [deg]]) => {
        const angle = eclToAngle(deg, ascDeg)
        const i1 = polar(angle, R_INNER)
        const i2 = polar(angle, R_INNER + 4)
        const o1 = polar(angle, R_HOUSE_OUTER)
        const o2 = polar(angle, R_HOUSE_OUTER - 4)
        return (
          <g key={name}>
            <line x1={i1.x} y1={i1.y} x2={i2.x} y2={i2.y}
              stroke={t.notch} strokeWidth={1.5} />
            <line x1={o1.x} y1={o1.y} x2={o2.x} y2={o2.y}
              stroke={t.notch} strokeWidth={1.5} />
          </g>
        )
      })}

      {/* Aspect lines */}
      {data.aspects.map((asp: any, i: number) => {
        const fromDeg = data.planets[asp.point?.name]?.[0]
        const toDeg = data.planets[asp.toPoint?.name]?.[0]
        if (fromDeg == null || toDeg == null) return null
        if (asp.aspect?.name?.toLowerCase() === 'semisquare') return null
        const p1 = polar(eclToAngle(fromDeg, ascDeg), R_INNER)
        const p2 = polar(eclToAngle(toDeg, ascDeg), R_INNER)
        const color = ASPECT_COLORS[asp.aspect?.name?.toLowerCase()] ?? t.aspectFallback
        return (
          <line key={i} x1={p1.x} y1={p1.y} x2={p2.x} y2={p2.y}
            stroke={color} strokeWidth={0.9} opacity={0.65} />
        )
      })}

      {/* Planet glyphs and arc-minute labels */}
      {Object.entries(data.planets).map(([name, [deg]]) => {
        const { angle, inset } = planetLayout.get(name) ?? { angle: eclToAngle(deg, ascDeg), inset: 0 }
        const pg = polar(angle, R_PLANET - inset)
        const pl = labelCenter(angle, inset)
        const glyph = PLANET_GLYPHS[name] ?? name.slice(0, 2)
        return (
          <g key={name}>
            <text x={pg.x} y={pg.y} textAnchor="middle" dominantBaseline="middle"
              fontSize={PLANET_FONT_SIZES[name] ?? 24} fill={t.planetGlyph}>{glyph}</text>
            <text x={pl.x} y={pl.y} textAnchor="middle" dominantBaseline="middle"
              fontSize={9} fill={t.planetLabel}>{formatArcMin(deg)}</text>
          </g>
        )
      })}
    </svg>
  )
}
