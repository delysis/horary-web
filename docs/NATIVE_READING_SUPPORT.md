# Native reading support

Generated from the current Rust catalogue and native support inventory. This is an implementation audit of all 42 substantive families, using the prior [elicitation design](ELICITATION_DESIGN.md), [reading contracts](READING_CONTRACTS.md) and Frawley printed-page witnesses. It is not a model accuracy report or Eileen's acceptance.

Baseline 9abfa83 permitted 16 families and blocked 26 before adjudication. This implementation permits 18 families, including narrowly supported Parcel and Visit arrivals, and blocks 24 substantive families. Wish and Unclassified are two additional routing sentinels, not missing reading families. “Permit” means the native readiness gate can run the supported program; every family's source-correct interpretation remains unqualified.

| Family | Printed pages | Native handoff | Native support | Remaining obligations |
|---|---|---|---|---|
| `relationship` | 140, 191–200 | Supported branches | Operative person/partner houses, baseline, Moon claim priority; general evidence checks | Natural roles, arranged-wedding defaults and contextual synthesis remain semantic judgments; source-correct interpretations are not certified. |
| `lost_object` | 146–153, 244 | Supported branches | Owner's turned second; own-object second/fourth comparison; native location worksheet | Object choice, plausible places, recovery exceptions and physical descriptions require source review; no exact GPS or complete recovery-event calculation. |
| `lost_animal` | 1–3, 146–153 | Supported branches | Actual animal kind chooses ordinary sixth/twelfth; lost-matter/location path | Recovery and location interpretation need context; no exact location, safety or complete event certification. |
| `missing_person` | 146–153, 182–188 | Supported branches | Bound person's operative relationship; lost-matter/location path | Safety/prognosis, recovery and whereabouts are distinct source obligations; a generic contact does not certify return alive. |
| `movable_deal` | 156–161, 167–172 | Supported branches | Actual contracting parties; contextual asset, incoming money and beneficiary pocket roles | Intermediation, regret, amount/quality versus completion require semantic adjudication; exact sales tallies remain unsupported. |
| `money` | 156–161 | Supported branches | Source-specific money house; recipient/pocket and principal-only Moon obligations | Whether receipt is likely versus whether an amount is good remains semantic; no exact currency value or complete event chain. |
| `investment` | 156–161 | Supported branches | Owned possession/second-house role; general condition/reception checks | Comparative value/profit synthesis remains source review; no exact return or indiscriminate payment-arrival aspect. |
| `new_job` | 222–224 | Supported branches | External tenth with tenth-person exception; genuine worker/Moon obligations | Intermediary, motive, competition and perfection synthesis remain semantic; certified symbolic timing unavailable. |
| `existing_job` | 224–230 | Supported branches | Relevant person's turned tenth; general condition/reception/contact checks | Existing stability versus disruption/default and career/boss collisions remain contextual judgments. |
| `return_to_job` | 225–226 | Supported branches | Relevant worker/job with principal-only Moon for re-entry | Old-job re-entry/default, mediation and sequence are semantic obligations; not a generic acquisition proof. |
| `job_offer` | 224–226 | Supported branches | Job plus separate wages role and pay-priority obligation | Quality and the person's priorities need contextual synthesis; available offer does not require invented acquisition. |
| `work_person` | 224–225 | Supported branches | Native boss tenth, subordinate sixth, colleague seventh overrides | Collisions and multiple work capacities need contextual review; the role guard alone is not an interpretation. |
| `property` | 167–171 | Supported branches | Actual parties; fourth-house asset, tenth-house price and contextual property profit | Pure completion, price/condition and opposition exception need semantic separation; no valuation amount or complete event proof. |
| `rental` | 170–171 | Supported branches | Actual tenant/landlord parties; asset/price/profit roles; comparison inputs retained | Quality versus finding a tenancy, opposition regret and alternatives remain semantic; not ordinary servant-sixth allocation. |
| `business_property` | 170–171 | Blocked | Elicitation contract only; generic role helper is not a business adjudication | Dedicated business/property-profit role allocation and profit testimony are missing. |
| `choice` | 201–203 | Blocked | Current option/alternatives contract only | Dedicated first/seventh stay/change versus contextual-houses comparison and aggregation are missing. |
| `hiring` | 189–190 | Blocked | Candidate-input contract only | Dedicated sixth-house candidate matching/quality without compulsory acquisition aspect is missing. |
| `contact` | 165–166 | Supported branches | Person's actual relationship house; generic future contacts and condition/reception | A contact is not parcel arrival or a home visit; expectedness/long-lost defaults and timing are not separately enforced arrival programs. |
| `parcel` | 165–166 | Supported branches | Source-bound sender/receiver; actual task allocation; physical-contact gate | Past-receipt separating history/receipt-house proof and post-receipt question programs remain blocked; no numeric timing, complete no-aspect test or certified complex mediation. |
| `visit` | 166 | Supported branches | Bound visitor/receiver; friendship versus service capacity; expectedness retained; physical-contact gate | Ascendant/home-cusp contacts and their preference are unknown; no numeric timing, complete absence proof or certified complex mediation. |
| `contest` | 203–208 | Blocked | Competition/affiliation contract only | Dedicated allegiance/observer assignment, match versus championship and nonordinary Moon adjudication are missing. |
| `bet` | 156–161, 203–204 | Blocked | Money-question input contract only | Dedicated stake/profit and winnings program distinct from team-performance adjudication is missing. |
| `court_case` | 208–212 | Blocked | Party/claim input contract only | Dedicated parties, legal process tenth, verdict fourth and trial/default/perfection adjudication are missing. |
| `vehicle` | 142–143 | Blocked | Vehicle/capacity input contract only | Dedicated aboard/voyage first versus owned-possession second and safety testimony are missing. |
| `person_description` | 143–145, 191, 196 | Blocked | Native named-person/future-spouse role distinction exists; adjudication blocked | Dedicated main-significator descriptive synthesis and scope guards are missing; no exact height/ethnicity/marks inference. |
| `information` | 164–165 | Blocked | Claim/underlying-matter input contract only | Dedicated veracity versus relocated substantive matter and prophecy testimony are missing. |
| `trust` | 165 | Supported branches | Actual person's operative house; native own dignity/ability evidence | Contextual honesty/Mercury exceptions remain judgments; supplied dignity does not prove trustworthiness or accusations. |
| `pregnancy` | 173–174 | Blocked | Current-state/principal input contract only | Dedicated current pregnancy/parent roles and present-state testimony are missing; no clinical determination. |
| `fertility` | 174–177 | Blocked | Scope/principal input contract only | Dedicated conception, carrying, lifetime potential, coarse child-count and description programs are missing; no generic count substitute. |
| `adoption` | 177–178 | Blocked | Prospective/completed/known-parent input contract only | Dedicated someone-else's child versus own-fifth roles and adoption/perfection testimony are missing. |
| `medical` | 179–189 | Blocked | Patient/task/treatment input contract only | Dedicated diagnosis/prognosis/treatment role and clinical-scope program are missing; Lord 6 is not automatically illness. |
| `politics` | 212–214 | Blocked | Office/incumbency/observer input contract only | Dedicated supporter/impartial/foreign observer candidate allocation and election judgment are missing. |
| `knowledge` | 216–218 | Blocked | Knowledge-quality versus earnings input contract only | Dedicated ninth-house knowledge/tenth-house earnings and existing-profit interpretation are missing. |
| `exam` | 218 | Blocked | Passing versus admission input contract only | Dedicated result/profit-from-knowledge testimony and contextual default are missing. |
| `undertaking` | 219 | Blocked | Activity/principal input contract only | Dedicated undertaking quality versus its profit roles and benefit synthesis are missing. |
| `dream` | 219 | Blocked | Meaning versus prophetic truth/dream-account input contract only | Dedicated contextual characters versus ninth-house prophecy and interpretation are missing. |
| `education` | 219–221 | Blocked | School/university/task input contract only | Dedicated radical third/ninth institution and admission/enjoyment/quality/comparison program are missing. |
| `tax` | 231–232 | Blocked | Principal/tax-question input contract only | Dedicated government tenth/coffers eleventh/principal money second and assessment/ability interpretation are missing. |
| `allegation` | 233–234 | Blocked | Reported claim input contract only | Dedicated relationship/past-act testimony and allegation-scope judgment are missing; future contact cannot certify past harm. |
| `custody` | 234–237 | Blocked | Already-held versus not-held input contract only | Dedicated radical/turned twelfth roles and entry/release default/testimony are missing. |
| `weather` | 238–240 | Blocked | Target locality/period versus chart anchor contract only | Dedicated general/event/season house allocation and ruler-weather interpretation are missing. |
| `election` | 241–242 | Blocked | Action/window/constraints input contract only | Dedicated horary election program and exact future-action perfection calculations are missing. |

