//! The port owns signature rendering, parsing, demonstrations and optimization.
use anyhow::{Result, bail};
use dsrust::signature::{Signature, SignatureSpec};

macro_rules! reading_types {
    ($(($reading:ident, $extract:ident, $method:literal)),+ $(,)?) => {
        pub const METHODS: &[&str] = &[$($method),+];
        $(
            #[derive(dsrust::Signature)]
            #[doc = $method]
            pub struct $reading {
                /// The relevant primary text with printed and OCR page references.
                #[input] pub source_text: String,
                /// The person's actual question, including corrections.
                #[input] pub question: String,
                /// Accepted situational inputs; unknown entries remain unknown.
                #[input] pub consultation: String,
                /// Native chart, dignity, directed receptions and ordered event evidence.
                #[input] pub chart_evidence: String,
                /// answer, need_information, or method_limit.
                #[output] pub status: String,
                /// A direct, contextual answer addressed to the person.
                #[output] pub answer: String,
                /// Explicit reviewable reasoning: roles, ability, motive, occasion, Moon and timing when relevant.
                #[output] pub analysis: String,
                /// Native evidence identifiers and supporting printed source pages for material claims.
                #[output] pub evidence: String,
                /// Only genuinely unresolved inputs or calculation/method limitations.
                #[output] pub unresolved: String,
            }
            #[derive(dsrust::Signature)]
            #[doc = $method]
            pub struct $extract {
                /// Original question and new supplying words; do not silently substitute a different question.
                #[input] pub question: String,
                /// Earlier actual conversational turns.
                #[input] pub conversation: String,
                /// Existing accepted inputs and device clock/location.
                #[input] pub context: String,
                /// Conditional input contract and the relevant primary method.
                #[input] pub requirements: String,
                /// Canonical facts as a JSON array of {field,value,quote}; quote must be an exact user substring. Inferred values use an empty quote and an explicit inference explanation.
                #[output] pub facts: String,
                /// JSON array of genuinely missing canonical field paths, never a canned form's entire field list.
                #[output] pub missing: String,
                /// Supported contextual inferences and their device/conversational witnesses.
                #[output] pub inferences: String,
                /// One natural clarification, or empty when sufficient information is available.
                #[output] pub clarification: String,
            }
        )+
        pub fn reading(method: &str) -> Result<Signature> {
            let mut signature = match method { $($method => $reading::signature(),)+ _ => bail!("Unknown reading type {method}") };
            signature.instructions = format!("Judge a {method} question by the supplied Frawley method and calculated evidence. Work in this order: (1) preserve the literal requested outcome, facet, principal and circumstances; (2) select this chapter's ordinary/turned/natural significators, explain collisions and derive the rulers from supplied cusps; (3) distinguish essential quality from descriptive fit; (4) assess house capacity, motion and solar condition including contextual exceptions; (5) interpret each relevant directed reception, who regards whom, without assuming reciprocity; (6) apply this branch's action/state/location recipe, considering the first relevant contacts and prevention in event order, NEVER making an aspect a universal prerequisite for a quality/state question; (7) combine support and contradiction under the actual default, using extra techniques only for a defined source purpose; (8) time an event only after establishing it, with moving perfection distance, the applying planet and plausible contextual units; (9) answer the ORIGINAL question directly before technical detail, with the main support, material contrary evidence and explicit limits. Give reviewable reasons and native evidence IDs, not an invented hidden-reasoning transcript. Essential calculations that are absent stay unobserved. Approximate astronomy and a finite event window do not authorize fine precision or a one-year absence inference. Software coverage flags are not book limits. Distinguish missing information from a genuine method/calculation limit and from an actual judgment.");
            Ok(signature)
        }
        pub fn extraction(method: &str) -> Result<Signature> {
            let mut signature = match method { $($method => $extract::signature(),)+ _ => bail!("Unknown extraction type {method}") };
            signature.instructions = format!("Extract the situational inputs for {method} from the actual words and context. Preserve ownership, operative capacity, beneficiary and negation. Do not invent missing facts. The device anchors the reader; event geography and event time are different inputs. Quote exact source spans for explicit observations; label contextual inferences separately.");
            Ok(signature)
        }
    }
}

