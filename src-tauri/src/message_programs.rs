//! Native programs for physical parcel receipt and personal arrival.
//! Frawley printed pp. 165–166, with pp. 15–38, 84–100, 127–136.
//! These guards establish role/evidence binding, never interpretation truth.
#![forbid(unsafe_code)]

use crate::{
    horary_lessons::Stage,
    horary_role_options::{self as role_options, Choice, ConditionalRole, Options, Subject},
    reading_contracts::{Consultation, Field, Limitation, Method, ReadyReading},
    reading_method::{Fact, NaturalRole, Role},
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

pub const VERSION: &str = "frawley-physical-arrival-2026-10-12.1";
const PRIMARY: &str = "subject.primary";
const RECEIVER: &str = "arrival.receiver";
const POCKET: &str = "parcel.receiver_pocket";

fn principal(case: &Consultation) -> Option<&str> {
    match case.text(Field::PrincipalMode)? {
        "relay" => case.text(Field::PrincipalId),
        "self" | "concerning_other" => Some("querent"),
        _ => None,
    }
}

/// An ordinary incoming self/relay question supplies its receiver by scope.
/// An item sent by that principal, or a question concerning another receiver,
/// must bind the actual intended receiver. Sending is not evidence of receipt.
pub fn receiver(case: &Consultation) -> Option<&str> {
    case.text(Field::Receiver).or_else(|| {
        if case.text(Field::PrincipalMode) == Some("concerning_other")
            || case.method() == Some(Method::Parcel)
                && case.text(Field::ParcelTask) != Some("received_possession")
                && case.text(Field::Sender).is_some()
                && case.text(Field::Sender) == principal(case)
        {
            None
        } else {
            principal(case)
        }
    })
}

fn participant_house(case: &Consultation, id: &str) -> Option<u8> {
    if id == "querent" || Some(id) == principal(case) {
        Some(1)
    } else {
        case.people
            .get(id)
            .and_then(|person| role_options::relationship_house(&person.relationship))
    }
}

/// Conservative evidence for a method-local service role, not a biographical
/// employee relationship. Exact quotation/participant identity is checked by
/// the consultation reducer. Ambiguous trades remain elicitation work.
pub fn service_capacity_evidence(quote: &str, source: &str) -> bool {
    role_options::service_capacity_evidence(quote, source)
}

pub fn limitation(case: &Consultation) -> Option<Limitation> {
    let (code, message) = match case.method()? {
        Method::Parcel => match case.text(Field::ParcelTask) {
            Some("past_receipt") => ("parcel_past_receipt_unverified", "This past-receipt question needs separating history or receipt-house testimony. The current native arrival program does not certify that past event; an upcoming contact cannot establish that the item was already received."),
            Some("received_possession") => ("parcel_received_program_needs_review", "The item is already the receiver's second-house possession. The current arrival program cannot answer a different post-receipt question by keeping the sender's third-house role."),
            _ => return None,
        },
        _ => return None,
    };
    Some(Limitation {
        code: code.into(),
        message: message.into(),
        printed_pages: "165–166".into(),
    })
}

fn add(options: &mut Options, id: &str, label: &str, house: u8, basis: String) {
    options.choices.push(Choice {
        id: id.into(),
        label: label.into(),
        house: Some(house),
        natural: None,
        basis,
    });
    options.required_groups.push(vec![id.into()]);
}

/// Each house is derived from a source-bound participant/capacity. No title
/// owner, seller default, invented visitor or destination is manufactured.
pub fn roles(case: &Consultation, subject: &Subject) -> Option<Options> {
    let method = case.method()?;
    if !matches!(method, Method::Parcel | Method::Visit) {
        return None;
    }
    let mut options = Options {
        choices: Vec::new(),
        required_groups: Vec::new(),
        conditional_roles: Vec::new(),
        missing: Vec::new(),
        compare: Vec::new(),
    };
    let Some(receiver) = receiver(case) else {
        options
            .missing
            .push("The actual receiver is unresolved.".into());
        return Some(options);
    };
    let Some(receiver_house) = participant_house(case, receiver) else {
        options
            .missing
            .push("The receiver's operative relationship is unresolved.".into());
        return Some(options);
    };
    add(&mut options, RECEIVER, "The receiver", receiver_house, format!("Source-bound receiver {receiver} has house {receiver_house}. This is the physical recipient, not automatically the title owner or sender (Frawley printed pp. 165–166)."));
    match method {
        Method::Parcel => {
            let house = match case.text(Field::ParcelTask) {
                Some("received_possession") => Some(role_options::turn(receiver_house, 2)),
                Some("arrival" | "past_receipt") => case
                    .text(Field::Sender)
                    .and_then(|sender| participant_house(case, sender))
                    .map(|sender_house| role_options::turn(sender_house, 3)),
                _ => None,
            };
            if let Some(house) = house {
                add(&mut options, PRIMARY, &subject.name, house, format!("Native parcel task {:?}: before receipt use the bound sender's third; after receipt use the bound receiver's second. Rust derives house {house}; the sender's own house is not the parcel (Frawley printed pp. 165–166).", case.text(Field::ParcelTask)));
                if case.text(Field::ParcelTask) == Some("arrival") {
                    add(&mut options, POCKET, "The receiver's possession", role_options::turn(receiver_house, 2), "The arriving parcel can contact the receiver's second-house pocket. This role differs from the parcel even when both houses share a ruler (Frawley printed p. 166).".into());
                }
            } else {
                options
                    .missing
                    .push("The parcel's actual task or sender capacity is unresolved.".into());
            }
        }
        Method::Visit => {
            let house = case.text(Field::Visitor).and_then(|visitor| {
                if case.text(Field::VisitorCapacity) == Some("service_provider") {
                    // This is an astrological capacity relative to the receiver,
                    // not a mutation of the person's recorded relationship.
                    Some(role_options::turn(receiver_house, 6))
                } else {
                    participant_house(case, visitor)
                }
            });
            if let Some(house) = house {
                add(&mut options, PRIMARY, &subject.name, house, format!("The actual visitor's operative capacity derives house {house}: friendship is eleventh, a hired service provider is sixth from the receiver. Do not relabel them from a name or invert the journey because the receiver's ruler applies (Frawley printed p. 166)."));
            } else {
                options
                    .missing
                    .push("The actual visitor capacity is unresolved.".into());
            }
        }
        _ => unreachable!("The entry guard selects physical-arrival methods"),
    }
    options.choices.push(Choice {
        id: "moon.contextual".into(),
        label: "The effective principal's Moon as receiver".into(),
        house: None,
        natural: Some(NaturalRole::Moon),
        basis: "Moon is receiver testimony only for the genuine principal, unless a selected house ruler already claims it. It is not transferred to a person merely asked about (Frawley printed pp. 32, 165–166).".into(),
    });
    if Some(receiver) == principal(case) {
        options.conditional_roles.push(ConditionalRole {
            choice_id: "moon.contextual".into(),
            unless_house_claims: NaturalRole::Moon,
            basis: "Retain the principal's Moon route for physical arrival unless a selected house ruler has first claim on Moon (Frawley printed pp. 32, 166).".into(),
        });
    } else {
        // Offering an optional transferable Moon could silently make another
        // receiver into the querent. The full chart still preserves Moon events.
        options
            .choices
            .retain(|choice| choice.id != "moon.contextual");
    }
    Some(options)
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct BoundRole {
    pub selected_id: String,
    pub planet: String,
    pub house: Option<u8>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ArrivalCandidate {
    pub fact_id: String,
    pub channel: String,
    pub within_current_signs: Option<bool>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Clipboard {
    pub version: String,
    pub method: Method,
    pub reading_input_sha256: String,
    pub parcel_task: Option<String>,
    pub expected: Option<bool>,
    pub receiver_id: String,
    pub primary: BoundRole,
    pub receiver: BoundRole,
    pub pocket: Option<BoundRole>,
    pub receiver_moon: Option<BoundRole>,
    pub candidates: Vec<ArrivalCandidate>,
    pub unknown_channels: Vec<String>,
    pub printed_pages: String,
}

fn selected_roles(worksheet: &Value, roles: &[Role]) -> Result<Vec<BoundRole>, String> {
    let selections = worksheet["selections"]
        .as_array()
        .ok_or("Arrival binding requires accepted selections")?;
    if selections.len() != roles.len() {
        return Err("Arrival selections and native derived roles must remain paired".into());
    }
    selections
        .iter()
        .zip(roles)
        .map(|(selection, role)| {
            Ok(BoundRole {
                selected_id: selection["id"]
                    .as_str()
                    .ok_or("An arrival role requires its selected ID")?
                    .into(),
                planet: role.planet.clone(),
                house: role.house,
            })
        })
        .collect()
}

fn candidates(
    primary: &BoundRole,
    receiver: &BoundRole,
    pocket: Option<&BoundRole>,
    moon: Option<&BoundRole>,
    facts: &[Fact],
) -> Vec<ArrivalCandidate> {
    facts
        .iter()
        .filter_map(|fact| {
            let event = fact.event.as_ref().filter(|_| fact.kind == "event")?;
            if fact.planets.len() != 2 || !fact.planets.contains(&primary.planet) {
                return None;
            }
            let channel = [
                Some((receiver, "receiver")),
                moon.map(|role| (role, "receiver_moon")),
                pocket.map(|role| (role, "receiver_possession")),
            ]
            .into_iter()
            .flatten()
            .find_map(|(target, channel)| {
                // Equal rulers do not fabricate a planet-to-itself event.
                (target.planet != primary.planet && fact.planets.contains(&target.planet))
                    .then_some(channel)
            })?;
            Some(ArrivalCandidate {
                fact_id: fact.id.clone(),
                channel: channel.into(),
                within_current_signs: event.within_current_signs,
            })
        })
        .collect()
}

/// Built from the immutable native handoff and the accepted role permit, never
/// from model-selected labels, expected facts or a transplanted checkpoint.
pub fn clipboard(
    ready: &ReadyReading,
    worksheet: &Value,
    roles: &[Role],
    facts: &[Fact],
) -> Result<Option<Clipboard>, String> {
    let method = ready.binding().frame.method;
    if !matches!(method, Method::Parcel | Method::Visit) {
        return Ok(None);
    }
    let request = ready.input();
    let inputs = &request["inputs"];
    let selected = selected_roles(worksheet, roles)?;
    let get = |id: &str| selected.iter().find(|role| role.selected_id == id).cloned();
    let primary = get(PRIMARY).ok_or("The actual parcel or visitor role is missing")?;
    let receiver = get(RECEIVER).ok_or("The actual receiver role is missing")?;
    let receiver_id = inputs["receiver"]
        .as_str()
        .or_else(|| match inputs["principal_mode"].as_str()? {
            "self" => Some("querent"),
            "relay" => inputs["principal_id"].as_str(),
            _ => None,
        })
        .ok_or("The immutable request does not bind a receiver")?
        .to_string();
    let principal_id = if inputs["principal_mode"] == "relay" {
        inputs["principal_id"].as_str()
    } else {
        Some("querent")
    };
    let pocket = get(POCKET);
    let moon = (Some(receiver_id.as_str()) == principal_id)
        .then(|| get("moon.contextual"))
        .flatten();
    let mut unknown_channels = vec![
        "separating_history_unverified".into(),
        "symbolic_travel_to_perfection_unverified".into(),
        "short_window_absence_is_not_no_arrival".into(),
        "after_sign_change_arrival_judgment_unverified".into(),
    ];
    if method == Method::Visit {
        unknown_channels.extend([
            "ascendant_contact_unverified".into(),
            "receiver_home_cusp_contact_unverified".into(),
        ]);
    }
    Ok(Some(Clipboard {
        version: VERSION.into(),
        method,
        reading_input_sha256: ready.binding().input_sha256.clone(),
        parcel_task: inputs["parcel_task"].as_str().map(str::to_string),
        expected: match inputs["expected"].as_str() {
            Some("yes") => Some(true),
            Some("no") => Some(false),
            _ => None,
        },
        receiver_id,
        primary: primary.clone(),
        receiver: receiver.clone(),
        pocket: pocket.clone(),
        receiver_moon: moon.clone(),
        candidates: candidates(&primary, &receiver, pocket.as_ref(), moon.as_ref(), facts),
        unknown_channels,
        printed_pages: "32, 84–100, 127–136, 165–166".into(),
    }))
}

/// The existing stage schema/checks remain authoritative. This additional
/// gate forbids a seller-only event or an invented angle from becoming a
/// positive physical-arrival proposal. Complex mediation stays unverified.
pub fn check(stage: Stage, input: &Value, value: &Value, facts: &[Fact]) -> Result<(), String> {
    if !matches!(stage, Stage::Contacts | Stage::Judgment) {
        return Ok(());
    }
    let Some(raw) = input.get("native_arrival").filter(|raw| !raw.is_null()) else {
        return if matches!(
            input["reading_request"]["binding"]["frame"]["method"].as_str(),
            Some("parcel" | "visit")
        ) {
            Err("This physical-arrival reading requires its actual native arrival clipboard; do not bypass the method guard.".into())
        } else {
            Ok(())
        };
    };
    let clipboard: Clipboard = serde_json::from_value(raw.clone())
        .map_err(|error| format!("Invalid native arrival binding: {error}"))?;
    if clipboard.version != VERSION
        || !matches!(clipboard.method, Method::Parcel | Method::Visit)
        || input["reading_request"]["binding"]["input_sha256"] != clipboard.reading_input_sha256
        || input["reading_request"]["binding"]["frame"]["method"]
            != serde_json::to_value(clipboard.method).map_err(|e| e.to_string())?
    {
        return Err(
            "The arrival clipboard must belong to this immutable native reading request".into(),
        );
    }
    if clipboard.candidates
        != candidates(
            &clipboard.primary,
            &clipboard.receiver,
            clipboard.pocket.as_ref(),
            clipboard.receiver_moon.as_ref(),
            facts,
        )
    {
        return Err("Arrival candidates must bind the actual supplied parcel/visitor and receiver endpoints".into());
    }
    let cites_arrival = |ids: &Value| {
        ids.as_array().is_some_and(|ids| {
            ids.iter().any(|id| {
                clipboard.candidates.iter().any(|candidate| {
                    id.as_str() == Some(candidate.fact_id.as_str())
                        && candidate.within_current_signs == Some(true)
                })
            })
        })
    };
    if stage == Stage::Contacts
        && value["basis"] == "direct_candidate"
        && (!cites_arrival(&value["candidate_ids"])
            || !cites_arrival(&value["checks"]["applying_or_separating"]["evidence"]))
    {
        return Err("A direct physical-arrival contact basis must cite an actual native parcel/visitor-to-receiver arrival candidate in candidate_ids and applying_or_separating. Seller-only contact, absent coverage or an uncalculated angle cannot establish arrival; use an unverified/unknown basis when this channel is not established.".into());
    }
    if stage == Stage::Judgment && value["verdict"] == "likely_yes" {
        if !cites_arrival(&value["checks"]["supporting_testimony"]["evidence"])
            || !cites_arrival(&value["evidence"])
        {
            return Err("A positive physical-arrival proposal must cite a supplied native parcel/visitor-to-receiver candidate before sign change in both supporting_testimony and the answer's evidence. Contact with the sender alone, shared rulers without an event, or unknown angle/timing calculations do not establish delivery.".into());
        }
        if !cites_arrival(&input["contacts"]["candidate_ids"]) {
            return Err("The positive arrival proposal needs its actual arrival candidate accepted by the contacts step; do not bypass an unresolved contact finding.".into());
        }
    }
    Ok(())
}

pub fn guide(method: Method) -> &'static str {
    match method {
        Method::Parcel => "NATIVE PHYSICAL-RECEIPT PROGRAM (Frawley pp. 165–166): 1. Follow reading_request parcel_task and actual sender/receiver: arrival asks about future physical receipt, using sender's third before receipt; past_receipt asks whether receipt already happened, needing separating/receipt-house history currently unverified; received_possession concerns an already received receiver's second-house item, a different program. Sending a letter does not make its receipt a past event. An outgoing future-arrival question still uses the bound sender's third and actual other receiver, never an automatic return to sender. 2. Preserve parcel and receiver/pocket separately even when rulers coincide. Preserve Moon only for the genuine principal unless a house ruler claims it. 3. native_arrival lists actual parcel-to-receiver, appropriate Moon or receiver's second candidates. Seller-only contact and shared rulers alone do not establish delivery. 4. Inspect sign changes, intervening contacts, condition and directed reception before contextual interpretation. Candidate identity is necessary evidence, not sufficient truth. Complex mediation stays unverified. 5. No short-window candidate is unknown, not the book's complete no-aspect test. Answer the original question and limits directly. Exact timing stays unknown; astronomical hours/current separation are not symbolic calendar time.",
        Method::Visit => "NATIVE PHYSICAL-VISIT PROGRAM (Frawley p. 166): 1. Use the actual visitor's operative capacity and source-bound receiver. Friend: eleventh; hired service provider: sixth from receiver, without making a biographical employee assertion. 2. Preserve expected=yes/no; will-they-come wording alone does not imply an agreement. 3. Source preference is Ascendant, receiver's ruler, appropriate Moon, receiver's home cusp. This calculation supplies only planet-to-planet routes: native_arrival records Ascendant/home-cusp routes as unknown, not absent. An unexpected visitor's Ascendant contact alone would show preoccupation unless other testimony confirms physical arrival. 4. Follow actual visitor/receiver contacts and the full intervening graph. Which planet applies does not change who travels to whom; the question sets travel direction. 5. Keep the stated afternoon/appointment horizon, but no numerical symbolic timing is certified. Arcminutes-to-minutes in this specific textbook context needs exact native travel/angle data; approximate astronomical hours are not that data. No short-window candidate is unknown, not a negative arrival verdict. 6. Integrate relevant condition/reception and source limitations into a contextual answer; a bound event ID does not prove the interpretation.",
        _ => "",
    }
}

pub fn with_clipboard(mut input: Value, clipboard: &Option<Clipboard>) -> Value {
    if let Some(clipboard) = clipboard {
        input["native_arrival"] = json!(clipboard);
    }
    input
}

/// Read-only implementation inventory. `Implemented` permits a native journey;
/// it is not a source-correct reading score or empirical model qualification.
#[cfg(test)]
fn support(method: Method) -> (&'static str, &'static str) {
    use Method::*;
    match method {
        Relationship => ("Operative person/partner houses, baseline, Moon claim priority; general evidence checks", "Natural roles, arranged-wedding defaults and contextual synthesis remain semantic judgments; source-correct interpretations are not certified."),
        LostObject => ("Owner's turned second; own-object second/fourth comparison; native location worksheet", "Object choice, plausible places, recovery exceptions and physical descriptions require source review; no exact GPS or complete recovery-event calculation."),
        LostAnimal => ("Actual animal kind chooses ordinary sixth/twelfth; lost-matter/location path", "Recovery and location interpretation need context; no exact location, safety or complete event certification."),
        MissingPerson => ("Bound person's operative relationship; lost-matter/location path", "Safety/prognosis, recovery and whereabouts are distinct source obligations; a generic contact does not certify return alive."),
        MovableDeal => ("Actual contracting parties; contextual asset, incoming money and beneficiary pocket roles", "Intermediation, regret, amount/quality versus completion require semantic adjudication; exact sales tallies remain unsupported."),
        Money => ("Source-specific money house; recipient/pocket and principal-only Moon obligations", "Whether receipt is likely versus whether an amount is good remains semantic; no exact currency value or complete event chain."),
        Investment => ("Owned possession/second-house role; general condition/reception checks", "Comparative value/profit synthesis remains source review; no exact return or indiscriminate payment-arrival aspect."),
        NewJob => ("External tenth with tenth-person exception; genuine worker/Moon obligations", "Intermediary, motive, competition and perfection synthesis remain semantic; certified symbolic timing unavailable."),
        ExistingJob => ("Relevant person's turned tenth; general condition/reception/contact checks", "Existing stability versus disruption/default and career/boss collisions remain contextual judgments."),
        ReturnToJob => ("Relevant worker/job with principal-only Moon for re-entry", "Old-job re-entry/default, mediation and sequence are semantic obligations; not a generic acquisition proof."),
        JobOffer => ("Job plus separate wages role and pay-priority obligation", "Quality and the person's priorities need contextual synthesis; available offer does not require invented acquisition."),
        WorkPerson => ("Native boss tenth, subordinate sixth, colleague seventh overrides", "Collisions and multiple work capacities need contextual review; the role guard alone is not an interpretation."),
        Property => ("Actual parties; fourth-house asset, tenth-house price and contextual property profit", "Pure completion, price/condition and opposition exception need semantic separation; no valuation amount or complete event proof."),
        Rental => ("Actual tenant/landlord parties; asset/price/profit roles; comparison inputs retained", "Quality versus finding a tenancy, opposition regret and alternatives remain semantic; not ordinary servant-sixth allocation."),
        Contact => ("Person's actual relationship house; generic future contacts and condition/reception", "A contact is not parcel arrival or a home visit; expectedness/long-lost defaults and timing are not separately enforced arrival programs."),
        Trust => ("Actual person's operative house; native own dignity/ability evidence", "Contextual honesty/Mercury exceptions remain judgments; supplied dignity does not prove trustworthiness or accusations."),
        Parcel => ("Source-bound sender/receiver; actual task allocation; physical-contact gate", "Past-receipt separating history/receipt-house proof and post-receipt question programs remain blocked; no numeric timing, complete no-aspect test or certified complex mediation."),
        Visit => ("Bound visitor/receiver; friendship versus service capacity; expectedness retained; physical-contact gate", "Ascendant/home-cusp contacts and their preference are unknown; no numeric timing, complete absence proof or certified complex mediation."),
        BusinessProperty => ("Elicitation contract only; generic role helper is not a business adjudication", "Dedicated business/property-profit role allocation and profit testimony are missing."),
        Choice => ("Current option/alternatives contract only", "Dedicated first/seventh stay/change versus contextual-houses comparison and aggregation are missing."),
        Hiring => ("Candidate-input contract only", "Dedicated sixth-house candidate matching/quality without compulsory acquisition aspect is missing."),
        Contest => ("Competition/affiliation contract only", "Dedicated allegiance/observer assignment, match versus championship and nonordinary Moon adjudication are missing."),
        Bet => ("Money-question input contract only", "Dedicated stake/profit and winnings program distinct from team-performance adjudication is missing."),
        CourtCase => ("Party/claim input contract only", "Dedicated parties, legal process tenth, verdict fourth and trial/default/perfection adjudication are missing."),
        Vehicle => ("Vehicle/capacity input contract only", "Dedicated aboard/voyage first versus owned-possession second and safety testimony are missing."),
        PersonDescription => ("Native named-person/future-spouse role distinction exists; adjudication blocked", "Dedicated main-significator descriptive synthesis and scope guards are missing; no exact height/ethnicity/marks inference."),
        Information => ("Claim/underlying-matter input contract only", "Dedicated veracity versus relocated substantive matter and prophecy testimony are missing."),
        Pregnancy => ("Current-state/principal input contract only", "Dedicated current pregnancy/parent roles and present-state testimony are missing; no clinical determination."),
        Fertility => ("Scope/principal input contract only", "Dedicated conception, carrying, lifetime potential, coarse child-count and description programs are missing; no generic count substitute."),
        Adoption => ("Prospective/completed/known-parent input contract only", "Dedicated someone-else's child versus own-fifth roles and adoption/perfection testimony are missing."),
        Medical => ("Patient/task/treatment input contract only", "Dedicated diagnosis/prognosis/treatment role and clinical-scope program are missing; Lord 6 is not automatically illness."),
        Politics => ("Office/incumbency/observer input contract only", "Dedicated supporter/impartial/foreign observer candidate allocation and election judgment are missing."),
        Knowledge => ("Knowledge-quality versus earnings input contract only", "Dedicated ninth-house knowledge/tenth-house earnings and existing-profit interpretation are missing."),
        Exam => ("Passing versus admission input contract only", "Dedicated result/profit-from-knowledge testimony and contextual default are missing."),
        Undertaking => ("Activity/principal input contract only", "Dedicated undertaking quality versus its profit roles and benefit synthesis are missing."),
        Dream => ("Meaning versus prophetic truth/dream-account input contract only", "Dedicated contextual characters versus ninth-house prophecy and interpretation are missing."),
        Education => ("School/university/task input contract only", "Dedicated radical third/ninth institution and admission/enjoyment/quality/comparison program are missing."),
        Tax => ("Principal/tax-question input contract only", "Dedicated government tenth/coffers eleventh/principal money second and assessment/ability interpretation are missing."),
        Allegation => ("Reported claim input contract only", "Dedicated relationship/past-act testimony and allegation-scope judgment are missing; future contact cannot certify past harm."),
        Custody => ("Already-held versus not-held input contract only", "Dedicated radical/turned twelfth roles and entry/release default/testimony are missing."),
        Weather => ("Target locality/period versus chart anchor contract only", "Dedicated general/event/season house allocation and ruler-weather interpretation are missing."),
        Election => ("Action/window/constraints input contract only", "Dedicated horary election program and exact future-action perfection calculations are missing."),
        Wish | Unclassified => ("Routing sentinel; not a substantive reading family", "Select the underlying disclosed matter without guessing an eleventh-house catch-all."),
    }
}

#[cfg(test)]
pub fn inventory() -> Vec<Value> {
    crate::reading_contracts::CATALOGUE.iter().filter(|card|!matches!(card.method,Method::Wish|Method::Unclassified)).map(|card| {
        let (present,gaps)=support(card.method);
        json!({"method":card.method,"printed_pages":card.printed_pages,"native_handoff":matches!(card.coverage,crate::reading_contracts::Coverage::Implemented),"native_support":present,"remaining_obligations":gaps,"source_correct_interpretation":"unqualified"})
    }).collect()
}

#[cfg(test)]
pub fn support_documentation() -> String {
    let cards = inventory();
    let permitted = cards
        .iter()
        .filter(|card| card["native_handoff"] == true)
        .count();
    let mut out=format!("# Native reading support\n\nGenerated from the current Rust catalogue and native support inventory. This is an implementation audit of all {} substantive families, using the prior [elicitation design](ELICITATION_DESIGN.md), [reading contracts](READING_CONTRACTS.md) and Frawley printed-page witnesses. It is not a model accuracy report or Eileen's acceptance.\n\nBaseline 9abfa83 permitted 16 families and blocked 26 before adjudication. This implementation permits {permitted} families, including narrowly supported Parcel and Visit arrivals, and blocks {} substantive families. Wish and Unclassified are two additional routing sentinels, not missing reading families. “Permit” means the native readiness gate can run the supported program; every family's source-correct interpretation remains unqualified.\n\n| Family | Printed pages | Native handoff | Native support | Remaining obligations |\n|---|---|---|---|---|\n",cards.len(),cards.len()-permitted);
    for card in cards {
        out.push_str(&format!(
            "| `{}` | {} | {} | {} | {} |\n",
            card["method"].as_str().unwrap(),
            card["printed_pages"].as_str().unwrap(),
            if card["native_handoff"] == true {
                "Supported branches"
            } else {
                "Blocked"
            },
            card["native_support"].as_str().unwrap(),
            card["remaining_obligations"].as_str().unwrap()
        ));
    }
    out.push_str("\n## Physical-arrival program boundaries\n\nThe [native program](../src-tauri/src/message_programs.rs) implements Frawley printed pp. 165–166 with common house/Moon, aspect, reception and timing chapters. The private OCR is pinned by SHA256 `cd5853df311b12f2ec7fcc612f49b0a5248a5c9b87ef7780730fbdcd618d32d4`; OCR pages 174–175 correspond to printed 165–166.\n\n- Incoming parcel: bind the actual sender and receiver; before receipt, turn the sender's third. After receipt the role is the receiver's second, but that different judgment branch remains blocked. Sent-letter past receipt also remains blocked because separating history/receipt-house adjudication is not implemented.\n- A self/relay receiver defaults only from the genuine principal scope. Other recipients require an actual bound actor; title ownership or an address is not a universal substitute.\n- The incoming parcel has distinct receiver and receiver-possession routes plus the principal's Moon unless a selected house ruler already claims Moon. A seller-only contact cannot certify delivery. Shared rulers alone cannot invent an event.\n- Visitor capacity chooses the actual relationship house or a source-backed hired service provider's sixth from the receiver. Expectedness remains explicit; no agreement is inferred from “will they come.” Who travels is set by the question, not by which ruler applies.\n- The book's visit preference is Ascendant, receiver ruler, appropriate Moon, home cusp. Native Ascendant/home-cusp contacts are currently unknown. Unexpected Ascendant contact alone would not prove arrival; this program cannot fabricate that channel.\n- Contacts retains the full intervening graph. A direct positive arrival proposal must cite an actual native parcel/visitor-to-receiver candidate before sign change and the prior accepted contacts worksheet. Complex mediation remains unverified, not silently promoted. Conditions and directed reception still require the existing evidence coverage checks.\n- No short-window candidate is unknown, not a completed “no aspect, no arrival” test. Existing negative-evidence and numeric-timing guards remain active. Astronomical hours, current angular separation, approximate positions and native fact coverage do not certify calendar predictions or interpretation truth.\n\nThe static eight stage prompts, optimizer, model case banks and gold are not changed by this native implementation. New source-derived method profiles and typed input contracts still need controlled fresh native/model qualification.\n");
    out
}

#[cfg(test)]
mod tests {
    mod journeys;
    use super::*;
    use crate::{
        horary_lessons::Matter,
        horary_role_options::Person,
        reading_contracts::{
            Anchor, Evidence, Facet, Frame, Intent, Observation, Slot, Turn, Update, UpdateMode,
        },
        reading_method::EventCandidate,
    };

    fn anchor() -> Anchor {
        Anchor {
            timestamp_ms: 1789387200000.,
            latitude: 38.657,
            longitude: -77.249,
            timezone: "America/New_York".into(),
        }
    }

    fn patch(method: Method, words: &str, actor: Person, fields: &[(Field, &str)]) -> Turn {
        Turn {
            intent: Intent::Read,
            question: Some(words.into()),
            frame: Some(Frame {
                method,
                facet: Facet::Event,
            }),
            people: vec![actor.clone()],
            subject: Some(Subject {
                name: if method == Method::Parcel {
                    "The ordered book".into()
                } else {
                    actor.label.clone()
                },
                kind: if method == Method::Parcel {
                    "movable"
                } else {
                    "person"
                }
                .into(),
                owner_id: if method == Method::Parcel {
                    String::new()
                } else {
                    actor.id
                },
                source_quote: words.into(),
            }),
            updates: fields
                .iter()
                .map(|(field, value)| Update {
                    field: *field,
                    value: (*value).into(),
                    quote: words.into(),
                    mode: UpdateMode::Supply,
                })
                .collect(),
            heard: String::new(),
            unavailable_quote: String::new(),
            focus: "judgment".into(),
            restore_revision: None,
        }
    }

    fn parcel() -> Consultation {
        let words = "Will the book the seller mailed to me arrive? I have not received it.";
        let mut case = Consultation::default();
        case.apply(
            &patch(
                Method::Parcel,
                words,
                Person {
                    id: "seller".into(),
                    label: "Seller".into(),
                    relationship: "other_party".into(),
                    source_quote: "the seller".into(),
                },
                &[(Field::ParcelTask, "arrival"), (Field::Sender, "seller")],
            ),
            1,
            words,
            false,
        )
        .unwrap();
        case
    }

    fn visit(relationship: &str, service: bool) -> Consultation {
        let words = if service {
            "When will the electrician I hired arrive? They agreed to come this afternoon."
        } else {
            "Will my friend Lee come today as we agreed?"
        };
        let mut fields = vec![(Field::Visitor, "visitor"), (Field::Expected, "yes")];
        if service {
            fields.push((Field::VisitorCapacity, "service_provider"));
        }
        let mut case = Consultation::default();
        case.apply(
            &patch(
                Method::Visit,
                words,
                Person {
                    id: "visitor".into(),
                    label: if service { "The electrician" } else { "Lee" }.into(),
                    relationship: relationship.into(),
                    source_quote: words.into(),
                },
                &fields,
            ),
            1,
            words,
            false,
        )
        .unwrap();
        case
    }

    fn fact(id: &str, kind: &str, planets: &[&str], event: bool) -> Fact {
        Fact {
            id: id.into(),
            kind: kind.into(),
            label: id.into(),
            detail: "Synthetic native evidence, no interpretation claim".into(),
            planets: planets.iter().map(|p| (*p).into()).collect(),
            condition_facet: None,
            event: event.then_some(EventCandidate {
                within_current_signs: Some(true),
                astronomical_hours: 3.,
                perfection_motion: None,
            }),
            reception: None,
        }
    }

    fn house_facts() -> Vec<Fact> {
        let chart = json!({"houses":[
            {"number":1,"sign":"Aries","degree":10.},
            {"number":2,"sign":"Taurus","degree":10.},
            {"number":3,"sign":"Gemini","degree":10.},
            {"number":4,"sign":"Cancer","degree":10.},
            {"number":5,"sign":"Leo","degree":10.},
            {"number":6,"sign":"Virgo","degree":10.},
            {"number":7,"sign":"Libra","degree":10.},
            {"number":8,"sign":"Scorpio","degree":10.},
            {"number":9,"sign":"Sagittarius","degree":10.},
            {"number":10,"sign":"Capricorn","degree":10.},
            {"number":11,"sign":"Aquarius","degree":10.},
            {"number":12,"sign":"Pisces","degree":10.}
        ]});
        crate::reading_method::facts(Some(&chart))
    }

    fn accepted(case: &Consultation, facts: &[Fact]) -> (ReadyReading, Value, Vec<Role>) {
        let ready = ReadyReading::prepare(case, anchor()).unwrap_or_else(|plan| {
            panic!(
                "Native handoff blocked: {}",
                serde_json::to_value(plan).unwrap()
            )
        });
        let options = crate::horary_role_options::build_for(
            case,
            Matter::Other,
            &ready.brief().people,
            &ready.brief().subject,
        );
        let worksheet = json!({"selections":options.choices.iter().map(|choice|json!({"id":choice.id,"reason":"Actual native option in synthetic offline permit"})).collect::<Vec<_>>(),"summary":"Synthetic offline method handoff","unknowns":[]});
        let input = json!({"reading_request":ready.input(),"native_role_options":options});
        let checked = crate::horary_step::check(
            Stage::Significators,
            Matter::Other,
            &worksheet,
            &input,
            facts,
        )
        .unwrap();
        let data = match checked {
            crate::horary_step::Checked::Data(data) => data,
            _ => panic!("A role permit was required"),
        };
        (ready, worksheet, data.roles().to_vec())
    }

    fn stage_worksheet(stage: Stage) -> Value {
        let checks = stage.checks().iter().map(|key|((*key).to_string(),json!({"state":"unestablished","evidence":[],"finding":"Synthetic mechanics; semantic interpretation unobserved"}))).collect::<serde_json::Map<_,_>>();
        let mut value = json!({"checks":checks,"summary":"Synthetic method guard","unknowns":[]});
        if stage == Stage::Contacts {
            value["basis"] = json!("direct_candidate");
            value["candidate_ids"] = json!([]);
            value["candidate_signs"] = json!([]);
        }
        if stage == Stage::Judgment {
            value["verdict"] = json!("likely_yes");
            value["answer"]=json!("The synthetic fixture demonstrates an evidence-bound arrival proposal, not a source-correct interpretation.");
            value["evidence"] = json!([]);
        }
        value
    }

    #[test]
    fn physical_parcel_handoff_uses_senders_third_without_title_or_destination() {
        let case = parcel();
        let (ready, worksheet, roles) = accepted(&case, &house_facts());
        assert_eq!(ready.input()["subject"]["owner_id"], "");
        assert!(ready.input()["inputs"]["receiver"].is_null());
        let native = clipboard(&ready, &worksheet, &roles, &[]).unwrap().unwrap();
        assert_eq!(native.primary.house, Some(9));
        assert_eq!(native.receiver.house, Some(1));
        assert_eq!(native.pocket.as_ref().unwrap().house, Some(2));
        assert_eq!(native.receiver_id, "querent");
        assert_eq!(
            ready.binding().question,
            case.question.resolved().unwrap().as_str()
        );
    }

    #[test]
    fn sender_relative_turning_and_received_roles_are_distinct() {
        let mut case = parcel();
        case.people.get_mut("seller").unwrap().relationship = "mother".into();
        let choices = roles(&case, case.subject.resolved().unwrap()).unwrap();
        assert_eq!(
            choices
                .choices
                .iter()
                .find(|c| c.id == PRIMARY)
                .unwrap()
                .house,
            Some(12)
        );
        case.facts.insert(
            Field::ParcelTask,
            Slot::Resolved {
                observation: Observation {
                    value: "received_possession".into(),
                    evidence: Evidence::Migration {
                        detail: "Synthetic already-received branch".into(),
                    },
                },
            },
        );
        let choices = roles(&case, case.subject.resolved().unwrap()).unwrap();
        assert_eq!(
            choices
                .choices
                .iter()
                .find(|c| c.id == PRIMARY)
                .unwrap()
                .house,
            Some(2)
        );
        assert_eq!(
            case.plan(Some(&anchor())).limitation.unwrap().code,
            "parcel_received_program_needs_review"
        );
        assert!(ReadyReading::prepare(&case, anchor()).is_err());
    }

    #[test]
    fn sent_history_stays_explicitly_unverified_and_missing_receiver_is_not_fabricated() {
        let mut case = parcel();
        case.facts.insert(
            Field::Sender,
            Slot::Resolved {
                observation: Observation {
                    value: "querent".into(),
                    evidence: Evidence::Migration {
                        detail: "Synthetic outgoing letter".into(),
                    },
                },
            },
        );
        case.facts.insert(
            Field::ParcelTask,
            Slot::Resolved {
                observation: Observation {
                    value: "past_receipt".into(),
                    evidence: Evidence::Migration {
                        detail: "Synthetic sent-history branch".into(),
                    },
                },
            },
        );
        assert_eq!(receiver(&case), None);
        let plan = case.plan(Some(&anchor()));
        assert!(plan
            .needs
            .iter()
            .any(|n| n.key == crate::reading_contracts::RequirementKey::Field(Field::Receiver)));
        assert_eq!(
            plan.limitation.unwrap().code,
            "parcel_past_receipt_unverified"
        );
        assert!(ReadyReading::prepare(&case, anchor()).is_err());
    }

    fn outgoing() -> Consultation {
        let words = "Will the letter I mailed to my father arrive tomorrow?";
        let mut proposal = patch(
            Method::Parcel,
            words,
            Person {
                id: "father".into(),
                label: "My father".into(),
                relationship: "father".into(),
                source_quote: "my father".into(),
            },
            &[
                (Field::ParcelTask, "arrival"),
                (Field::Sender, "querent"),
                (Field::Receiver, "father"),
            ],
        );
        proposal.subject.as_mut().unwrap().name = "The letter".into();
        let mut case = Consultation::default();
        case.apply(&proposal, 1, words, false).unwrap();
        case
    }

    #[test]
    fn outgoing_future_delivery_uses_senders_third_and_actual_other_receiver() {
        let case = outgoing();
        let (ready, worksheet, roles) = accepted(&case, &house_facts());
        let native = clipboard(&ready, &worksheet, &roles, &[]).unwrap().unwrap();
        assert_eq!(native.parcel_task.as_deref(), Some("arrival"));
        assert_eq!(native.primary.house, Some(3));
        assert_eq!(native.receiver_id, "father");
        assert_eq!(native.receiver.house, Some(4));
        assert!(native.receiver_moon.is_none());
        assert!(limitation(&case).is_none());
    }

    #[test]
    fn visits_derive_friend_and_service_capacities_without_rewriting_people() {
        for (case, house) in [(visit("friend", false), 11), (visit("unknown", true), 6)] {
            let (ready, worksheet, roles) = accepted(&case, &house_facts());
            let native = clipboard(&ready, &worksheet, &roles, &[]).unwrap().unwrap();
            assert_eq!(native.primary.house, Some(house));
            assert_eq!(native.expected, Some(true));
            assert!(native
                .unknown_channels
                .contains(&"ascendant_contact_unverified".into()));
            assert!(native
                .unknown_channels
                .contains(&"receiver_home_cusp_contact_unverified".into()));
            assert_eq!(
                ready.input()["people"]["visitor"]["relationship"],
                if house == 6 { "unknown" } else { "friend" }
            );
        }
    }

    #[test]
    fn unknown_visitor_and_expectedness_stay_genuine_input_gaps() {
        let mut case = visit("unknown", true);
        case.facts.remove(&Field::VisitorCapacity);
        case.facts.remove(&Field::Expected);
        let plan = case.plan(Some(&anchor()));
        for field in [Field::VisitorCapacity, Field::Expected] {
            assert!(plan
                .needs
                .iter()
                .any(|n| n.key == crate::reading_contracts::RequirementKey::Field(field)));
        }
        assert!(ReadyReading::prepare(&case, anchor()).is_err());
    }

    #[test]
    fn unsupported_service_claim_and_unbound_actor_do_not_reach_handoff() {
        let mut case = visit("friend", false);
        let words = "My friend Lee is coming.";
        let mut proposal = patch(
            Method::Visit,
            words,
            Person {
                id: "visitor".into(),
                label: "Lee".into(),
                relationship: "friend".into(),
                source_quote: words.into(),
            },
            &[(Field::VisitorCapacity, "service_provider")],
        );
        proposal.intent = Intent::Clarify;
        proposal.question = None;
        proposal.frame = None;
        proposal.subject = None;
        assert!(case
            .apply(&proposal, 2, words, false)
            .err()
            .unwrap()
            .contains("explicit hired-service"));
        proposal.people.clear();
        proposal.updates[0].field = Field::Visitor;
        proposal.updates[0].value = "invented".into();
        assert!(case
            .apply(&proposal, 2, words, false)
            .err()
            .unwrap()
            .contains("known participant"));
    }

    #[test]
    fn shortened_service_quote_cannot_erase_a_source_negation() {
        assert!(service_capacity_evidence(
            "electrician",
            "The electrician I hired is coming."
        ));
        assert!(!service_capacity_evidence(
            "electrician",
            "Lee is not an electrician; he is my friend."
        ));
        let mut case = visit("friend", false);
        let words = "Lee is not an electrician; he is my friend.";
        let mut proposal = case.changes[0].proposal.clone();
        proposal.intent = Intent::Clarify;
        proposal.question = None;
        proposal.frame = None;
        proposal.subject = None;
        proposal.people.clear();
        proposal.updates = vec![Update {
            field: Field::VisitorCapacity,
            value: "service_provider".into(),
            quote: "electrician".into(),
            mode: UpdateMode::Supply,
        }];
        assert!(case.apply(&proposal, 2, words, false).is_err());
    }

    #[test]
    fn receiver_is_not_a_supplied_title_owner_and_another_recipient_gets_no_transferred_moon() {
        let mut case = parcel();
        case.people.insert(
            "father".into(),
            Person {
                id: "father".into(),
                label: "My father".into(),
                relationship: "father".into(),
                source_quote: "my father".into(),
            },
        );
        case.facts.insert(
            Field::Receiver,
            Slot::Resolved {
                observation: Observation {
                    value: "father".into(),
                    evidence: Evidence::User {
                        turn: 2,
                        quote: "It is coming to my father".into(),
                    },
                },
            },
        );
        let (ready, worksheet, roles) = accepted(&case, &house_facts());
        let native = clipboard(&ready, &worksheet, &roles, &[]).unwrap().unwrap();
        assert_eq!(native.receiver.house, Some(4));
        assert_eq!(native.pocket.as_ref().unwrap().house, Some(5));
        assert!(native.receiver_moon.is_none());
        assert_eq!(ready.input()["subject"]["owner_id"], "");
    }

    #[test]
    fn seller_only_contact_is_rejected_then_actual_arrival_repairs_same_stage() {
        let case = parcel();
        let mut facts = house_facts();
        facts.push(fact("arrival", "event", &["Jupiter", "Mars"], true));
        facts.push(fact("seller_only", "event", &["Venus", "Mars"], true));
        let (ready, worksheet, roles) = accepted(&case, &facts);
        let native = clipboard(&ready, &worksheet, &roles, &facts).unwrap();
        let input = json!({"reading_request":ready.input(),"native_arrival":native});
        let mut contact = stage_worksheet(Stage::Contacts);
        contact["candidate_ids"] = json!(["seller_only"]);
        contact["candidate_signs"] = json!([{"id":"seller_only","within_current_signs":true}]);
        contact["checks"]["applying_or_separating"]["evidence"] = json!(["seller_only"]);
        assert!(crate::horary_step::check(
            Stage::Contacts,
            Matter::Other,
            &contact,
            &input,
            &facts
        )
        .err()
        .unwrap()
        .contains("physical-arrival"));
        contact["candidate_ids"] = json!(["arrival"]);
        contact["candidate_signs"] = json!([{"id":"arrival","within_current_signs":true}]);
        contact["checks"]["applying_or_separating"]["evidence"] = json!(["arrival"]);
        assert!(matches!(
            crate::horary_step::check(Stage::Contacts, Matter::Other, &contact, &input, &facts)
                .unwrap(),
            crate::horary_step::Checked::Data(_)
        ));
        let mut judgment = stage_worksheet(Stage::Judgment);
        let boundary = facts
            .iter()
            .find(|f| f.kind == "boundary")
            .unwrap()
            .id
            .clone();
        judgment["checks"]["scope_of_answer"]["evidence"] = json!([boundary]);
        judgment["evidence"] = json!(["seller_only"]);
        judgment["checks"]["supporting_testimony"]["evidence"] = json!(["seller_only"]);
        let mut input = input;
        input["contacts"] = contact;
        assert!(crate::horary_step::check(
            Stage::Judgment,
            Matter::Other,
            &judgment,
            &input,
            &facts
        )
        .err()
        .unwrap()
        .contains("positive physical-arrival"));
        judgment["evidence"] = json!(["arrival"]);
        judgment["checks"]["supporting_testimony"]["evidence"] = json!(["arrival"]);
        assert!(matches!(
            crate::horary_step::check(Stage::Judgment, Matter::Other, &judgment, &input, &facts)
                .unwrap(),
            crate::horary_step::Checked::Data(_)
        ));
        input["contacts"]["candidate_ids"] = json!([]);
        assert!(check(Stage::Judgment, &input, &judgment, &facts)
            .err()
            .unwrap()
            .contains("contacts step"));
    }

    #[test]
    fn all_receiver_routes_shared_rulers_sign_changes_and_empty_coverage_remain_truthful() {
        let case = parcel();
        let mut facts = house_facts();
        facts.extend([
            fact("lord1", "event", &["Jupiter", "Mars"], true),
            fact("pocket", "event", &["Jupiter", "Venus"], true),
            fact("moon", "event", &["Jupiter", "Moon"], true),
        ]);
        let (ready, worksheet, roles) = accepted(&case, &facts);
        let native = clipboard(&ready, &worksheet, &roles, &facts)
            .unwrap()
            .unwrap();
        assert_eq!(native.candidates.len(), 3);
        assert!(native
            .candidates
            .iter()
            .any(|c| c.channel == "receiver_possession"));
        assert!(native
            .candidates
            .iter()
            .any(|c| c.channel == "receiver_moon"));
        let mut aliased = native.clone();
        aliased.receiver.planet = aliased.primary.planet.clone();
        aliased.pocket.as_mut().unwrap().planet = aliased.primary.planet.clone();
        aliased.receiver_moon = None;
        assert!(candidates(
            &aliased.primary,
            &aliased.receiver,
            aliased.pocket.as_ref(),
            None,
            &facts
        )
        .is_empty());
        facts
            .iter_mut()
            .filter_map(|f| f.event.as_mut())
            .for_each(|e| e.within_current_signs = Some(false));
        let native = clipboard(&ready, &worksheet, &roles, &facts).unwrap();
        let input = json!({"reading_request":ready.input(),"native_arrival":native,"contacts":{"candidate_ids":["lord1"]}});
        let mut value = stage_worksheet(Stage::Judgment);
        value["evidence"] = json!(["lord1"]);
        value["checks"]["supporting_testimony"]["evidence"] = json!(["lord1"]);
        assert!(check(Stage::Judgment, &input, &value, &facts).is_err());
        let empty = clipboard(&ready, &worksheet, &roles, &[]).unwrap().unwrap();
        assert!(empty.candidates.is_empty());
        assert!(empty
            .unknown_channels
            .contains(&"short_window_absence_is_not_no_arrival".into()));
        value["verdict"] = json!("unresolved");
        assert!(check(
            Stage::Judgment,
            &json!({"reading_request":ready.input(),"native_arrival":empty}),
            &value,
            &[]
        )
        .is_ok());
    }

    #[test]
    fn unexpected_visit_does_not_gain_uncomputed_angle_or_symbolic_time() {
        let mut case = visit("friend", false);
        if let Some(Slot::Resolved { observation }) = case.facts.get_mut(&Field::Expected) {
            observation.value = "no".into();
        }
        let facts = house_facts();
        let (ready, worksheet, roles) = accepted(&case, &facts);
        let native = clipboard(&ready, &worksheet, &roles, &facts)
            .unwrap()
            .unwrap();
        assert_eq!(native.expected, Some(false));
        assert!(native.candidates.is_empty());
        let input = json!({"reading_request":ready.input(),"native_arrival":native,"contacts":{"candidate_ids":[]}});
        assert!(check(
            Stage::Judgment,
            &input,
            &stage_worksheet(Stage::Judgment),
            &facts
        )
        .is_err());
        let mut timing = stage_worksheet(Stage::Timing);
        timing["timing_status"] = json!("tentative");
        timing["unit"] = json!("hours");
        timing["number"] = json!(3);
        assert!(
            crate::horary_step::check(Stage::Timing, Matter::Other, &timing, &input, &facts)
                .err()
                .unwrap()
                .contains("travel-to-perfection")
        );
    }

    #[test]
    fn arrival_clipboard_cannot_be_rebound_to_another_handoff_or_event() {
        let case = parcel();
        let mut facts = house_facts();
        facts.push(fact("arrival", "event", &["Jupiter", "Mars"], true));
        let (ready, worksheet, roles) = accepted(&case, &facts);
        let native = clipboard(&ready, &worksheet, &roles, &facts)
            .unwrap()
            .unwrap();
        let mut input = json!({"reading_request":ready.input(),"native_arrival":native});
        input["reading_request"]["binding"]["input_sha256"] = json!("another-request");
        assert!(check(
            Stage::Contacts,
            &input,
            &stage_worksheet(Stage::Contacts),
            &facts
        )
        .err()
        .unwrap()
        .contains("immutable native"));
        input["reading_request"] = ready.input();
        input["native_arrival"]["candidates"][0]["fact_id"] = json!("invented");
        assert!(check(
            Stage::Contacts,
            &input,
            &stage_worksheet(Stage::Contacts),
            &facts
        )
        .err()
        .unwrap()
        .contains("endpoints"));
    }

    #[test]
    fn migration_retains_unbound_legacy_words_as_proposals_without_invented_state() {
        let mut case = parcel();
        case.catalogue_version = "frawley-reading-contracts-2026-10-10.2".into();
        let question = serde_json::to_value(&case.question).unwrap();
        let original =
            serde_json::to_value(&case.facts[&Field::Sender]).unwrap()["observation"].clone();
        case.facts.remove(&Field::ParcelTask);
        let receipt = crate::reading_contracts::upgrade_catalogue(&mut case).unwrap();
        assert_eq!(receipt.from, "frawley-reading-contracts-2026-10-10.2");
        assert_eq!(serde_json::to_value(&case.question).unwrap(), question);
        assert_eq!(
            serde_json::to_value(&case.facts[&Field::Sender]).unwrap()["observation"],
            original
        );
        assert!(matches!(case.facts[&Field::Sender], Slot::Proposed { .. }));
        assert_eq!(case.text(Field::ParcelTask), None);
        assert!(ReadyReading::prepare(&case, anchor()).is_err());
    }

    #[test]
    fn inventory_has_every_substantive_family_without_qualifying_model_answers() {
        let rows = inventory();
        assert_eq!(rows.len(), 42);
        assert_eq!(
            rows.iter()
                .filter(|row| row["native_handoff"] == true)
                .count(),
            18
        );
        assert_eq!(
            rows.iter()
                .filter(|row| row["native_handoff"] == false)
                .count(),
            24
        );
        for row in rows {
            assert_eq!(row["source_correct_interpretation"], "unqualified");
            assert!(!row["remaining_obligations"].as_str().unwrap().is_empty());
        }
        assert_eq!(
            with_clipboard(json!({"original":"unchanged"}), &None),
            json!({"original":"unchanged"})
        );
    }

    #[test]
    fn principal_relay_receiver_uses_first_and_keeps_actor_record() {
        let mut case = parcel();
        case.people.insert(
            "principal".into(),
            Person {
                id: "principal".into(),
                label: "The relayed principal".into(),
                relationship: "unknown".into(),
                source_quote: "Their own question, relayed with permission".into(),
            },
        );
        for (field, value) in [
            (Field::PrincipalMode, "relay"),
            (Field::PrincipalId, "principal"),
        ] {
            case.facts.insert(
                field,
                Slot::Resolved {
                    observation: Observation {
                        value: value.into(),
                        evidence: Evidence::Migration {
                            detail: "Authored relay native-scope test; not semantic recognition"
                                .into(),
                        },
                    },
                },
            );
        }
        let (ready, worksheet, roles) = accepted(&case, &house_facts());
        let native = clipboard(&ready, &worksheet, &roles, &[]).unwrap().unwrap();
        assert_eq!(native.receiver_id, "principal");
        assert_eq!(native.receiver.house, Some(1));
        assert!(native.receiver_moon.is_some());
        assert_eq!(
            ready.input()["people"]["principal"]["relationship"],
            "unknown"
        );
    }

    #[test]
    fn required_arrival_clipboard_and_existing_negative_guard_cannot_be_skipped() {
        let case = parcel();
        let facts = house_facts();
        let (ready, worksheet, roles) = accepted(&case, &facts);
        let mut value = stage_worksheet(Stage::Judgment);
        value["verdict"] = json!("likely_no");
        let boundary = facts
            .iter()
            .find(|f| f.kind == "boundary")
            .unwrap()
            .id
            .clone();
        value["checks"]["scope_of_answer"]["evidence"] = json!([boundary]);
        let input = json!({"reading_request":ready.input()});
        assert!(check(
            Stage::Contacts,
            &input,
            &stage_worksheet(Stage::Contacts),
            &facts
        )
        .err()
        .unwrap()
        .contains("requires its actual"));
        let native = clipboard(&ready, &worksheet, &roles, &facts).unwrap();
        let input = with_clipboard(input, &native);
        assert!(
            crate::horary_step::check(Stage::Judgment, Matter::Other, &value, &input, &facts)
                .err()
                .unwrap()
                .contains("absence of a short-window contact")
        );
    }
}