## Physical-arrival program boundaries

The [native program](../src-tauri/src/message_programs.rs) implements Frawley printed pp. 165–166 with common house/Moon, aspect, reception and timing chapters. The private OCR is pinned by SHA256 `cd5853df311b12f2ec7fcc612f49b0a5248a5c9b87ef7780730fbdcd618d32d4`; OCR pages 174–175 correspond to printed 165–166.

- Incoming parcel: bind the actual sender and receiver; before receipt, turn the sender's third. After receipt the role is the receiver's second, but that different judgment branch remains blocked. Sent-letter past receipt also remains blocked because separating history/receipt-house adjudication is not implemented.
- A self/relay receiver defaults only from the genuine principal scope. Other recipients require an actual bound actor; title ownership or an address is not a universal substitute.
- The incoming parcel has distinct receiver and receiver-possession routes plus the principal's Moon unless a selected house ruler already claims Moon. A seller-only contact cannot certify delivery. Shared rulers alone cannot invent an event.
- Visitor capacity chooses the actual relationship house or a source-backed hired service provider's sixth from the receiver. Expectedness remains explicit; no agreement is inferred from “will they come.” Who travels is set by the question, not by which ruler applies.
- The book's visit preference is Ascendant, receiver ruler, appropriate Moon, home cusp. Native Ascendant/home-cusp contacts are currently unknown. Unexpected Ascendant contact alone would not prove arrival; this program cannot fabricate that channel.
- Contacts retains the full intervening graph. A direct positive arrival proposal must cite an actual native parcel/visitor-to-receiver candidate before sign change and the prior accepted contacts worksheet. Complex mediation remains unverified, not silently promoted. Conditions and directed reception still require the existing evidence coverage checks.
- No short-window candidate is unknown, not a completed “no aspect, no arrival” test. Existing negative-evidence and numeric-timing guards remain active. Astronomical hours, current angular separation, approximate positions and native fact coverage do not certify calendar predictions or interpretation truth.

The static eight stage prompts, optimizer, model case banks and gold are not changed by this native implementation. New source-derived method profiles and typed input contracts still need controlled fresh native/model qualification.
