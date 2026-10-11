import { useEffect, useState } from 'react'
import pipeline from './ai/horary-judgement-pipeline.json'
import { buildInterpretationPrompt } from './ai/aiCoreWasm'
import { saveReviewExport, type HoraryInterpretation } from './tauriBridge.ts'

export type MethodStep = { id: string; title: string; instruction: string }
type Assignment = { actor: string; house: number; significator: string | null; canonicalEvidence: string }
type EvidenceChart = {
  bodies?: Array<{ name: string; sign: string; degree: number; house: number; geometricHouse?: number; dignity?: Record<string, unknown>; accidentalDignity?: { houseCapacity?: string; solarCondition?: string } }>
  derived?: { receptions?: Array<{ guestPlanet: string; hostPlanet: string; dignity: string; polarity: string }>; eventSearch?: { checkedUntilHours: number; method: string; events: Array<{ planet1: string; planet2: string; aspectName: string; estimatedPerfectsWithinHours: number; withinCurrentSigns: boolean }> } }
}

type MethodReviewProps = {
  question: string
  chart: unknown
  interpretation: HoraryInterpretation | null
}

export function MethodReview({ question, chart, interpretation }: MethodReviewProps) {
  const evidence = chart as EvidenceChart | null
  const [exportMessage, setExportMessage] = useState('')
  const [assignments, setAssignments] = useState<Assignment[]>([])
  useEffect(() => {
    let current = true
    setAssignments([])
    if (question.trim() && chart) {
      void buildInterpretationPrompt({ question, chart }).then(prompt => {
        const payload = JSON.parse(prompt.messages.find(m => m.role === 'user')!.content)
        if (current) setAssignments(payload.deterministicAssignments)
      }).catch(() => { if (current) setAssignments([]) })
    }
    return () => { current = false }
  }, [question, chart])

  async function exportReading() {
    try {
      const saved = await saveReviewExport(`horary-reading-${new Date().toISOString().slice(0, 10)}.json`, {
        format: 'horary-reading-review', version: 1, build: __HORARY_BUILD__, exportedAt: new Date().toISOString(),
        question, chart, interpretation, method: pipeline,
      })
      setExportMessage(saved ? 'Reading exported.' : 'Export cancelled.')
    } catch (error) { setExportMessage(`Could not export reading: ${String(error)}`) }
  }

  return <section className="method-review" aria-label="Reading method and evidence">
    <h2>Reading method &amp; evidence</h2>
    <p>Working from John Frawley’s <cite>The Horary Textbook</cite> (2005). Open a step to compare the method, its printed page references, and what this reading actually reported.</p>
    <p className="ai-help">The app calculates chart facts, then the model interprets them in one call. A model’s reported finding is an interpretation, not an independent verification. Keep notes in whatever form suits you; the export preserves this question, chart, reading, method, and app version alongside them.</p>
    <button disabled={!chart} onClick={exportReading}>Export this reading for review</button>
    {exportMessage ? <p role="status">{exportMessage}</p> : null}
    <p className="ai-help">The file includes the question and location. Nothing is sent automatically.</p>
    <details>
      <summary>House suggestions for this question</summary>
      <p>Keyword hints for review, not settled house assignments.</p>
      {assignments.length ? <table><caption>Current suggestions</caption><thead><tr><th>Role</th><th>House</th><th>Ruler</th></tr></thead>
        <tbody>{assignments.map((a, i) => <tr key={i}><td>{a.actor.replaceAll('_', ' ')}</td><td>{a.house}</td><td>{a.significator || 'Unavailable'}</td></tr>)}</tbody></table>
        : <p>Cast a chart and enter a question to see the suggestions.</p>}
    </details>
    <details>
      <summary>Overall rules and house meanings</summary>
      <ul>{pipeline.globalRules.map((rule, i) => <li key={i}>{rule}</li>)}</ul>
      <ul>{pipeline.commonHouseMap.map((house, i) => <li key={i}>House {house.house}: {house.core}</li>)}</ul>
    </details>
    <ol className="method-steps">
      {pipeline.microTasks.map(step => {
        const reported = interpretation?.judgementTrace.filter(trace => trace.stepId === step.id) || []
        return <li key={step.id}>
          <details>
            <summary>{step.title}{reported.length ? ' · reported in this reading' : ''}</summary>
            <p>{step.objective}</p>
            <p className="ai-help">Frawley, printed pages {step.sourcePages.join(', ')}.</p>
            <p><strong>Instruction sent to the model:</strong> {step.compactPrompt}</p>
            <details><summary>What this step considers</summary><ul>{step.detailedChecks.map((check, i) => <li key={i}>{check}</li>)}</ul></details>
            {reported.map((trace, i) => <div key={i}><p><strong>Model report:</strong> {trace.finding}</p><p><strong>Cited evidence:</strong> {trace.chartEvidence}</p></div>)}
            {!reported.length && interpretation ? <p className="ai-help">This reading did not report this step.</p> : null}
          </details>
        </li>
      })}
    </ol>
    <details><summary>Calculated chart evidence</summary>
      <p>These are the supplied facts, including named dignities, directed receptions, and any adjustment for a planet near the next house cusp. They do not establish the interpretation.</p>
      <table><thead><tr><th>Planet</th><th>Placement</th><th>Essential condition</th><th>Ability &amp; visibility</th></tr></thead>
        <tbody>{evidence?.bodies?.filter(body => body.dignity).map(body => <tr key={body.name}>
          <td>{body.name}</td><td>{body.sign} {body.degree.toFixed(2)}° · house {body.house}{body.geometricHouse !== undefined && body.geometricHouse !== body.house ? ` (advanced from ${body.geometricHouse} near cusp)` : ''}</td>
          <td>{Object.entries(body.dignity || {}).filter(([, value]) => value === true).map(([key]) => key.replace(/Ruler$/, '').replace(/([A-Z])/g, ' $1').toLowerCase()).join(', ') || 'Not established'}</td>
          <td>{body.accidentalDignity?.houseCapacity || 'Unknown'} by house; {body.accidentalDignity?.solarCondition?.replace(/([A-Z])/g, ' $1').toLowerCase() || '—'}</td>
        </tr>)}</tbody>
      </table>
      <details><summary>Receptions, including detriment and fall</summary><p>Read from the guest toward the host. Each direction is separate; context determines what the inclination means.</p>
        <table><thead><tr><th>Guest</th><th>Occupies the host’s</th><th>Host</th></tr></thead><tbody>{evidence?.derived?.receptions?.map((r,i) => <tr key={i}><td>{r.guestPlanet}</td><td>{r.dignity.replace(/Ruler$/, '')} · {r.polarity}</td><td>{r.hostPlanet}</td></tr>)}</tbody></table>
      </details>
      {evidence?.derived?.eventSearch ? <details><summary>Upcoming astronomical contacts</summary><p>{evidence.derived.eventSearch.method} Search: {evidence.derived.eventSearch.checkedUntilHours} hours. These intervals are not predictions of when the question’s outcome happens.</p>
        <table><thead><tr><th>Contact</th><th>Estimated hours</th><th>Signs</th></tr></thead><tbody>{evidence.derived.eventSearch.events.map((event,i) => <tr key={i}><td>{event.planet1} {event.aspectName.toLowerCase()} {event.planet2}</td><td>{event.estimatedPerfectsWithinHours.toFixed(1)}</td><td>{event.withinCurrentSigns ? 'Before either changes sign' : 'After a sign change'}</td></tr>)}</tbody></table>
      </details> : null}
    </details>
  </section>
}