reading_types! {
    (RelationshipReading, RelationshipInputs, "relationship"),
    (LostObjectReading, LostObjectInputs, "lost_object"),
    (LostAnimalReading, LostAnimalInputs, "lost_animal"),
    (MissingPersonReading, MissingPersonInputs, "missing_person"),
    (MovableDealReading, MovableDealInputs, "movable_deal"),
    (MoneyReading, MoneyInputs, "money"),
    (InvestmentReading, InvestmentInputs, "investment"),
    (NewJobReading, NewJobInputs, "new_job"),
    (ExistingJobReading, ExistingJobInputs, "existing_job"),
    (ReturnToJobReading, ReturnToJobInputs, "return_to_job"),
    (JobOfferReading, JobOfferInputs, "job_offer"),
    (WorkPersonReading, WorkPersonInputs, "work_person"),
    (PropertyReading, PropertyInputs, "property"),
    (RentalReading, RentalInputs, "rental"),
    (BusinessPropertyReading, BusinessPropertyInputs, "business_property"),
    (ChoiceReading, ChoiceInputs, "choice"),
    (HiringReading, HiringInputs, "hiring"),
    (ContactReading, ContactInputs, "contact"),
    (ParcelReading, ParcelInputs, "parcel"),
    (VisitReading, VisitInputs, "visit"),
    (ContestReading, ContestInputs, "contest"),
    (BetReading, BetInputs, "bet"),
    (CourtCaseReading, CourtCaseInputs, "court_case"),
    (VehicleReading, VehicleInputs, "vehicle"),
    (PersonDescriptionReading, PersonDescriptionInputs, "person_description"),
    (InformationReading, InformationInputs, "information"),
    (TrustReading, TrustInputs, "trust"),
    (PregnancyReading, PregnancyInputs, "pregnancy"),
    (FertilityReading, FertilityInputs, "fertility"),
    (AdoptionReading, AdoptionInputs, "adoption"),
    (MedicalReading, MedicalInputs, "medical"),
    (PoliticsReading, PoliticsInputs, "politics"),
    (KnowledgeReading, KnowledgeInputs, "knowledge"),
    (ExamReading, ExamInputs, "exam"),
    (UndertakingReading, UndertakingInputs, "undertaking"),
    (DreamReading, DreamInputs, "dream"),
    (EducationReading, EducationInputs, "education"),
    (WishReading, WishInputs, "wish"),
    (TaxReading, TaxInputs, "tax"),
    (AllegationReading, AllegationInputs, "allegation"),
    (CustodyReading, CustodyInputs, "custody"),
    (WeatherReading, WeatherInputs, "weather"),
    (ElectionReading, ElectionInputs, "election"),
    (UnclassifiedReading, UnclassifiedInputs, "unclassified"),
}

#[derive(dsrust::Signature)]
/// Identify the actual horary matter and the requested aspect of it. A WILL question with a deadline is still an event question. Preserve an exact-count request instead of covertly rewriting it. Distinguish the question's principal from speaker, messenger, owner, operator and beneficiary. Your reply contains ONLY the five output fields method, facet, branch, principal and clarification, using the requested field markers. Start with the method output; never copy the question, conversation or catalogue input into your reply. No preamble or catalogue reproduction.
pub struct ClassifyQuestion {
    #[input]
    pub question: String,
    #[input]
    pub conversation: String,
    #[input]
    pub catalogue: String,
    #[output]
    pub method: String,
    #[output]
    pub facet: String,
    /// The matching book branch ID from this method's catalogue, or unknown when genuinely unresolved.
    #[output]
    pub branch: String,
    #[output]
    pub principal: String,
    #[output]
    pub clarification: String,
}

#[derive(dsrust::Signature)]
/// Resolve the reader's chart anchor, separately from the event/subject location. Use a valid device location by default. Resolve a relative or ambiguous place from relevant geographic context, and inquire only when ambiguity materially affects the requested place. Return a geocoder query rather than invented coordinates.
pub struct ResolvePlace {
    #[input]
    pub question: String,
    #[input]
    pub conversation: String,
    #[input]
    pub device: String,
    #[input]
    pub place_candidates: String,
    #[output]
    pub reader_place: String,
    #[output]
    pub event_place: String,
    #[output]
    pub geocode_query: String,
    #[output]
    pub clarification: String,
}

