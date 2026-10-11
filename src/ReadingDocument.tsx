import { useState } from 'react'
import { ChartWheel } from './ChartWheel'

export type Fact = { id: string; kind: string; label: string; detail: string; planets: string[] }
export type Rule = { id: string; title: string; explanation: string; pages: string; quoted?: boolean }
export type Role = { label: string; house: number | null; planet: string; reason: string }
export type Section = { title: string; body: string; evidence: string[]; revision: number; after_message?: number; step?: string; method_stage?: string; rules?: Rule[]; facts?: Fact[]; because?: string; draft?: string; roles?: Role[]; worksheet?: { checks?: Record<string, { state: string; finding: string }>; unknowns?: string[] } }
export type Chart = { timestampMs: number; houses: { longitude: number }[]; bodies: { name: string; longitude: number; retrograde: boolean }[]; aspects?: { planet1: string; planet2: string; aspect: string }[] }
export type Revision = { number: number; question: string; chart: Chart | null; place: { label: string; timezone: string } | null; sections: Section[] }
export type Progress = { atMs: number; event: string; detail: string; elapsedMs: number }

function factText(detail: string) {
  return detail.replaceAll('triplicityRuler', 'triplicity').replaceAll('termRuler', 'term').replaceAll('faceRuler', 'face').replaceAll('underBeams', 'under the Sun’s beams')
}

function MoonPhase({ chart }: { chart: Chart }) {
  const sun = chart.bodies.find(b => b.name === 'Sun')?.longitude
  const moon = chart.bodies.find(b => b.name === 'Moon')?.longitude
  if (sun == null || moon == null) return null
  const angle = ((moon - sun) % 360 + 360) % 360
  const k = Math.cos(angle * Math.PI / 180)
  const waxing = angle < 180
  const sweep = waxing ? 1 : 0
  const returnSweep = k >= 0 ? 1 - sweep : sweep
  const terminator = Math.abs(k) < .001 ? 'L0 -10' : `A${Math.abs(k) * 10} 10 0 0 ${returnSweep} 0 -10`
  const description = `${waxing ? 'Waxing' : 'Waning'} Moon · ${Math.round((1 - k) * 50)}% illuminated`
  return <span className="moon-phase" title={description}><svg viewBox="-12 -12 24 24" role="img" aria-label={description}><circle r="10" fill="currentColor" opacity=".13" /><path d={`M0 -10 A10 10 0 0 ${sweep} 0 10 ${terminator} Z`} fill="currentColor" /></svg><span>{waxing ? 'Waxing' : 'Waning'}</span></span>
}

export function ReadingChart({ chart, place, dark, facts = [] }: { chart: Chart; place: { label: string; timezone: string } | null; dark: boolean; facts?: Fact[] }) {
  const [selected, setSelected] = useState<string | null>(null)
  const data = { planets: Object.fromEntries(chart.bodies.map(b => [b.name, [b.longitude, b.retrograde ? -1 : 1]])), cusps: chart.houses.map(h => h.longitude), aspects: (chart.aspects || []).map(a => ({ point: { name: a.planet1 }, toPoint: { name: a.planet2 }, aspect: { name: a.aspect.toLowerCase() } })) }
  return <figure className="document-chart" id="reading-chart">
    <ChartWheel data={data} darkMode={dark} selectedPlanet={selected} onSelectPlanet={setSelected} />
    <figcaption><span>{place?.label} · {new Date(chart.timestampMs).toLocaleString(undefined, { timeZone: place?.timezone, dateStyle: 'medium', timeStyle: 'short' })}</span><MoonPhase chart={chart} /></figcaption>
    {selected && <aside className="planet-thread" aria-label={`${selected}'s calculated testimony`} title={!facts.length ? 'No testimony was retained for this earlier chart.' : undefined}><h3>{selected}</h3>{facts.filter(f => f.planets.includes(selected) && f.kind !== 'event').map(f => <p key={f.id}>{factText(f.detail)}</p>)}</aside>}
    <details className="margin-note"><summary title="The moment and place used for this chart">Moment</summary><p>This chart keeps the moment at which the question became clear. Questions on the same matter keep that moment; a different matter begins another chart.</p><p className="source-note">Frawley, The Horary Textbook · printed pp. 7–8.</p><p className="source-note">Planetary positions are approximate. Fine timing, stars and antiscia need further verification.</p></details>
  </figure>
}

export function ReadingPassage({ section }: { section: Section }) {
  return <section className="document-section" data-step={section.step}>
    <h2>{section.title}</h2>
    {!!section.roles?.length && <div className="roles-thread" aria-label="Who the chart represents">{section.roles.map((role, i) => <p key={i}><span>{role.label}</span><span className="thread-line" /><a href="#reading-chart" title={role.reason}>{role.planet}</a><small>{role.house ? `house ${role.house}` : 'natural role'}</small></p>)}</div>}
    {section.body.split('\n\n').map((p, n) => <p key={n}>{p}</p>)}
    <details className="margin-note"><summary title="The evidence and reasoning behind this passage">Evidence</summary>
      {section.worksheet?.checks && <><h3>Checks</h3>{Object.entries(section.worksheet.checks).map(([key, check]) => <p key={key}><em>{key.replaceAll('_', ' ')}:</em> {check.finding}</p>)}</>}
      {!!section.worksheet?.unknowns?.length && <><h3>Uncertainty</h3>{section.worksheet.unknowns.map((s,i)=><p key={i}>{s}</p>)}</>}
      {section.facts?.length ? <><h3>Chart</h3>{section.facts.map(f => <p key={f.id}>{factText(f.detail)}</p>)}</> : <p>Unrecorded.</p>}
      {!!section.roles?.length && <><h3>Roles</h3>{section.roles.map((r, i) => <p key={i}><em>{r.label}:</em> {r.reason}</p>)}</>}
      {!!section.rules?.length && <><h3>Source</h3>{section.rules.map(rule => <div key={rule.id}>{rule.quoted ? <blockquote>{rule.explanation}</blockquote> : <p><em>{rule.title}.</em> {rule.explanation}</p>}<p className="source-note">The Horary Textbook · printed pp. {rule.pages} · {rule.quoted ? 'source extract' : 'editorial paraphrase'}</p></div>)}</>}
      {section.step === 'judgment' && section.worksheet && <p className="source-note">This is a working interpretation to assess against these facts and the book.</p>}
      {section.because && <><h3>Interpretation</h3>{section.draft?.split('\n\n').map((p,i)=><p key={i}>{p}</p>)}<p>{section.because}</p><p className="source-note">A working proposal to assess against the facts above. The cited facts and rule let you assess it; they do not prove it correct.</p></>}
    </details>
  </section>
}
