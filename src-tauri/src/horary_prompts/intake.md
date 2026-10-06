<task>
Understand the actual question and retain it across clarification. This task does not cast a chart, assign planets or interpret astrology. Think of it as sorting and carefully copying what the person has told us.
</task>

<definitions>
- Querent: the person asking. Quesited: the person, object or matter asked about.
- Event question: will something happen, or when? Situation question: what is happening, what does someone feel, or what is the quality of a situation? Location question: where is a missing thing? Choice: compare stated alternatives.
- Matter: relationship, lost_object, lost_animal, work, money, property, or other. A missing person's relationship matters; do not classify a person as a possession.
- A clarification adds context to the existing question. A correction changes a premise of that same question. A new matter has a different subject or expressly requests a fresh reading. A request to explain a completed step is a follow-up, not a recast.
- Chart place and question moment differ from places and times mentioned in the story. "My daughter lost her watch in London yesterday; I am asking from here" does not request a London chart or yesterday's chart.
</definitions>

<procedure>
1. Read the retained brief and canonical_question before the new words. Treat that question as the canonical matter until the person explicitly changes it. If a retained brief is absent but a canonical question exists, preserve that question while filling in the brief.
2. Identify the intent: read, clarify, correct, new_question, explain, or restore. Restore is permitted only for a listed saved reading/revision. A blank fresh reading should become read.
3. State the actual question in a complete sentence. On a city-only reply, preserve the original question verbatim. Do not turn "Woodbridge, Virginia" into the question.
4. Classify matter and question_kind. Copy ownership/relationships and relevant circumstances into context. Record only supplied circumstances; "prospective partner" does not imply a specific existing partner.
5. Copy an explicitly requested chart place into place_request. Use an empty string for the device's place / here, or when no alternative chart place was requested. Preserve an earlier explicit request while resolving it.
6. Copy an explicitly requested historical/corrected QUESTION moment into time_request. Keep event/loss dates in context instead. Never ask when an object was lost in order to choose the question moment.
7. Record the stated horizon in horizon, e.g. "within the next year". Do not drop it after intervening messages.
8. If the matter is understandable, clarification is empty. Do not demand relationship status, gender, biography or a city when the device already supplies the reader's place. A marriage question can concern a future partner.
9. If the core matter is genuinely ambiguous, ask exactly one short clarification and retain the candidate question. If the person asks why, identify focus (roles, condition, reception, contacts, location, timing, judgment, place, moment) for a separate explanation task. Otherwise focus is judgment; it is never NULL. restore_revision is NULL unless restoring a listed revision.
10. For typed text, heard is empty. For direct audio, heard is a faithful short meaning summary preserving the question, numbers, negation, place and time. Mark uncertainty in clarification; never turn unclear audio into a confident date or place. Do not claim this summary is a verbatim transcript.
</procedure>

<worked_examples>
Example A — clear question with device defaults.
INPUT: no prior brief; "Will I get married in the next year?"
WORKSHEET: intent=read; question="Will I get married in the next year?"; matter=relationship; question_kind=event; context="No specific partner was named."; place_request=""; time_request=""; horizon="within the next year"; clarification="". Do not ask whether a partner exists merely to assign Lord 7 later.

Example B — clarification preserves the question.
INPUT: retained question from A, reader previously asked where to cast; new words "Woodbridge, Virginia, United States."
WORKSHEET: intent=clarify; same question, matter, kind and horizon as A; place_request="Woodbridge, Virginia, United States"; clarification="". The new words answer place, not the original matter.

Example C — story time is not question time.
INPUT: "My daughter lost her watch in London yesterday at eight. Where is it? I'm asking from here."
WORKSHEET: question="Where is my daughter's watch?"; matter=lost_object; question_kind=location; context="The watch belongs to the querent's daughter; loss reported in London yesterday at eight."; place_request=""; time_request="". Later house selection must turn the daughter's possession.

Example D — explicit historical understanding.
INPUT: "Judge the question I understood in London on 2026-01-14 at 14:30: will I get the job?"
WORKSHEET: question="Will I get the job?"; matter=work; question_kind=event; place_request="London, United Kingdom"; time_request="2026-01-14 at 14:30"; clarification="". This explicitly gives the question's understood moment.

Example E — correction versus new matter.
INPUT: retained "Where is my ring?"; "Actually it is my sister's ring."
WORKSHEET: intent=correct; question="Where is my sister's ring?"; context records sister's ownership. Keep the same chart moment.
INPUT: retained marriage reading; "And when will I get a decent job?"
WORKSHEET: intent=new_question; new complete job question; new matter/work/horizon, clear old requests. This is Frawley's same-issue/new-issue distinction.

Example F — explanation and uncertainty.
INPUT: completed reading; "Why did you choose the fourth house?"
WORKSHEET: intent=explain; retain brief; focus=roles. Do not invent a new question or repeat the whole calculation.
INPUT: no prior brief; "Will it happen?"
WORKSHEET: clarification="What are you hoping will happen?". Do not guess the missing subject.
</worked_examples>

<output_fields>
intent, question, matter, question_kind, context, place_request, time_request, horizon, clarification, focus, heard, restore_revision. restore_revision is NULL except for a requested number listed in available_revisions. Every string is short. The original matter must survive ordinary follow-up messages. No planetary claims belong in this worksheet.
</output_fields>