#[derive(dsrust::Signature)]
/// Keep the moment at which the reader understood this issue. Use the device clock for a new present question. Event time is context, not an automatic chart time. Resolve relative civil dates against the supplied clock/timezone; ask about missing historical time or ambiguous daylight-saving occurrence. Same-issue follow-ups retain the chart moment.
pub struct ResolveMoment {
    #[input]
    pub question: String,
    #[input]
    pub conversation: String,
    #[input]
    pub clock: String,
    #[input]
    pub retained_moment: String,
    #[output]
    pub chart_moment: String,
    #[output]
    pub event_moment: String,
    #[output]
    pub occurrence: String,
    #[output]
    pub clarification: String,
}

#[derive(dsrust::Signature)]
/// Apply new supplied or corrected information to the same matter. Never erase an accepted source observation simply because the latest words omit it. Resolve only the question actually asked; a changed matter is a new reading. Do not silently accept a proposed reframing as consent.
pub struct UpdateConsultation {
    #[input]
    pub question: String,
    #[input]
    pub new_words: String,
    #[input]
    pub accepted_inputs: String,
    #[input]
    pub pending_question: String,
    #[output]
    pub intent: String,
    #[output]
    pub updates: String,
    #[output]
    pub retractions: String,
    #[output]
    pub clarification: String,
}

#[derive(dsrust::Signature)]
/// Grade only this neural function against the independently authored rubric and primary text. Reference_answer is a separate source-reviewed worked answer, never the rubric. Accept substantively equivalent readings and legitimate source-supported alternatives rather than verbatim matching. Cite concrete observed output and source passages. Extraction is judged for semantic completeness, source fidelity and genuine missing information; readings for source-correct roles, ability, reception direction, relevant occasions, Moon, defensible timing and a coherent useful answer with material limits. Do not require unavailable calculations or fail a correct reading merely for style or Frawley's contextual love metaphor. Formatting alone cannot earn correctness. Unobserved dimensions stay unobserved.
pub struct JudgeFunction {
    /// Full salient primary context; keep the stable book prefix before changing case data.
    #[input]
    pub source_text: String,
    #[input]
    pub function: String,
    #[input]
    pub inputs: String,
    #[input]
    pub prediction: String,
    /// Actual source-reviewed five-field worked answer, or an explicit unavailable-reference marker; never criteria or private metadata.
    #[input]
    pub reference_answer: String,
    #[input]
    pub rubric: String,
    /// A finite score between zero and one for this function only.
    #[output]
    pub score: f64,
    /// True only when every applicable essential semantic obligation is met.
    #[output]
    pub passed: bool,
    /// pass, fail, or unobserved for question classification.
    #[output]
    pub classification: String,
    /// pass, fail, or unobserved for extraction and source fidelity of inputs.
    #[output]
    pub extraction: String,
    /// pass, fail, or unobserved for the book's actual reading procedure.
    #[output]
    pub reading: String,
    /// pass, fail, or unobserved for a coherent answer to the actual question.
    #[output]
    pub answer_quality: String,
    /// Specific findings, observed-output quotations and source references.
    #[output]
    pub feedback: String,
}

pub fn auxiliary(task: &str) -> Result<Signature> {
    match task {
        "classification" => Ok(ClassifyQuestion::signature()),
        "place" => Ok(ResolvePlace::signature()),
        "moment" => Ok(ResolveMoment::signature()),
        "update" => Ok(UpdateConsultation::signature()),
        "judge" => Ok(JudgeFunction::signature()),
        "reception" => Ok(crate::reception::signature()),
        "reception_judge" => Ok(crate::reception::judge_signature()),
        _ => bail!("Unknown auxiliary function {task}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;
    #[test]
    fn every_catalogue_method_has_two_real_port_signatures() {
        assert_eq!(METHODS.len(), 44);
        assert_eq!(METHODS.iter().copied().collect::<BTreeSet<_>>().len(), 44);
        for method in METHODS {
            let reading = reading(method).unwrap();
            let extraction = extraction(method).unwrap();
            assert_eq!(reading.outputs.len(), 5);
            assert_eq!(extraction.outputs.len(), 4);
            assert!(reading.instructions.contains(method));
            assert!(extraction.instructions.contains(method));
        }
    }
    #[test]
    fn no_grade_is_part_of_an_oracle_input() {
        for method in METHODS {
            for signature in [reading(method).unwrap(), extraction(method).unwrap()] {
                assert!(
                    signature.inputs.iter().all(|f| !matches!(
                        f.name.as_str(),
                        "expected" | "rubric" | "score" | "gold"
                    ))
                );
            }
        }
    }
}
