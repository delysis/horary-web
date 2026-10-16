<task>Select the question's understood moment. This is not the date of the event about which someone asks.</task>

<procedure>
1. Inspect time_request, retained question, whether a chart exists, and the native current civil date in the selected reader place.
2. No explicit question-moment override + no chart: mode=now. Rust uses this turn's receive time, captured before model preparation. Leave local_time empty. Never manufacture today's date from model memory.
3. Same matter + existing chart + no explicit correction: mode=keep. Keep the chart timestamp, even if the clock has advanced or an ownership detail changes.
4. If clarification was required BEFORE first casting, use the receive time of the clarification that made the question intelligible. If we later discover the original understanding was faulty AFTER casting, keep that chart's moment unless the person explicitly corrects it.
5. For an explicit historical/corrected question moment, mode=explicit. Convert only the supplied civil date/time to YYYY-MM-DDTHH:MM using the selected place's zone. Use the supplied native date to resolve a stated relative question date; do not substitute the device zone for a different reader place.
6. occurrence is empty except for a repeated autumn clock time. Use earlier/later only if the person specified the occurrence. If not, ask which occurrence. A spring clock gap is impossible; ask for a valid corrected time after Rust reports the gap.
7. If a necessary date or AM/PM distinction is missing, ask one question. A loss time, wedding time or birthday is context, not a reason to request a historical chart.
8. Return basis explaining whose understood moment is used and why. Do not answer the horary question.
</procedure>

<worked_examples>
A: "I'm asking now: will I marry within a year?"; no chart. now; local_time="". Rust's receive timestamp survives a slow download or inference.
B: "I lost my keys yesterday at eight; where are they?"; no explicit QUESTION moment. now, not yesterday at eight.
C: original question unclear; final clarification says "I mean my own missing ring"; no chart. now at that clarification's receive time, not the start of the conversation.
D: existing chart; "Actually, my sister owns it." keep. This corrects ownership, not the question's understood moment.
E: "I understood this question in London on 2026-01-14 at 14:30." explicit; local_time="2026-01-14T14:30"; occurrence=""; selected Europe/London place.
F: historical New York 2026-11-01 01:30, no occurrence supplied; Rust says it occurs twice. ask="Was that the first or the second 1:30 that morning?" Do not default to either occurrence.
G: historical New York 2026-03-08 02:30; Rust reports a clock gap. ask for a valid time. No chart for an impossible moment.
H: "I woke last night and decided to cast at 02:15; use that." This expressly supplies the self-question's decision time. Resolve the date from native current date and ask if the date/occurrence is unclear; do not substitute now.
</worked_examples>

<output_fields>mode=now|keep|explicit|ask; local_time and occurrence are used only for explicit; clarification only for ask; basis records the moment distinction.</output_fields>
