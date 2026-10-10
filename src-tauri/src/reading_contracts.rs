//! Executable consultation contracts. Pure reduction and planning; no I/O.
//! The catalogue drives recognition, elicitation, handoff validation and docs.
#![forbid(unsafe_code)]

use crate::{
    horary_lessons::Matter,
    horary_role_options::{Person, Subject},
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;

pub const VERSION: &str = "frawley-reading-contracts-2026-10-10.2";
const PREVIOUS_VERSION: &str = "frawley-reading-contracts-2026-10-07.1";

macro_rules! names {
    ($name:ident { $($variant:ident => $text:literal),+ $(,)? }) => {
        #[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord)]
        #[serde(rename_all = "snake_case")]
        pub enum $name { $($variant),+ }
        impl $name {
            pub const ALL: &'static [Self] = &[$(Self::$variant),+];
            pub const fn name(self) -> &'static str { match self { $(Self::$variant => $text),+ } }
        }
    }
}

names!(Method {
    Relationship => "relationship", LostObject => "lost_object", LostAnimal => "lost_animal",
    MissingPerson => "missing_person", MovableDeal => "movable_deal", Money => "money",
    Investment => "investment", NewJob => "new_job", ExistingJob => "existing_job",
    ReturnToJob => "return_to_job", JobOffer => "job_offer", WorkPerson => "work_person",
    Property => "property", Rental => "rental", BusinessProperty => "business_property",
    Choice => "choice", Hiring => "hiring", Contact => "contact", Parcel => "parcel",
    Visit => "visit", Contest => "contest", Bet => "bet", CourtCase => "court_case", Vehicle => "vehicle",
    PersonDescription => "person_description", Information => "information", Trust => "trust",
    Pregnancy => "pregnancy", Fertility => "fertility", Adoption => "adoption",
    Medical => "medical", Politics => "politics", Knowledge => "knowledge", Exam => "exam",
    Undertaking => "undertaking", Dream => "dream", Education => "education", Wish => "wish",
    Tax => "tax", Allegation => "allegation", Custody => "custody", Weather => "weather",
    Election => "election", Unclassified => "unclassified"
});
names!(Facet {
    Event => "event", Situation => "situation", Quantity => "quantity", Location => "location",
    Choice => "choice", Timing => "timing", Description => "description", Truth => "truth",
    Profit => "profit", Safety => "safety", Unknown => "unknown"
});
names!(Field {
    PrincipalMode => "principal_mode", PrincipalId => "principal_id", Context => "context",
    ReaderPlace => "reader_place", QuestionTime => "question_time", TimeOccurrence => "time_occurrence", EventPlace => "event_place",
    EventTime => "event_time", Horizon => "horizon", Baseline => "baseline",
    Description => "description", AnimalKind => "animal_kind", TheftRaised => "theft_raised",
    SearchContext => "search_context", DealCapacity => "deal_capacity", DealActor => "deal_actor", DealBeneficiary => "deal_beneficiary", DealParty => "deal_party", Seller => "seller",
    MoneySource => "money_source", Discretionary => "discretionary", JobContext => "job_context",
    WorkCapacity => "work_capacity", Priorities => "priorities", CurrentOption => "current_option",
    Alternatives => "alternatives", HomeMeaning => "home_meaning", Candidates => "candidates",
    Sender => "sender", Visitor => "visitor", Expected => "expected", Affiliation => "affiliation",
    Competition => "competition", Claim => "claim", Parenthood => "parenthood",
    FertilityScope => "fertility_scope", AdoptionState => "adoption_state",
    BirthParent => "birth_parent", MedicalTask => "medical_task", Treatment => "treatment",
    PoliticalCapacity => "political_capacity", ElectionState => "election_state",
    Office => "office", KnowledgeTask => "knowledge_task", ExamTask => "exam_task",
    DreamTask => "dream_task", DreamAccount => "dream_account", SchoolLevel => "school_level",
    SchoolTask => "school_task", CustodyState => "custody_state", WeatherScope => "weather_scope",
    TargetPlace => "target_place", TargetPeriod => "target_period", Action => "action",
    ActionWindow => "action_window", Constraints => "constraints", Unit => "unit"
});

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Frame {
    pub method: Method,
    pub facet: Facet,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(tag = "source", rename_all = "snake_case", deny_unknown_fields)]
pub enum Evidence {
    User {
        turn: usize,
        quote: String,
    },
    Convention {
        rule: String,
    },
    Migration {
        detail: String,
    },
    RetainedQuestion {
        quote: String,
    },
    NativePlace {
        query: String,
        candidate_id: String,
        context: String,
        original: Box<Evidence>,
    },
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Observation<T> {
    pub value: T,
    pub evidence: Evidence,
}

#[derive(Clone, Default, Deserialize, Serialize)]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
pub enum Slot<T> {
    #[default]
    Missing,
    Proposed {
        observation: Observation<T>,
    },
    Resolved {
        observation: Observation<T>,
    },
    Conflicting {
        observations: Vec<Observation<T>>,
    },
    Unavailable {
        reason: String,
        evidence: Evidence,
    },
}
impl<T> Slot<T> {
    pub fn resolved(&self) -> Option<&T> {
        match self {
            Self::Resolved { observation } => Some(&observation.value),
            _ => None,
        }
    }
    fn reason(&self) -> &'static str {
        match self {
            Self::Missing => "missing",
            Self::Proposed { .. } => "proposed",
            Self::Resolved { .. } => "resolved",
            Self::Conflicting { .. } => "conflicting",
            Self::Unavailable { .. } => "unavailable",
        }
    }
}
impl<T: Clone + PartialEq> Slot<T> {
    fn set(&mut self, observation: Observation<T>, correct: bool, proposed: bool) {
        if proposed {
            *self = Self::Proposed { observation };
            return;
        }
        if correct {
            *self = Self::Resolved { observation };
            return;
        }
        match self {
            Self::Resolved { observation: old } if old.value != observation.value => {
                *self = Self::Conflicting {
                    observations: vec![old.clone(), observation],
                };
            }
            Self::Conflicting { observations } => {
                if !observations
                    .iter()
                    .any(|old| old.value == observation.value)
                {
                    observations.push(observation);
                }
            }
            _ => *self = Self::Resolved { observation },
        }
    }
}

names!(Intent {
    Read => "read", Clarify => "clarify", Correct => "correct", NewQuestion => "new_question",
    Explain => "explain", Resume => "resume", Restore => "restore", UseDevice => "use_device",
    Pause => "pause"
});
names!(UpdateMode { Supply => "supply", Correct => "correct", Propose => "propose", Unavailable => "unavailable" });

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Update {
    pub field: Field,
    pub value: String,
    pub quote: String,
    pub mode: UpdateMode,
}

/// Omitted facts are untouched. This is a patch, never a replacement brief.
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Turn {
    pub intent: Intent,
    pub question: Option<String>,
    pub frame: Option<Frame>,
    pub people: Vec<Person>,
    pub subject: Option<Subject>,
    pub updates: Vec<Update>,
    pub heard: String,
    pub unavailable_quote: String,
    pub focus: String,
    pub restore_revision: Option<u64>,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Consultation {
    pub catalogue_version: String,
    pub revision: u64,
    pub question: Slot<String>,
    pub frame: Slot<Frame>,
    pub people: BTreeMap<String, Person>,
    pub subject: Slot<Subject>,
    pub facts: BTreeMap<Field, Slot<String>>,
    pub changes: Vec<Change>,
    pub requested: Option<RequirementKey>,
    pub additional: Vec<InformationNeed>,
    /// Explicit location-only control; survives recognition snapshots.
    /// Historical question time remains independent.
    #[serde(default)]
    pub device_reader_place: bool,
    #[serde(default)]
    pub unavailable: Vec<UnavailableNeed>,
}
impl Default for Consultation {
    fn default() -> Self {
        Self { catalogue_version: VERSION.into(), revision: 0, question: Slot::Missing,
            frame: Slot::Missing, people: BTreeMap::new(), subject: Slot::Missing,
            facts: BTreeMap::from([(Field::PrincipalMode, Slot::Resolved { observation: Observation {
                value: "self".into(), evidence: Evidence::Convention { rule: "The speaker is the principal unless relay or ambiguity is stated; Frawley pp. 137–138.".into() }
            }})]), changes: Vec::new(), requested: None, additional: Vec::new(), device_reader_place:false, unavailable:Vec::new() }
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct CatalogueMigration {
    pub from: String,
    pub to: String,
    pub before_revision: u64,
    pub after_revision: u64,
    pub recheck: Vec<String>,
}

/// Reopen ambiguous legacy observations, retaining their exact source and all
/// earlier revisions. Never manufacture a title owner or deal action from an
/// old role binding. Unknown catalogue versions remain blocked by `plan`.
pub fn upgrade_catalogue(case: &mut Consultation) -> Option<CatalogueMigration> {
    if case.catalogue_version != PREVIOUS_VERSION {
        return None;
    }
    let before_revision = case.revision;
    let mut recheck = Vec::new();
    if case.is_deal() {
        recheck.push(Field::DealActor.name().into());
        if case
            .frame
            .resolved()
            .is_some_and(|frame| frame.facet == Facet::Profit)
        {
            recheck.push(Field::DealBeneficiary.name().into());
        }
        if let Some(slot) = case.facts.get_mut(&Field::DealCapacity) {
            if slot
                .resolved()
                .is_some_and(|value| !allowed_values(Field::DealCapacity).contains(&value.as_str()))
            {
                if let Slot::Resolved { observation } = slot.clone() {
                    *slot = Slot::Proposed { observation };
                    recheck.push(Field::DealCapacity.name().into());
                }
            }
        }
        if matches!(
            case.method(),
            Some(Method::Rental | Method::BusinessProperty)
        ) || case.text(Field::DealCapacity) != Some("sell")
        {
            if let Slot::Resolved { observation } = case.subject.clone() {
                case.subject = Slot::Proposed { observation };
                recheck.push("subject_title_ownership".into());
            }
        }
    }
    case.catalogue_version = VERSION.into();
    case.revision = case.revision.saturating_add(1);
    Some(CatalogueMigration {
        from: PREVIOUS_VERSION.into(),
        to: VERSION.into(),
        before_revision,
        after_revision: case.revision,
        recheck,
    })
}
#[derive(Clone, Deserialize, Serialize)]
pub struct Change {
    pub turn: usize,
    pub before_revision: u64,
    pub proposal: Turn,
}

#[derive(Clone, Deserialize, Serialize)]
pub struct UnavailableNeed {
    pub key: RequirementKey,
    pub evidence: Evidence,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(tag = "kind", content = "id", rename_all = "snake_case")]
pub enum RequirementKey {
    Question,
    Frame,
    Subject,
    Owner,
    PersonRelationship(String),
    Field(Field),
    ChartPlace,
    ChartMoment,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct InformationNeed {
    pub key: RequirementKey,
    pub reason: String,
    #[serde(default)]
    pub question: Option<String>,
}

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(tag = "condition", rename_all = "snake_case")]
pub enum Guard {
    Always,
    Equals { field: Field, value: &'static str },
    AnswerFacet { facet: Facet },
    PrincipalOwnsSubject,
}
#[derive(Clone, Copy, Debug, Serialize)]
pub struct Requirement {
    pub field: Field,
    pub guard: Guard,
}
const fn required(field: Field) -> Requirement {
    Requirement {
        field,
        guard: Guard::Always,
    }
}
const fn when(field: Field, control: Field, value: &'static str) -> Requirement {
    Requirement {
        field,
        guard: Guard::Equals {
            field: control,
            value,
        },
    }
}
const fn for_facet(field: Field, facet: Facet) -> Requirement {
    Requirement {
        field,
        guard: Guard::AnswerFacet { facet },
    }
}

#[derive(Clone, Copy, Debug, Serialize)]
pub enum OwnerRule {
    None,
    Required,
    Principal,
    /// Title matters to an asset assessment, not to the book's pure party
    /// completion test. A prospective buyer's asset is their potential one.
    DealContext,
}
#[derive(Clone, Copy, Debug, Serialize)]
pub enum Coverage {
    Implemented,
    ExpertReview,
}
#[derive(Clone, Copy, Debug, Serialize)]
pub struct Contract {
    pub method: Method,
    pub title: &'static str,
    pub printed_pages: &'static str,
    pub owner: OwnerRule,
    pub requirements: &'static [Requirement],
    pub facets: &'static [Facet],
    pub coverage: Coverage,
    pub roles: &'static str,
    pub judgment: &'static str,
    pub subject_kinds: &'static [&'static str],
}

const fn subject_kinds(method: Method) -> &'static [&'static str] {
    match method {
        Method::NewJob | Method::ExistingJob | Method::ReturnToJob | Method::JobOffer => &["job"],
        Method::Relationship
        | Method::MissingPerson
        | Method::WorkPerson
        | Method::Contact
        | Method::Trust => &["person"],
        Method::PersonDescription => &["person", "person_role"],
        Method::LostObject | Method::MovableDeal => &["movable"],
        Method::Money | Method::Investment | Method::Bet => &["money", "movable"],
        Method::Property | Method::Rental | Method::BusinessProperty => &["property"],
        Method::LostAnimal => &["small_animal", "large_animal", "animal"],
        _ => &[
            "person",
            "movable",
            "money",
            "property",
            "job",
            "small_animal",
            "large_animal",
            "other",
        ],
    }
}
const EVENT_STATE: &[Facet] = &[Facet::Event, Facet::Situation, Facet::Timing];
const DEAL: &[Facet] = &[Facet::Event, Facet::Situation, Facet::Choice, Facet::Profit];
const LOST: &[Facet] = &[
    Facet::Location,
    Facet::Event,
    Facet::Timing,
    Facet::Description,
];

macro_rules! card {
    ($m:ident, $title:literal, $pages:literal, $owner:ident, $requirements:expr, $facets:expr, $coverage:ident, $roles:literal, $judgment:literal) => {
        Contract {
            method: Method::$m,
            title: $title,
            printed_pages: $pages,
            owner: OwnerRule::$owner,
            requirements: $requirements,
            facets: $facets,
            coverage: Coverage::$coverage,
            roles: $roles,
            judgment: $judgment,
            subject_kinds: subject_kinds(Method::$m),
        }
    };
}

/// Coverage states concern the existing calculation/worksheet implementation,
/// not empirical accuracy. Specialist cards remain executable intake contracts
/// with an explicit expert boundary until their judgment programs are qualified.
pub const CATALOGUE: &[Contract] = &[
    card!(Relationship, "Relationship, marriage and feelings", "140, 191–200", None,
        &[required(Field::Baseline)], EVENT_STATE, Implemented,
        "Prospective partner is seventh even if no partner is named. Use operative capacity for a specific person's feelings (e.g. neighbour: third). House rulers have priority; never infer gender or allocate Sun/Venus by a name.",
        "Distinguish hoped-for formation, current feelings and an arranged wedding. An unnamed prospective partner is a role: do not claim an existing person currently loves the querent or has already met them. Directed reception describes chart testimony; event perfection is a separate question. No contact is not an automatic cancellation of an arranged wedding."),
    card!(LostObject, "Lost inanimate possession", "146–153, 244", Required,
        &[Requirement { field:Field::Description,guard:Guard::PrincipalOwnsSubject }], LOST, Implemented,
        "Own lost object: compare Lords 2 and 4 with its supplied description. Someone else's: owner's turned second only. Prefer one justified main ruler; Moon is secondary only with a reason.",
        "Consider recovery before proposing a search. Angularity and clear whereabouts can support recovery without an applying aspect. Keep search reports fallible; no thief unless raised. A failed search revisits this chart."),
    card!(LostAnimal, "Lost animal", "1–3, 146–153", None,
        &[required(Field::AnimalKind)], LOST, Implemented,
        "Kind determines sixth (dog/cat) versus twelfth (horse), not measured size. Do not turn every animal from its owner: the neighbour's cat example uses the ordinary sixth.",
        "Assess recovery and plausible whereabouts from supplied context, without exact GPS promises or automatic void-Moon rejection."),
    card!(MissingPerson, "Missing person", "146–153", Required,
        &[], LOST, Implemented, "Use the person's actual operative relationship, not the movable-object recipe or an automatic seventh for every missing person.",
        "Consider recovery/contact and location separately. Retain reported circumstances as reports."),
    card!(MovableDeal, "Sale or purchase of movable goods", "156–161, 167–172", DealContext,
        &[required(Field::DealCapacity), required(Field::DealActor), for_facet(Field::DealBeneficiary, Facet::Profit)], DEAL, Implemented,
        "Transaction action and requested outcome are separate. For pure completion, use the actual contracting parties without demanding title ownership or an asset-condition role. An unspecified counterparty is the deal actor's seventh; an identified relative keeps their own operative house. For quality/profit, goods sold are the relevant owner's second; a buyer's potential goods are the buyer's second. Title owner, contracting party and intermediary are distinct.",
        "Preserve deal, quality or profit as asked. Non-property opposition may complete with regret. This recipe provides no exact count of books/tulips sold. A venue/date is event context, never the chart anchor."),
    card!(Money, "Payment, debt, gift or grant", "156–161", Required,
        &[required(Field::MoneySource), when(Field::Discretionary, Field::MoneySource, "government"), when(Field::Sender, Field::MoneySource, "relative")],
        &[Facet::Event, Facet::Situation, Facet::Timing, Facet::Profit], Implemented,
        "Customers/spouse: eighth; job or government money: eleventh; known relative's money: their turned second. Preserve entitlement versus discretionary gift. For arrival, retain the recipient's own role and second-house pocket, plus Moon when the recipient is the effective principal and a house ruler does not already claim Moon (p. 158).",
        "Arrival considers money contacting the recipient, their pocket, or the appropriate Moon; an absent direct money/Lord 1 contact cannot discard the other routes. Amount/quality does not universally require an arrival aspect. Do not turn signs/degrees into an invented exact currency amount."),
    card!(Investment, "Shares and investments", "156–161", Required, &[],
        &[Facet::Situation, Facet::Profit, Facet::Choice], Implemented,
        "Owned shares are the principal's second-house possessions, not automatically eighth-house money.", "Judge condition/value and scope. Do not promise an exact return or treat this as a payment-arrival question."),
    card!(NewJob, "Getting a new external job", "222–224", Required, &[], EVENT_STATE, Implemented,
        "Principal's own house and radical tenth for the external job, even for a third-party principal. If the person is themselves tenth-house, use their turned tenth. Wages are a separate role.", "Establish getting the job, not merely contacting wages. An explicitly mentioned intermediary may matter; do not invent one."),
    card!(ExistingJob, "Keeping a job or existing career", "224–226", Required, &[], EVENT_STATE, Implemented,
        "Current job/career/boss uses the relevant person's turned tenth. Distinguish co-worker seventh and subordinate sixth.", "Default is an existing job: assess stability/disruption, not a compulsory acquisition aspect."),
    card!(ReturnToJob, "Returning to an old job", "225–226", Required, &[], EVENT_STATE, Implemented,
        "Principal and relevant job; keep the old-job re-entry context.", "Use re-entry testimony, not the generic get/keep assumption. State limitations of contact coverage."),
    card!(JobOffer, "Assessing an available job", "224–226", Required, &[],
        &[Facet::Situation, Facet::Choice, Facet::Profit], Implemented,
        "The external job is radical tenth, except a tenth-house worker uses their turned tenth (seventh). Select job.wages when assessing pay: second from the bound job, normally eleventh, or eighth in that exception. Job, wages and worker's pocket are distinct roles (printed pp. 223–227). An offer already available is not a new acquisition.", "Judge quality and stated priorities using their relevant roles. A pay priority needs wages testimony; a wages aspect does not prove job acquisition. Ask priorities only if the answer truly depends on what 'good' means."),
    card!(WorkPerson, "Boss, colleague or subordinate", "224–225", None,
        &[required(Field::WorkCapacity)], EVENT_STATE, Implemented,
        "Co-worker seventh, subordinate sixth, boss tenth when directly asked about. Job/boss collisions need a justified contextual allocation.", "Address the actual work relationship; do not reclassify a friendly colleague as eleventh by habit."),
    card!(Property, "Buying or selling property", "167–171", DealContext,
        &[required(Field::DealCapacity), required(Field::DealActor), for_facet(Field::DealBeneficiary, Facet::Profit)], DEAL, Implemented,
        "Ordinary actual contracting parties are first/seventh; a specific relative may take their own house. A routine estate agent is not the contracting seller and does not acquire a turned seventh by handling the sale. Pure completion does not require deed ownership. For asset assessment, property fourth and price tenth in the relevant frame; the buyer's prospective interest is not current title. Profit is distinct.", "Assess condition, price and completion separately. Opposition may complete a sale; don't apply this exception to an ongoing rental."),
    card!(Rental, "Rental agreement", "170", None,
        &[required(Field::DealCapacity), required(Field::DealActor), for_facet(Field::DealBeneficiary, Facet::Profit)], DEAL, Implemented,
        "Modern tenant/landlord deal: first/seventh, not an automatic sixth-house servant.", "An opposition can imply regret in this ongoing relationship. Distinguish an available tenancy's quality from finding one."),
    card!(BusinessProperty, "Property used for business", "170–171", None,
        &[required(Field::DealActor), for_facet(Field::DealBeneficiary, Facet::Profit)], DEAL, ExpertReview,
        "Property to work on/in uses the book's business/property-profit distinction, not an indiscriminate ordinary home-price allocation.", "Dedicated profit testimony and reviewed role allocation are required before this program can deliver a judgment."),
    card!(Choice, "Stay, change or compare alternatives", "201–203", None,
        &[required(Field::CurrentOption), required(Field::Alternatives)], &[Facet::Choice, Facet::Situation], ExpertReview,
        "General stay/change: first is things as they are, seventh the changed situation. Specific work versus college can use the relevant houses. Current home versus homeland is conditional context.", "Do not mechanically place Lord 1 in seventh. A finite shortlist and meaningful comparison must remain visible."),
    card!(Hiring, "Hiring staff", "189–190", None,
        &[required(Field::Candidates)], &[Facet::Choice, Facet::Situation], ExpertReview,
        "Employee sixth. Candidate descriptions support a reviewed matching decision, not arbitrary planets.", "Availability is assumed in this recipe; no obligatory acquisition aspect. Dedicated candidate-matching program awaits review."),
    card!(Contact, "Contact with someone", "165–166", Required, &[], EVENT_STATE, Implemented,
        "The person takes their operative relationship house.", "Contact with the person differs from the arrival of their parcel. An expected contact and long-lost return have different defaults."),
    card!(Parcel, "Arrival of a letter or parcel", "165–166", None,
        &[required(Field::Sender)], EVENT_STATE, ExpertReview,
        "Before receipt, the parcel is the sender's relevant possession; afterwards it is the receiver's.", "An aspect to the seller alone does not show arrival of the parcel. Dedicated sender/parcel program needs review."),
    card!(Visit, "Expected visit", "166", None,
        &[required(Field::Visitor), required(Field::Expected)], EVENT_STATE, ExpertReview,
        "Visitor's capacity matters: plumber is not a friend. Establish whether the visit is actually expected.", "Use the naturally limited contextual scale. Do not request birth data or full street address by habit."),
    card!(Contest, "Sporting match or championship", "203–208", None,
        &[required(Field::Competition), required(Field::Affiliation)], EVENT_STATE, ExpertReview,
        "Us/them depends on actual allegiance, not home/away or name order. Moon is not automatically the ordinary querent co-significator.", "Match versus season/championship differs. An indifferent spectator has no invented allegiance. Dedicated contest program awaits review."),
    card!(Bet, "Profit from a bet", "156–161, 203–204", Principal, &[],
        &[Facet::Profit, Facet::Situation], ExpertReview,
        "Money/profit question, not the team's us/them match recipe.", "Assess the principal's money and relevant gains without promising an exact payout."),
    card!(CourtCase, "Civil trial or legal dispute", "208–209", Required,
        &[required(Field::Claim)], EVENT_STATE, ExpertReview,
        "Parties first/seventh, legal process tenth, verdict fourth. A lawyer can relay the client's genuine question.",
        "Dedicated trial/perfection program awaits review; identify the actual party and issue without inferring a legal outcome from a generic deal."),
    card!(Vehicle, "Vehicle or journey safety", "142–143", Required, &[],
        &[Facet::Safety, Facet::Situation, Facet::Event], ExpertReview,
        "Ship I sail in is first in that capacity; movable possession is second. Presence aboard is not a prerequisite.", "Dedicated safety judgment requires review; identify the undertaking without replacing the question time with departure time."),
    card!(PersonDescription, "Description of a person", "143–145, 191, 196", Required, &[],
        &[Facet::Description, Facet::Situation], ExpertReview,
        "An identified person uses kind=person and owner_id=their actual ID; their operative relationship fixes their ruler. An explicitly unnamed future marriage partner uses kind=person_role, name=Future marriage partner, and owner_id=the role's principal (querent or an identified participant). Derive that principal's seventh; never invent a spouse identity.", "Broad comparative description, not exact height, ethnicity or invented marks. Use the main significator, not a natural cosignificator. Dedicated descriptive program needs review."),
    card!(Information, "Whether information is true", "164–165", None,
        &[required(Field::Claim)], &[Facet::Truth], ExpertReview,
        "A substantive relationship/job/etc. question normally uses its underlying matter; superficial 'is it true' wording is not a new universal recipe.", "Preserve the specific reported claim. Dedicated veracity program awaits review."),
    card!(Trust, "Trustworthiness in a capacity", "165", Required, &[],
        &[Facet::Situation, Facet::Truth], Implemented,
        "Judge the relevant person's condition, not a generic message-veracity house.", "State the capacity and the actual concern; avoid accusations unsupported by the reading."),
    card!(Pregnancy, "Current pregnancy", "173–174", Required, &[],
        &[Facet::Situation], ExpertReview,
        "Current state differs from future conception. Turning follows the principal and parenthood context; never infer paternity from a name.", "Dedicated pregnancy/medical qualification is required; the chart is not a clinical test."),
    card!(Fertility, "Conception and fertility", "174–177", Required,
        &[required(Field::FertilityScope)], &[Facet::Event, Facet::Situation, Facet::Timing, Facet::Quantity], ExpertReview,
        "Conception, carrying to term and lifetime potential are different scopes; relevant parent roles need explicit facts.", "The source's coarse childbirth count is specific to this recipe, not transferable to sales. Dedicated program awaits review."),
    card!(Adoption, "Adoption", "177–178", Required,
        &[required(Field::AdoptionState), when(Field::BirthParent, Field::AdoptionState, "prospective_known_parent")], EVENT_STATE, ExpertReview,
        "Prospective adoption is someone else's child; a completed adoption is one's own fifth-house child.", "Do not erase adoption state. Dedicated role/testimony program awaits review."),
    card!(Medical, "Illness or treatment", "179–189", Required,
        &[required(Field::MedicalTask), when(Field::Treatment, Field::MedicalTask, "treatment")],
        &[Facet::Situation, Facet::Choice, Facet::Event, Facet::Timing], ExpertReview,
        "Patient/capacity, diagnosis/prognosis/treatment differ. Lord 6 is not automatically the illness.", "The textbook's medical overview is not complete diagnosis training. Dedicated clinical-scope and judgment qualification is required."),
    card!(Politics, "Political election", "212–214", None,
        &[required(Field::Office), required(Field::ElectionState), required(Field::PoliticalCapacity)], EVENT_STATE, ExpertReview,
        "Incumbent/open contest and supporter/impartial citizen/foreign observer change assignment. Citizenship is not inferred from device location.", "Dedicated election program awaits review; preserve candidates and actual observer capacity."),
    card!(Knowledge, "Knowledge and its earnings", "216–218", Principal,
        &[required(Field::KnowledgeTask)], &[Facet::Situation, Facet::Profit], ExpertReview,
        "Knowledge ninth, profit tenth; employed job tenth/wages eleventh is different.", "Profit already arising from knowledge does not universally need an arrival aspect. Dedicated program awaits review."),
    card!(Exam, "Examination", "218", Required,
        &[required(Field::ExamTask)], EVENT_STATE, ExpertReview,
        "Exam result/profit from knowledge is not just a ninth-house object.", "Admission and passing can differ; preserve which was asked. Dedicated program awaits review."),
    card!(Undertaking, "Voyage, course or fair benefit", "219", Principal, &[],
        &[Facet::Situation, Facet::Profit], ExpertReview,
        "Undertaking quality differs from its profit; don't blindly reuse movable-goods sale roles.", "Identify the actual benefit asked about. Dedicated program awaits review."),
    card!(Dream, "Dream meaning or prophetic truth", "219", None,
        &[required(Field::DreamTask), required(Field::DreamAccount)], &[Facet::Situation, Facet::Truth], ExpertReview,
        "Dream meaning uses contextual ordinary roles; prophetic truth has its own ninth-house distinction.", "Chart the question's time, not the dream's time. Dedicated interpretation program awaits review."),
    card!(Education, "School or university", "219–220", Required,
        &[required(Field::SchoolLevel), required(Field::SchoolTask)], EVENT_STATE, ExpertReview,
        "Institution radical third/ninth, not blindly turned from a child.", "Admission, enjoyment and quality are distinct. Dedicated education program awaits review."),
    card!(Wish, "An unspecified wish", "164–165, 231", None, &[],
        &[Facet::Event, Facet::Situation], ExpertReview,
        "Specific matters use their own method; not every wish is eleventh.", "Ask what the concern is without demanding disclosure. No invented judgment for an unspecified wish."),
    card!(Tax, "Tax and assessment", "231–232", Principal, &[],
        &[Facet::Situation, Facet::Profit], ExpertReview,
        "Government tenth, its coffers eleventh, principal's money second.", "Amount/assessment and ability to bear it differ. No fabricated exact tax amount; dedicated program awaits review."),
    card!(Allegation, "Reported harmful practice", "233–234", None,
        &[required(Field::Claim)], &[Facet::Truth, Facet::Situation], ExpertReview,
        "Keep allegations as reported concerns. Special relationships matter only when supplied.", "A future applying aspect cannot prove a past act. No unprompted accusation; dedicated program awaits review."),
    card!(Custody, "Imprisonment or release", "234–237", Required,
        &[required(Field::CustodyState)], EVENT_STATE, ExpertReview,
        "Whether already in custody is indispensable. Relevant radical and turned twelfth need explicit consideration.", "Entry and release have different defaults; dedicated program awaits review."),
    card!(Weather, "Weather in a place or at an event", "238–240", None,
        &[required(Field::WeatherScope), required(Field::TargetPlace), required(Field::TargetPeriod)], EVENT_STATE, ExpertReview,
        "Target locality/season and event's house are context, not chart coordinates or question time.", "Dedicated weather program awaits review; do not treat this as a forecast from meteorological observations."),
    card!(Election, "Choosing when to act by horary", "241–242", None,
        &[required(Field::Action), required(Field::ActionWindow)], &[Facet::Choice, Facet::Timing], ExpertReview,
        "Horary election works from the original question chart, not a new future chart or a full natal election.", "Feasible action window and real constraints matter. Native exact-perfection timing is currently insufficient; dedicated program awaits review."),
    card!(Unclassified, "Matter not yet identified", "14, 26, 137–140", None, &[], &[], ExpertReview,
        "Do not guess the subject or force it into a familiar house.", "Elicit the actual concern; retain ambiguity until a method can be selected.")
];

pub fn contract(method: Method) -> &'static Contract {
    CATALOGUE
        .iter()
        .find(|c| c.method == method)
        .expect("Every Method has exactly one catalogue entry")
}

pub fn field_prompt(field: Field) -> &'static str {
    match field {
        Field::TimeOccurrence => "That clock time happened twice. Do you mean the earlier occurrence or the later one?",
        Field::PrincipalMode => "Are you asking for yourself, about someone else, or passing on their own question?",
        Field::PrincipalId => "Whose own question are you passing on?",
        Field::ReaderPlace => "Where are you asking from? A city and country will do; the event's venue is separate.",
        Field::QuestionTime => "What date and local time did you understand this earlier question?",
        Field::Baseline => "Is this about a hoped-for relationship, one already under way, or an arranged wedding?",
        Field::Description => "What does the missing object look like—its colour, material or shape?",
        Field::AnimalKind => "What kind of animal is missing?",
        Field::DealCapacity => "Are you buying, selling or renting?",
        Field::DealActor => "Whose purchase, sale or rental agreement are we considering?",
        Field::DealBeneficiary => "Whose financial benefit are you asking about?",
        Field::MoneySource => "Where is the money coming from—a customer, job, government, or someone you know?",
        Field::Discretionary => "Is this money already owed to you, or a gift or grant they can choose to give?",
        Field::WorkCapacity => "Is this person your boss, a colleague, or someone who works for you?",
        Field::CurrentOption => "What would staying with things as they are mean?",
        Field::Alternatives => "What are the alternatives you are comparing?",
        Field::Candidates => "Who are the candidates you could hire? A brief description of each will do.",
        Field::Sender => "Who is sending the parcel, and in what capacity?",
        Field::Visitor => "Who is coming to visit, and who are they to you?",
        Field::Expected => "Is this visit already expected, or are you hoping they will come?",
        Field::Affiliation => "Which side do you support, or are you asking about a bet's profit?",
        Field::Competition => "Is this about one match, a season, or a championship?",
        Field::Claim => "What specific claim or concern would you like me to consider?",
        Field::FertilityScope => "Are you asking about conceiving soon, carrying a pregnancy, or ever having children?",
        Field::AdoptionState => "Is this adoption still prospective, or is the child already yours?",
        Field::BirthParent => "Who is the child's birth parent to you?",
        Field::MedicalTask => "Is your question about the illness, its course, or a particular treatment?",
        Field::Treatment => "Which treatment or treatments are you asking about?",
        Field::PoliticalCapacity => "Are you supporting a candidate, an impartial citizen, or an observer from elsewhere?",
        Field::ElectionState => "Is there an incumbent standing again, or is this an open contest?",
        Field::Office => "Which office and election are you asking about?",
        Field::KnowledgeTask => "Are you asking about the knowledge itself, or earnings from using it?",
        Field::ExamTask => "Are you asking about passing this exam, or being admitted?",
        Field::DreamTask => "Would you like to explore the dream's meaning, or whether it foretells something?",
        Field::DreamAccount => "What happened in the dream?",
        Field::SchoolLevel => "Is this a school or a university?",
        Field::SchoolTask => "Are you asking about admission, enjoyment, or the quality of the education?",
        Field::CustodyState => "Is this person already in custody, or are you asking whether they will be?",
        Field::WeatherScope => "Is this about the weather generally, or at a particular event?",
        Field::TargetPlace => "Where is the weather you are asking about? This is separate from where we cast the question.",
        Field::TargetPeriod => "What day or season is this weather question about?",
        Field::Action => "What action would you like to find a time for?",
        Field::ActionWindow => "What dates or time window are actually available for that action?",
        Field::Priorities => "What would make this option good for you in this question?",
        Field::HomeMeaning => "By home, do you mean your current home or your homeland?",
        Field::Parenthood => "Whose parenthood is involved in this question?",
        Field::TheftRaised => "What concern about theft have you already raised?",
        Field::SearchContext => "What happened when you searched the suggested place?",
        Field::DealParty => "Who is the other party in this particular deal?",
        Field::Seller => "Who is selling the goods? That may be different from who owns them.",
        Field::JobContext => "Is this a new job, your current job, a return, or an offer you can already take?",
        Field::Constraints => "What real constraints affect when you can act?",
        Field::Unit => "What quantity are you asking me to estimate?",
        Field::Context => "What circumstance do I need to understand about this question?",
        Field::EventPlace => "Where is the event taking place?",
        Field::EventTime => "When is that event taking place?",
        Field::Horizon => "What time span does your question concern?",
    }
}

pub fn allowed_values(field: Field) -> &'static [&'static str] {
    match field {
        Field::TimeOccurrence => &["earlier", "later"],
        Field::PrincipalMode => &["self", "relay", "concerning_other"],
        Field::Baseline => &["hoped_for", "ongoing", "arranged_wedding"],
        Field::AnimalKind => &["small_kind", "large_kind"],
        Field::DealCapacity => &["buy", "sell", "rent"],
        Field::MoneySource => &[
            "customer",
            "partner",
            "job",
            "government",
            "relative",
            "other",
        ],
        Field::Discretionary => &["owed", "discretionary"],
        Field::WorkCapacity => &["boss", "colleague", "subordinate"],
        Field::Expected | Field::TheftRaised => &["yes", "no"],
        Field::Competition => &["match", "season", "championship"],
        Field::FertilityScope => &["conception", "carrying_to_term", "lifetime"],
        Field::AdoptionState => &["prospective", "prospective_known_parent", "completed"],
        Field::MedicalTask => &["diagnosis", "prognosis", "treatment"],
        Field::PoliticalCapacity => &["supporter", "impartial_citizen", "foreign_observer"],
        Field::ElectionState => &["incumbent", "open"],
        Field::KnowledgeTask => &["quality", "profit"],
        Field::ExamTask => &["passing", "admission"],
        Field::DreamTask => &["meaning", "prophetic_truth"],
        Field::SchoolLevel => &["school", "university"],
        Field::SchoolTask => &["admission", "enjoyment", "quality"],
        Field::CustodyState => &["already_held", "not_held"],
        Field::WeatherScope => &["general", "event"],
        Field::HomeMeaning => &["current_home", "homeland"],
        _ => &[],
    }
}

/// Validate the canonical syntax before checking the exact source proof.
/// A supplied second clock occurrence still needs value=later, not second.
fn validate_update_value(update: &Update) -> Result<(), String> {
    let allowed = allowed_values(update.field);
    if update.mode != UpdateMode::Unavailable
        && !allowed.is_empty()
        && !allowed.contains(&update.value.as_str())
    {
        let occurrence = if update.field == Field::TimeOccurrence {
            " For a repeated civil clock, first/earlier maps to earlier and second/later maps to later; keep the person's exact words in quote."
        } else {
            " Keep the person's exact words in quote."
        };
        return Err(format!(
            "{} value {:?} is not canonical. Use one of: {}.{}",
            update.field.name(),
            update.value,
            allowed.join(", "),
            occurrence
        ));
    }
    if update.value.trim().is_empty() || update.value.len() > 700 {
        return Err("A fact update needs a nonempty bounded value.".into());
    }
    Ok(())
}

fn actor_reference_field(field: Field, method: Option<Method>) -> bool {
    match field {
        Field::PrincipalId => true,
        Field::DealActor | Field::DealBeneficiary | Field::Seller | Field::DealParty => matches!(
            method,
            Some(
                Method::MovableDeal | Method::Property | Method::Rental | Method::BusinessProperty
            )
        ),
        Field::Sender => method == Some(Method::Money),
        _ => false,
    }
}

/// Bind only an existing ID or one unambiguous whole display label. The raw
/// proposal and its exact quote remain in the change receipt; only the stored
/// actor reference becomes canonical. A failed extraction is not a user gap.
fn bind_actor_reference(
    field: Field,
    value: &str,
    people: &BTreeMap<String, Person>,
) -> Result<String, String> {
    let value = value.trim();
    if value == "querent" || people.contains_key(value) {
        return Ok(value.into());
    }
    let mut matching = people
        .iter()
        .filter(|(_, person)| person.label.trim().eq_ignore_ascii_case(value));
    let Some((id, _)) = matching.next() else {
        return Err(format!(
            "{} value {value:?} does not bind a known participant. Use the actual stable ID from people or querent; include a source-backed participant if needed. If the source leaves the identity unresolved, leave the fact missing or use mode=propose rather than resolving an invented actor ID.",
            field.name()
        ));
    };
    if matching.next().is_some() {
        return Err(format!(
            "{} label {value:?} matches more than one participant. Use a source-backed stable ID only when the source distinguishes them; otherwise use mode=propose so the genuine ambiguity can be elicited.",
            field.name()
        ));
    }
    Ok(id.clone())
}

fn field_prompt_for(method: Option<Method>, field: Field) -> &'static str {
    match (method, field) {
        (Some(Method::Money), Field::Sender) => {
            "Who is this relative sending the money, and who are they to you?"
        }
        _ => field_prompt(field),
    }
}

/// A narrow repair for the observed Investment name=quote false rejection.
/// This accepts an affirmative speaker-owned holding clause, not arbitrary
/// ownership prose. Exact quotation and owner-change guards still run later.
fn explicit_speaker_holding(quote: &str, source: &str) -> bool {
    let quote = quote.to_ascii_lowercase();
    let source = source.to_ascii_lowercase();
    let tokens = |text: &str| {
        text.split(|ch: char| !ch.is_alphanumeric() && !matches!(ch, '\'' | '’'))
            .filter(|word| !word.is_empty())
            .map(str::to_owned)
            .collect::<Vec<_>>()
    };
    if tokens(&quote).len() < 3 {
        return false;
    }
    source.match_indices(&quote).any(|(start, _)| {
        quote.match_indices("i own").any(|(relative, _)| {
            let cue = start + relative;
            if source[..cue]
                .chars()
                .next_back()
                .is_some_and(|ch| ch.is_alphanumeric())
                || source[cue + 5..]
                    .chars()
                    .next()
                    .is_some_and(|ch| ch.is_alphanumeric())
            {
                return false;
            }
            let before = &source[..cue];
            // This deliberately narrow exception refuses quoted first-person
            // text, including a quotation that crossed a sentence boundary.
            let quoted = before.contains(['"', '“', '”', '‘'])
                || before.char_indices().any(|(at, ch)| {
                    if !matches!(ch, '\'' | '’') {
                        return false;
                    }
                    let previous = before[..at].chars().next_back();
                    let next = before[at + ch.len_utf8()..].chars().next();
                    !previous.is_some_and(char::is_alphanumeric)
                        || !next.is_some_and(char::is_alphanumeric)
                });
            if quoted {
                return false;
            }
            // A reported/quoted first person belongs to its reported speaker.
            let sentence = before
                .rsplit(['.', '!', '?', ';', '\n'])
                .next()
                .unwrap_or(before);
            let sentence_tokens = tokens(sentence);
            if sentence.contains([':', '"', '“', '”', '‘'])
                || sentence_tokens.iter().any(|word| {
                    matches!(
                        word.as_str(),
                        "said"
                            | "say"
                            | "saying"
                            | "says"
                            | "wrote"
                            | "writes"
                            | "quoted"
                            | "claims"
                            | "claimed"
                            | "thinks"
                            | "thought"
                            | "think"
                            | "suppose"
                            | "assume"
                            | "whether"
                            | "wonder"
                            | "unsure"
                            | "uncertain"
                            | "false"
                            | "untrue"
                    )
                })
            {
                return false;
            }
            let clause = sentence.rsplit(',').next().unwrap_or(sentence);
            let preceding = tokens(clause);
            if preceding.iter().any(|word| {
                matches!(
                    word.as_str(),
                    "not"
                        | "no"
                        | "never"
                        | "neither"
                        | "isn't"
                        | "isn’t"
                        | "aren't"
                        | "aren’t"
                        | "don't"
                        | "don’t"
                        | "doesn't"
                        | "doesn’t"
                        | "didn't"
                        | "didn’t"
                        | "wasn't"
                        | "wasn’t"
                        | "weren't"
                        | "weren’t"
                        | "if"
                )
            }) || preceding.last().is_some_and(|word| {
                matches!(
                    word.as_str(),
                    "will" | "would" | "could" | "might" | "may" | "can" | "do" | "did"
                )
            }) {
                return false;
            }
            !tokens(&source[cue + 5..]).first().is_some_and(|word| {
                matches!(word.as_str(), "not" | "no" | "none" | "nothing" | "neither")
            })
        })
    })
}

fn bare_subject_quote(name: &str, quote: &str) -> bool {
    let without_article = |text: &str| {
        let text = text
            .trim()
            .trim_end_matches(['.', '!', '?'])
            .to_ascii_lowercase();
        for article in ["the ", "a ", "an ", "this ", "that ", "these ", "those "] {
            if let Some(noun) = text.strip_prefix(article) {
                return noun.trim().to_owned();
            }
        }
        text
    };
    let name = without_article(name);
    let quote = quote
        .trim()
        .trim_end_matches(['.', '!', '?'])
        .to_ascii_lowercase();
    if without_article(&quote) == name {
        return true;
    }
    // A sale supplies an action, not title. Detect the same unsupported
    // evidence in a longer clause as well as in an extracted bare fragment.
    // This still does not certify arbitrary English as ownership proof.
    if ["own", "owns", "owned", "belong", "belongs", "title"]
        .iter()
        .any(|cue| crate::horary_role_options::mentions(&quote, cue))
    {
        return false;
    }
    ["sell ", "sells ", "selling ", "sold ", "to sell "]
        .iter()
        .any(|verb| {
            quote
                .strip_prefix(verb)
                .is_some_and(|object| without_article(object) == name)
                || ["the ", "a ", "an ", "this ", "that ", "these ", "those "]
                    .iter()
                    .any(|article| {
                        crate::horary_role_options::mentions(
                            &quote,
                            &format!("{verb}{article}{name}"),
                        )
                    })
        })
}

fn occurrence_evidence(
    value: &str,
    quote: &str,
    pending: bool,
    pending_moment: bool,
    question_moment: bool,
    words: &str,
) -> bool {
    let mentions = crate::horary_role_options::mentions;
    let choices: &[&str] = match value {
        "earlier" => &["first", "earlier"],
        "later" => &["second", "later"],
        _ => return false,
    };
    if !choices.iter().any(|choice| mentions(quote, choice)) {
        return false;
    }
    if pending {
        return true;
    }
    let occurrence = choices
        .iter()
        .any(|choice| mentions(quote, &format!("{choice} occurrence")));
    let explicit_overlap = [
        "clocks went back",
        "clocks moved back",
        "daylight saving",
        "dst",
        "clock overlap",
        "repeated clock time",
        "clock time happened twice",
    ]
    .iter()
    .any(|context| mentions(words, context));
    let clock_choice = words
        .split(|ch: char| !ch.is_ascii_digit() && ch != ':')
        .any(|clock| {
            let Some((hour, minute)) = clock.split_once(':') else {
                return false;
            };
            hour.parse::<u8>().is_ok_and(|hour| hour < 24)
                && minute.parse::<u8>().is_ok_and(|minute| minute < 60)
                && choices
                    .iter()
                    .any(|choice| mentions(words, &format!("{choice} {clock}")))
        });
    if pending_moment && question_moment && (explicit_overlap || clock_choice || occurrence) {
        return true;
    }
    occurrence && (question_moment || explicit_overlap)
}

impl Consultation {
    pub fn understood(&self) -> bool {
        self.question.resolved().is_some()
            && self.subject.resolved().is_some()
            && self
                .method()
                .is_some_and(|m| !matches!(m, Method::Unclassified | Method::Wish))
    }
    pub fn recognition_snapshot(&self) -> Self {
        let mut snapshot = self.clone();
        snapshot.changes.clear();
        snapshot
    }
    pub fn text(&self, field: Field) -> Option<&str> {
        self.facts
            .get(&field)
            .and_then(Slot::resolved)
            .map(String::as_str)
    }
    pub fn method(&self) -> Option<Method> {
        self.frame.resolved().map(|f| f.method)
    }
    pub fn apply(
        &mut self,
        turn: &Turn,
        turn_number: usize,
        words: &str,
        spoken: bool,
    ) -> Result<(), String> {
        // A rejection must leave the entire case unchanged, including receipts.
        let mut next = self.clone();
        next.apply_inner(turn, turn_number, words, spoken)?;
        *self = next;
        Ok(())
    }
    fn apply_inner(
        &mut self,
        turn: &Turn,
        number: usize,
        words: &str,
        spoken: bool,
    ) -> Result<(), String> {
        if self.catalogue_version != VERSION {
            return Err(
                "This consultation needs catalogue migration before accepting updates.".into(),
            );
        }
        if spoken && turn.heard.trim().is_empty() {
            return Err(
                "Direct audio must preserve its understood meaning before this step can complete."
                    .into(),
            );
        }
        let words = if spoken { turn.heard.as_str() } else { words };
        let retained_question = if turn.intent == Intent::NewQuestion {
            None
        } else {
            self.question.resolved().cloned()
        };
        let evidence = |quote: &str| -> Result<Evidence, String> {
            if quote.trim().is_empty() {
                return Err("A new fact needs an exact source quote.".into());
            }
            if words.contains(quote) {
                return Ok(Evidence::User {
                    turn: number,
                    quote: quote.into(),
                });
            }
            if retained_question
                .as_ref()
                .is_some_and(|question| question.contains(quote))
            {
                return Ok(Evidence::RetainedQuestion {
                    quote: quote.into(),
                });
            }
            Err(format!("The source quote {quote:?} does not occur in the current words or retained original question. Copy an exact span from the supplied words; do not stem, inflect or paraphrase a quote. Unchanged facts need no update."))
        };
        let correct = turn.intent == Intent::Correct;
        let understood = self.understood();
        let previous_frame = self.frame.resolved().cloned();
        let previous_question = self.question.resolved().cloned();
        let before = json!([
            self.question,
            self.frame,
            self.people,
            self.subject,
            self.facts,
            self.device_reader_place
        ]);
        if turn.intent == Intent::NewQuestion {
            *self = Self::default();
        }
        if turn.intent == Intent::UseDevice {
            self.facts.remove(&Field::ReaderPlace);
            self.device_reader_place = true;
            self.additional
                .retain(|need| need.key != RequirementKey::Field(Field::ReaderPlace));
        }
        if let Some(question) = &turn.question {
            if question.trim().is_empty() || question.len() > 500 {
                return Err("An actual question must be nonempty and at most 500 bytes.".into());
            }
            if self.question.resolved().is_some_and(|old| old != question)
                && !correct
                && understood
                && turn.intent != Intent::NewQuestion
            {
                return Err("A clarification must not replace the original question; use an explicit correction or new matter.".into());
            }
            self.question.set(
                Observation {
                    value: question.clone(),
                    evidence: Evidence::User {
                        turn: number,
                        quote: words.into(),
                    },
                },
                correct || !understood,
                false,
            );
        }
        if let Some(frame) = &turn.frame {
            self.frame.set(
                Observation {
                    value: frame.clone(),
                    evidence: Evidence::User {
                        turn: number,
                        quote: words.into(),
                    },
                },
                correct || !understood,
                false,
            );
        }
        for person in &turn.people {
            if person.id.is_empty()
                || person.id == "querent"
                || !person
                    .id
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
                || person.label.trim().is_empty()
            {
                return Err(
                    "Relevant people need a stable lowercase ID and a label; querent is reserved."
                        .into(),
                );
            }
            let proof = evidence(&person.source_quote)?;
            if person.relationship != "unknown" {
                let source = if matches!(proof, Evidence::RetainedQuestion { .. }) {
                    retained_question.as_deref().unwrap_or(words)
                } else {
                    words
                };
                if !crate::horary_role_options::relationship_evidence(
                    &person.relationship,
                    &person.source_quote,
                    source,
                    &person.label,
                    self.requested.as_ref()
                        == Some(&RequirementKey::PersonRelationship(person.id.clone())),
                ) {
                    return Err(format!("Person {:?} has relationship {:?}, but quote {:?} does not supply that capacity. A name alone must use relationship=unknown, not other_party. Keep the identified person with an unknown relationship; Rust will ask who they are. Do not drop the original question or subject.",person.label,person.relationship,person.source_quote));
                }
            }
            if self.people.get(&person.id).is_some_and(|old| {
                old.relationship != "unknown" && old.relationship != person.relationship
            }) && !correct
            {
                return Err(
                    "Changing an established person's capacity requires an explicit correction."
                        .into(),
                );
            }
            self.people.insert(person.id.clone(), person.clone());
        }
        if let Some(subject) = &turn.subject {
            if !subject.owner_id.is_empty()
                && matches!(
                    subject.kind.as_str(),
                    "movable" | "property" | "small_animal" | "large_animal" | "animal"
                )
                && bare_subject_quote(&subject.name, &subject.source_quote)
                && !(self.method() == Some(Method::Investment)
                    && subject.owner_id == "querent"
                    && explicit_speaker_holding(
                        &subject.source_quote,
                        if words.contains(&subject.source_quote) {
                            words
                        } else {
                            retained_question.as_deref().unwrap_or(words)
                        },
                    ))
                && self
                    .subject
                    .resolved()
                    .is_none_or(|old| old.owner_id != subject.owner_id)
            {
                return Err("The object's name alone, or a bare sale phrase, does not establish its owner. Leave owner_id empty unless ownership is explicitly stated; being the seller is not ownership.".into());
            }
            if subject.name.trim().is_empty() {
                return Err("A subject needs a name.".into());
            }
            let proof = evidence(&subject.source_quote)?;
            if !subject.owner_id.is_empty()
                && subject.owner_id != "querent"
                && !self.people.contains_key(&subject.owner_id)
            {
                return Err(
                    "Owner must refer to a known participant, never an invented ID.".into(),
                );
            }
            if subject.kind == "person_role" {
                let source = if matches!(proof, Evidence::RetainedQuestion { .. }) {
                    retained_question.as_deref().unwrap_or(words)
                } else {
                    words
                };
                if self.method() != Some(Method::PersonDescription)
                    || !subject.is_future_marriage_partner()
                    || !crate::horary_role_options::future_marriage_partner_evidence(
                        &subject.source_quote,
                        source,
                        &subject.owner_id,
                        self.people.get(&subject.owner_id),
                    )
                {
                    return Err("person_role is only the explicit unnamed Future marriage partner in person_description. owner_id is the person whose future spouse is described; it is not the spouse's ID. Preserve identified targets as kind=person. Other future roles, attached names and negated role phrases cannot supply this role.".into());
                }
            }
            let old = self.subject.resolved();
            let compatible = old.is_none_or(|old| {
                let named_person_refinement = old.kind == "person"
                    && subject.kind == "person"
                    && old.owner_id.is_empty()
                    && matches!(
                        old.name.to_ascii_lowercase().as_str(),
                        "relationship"
                            | "person"
                            | "partner"
                            | "prospective partner"
                            | "future partner"
                    )
                    && self.people.get(&subject.owner_id).is_some_and(|person| {
                        subject
                            .name
                            .trim()
                            .eq_ignore_ascii_case(person.label.trim())
                    });
                let animal_identification = self.method() == Some(Method::LostAnimal)
                    && self.requested == Some(RequirementKey::Field(Field::AnimalKind))
                    && old.kind == "animal"
                    && matches!(subject.kind.as_str(), "small_animal" | "large_animal")
                    && (old.owner_id.is_empty() || old.owner_id == subject.owner_id)
                    && (old.name.trim().eq_ignore_ascii_case(subject.name.trim())
                        || matches!(
                            old.name.trim().to_ascii_lowercase().as_str(),
                            "pet" | "animal"
                        ))
                    && crate::horary_role_options::mentions(&subject.source_quote, &subject.name);
                (old.name.trim().eq_ignore_ascii_case(subject.name.trim())
                    && old.kind == subject.kind
                    && (old.owner_id.is_empty() || old.owner_id == subject.owner_id))
                    || named_person_refinement
                    || animal_identification
            });
            if !compatible && !correct && understood {
                return Err(
                    "Changing an established subject or owner needs an explicit correction.".into(),
                );
            }
            self.subject = Slot::Resolved {
                observation: Observation {
                    value: subject.clone(),
                    evidence: evidence(&subject.source_quote)?,
                },
            };
        }
        if let (Some(method), Some(subject)) = (self.method(), self.subject.resolved()) {
            if !contract(method)
                .subject_kinds
                .contains(&subject.kind.as_str())
            {
                return Err(format!("The {} method needs a subject kind from {:?}; do not hand a different type to its reader.",method.name(),contract(method).subject_kinds));
            }
        }
        let mut updated = std::collections::BTreeSet::new();
        for update in &turn.updates {
            if update.field == Field::Baseline
                && self.method().is_some_and(|m| m != Method::Relationship)
            {
                return Err("Remove the baseline entry from updates: baseline applies only to a relationship question. A husband in a sale belongs in people with relationship=partner. Keep other explicitly supplied updates; if none remain, return updates=[].".into());
            }
            if !updated.insert(update.field) {
                return Err("A turn must not update the same field twice.".into());
            }
            validate_update_value(update)?;
            let recover_original_anchor =
                recoverable_original_anchor(self, update.field, &update.quote);
            let proof = if recover_original_anchor && !words.contains(&update.quote) {
                Evidence::RetainedQuestion {
                    quote: update.quote.clone(),
                }
            } else {
                evidence(&update.quote)?
            };
            if matches!(update.field, Field::ReaderPlace | Field::QuestionTime)
                && matches!(proof, Evidence::RetainedQuestion { .. })
                && !recover_original_anchor
            {
                return Err("A reader_place or question_time override must be stated in the CURRENT words, or answer the pending chart-anchor question. Remove this override: quoting a venue or event time from the retained question cannot change the chart anchor during an unrelated clarification.".into());
            }
            if update.field == Field::TimeOccurrence && update.mode != UpdateMode::Unavailable {
                let pending = matches!(
                    self.requested,
                    Some(RequirementKey::Field(Field::TimeOccurrence))
                );
                let question_moment = self.text(Field::QuestionTime).is_some()
                    || turn
                        .updates
                        .iter()
                        .any(|entry| entry.field == Field::QuestionTime);
                let pending_moment = self.requested == Some(RequirementKey::ChartMoment)
                    && self.text(Field::QuestionTime).is_some();
                if !occurrence_evidence(
                    &update.value,
                    &update.quote,
                    pending,
                    pending_moment,
                    question_moment,
                    words,
                ) {
                    return Err("time_occurrence selects the first/earlier or second/later occurrence of a repeated civil chart time. An event being 'this morning' or 'earlier' is not a clock-overlap choice. Remove this entry unless the person explicitly selects that occurrence or answers the pending overlap question.".into());
                }
            }
            let mut value = update.value.clone();
            if matches!(update.mode, UpdateMode::Supply | UpdateMode::Correct)
                && actor_reference_field(update.field, self.method())
            {
                value = bind_actor_reference(update.field, &value, &self.people)?;
            }
            let slot = self.facts.entry(update.field).or_default();
            if update.mode == UpdateMode::Unavailable {
                *slot = Slot::Unavailable {
                    reason: update.value.clone(),
                    evidence: proof,
                };
            } else {
                if matches!(update.field, Field::Context | Field::SearchContext)
                    && !correct
                    && update.mode == UpdateMode::Supply
                {
                    let previous = match &*slot {
                        Slot::Resolved { observation } | Slot::Proposed { observation } => {
                            Some(&observation.value)
                        }
                        _ => None,
                    };
                    if let Some(previous) = previous {
                        if !previous.contains(&value) {
                            value = format!("{previous}\n{value}");
                        }
                    }
                }
                slot.set(
                    Observation {
                        value,
                        evidence: proof,
                    },
                    update.mode == UpdateMode::Correct
                        || correct
                        || matches!(
                            update.field,
                            Field::PrincipalMode | Field::Context | Field::SearchContext
                        ),
                    update.mode == UpdateMode::Propose,
                );
            }
        }
        if turn.updates.iter().any(|update| {
            update.field == Field::ReaderPlace && update.mode != UpdateMode::Unavailable
        }) {
            self.device_reader_place = false;
        }
        if let Some(person) = self.unbound_named_subject() {
            return Err(format!("The identified person {:?} must be bound in subject.owner_id={:?}. An empty person ID is for an unnamed role; do not relabel this known person as a prospective stranger or use 'relationship' as the person. Preserve the question and supplied baseline.", person.label, person.id));
        }
        if self.method() == Some(Method::LostAnimal)
            && self.text(Field::AnimalKind).is_none()
            && self
                .subject
                .resolved()
                .is_some_and(|subject| subject.kind != "animal")
        {
            return Err("The animal's kind is unresolved. Use subject.kind=animal until animal_kind is supplied from an actual species; 'pet' does not establish small_animal or large_animal. Keep the question and known owner, and let the reader inquire about the kind.".into());
        }
        if !turn.unavailable_quote.is_empty() {
            let proof = evidence(&turn.unavailable_quote)?;
            let key = self
                .requested
                .clone()
                .ok_or("An unavailable answer must refer to the currently requested fact.")?;
            let field = match key {
                RequirementKey::ChartPlace => Some(Field::ReaderPlace),
                RequirementKey::ChartMoment => Some(Field::QuestionTime),
                RequirementKey::Field(field) => Some(field),
                _ => None,
            };
            if let Some(field) = field {
                self.facts.insert(
                    field,
                    Slot::Unavailable {
                        reason: turn.unavailable_quote.clone(),
                        evidence: proof.clone(),
                    },
                );
            }
            self.unavailable.retain(|note| note.key != key);
            self.unavailable.push(UnavailableNeed {
                key,
                evidence: proof,
            });
        }
        if previous_frame.as_ref() != self.frame.resolved()
            || previous_question.as_ref() != self.question.resolved()
        {
            // A factual need belongs to the program that discovered it. A
            // revised question is planned anew; earlier receipts retain it.
            self.additional.clear();
        }
        self.changes.push(Change {
            turn: number,
            before_revision: self.revision,
            proposal: turn.clone(),
        });
        record_anchor_obligation(self, words);
        if before
            != json!([
                self.question,
                self.frame,
                self.people,
                self.subject,
                self.facts,
                self.device_reader_place
            ])
        {
            self.revision = self
                .revision
                .checked_add(1)
                .ok_or("Consultation revision exhausted")?;
        }
        self.requested = None;
        Ok(())
    }

    fn unbound_named_subject(&self) -> Option<&Person> {
        let subject = self.subject.resolved()?;
        if subject.kind != "person" || !subject.owner_id.is_empty() {
            return None;
        }
        self.people.values().find(|person| {
            crate::horary_role_options::mentions(&subject.name, &person.label)
                || crate::horary_role_options::mentions(&subject.source_quote, &person.label)
                || (self.method() == Some(Method::Relationship)
                    && matches!(
                        self.text(Field::Baseline),
                        Some("ongoing" | "arranged_wedding")
                    )
                    && person.relationship == "partner")
        })
    }
}

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Truth {
    Yes,
    No,
    Unknown,
}
impl Guard {
    pub fn evaluate(self, case: &Consultation) -> Truth {
        match self {
            Self::Always => Truth::Yes,
            Self::Equals { field, value } => match case.text(field) {
                Some(found) if found == value => Truth::Yes,
                Some(_) => Truth::No,
                None => Truth::Unknown,
            },
            Self::AnswerFacet { facet } => match case.frame.resolved() {
                Some(frame) if frame.facet == facet => Truth::Yes,
                Some(_) => Truth::No,
                None => Truth::Unknown,
            },
            Self::PrincipalOwnsSubject => match case.subject.resolved() {
                Some(subject) if !subject.owner_id.is_empty() => {
                    let principal = if case.text(Field::PrincipalMode) == Some("relay") {
                        case.text(Field::PrincipalId)
                    } else {
                        Some("querent")
                    };
                    match principal {
                        Some(id) if subject.owner_id == id => Truth::Yes,
                        Some(_) => Truth::No,
                        None => Truth::Unknown,
                    }
                }
                _ => Truth::Unknown,
            },
        }
    }
}
#[derive(Clone, Debug, Serialize)]
pub struct Need {
    pub key: RequirementKey,
    pub state: String,
    pub question: String,
    pub reason: String,
}
#[derive(Clone, Debug, Serialize)]
pub struct Limitation {
    pub code: String,
    pub message: String,
    pub printed_pages: String,
}
#[derive(Clone, Debug, Serialize)]
pub struct Plan {
    pub needs: Vec<Need>,
    pub limitation: Option<Limitation>,
}

impl Consultation {
    pub fn question_for(&self, key: &RequirementKey) -> String {
        if matches!(
            key,
            RequirementKey::Field(Field::Context | Field::SearchContext)
        ) {
            if let Some(question) = self
                .additional
                .iter()
                .find(|need| &need.key == key)
                .and_then(|need| need.question.as_ref())
            {
                return question.clone();
            }
        }
        match key {
            RequirementKey::Question => "What would you like to know?".into(),
            RequirementKey::Frame => {
                "What is the particular concern you would like the reading to answer?".into()
            }
            RequirementKey::Subject => "Who or what is your question about?".into(),
            RequirementKey::Owner => match self.method() {
                Some(Method::LostObject) => "Whose missing object is this?",
                Some(Method::MovableDeal) => "Who owns the goods? The seller may be someone else.",
                Some(
                    Method::NewJob | Method::ExistingJob | Method::ReturnToJob | Method::JobOffer,
                ) => "Whose job are you asking about?",
                Some(Method::Money | Method::Investment) => {
                    "Whose money or investment are you asking about?"
                }
                Some(Method::MissingPerson) => "Who is the missing person?",
                _ => "Whose matter are you asking about?",
            }
            .into(),
            RequirementKey::PersonRelationship(id) => format!(
                "Who is {} to you in this question?",
                self.people
                    .get(id)
                    .map(|p| p.label.as_str())
                    .unwrap_or("this person")
            ),
            RequirementKey::Field(field) => field_prompt_for(self.method(), *field).into(),
            RequirementKey::ChartPlace => field_prompt(Field::ReaderPlace).into(),
            RequirementKey::ChartMoment => field_prompt(Field::QuestionTime).into(),
        }
    }
    pub fn is_deal(&self) -> bool {
        matches!(
            self.method(),
            Some(
                Method::MovableDeal | Method::Property | Method::Rental | Method::BusinessProperty
            )
        )
    }

    pub fn is_pure_deal_completion(&self) -> bool {
        matches!(
            self.method(),
            Some(Method::MovableDeal | Method::Property | Method::Rental)
        ) && self
            .frame
            .resolved()
            .is_some_and(|frame| matches!(frame.facet, Facet::Event | Facet::Timing))
    }

    /// A sourced legacy movable seller already identifies the contracting
    /// actor. It does not establish title ownership or a personal relationship.
    pub fn deal_actor(&self) -> Option<&str> {
        self.text(Field::DealActor).or_else(|| {
            (self.method() == Some(Method::MovableDeal)
                && self.text(Field::DealCapacity) == Some("sell"))
            .then(|| self.text(Field::Seller))
            .flatten()
        })
    }

    pub fn needs_title_owner(&self) -> bool {
        self.method()
            .is_some_and(|method| match contract(method).owner {
                OwnerRule::Required => true,
                OwnerRule::DealContext => {
                    !self.is_pure_deal_completion()
                        && self.text(Field::DealCapacity) == Some("sell")
                }
                OwnerRule::None | OwnerRule::Principal => false,
            })
    }

    pub fn plan(&self, anchor: Option<&Anchor>) -> Plan {
        let mut plan = Plan {
            needs: Vec::new(),
            limitation: None,
        };
        if self.catalogue_version != VERSION {
            plan.limitation=Some(Limitation{code:"catalogue_migration_required".into(),message:"This saved consultation uses a different method catalogue and needs a checked migration. Its original record is preserved.".into(),printed_pages:"7–8".into()});
            return plan;
        }
        let mut need = |key: RequirementKey, state: &str, reason: &str| {
            if !plan.needs.iter().any(|n| n.key == key) {
                plan.needs.push(Need {
                    question: if state == "conflicting" {
                        format!("I have two different accounts. {}", self.question_for(&key))
                    } else {
                        self.question_for(&key)
                    },
                    key: key.clone(),
                    state: if self.unavailable.iter().any(|note| note.key == key) {
                        "unavailable".into()
                    } else {
                        state.into()
                    },
                    reason: reason.into(),
                });
            }
        };
        if self.question.resolved().is_none() {
            need(
                RequirementKey::Question,
                self.question.reason(),
                "The actual question has not been understood.",
            );
        }
        let Some(frame) = self.frame.resolved() else {
            need(
                RequirementKey::Frame,
                self.frame.reason(),
                "No unambiguous reading program has been selected.",
            );
            return plan;
        };
        if frame.method == Method::Unclassified || frame.method == Method::Wish {
            need(
                RequirementKey::Frame,
                "unclassified",
                "The underlying concern determines its method; no universal wish recipe.",
            );
            return plan;
        }
        if original_question_words(self)
            .and_then(historical_anchor_request)
            .is_some()
        {
            for field in [Field::ReaderPlace, Field::QuestionTime] {
                if self.text(field).is_none()
                    && !(field == Field::ReaderPlace && self.device_reader_place)
                {
                    need(RequirementKey::Field(field), "explicit_historical_anchor_missing", "The original source explicitly selects an earlier consultation. Resolve its reader place and understood moment before any device/now reading permit; Frawley printed pp. 7–8.");
                }
            }
        }
        let card = contract(frame.method);
        if !card.facets.contains(&frame.facet) {
            plan.limitation = Some(Limitation {
                code: "unsupported_facet".into(),
                message: if frame.facet == Facet::Quantity {
                    "This method has no reviewed exact-count judgment. Preserve the requested quantity; a different question requires the person's choice.".into()
                } else {
                    "This method has no reviewed judgment for the requested answer facet.".into()
                },
                printed_pages: card.printed_pages.into(),
            });
        }
        let principal_mode = self.text(Field::PrincipalMode);
        if principal_mode.is_none() {
            need(
                RequirementKey::Field(Field::PrincipalMode),
                self.facts
                    .get(&Field::PrincipalMode)
                    .map_or("missing", Slot::reason),
                "The genuine principal determines the role frame.",
            );
        }
        if principal_mode == Some("relay") && self.text(Field::PrincipalId).is_none() {
            need(
                RequirementKey::Field(Field::PrincipalId),
                "missing",
                "A relay must identify the person whose own question this is.",
            );
        }
        for field in [
            Field::PrincipalId,
            Field::DealActor,
            Field::DealBeneficiary,
            Field::Seller,
            Field::Sender,
            Field::DealParty,
        ] {
            if !actor_reference_field(field, Some(frame.method)) {
                continue;
            }
            if let Some(id) = self.text(field) {
                if id != "querent" && !self.people.contains_key(id) {
                    need(
                        RequirementKey::Field(field),
                        "unbound_person",
                        "This actor ID must identify an actual participant, not free prose.",
                    );
                } else if id != "querent"
                    && !(principal_mode == Some("relay")
                        && Some(id) == self.text(Field::PrincipalId))
                    && self
                        .people
                        .get(id)
                        .is_some_and(|p| p.relationship == "unknown")
                {
                    need(
                        RequirementKey::PersonRelationship(id.into()),
                        "missing",
                        "This actor's operative relationship changes the role.",
                    );
                }
            }
        }
        for (field, key) in [
            (Field::ReaderPlace, RequirementKey::ChartPlace),
            (Field::QuestionTime, RequirementKey::ChartMoment),
        ] {
            if let Some(slot) = self.facts.get(&field) {
                if slot.resolved().is_none() {
                    need(key,slot.reason(),"An unresolved explicit override must never fall back silently to device defaults.");
                }
            }
        }
        match self.subject.resolved() {
            None => need(
                RequirementKey::Subject,
                self.subject.reason(),
                "The quesited must be identified.",
            ),
            Some(subject) => {
                if self.unbound_named_subject().is_some() {
                    need(
                        RequirementKey::Subject,
                        "unbound_person",
                        "The named person is already supplied but not bound to the person subject. Repair that binding before a reading handoff; do not replace them with an unnamed role.",
                    );
                }
                if self.needs_title_owner() && subject.owner_id.is_empty() {
                    need(
                        RequirementKey::Owner,
                        "missing",
                        "This method's role assignment depends on whose subject this is.",
                    );
                }
                if !subject.owner_id.is_empty()
                    && subject.owner_id != "querent"
                    && frame.method != Method::WorkPerson
                    && (!self.is_deal() || self.needs_title_owner())
                {
                    match self.people.get(&subject.owner_id) {
                        Some(person) if person.relationship != "unknown" => {},
                        _ if principal_mode == Some("relay") && self.text(Field::PrincipalId) == Some(subject.owner_id.as_str()) => {},
                        _ => need(RequirementKey::PersonRelationship(subject.owner_id.clone()), "missing", "This operative capacity affects the significator; do not infer it from a name."),
                    }
                }
            }
        }
        for requirement in card.requirements {
            let mut field = requirement.field;
            let truth = requirement.guard.evaluate(self);
            if truth == Truth::No {
                continue;
            }
            if truth == Truth::Unknown {
                if let Guard::Equals { field: control, .. } = requirement.guard {
                    field = control;
                }
                if matches!(requirement.guard, Guard::PrincipalOwnsSubject) {
                    need(
                        RequirementKey::Owner,
                        "missing",
                        "Resolve ownership before selecting the own-object comparison recipe.",
                    );
                    continue;
                }
            }
            if self.text(field).is_none()
                && !(field == Field::DealActor && self.deal_actor().is_some())
            {
                need(
                    RequirementKey::Field(field),
                    self.facts.get(&field).map_or("missing", Slot::reason),
                    if truth == Truth::Unknown {
                        "Resolve the guard before deciding whether the dependent fact is needed."
                    } else {
                        "Required by the selected book-led reading contract."
                    },
                );
            }
        }
        for extra in &self.additional {
            let satisfied = match &extra.key {
                RequirementKey::Field(field) => {
                    self.text(*field).is_some()
                        || (*field == Field::ReaderPlace && self.device_reader_place)
                }
                RequirementKey::Owner => self
                    .subject
                    .resolved()
                    .is_some_and(|s| !s.owner_id.is_empty()),
                RequirementKey::PersonRelationship(id) => self
                    .people
                    .get(id)
                    .is_some_and(|p| p.relationship != "unknown"),
                _ => false,
            };
            if !satisfied {
                need(extra.key.clone(), "requested_by_reading", &extra.reason);
            }
        }
        if anchor.is_none() {
            need(
                RequirementKey::ChartPlace,
                "native_acquisition_pending",
                "Rust must acquire reader coordinates; a venue is not a fallback.",
            );
        } else if anchor.is_some_and(|a| a.validate().is_err()) {
            need(
                RequirementKey::ChartMoment,
                "invalid_anchor",
                "The resolved native chart anchor is invalid.",
            );
        }
        if plan.limitation.is_none() && matches!(card.coverage, Coverage::ExpertReview) {
            plan.limitation = Some(Limitation { code: "judgment_program_needs_review".into(), message: format!("I have kept the {} question and its context. Its particular judgment method still needs Eileen's review before I can give you an interpretation.", card.title.to_lowercase()), printed_pages: card.printed_pages.into() });
        }
        if plan.limitation.is_none()
            && frame.method == Method::Money
            && self.text(Field::MoneySource) == Some("other")
        {
            plan.limitation=Some(Limitation{code:"money_source_needs_review".into(),message:"This source of money needs a more specific role assignment before I can interpret it. I have kept the question for review.".into(),printed_pages:card.printed_pages.into()});
        }
        plan
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Anchor {
    pub timestamp_ms: f64,
    pub latitude: f64,
    pub longitude: f64,
    pub timezone: String,
}
impl Anchor {
    pub fn validate(&self) -> Result<(), String> {
        if !self.timestamp_ms.is_finite()
            || !self.latitude.is_finite()
            || self.latitude.abs() >= 90.
            || !self.longitude.is_finite()
            || self.longitude.abs() > 180.
        {
            return Err("A reading needs native UTC and usable coordinates.".into());
        }
        horary_ai_core::chart_input::local_clock(self.timestamp_ms, &self.timezone).map(|_| ())
    }
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct Binding {
    pub catalogue_version: String,
    pub case_revision: u64,
    pub question: String,
    pub frame: Frame,
    pub input_sha256: String,
}

/// There is no Deserialize or public constructor for this permit. Every handoff
/// is re-evaluated against the catalogue and frozen before a worker receives it.
#[derive(Clone, Serialize)]
pub struct ReadyReading {
    binding: Binding,
    anchor: Anchor,
    subject: Subject,
    people: BTreeMap<String, Person>,
    inputs: BTreeMap<Field, String>,
    method: &'static Contract,
}
impl ReadyReading {
    pub fn prepare(case: &Consultation, anchor: Anchor) -> Result<Self, Plan> {
        let plan = case.plan(Some(&anchor));
        if case.catalogue_version != VERSION || !plan.needs.is_empty() || plan.limitation.is_some()
        {
            return Err(plan);
        }
        let frame = case
            .frame
            .resolved()
            .expect("Plan checked the frame")
            .clone();
        let inputs: BTreeMap<_, _> = case
            .facts
            .iter()
            .filter(|(f, _)| relevant_input(case, **f))
            .filter_map(|(f, s)| s.resolved().map(|v| (*f, v.clone())))
            .collect();
        let question = case
            .question
            .resolved()
            .expect("Plan checked the question")
            .clone();
        let subject = case
            .subject
            .resolved()
            .expect("Plan checked the subject")
            .clone();
        let people: BTreeMap<_, _> = case
            .people
            .iter()
            .filter(|(id, _)| {
                id.as_str() == subject.owner_id
                    || [
                        Field::PrincipalId,
                        Field::DealActor,
                        Field::DealBeneficiary,
                        Field::Seller,
                        Field::DealParty,
                        Field::Sender,
                    ]
                    .iter()
                    .any(|field| inputs.get(field).is_some_and(|actor| actor == *id))
            })
            .map(|(id, person)| (id.clone(), person.clone()))
            .collect();
        let hash = crate::horary_lessons::digest(&json!({"version":VERSION,"question":question,"frame":frame,"subject":subject,"people":people,"inputs":inputs,"anchor":anchor}).to_string());
        Ok(Self {
            binding: Binding {
                catalogue_version: VERSION.into(),
                case_revision: case.revision,
                question,
                frame: frame.clone(),
                input_sha256: hash,
            },
            anchor,
            subject,
            people,
            inputs,
            method: contract(frame.method),
        })
    }
    pub fn binding(&self) -> &Binding {
        &self.binding
    }
    pub fn input(&self) -> Value {
        serde_json::to_value(self).expect("Contract values are finite and serializable")
    }
    pub fn brief(&self) -> crate::horary_contract::Brief {
        let text = |field| self.inputs.get(&field).cloned().unwrap_or_default();
        crate::horary_contract::Brief {
            question: self.binding.question.clone(),
            matter: matter(self.binding.frame.method),
            question_kind: self.binding.frame.facet.name().into(),
            context: text(Field::Context),
            people: self.people.values().cloned().collect(),
            subject: self.subject.clone(),
            event_place: text(Field::EventPlace),
            event_time: text(Field::EventTime),
            horizon: text(Field::Horizon),
            ..Default::default()
        }
    }
    pub fn validate_current(&self, case: &Consultation) -> Result<(), String> {
        let current = Self::prepare(case, self.anchor.clone())
            .map_err(|_| "The consultation is no longer ready")?;
        if current.binding != self.binding {
            return Err(
                "A late reading result belongs to an older consultation input; do not publish it."
                    .into(),
            );
        }
        Ok(())
    }
}

/// Facts survive a reclassification in the consultation; only applicable
/// inputs and operative participants enter the selected reading program.
fn relevant_input(case: &Consultation, field: Field) -> bool {
    use Field::*;
    if matches!(
        field,
        PrincipalMode
            | PrincipalId
            | Context
            | ReaderPlace
            | QuestionTime
            | TimeOccurrence
            | EventPlace
            | EventTime
            | Horizon
    ) {
        return true;
    }
    let Some(method) = case.method() else {
        return false;
    };
    if contract(method)
        .requirements
        .iter()
        .any(|r| r.field == field && r.guard.evaluate(case) == Truth::Yes)
        || case
            .additional
            .iter()
            .any(|need| need.key == RequirementKey::Field(field))
    {
        return true;
    }
    match method {
        Method::LostObject | Method::LostAnimal | Method::MissingPerson => {
            matches!(field, Description | TheftRaised | SearchContext)
        }
        Method::MovableDeal | Method::Property | Method::Rental | Method::BusinessProperty => {
            matches!(
                field,
                DealCapacity | DealActor | DealBeneficiary | Seller | DealParty | Priorities
            )
        }
        Method::NewJob | Method::ExistingJob | Method::ReturnToJob | Method::JobOffer => {
            matches!(field, JobContext | Priorities)
        }
        _ => false,
    }
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(tag = "result", rename_all = "snake_case")]
pub enum ReadingResult {
    Judgment {
        binding: Binding,
        verdict: String,
        answer: String,
        evidence: Vec<String>,
        worksheet: Value,
    },
    NeedsInformation {
        need: InformationNeed,
    },
    Limited {
        limitation: LimitationRecord,
    },
}
#[derive(Clone, Deserialize, Serialize)]
pub struct LimitationRecord {
    pub code: String,
    pub message: String,
    pub printed_pages: String,
}
impl From<&Limitation> for LimitationRecord {
    fn from(v: &Limitation) -> Self {
        Self {
            code: v.code.clone(),
            message: v.message.clone(),
            printed_pages: v.printed_pages.clone(),
        }
    }
}

pub fn matter(method: Method) -> Matter {
    match method {
        Method::Relationship => Matter::Relationship,
        Method::LostObject => Matter::LostObject,
        Method::LostAnimal | Method::MissingPerson => Matter::LostAnimal,
        Method::NewJob
        | Method::ExistingJob
        | Method::ReturnToJob
        | Method::JobOffer
        | Method::WorkPerson => Matter::Work,
        Method::Money | Method::Investment | Method::Bet => Matter::Money,
        Method::Property | Method::Rental | Method::BusinessProperty => Matter::Property,
        _ => Matter::Other,
    }
}

/// Old saved readings remain inspectable. Their facts are retained with a
/// migration receipt; their inferred method is proposed, never silently ready.
pub fn migrate(brief: &crate::horary_contract::Brief) -> Consultation {
    let mut case = Consultation::default();
    let proof = Evidence::Migration {
        detail: "Retained pre-catalogue brief; method must be reclassified before judgment.".into(),
    };
    if !brief.question.is_empty() {
        case.question = Slot::Resolved {
            observation: Observation {
                value: brief.question.clone(),
                evidence: proof.clone(),
            },
        };
    }
    if !brief.subject.is_empty() {
        case.subject = Slot::Resolved {
            observation: Observation {
                value: brief.subject.clone(),
                evidence: proof.clone(),
            },
        };
    }
    for p in &brief.people {
        case.people.insert(p.id.clone(), p.clone());
    }
    for (field, value) in [
        (Field::Context, &brief.context),
        (Field::ReaderPlace, &brief.place_request),
        (Field::QuestionTime, &brief.time_request),
        (Field::EventPlace, &brief.event_place),
        (Field::EventTime, &brief.event_time),
        (Field::Horizon, &brief.horizon),
    ] {
        if !value.is_empty() {
            case.facts.insert(
                field,
                Slot::Resolved {
                    observation: Observation {
                        value: value.clone(),
                        evidence: proof.clone(),
                    },
                },
            );
        }
    }
    case
}

/// Analysis workers consume this compatibility projection; they never write it.
/// The consultation remains the only mutable authority for semantic facts.
pub fn brief(case: &Consultation, turn: &Turn) -> crate::horary_contract::Brief {
    let text = |field| case.text(field).unwrap_or("").to_string();
    crate::horary_contract::Brief {
        intent: turn.intent.name().into(),
        question: case.question.resolved().cloned().unwrap_or_default(),
        matter: case.method().map_or(Matter::Other, matter),
        question_kind: case
            .frame
            .resolved()
            .map_or("event", |f| f.facet.name())
            .into(),
        context: text(Field::Context),
        people: case.people.values().cloned().collect(),
        subject: case.subject.resolved().cloned().unwrap_or_default(),
        event_place: text(Field::EventPlace),
        event_time: text(Field::EventTime),
        place_request: text(Field::ReaderPlace),
        time_request: text(Field::QuestionTime),
        horizon: text(Field::Horizon),
        clarification: String::new(),
        focus: turn.focus.clone(),
        heard: turn.heard.clone(),
        restore_revision: turn.restore_revision,
    }
}

pub fn control(intent: Intent) -> Turn {
    Turn {
        intent,
        question: None,
        frame: None,
        people: vec![],
        subject: None,
        updates: vec![],
        heard: String::new(),
        unavailable_quote: String::new(),
        focus: "judgment".into(),
        restore_revision: None,
    }
}

/// A standalone first-person declaration may follow the actual question.
fn unquoted_sentence_starts(words: &str) -> Vec<usize> {
    let mut starts = vec![0];
    let mut quoted_until = None;
    for (at, ch) in words.char_indices() {
        let previous = words[..at].chars().next_back();
        let next = words[at + ch.len_utf8()..].chars().next();
        let apostrophe =
            previous.is_some_and(char::is_alphanumeric) && next.is_some_and(char::is_alphanumeric);
        if let Some(end) = quoted_until {
            if ch == end && !(matches!(ch, '\'' | '’') && apostrophe) {
                quoted_until = None;
            }
            continue;
        }
        match ch {
            '"' => quoted_until = Some('"'),
            '“' => quoted_until = Some('”'),
            '‘' => quoted_until = Some('’'),
            '\'' if !apostrophe && !previous.is_some_and(char::is_alphanumeric) => {
                quoted_until = Some('\'')
            }
            '.' | '?' | '!' | '\n' => starts.push(at + ch.len_utf8()),
            _ => {}
        }
    }
    starts
}

/// A narrow source-defined obligation, independent of the model's worksheet.
/// Direct question-anchor instructions differ from event places and dates.
struct HistoricalAnchorRequest<'a> {
    clause: &'a str,
    supplied_place: bool,
    supplied_time: bool,
}

fn historical_anchor_request(words: &str) -> Option<HistoricalAnchorRequest<'_>> {
    let starts = unquoted_sentence_starts(words);
    for (index, at) in starts.iter().enumerate() {
        let before = &words[..*at];
        let header = before
            .rsplit("\n\n")
            .next()
            .unwrap_or(before)
            .lines()
            .rev()
            .find(|line| !line.trim().is_empty())
            .unwrap_or("")
            .trim();
        if header.ends_with(':')
            && [
                "said", "says", "asked", "asks", "wrote", "writes", "told", "reported",
            ]
            .iter()
            .any(|cue| crate::horary_role_options::mentions(header, cue))
        {
            continue;
        }
        let end = starts.get(index + 1).copied().unwrap_or(words.len());
        let clause = words[*at..end].trim();
        let lower = clause.to_ascii_lowercase();
        let imperative = lower.strip_prefix("please ").unwrap_or(&lower);
        let prefixes = [
            "use my earlier consultation",
            "use the earlier consultation",
            "use my original consultation",
            "use the question i understood",
            "use the question i first understood",
        ];
        let Some(prefix) = prefixes.iter().find(|prefix| {
            imperative.starts_with(**prefix)
                && imperative[prefix.len()..]
                    .chars()
                    .next()
                    .is_none_or(|ch| !ch.is_alphanumeric())
        }) else {
            continue;
        };
        let tail = &imperative[prefix.len()..];
        let months = [
            "january",
            "february",
            "march",
            "april",
            "may",
            "june",
            "july",
            "august",
            "september",
            "october",
            "november",
            "december",
        ];
        let supplied_place = [" in ", " from "].iter().any(|cue| {
            tail.split_once(*cue).is_some_and(|(_, rest)| {
                let rest = rest.trim();
                let period = rest.trim_end_matches(['.', '?', '!']);
                let date_words = period.split_whitespace().collect::<Vec<_>>();
                let temporal_tail = date_words.first().is_some_and(|word|
                    ["yesterday", "today", "tomorrow"].contains(word))
                    || matches!(date_words.as_slice(), [month, year, ..]
                        if months.contains(month) && year.trim_matches(|ch: char| !ch.is_ascii_digit()).len() == 4
                            && year.trim_matches(|ch: char| !ch.is_ascii_digit()).chars().all(|ch| ch.is_ascii_digit()));
                !temporal_tail
                    && !rest.is_empty()
                    && !rest.starts_with(char::is_numeric)
                    && ![
                        "the morning",
                        "the afternoon",
                        "the evening",
                        "morning",
                        "afternoon",
                        "evening",
                    ]
                    .iter()
                    .any(|period| rest.starts_with(period))
            })
        });
        let has_clock = tail.split_whitespace().any(|word| {
            let clock = word.trim_matches(|ch: char| !ch.is_ascii_digit() && ch != ':');
            clock.split_once(':').is_some_and(|(hour, minute)| {
                !hour.is_empty()
                    && !minute.is_empty()
                    && hour.chars().all(|ch| ch.is_ascii_digit())
                    && minute.chars().all(|ch| ch.is_ascii_digit())
            })
        });
        let tokens: Vec<_> = tail
            .split(|ch: char| !ch.is_ascii_alphanumeric())
            .filter(|word| !word.is_empty())
            .collect();
        let has_year = tokens
            .iter()
            .any(|part| part.len() == 4 && part.chars().all(|ch| ch.is_ascii_digit()));

        let has_month_day = tokens.iter().enumerate().any(|(index, word)| {
            months.contains(word)
                && [index.checked_sub(1), index.checked_add(1)]
                    .into_iter()
                    .flatten()
                    .any(|next| {
                        tokens.get(next).is_some_and(|day| {
                            day.parse::<u8>().is_ok_and(|day| (1..=31).contains(&day))
                        })
                    })
        });
        let has_iso_date = tail.split(|ch: char| !ch.is_ascii_digit() && ch != '-').any(|word| {
            let parts: Vec<_> = word.split('-').collect();
            matches!(parts.as_slice(), [year, month, day] if year.len() == 4 && month.len() == 2 && day.len() == 2
                && parts.iter().all(|part| part.chars().all(|ch| ch.is_ascii_digit())))
        });
        return Some(HistoricalAnchorRequest {
            clause,
            supplied_place,
            supplied_time: has_clock && ((has_year && has_month_day) || has_iso_date),
        });
    }
    None
}

fn original_question_words(case: &Consultation) -> Option<&str> {
    match &case.question {
        Slot::Resolved { observation } | Slot::Proposed { observation } => {
            match &observation.evidence {
                Evidence::User { quote, .. } | Evidence::RetainedQuestion { quote } => Some(quote),
                _ => Some(&observation.value),
            }
        }
        _ => None,
    }
}

fn anchor_source_quote(evidence: &Evidence) -> Option<&str> {
    match evidence {
        Evidence::User { quote, .. } | Evidence::RetainedQuestion { quote } => Some(quote),
        Evidence::NativePlace { original, .. } => anchor_source_quote(original),
        _ => None,
    }
}

fn field_sourced_in(case: &Consultation, field: Field, clause: &str) -> bool {
    matches!(case.facts.get(&field), Some(Slot::Resolved { observation })
        if anchor_source_quote(&observation.evidence)
            .is_some_and(|quote| !quote.trim().is_empty() && clause.contains(quote)))
}

/// Complete supplied anchor components privately; genuinely absent ones are
/// retained as blocking requirements for the conversational reader.
pub fn validate_anchor_completion(
    case: &Consultation,
    turn: &Turn,
    words: &str,
) -> Result<(), String> {
    let current = historical_anchor_request(words);
    let retained = original_question_words(case).and_then(historical_anchor_request);
    let Some(request) = current.as_ref().or(retained.as_ref()) else {
        return Ok(());
    };
    let mut missing = Vec::new();
    for (field, supplied) in [
        (Field::ReaderPlace, request.supplied_place),
        (Field::QuestionTime, request.supplied_time),
    ] {
        let satisfied = if current.is_some() {
            field_sourced_in(case, field, request.clause)
        } else {
            // Retained instructions cannot outvote later validated corrections.
            case.text(field).is_some() || (field == Field::ReaderPlace && case.device_reader_place)
        };
        if supplied
            && !satisfied
            && !turn.updates.iter().any(|update| {
                let answers_pending_anchor = match &case.requested {
                    Some(RequirementKey::Field(requested)) => *requested == field,
                    Some(RequirementKey::ChartPlace) => field == Field::ReaderPlace,
                    Some(RequirementKey::ChartMoment) => field == Field::QuestionTime,
                    _ => false,
                };
                let authorized_current_answer = current.is_none()
                    && words.contains(&update.quote)
                    && (turn.intent == Intent::Correct || answers_pending_anchor);
                update.field == field
                    && update.mode != UpdateMode::Unavailable
                    && !update.quote.trim().is_empty()
                    && (request.clause.contains(&update.quote) || authorized_current_answer)
            })
        {
            missing.push(field.name());
        }
    }
    if missing.is_empty() {
        return Ok(());
    }
    Err(format!("Explicit historical chart-anchor information was omitted: {}. Extract those fields from the actual instruction {:?}, preserving exact source quotes. Do not replace this earlier consultation with device location or now; do not ask for information already present. Keep the unchanged question, subject and other facts. A genuinely unstated component stays missing for the native anchor need.", missing.join(", "), request.clause))
}

fn recoverable_original_anchor(case: &Consultation, field: Field, quote: &str) -> bool {
    matches!(field, Field::ReaderPlace | Field::QuestionTime)
        && !(field == Field::ReaderPlace && case.device_reader_place)
        && case.text(field).is_none()
        && !quote.trim().is_empty()
        && original_question_words(case)
            .and_then(historical_anchor_request)
            .is_some_and(|request| request.clause.contains(quote))
}

fn record_anchor_obligation(case: &mut Consultation, words: &str) {
    let Some(request) = historical_anchor_request(words) else {
        return;
    };
    case.device_reader_place = false;
    for field in [Field::ReaderPlace, Field::QuestionTime] {
        // Do not silently reuse an unrelated old anchor for a new instruction.
        if !field_sourced_in(case, field, request.clause) {
            if let Some(slot @ Slot::Resolved { .. }) = case.facts.get_mut(&field) {
                if let Slot::Resolved { observation } = slot.clone() {
                    *slot = Slot::Proposed { observation };
                }
            }
        }
        let key = RequirementKey::Field(field);
        if case.text(field).is_none() && !case.additional.iter().any(|need| need.key == key) {
            case.additional.push(InformationNeed {
                key,
                reason: format!("The person explicitly selected an earlier consultation: {:?}. Its reader place and understood moment must be resolved; device/now defaults do not establish them. Frawley printed pp. 7–8.", request.clause),
                question: Some(if field == Field::ReaderPlace { "Where were you when that earlier question became clear?" } else { "When did that earlier question become clear, including its local time?" }.into()),
            });
        }
    }
}

/// Quoted speech and ambiguous compound declarations remain with recognition.
pub fn reader_place_statement(words: &str) -> Option<Update> {
    let words = words.trim();
    let starts = unquoted_sentence_starts(words);
    let prefixes = ["i'm asking from ", "i am asking from ", "i’m asking from "];
    let declarations: Vec<_> = starts
        .iter()
        .filter_map(|at| {
            let before = &words[..*at];
            let paragraph = before.rsplit("\n\n").next().unwrap_or(before);
            let header = paragraph.lines().next().unwrap_or("").trim();
            if header.ends_with(':')
                && [
                    "said", "says", "asked", "asks", "wrote", "writes", "told", "reported",
                ]
                .iter()
                .any(|cue| crate::horary_role_options::mentions(header, cue))
            {
                return None; // A colon-led report is somebody's words, not this reader's declaration.
            }
            let statement = words[*at..].trim();
            let lower = statement.to_ascii_lowercase();
            prefixes
                .iter()
                .find(|prefix| lower.starts_with(**prefix))
                .map(|prefix| (statement, prefix.len()))
        })
        .collect();
    let [(statement, prefix_len)] = declarations.as_slice() else {
        return None;
    };
    let place = statement[*prefix_len..].trim().trim_end_matches(['.', '!']);
    let lower = place.to_ascii_lowercase();
    // Complete deictic phrases request the current device anchor, not a city
    // named "here, now". Keep explicit names such as Hereford or Here, Kansas.
    let deictic_words: Vec<_> = lower
        .split(|ch: char| ch.is_whitespace() || ch == ',')
        .filter(|word| !word.is_empty())
        .collect();
    let device = matches!(
        deictic_words.as_slice(),
        ["here"]
            | ["here", "now"]
            | ["right", "here"]
            | ["right", "here", "now"]
            | ["right", "here", "right", "now"]
            | ["here", "at", "the", "moment"]
            | ["where", "i", "am"]
            | ["where", "i", "am", "now"]
            | ["where", "i", "am", "right", "now"]
            | ["where", "i", "currently", "am"]
            | ["my", "present", "location"]
    ) || crate::horary_role_options::mentions(place, "my device")
        || crate::horary_role_options::mentions(place, "this device")
        || crate::horary_role_options::mentions(place, "current location");
    let extra_sentence = place.match_indices('.').any(|(at, _)| {
        let suffix = &place[at + 1..];
        let prefix_word = place[..at].split_whitespace().next_back().unwrap_or("");
        !suffix.trim().is_empty()
            && suffix.starts_with(char::is_whitespace)
            && !matches!(
                prefix_word.to_ascii_lowercase().as_str(),
                "st" | "mt" | "ft" | "u" | "s"
            )
    });
    if place.is_empty()
        || place.len() > 700
        || place.contains(['?', ';', '\n', '"', '“', '”', '‘'])
        || place.ends_with(['\'', '’'])
        || device
        || extra_sentence
    {
        return None;
    }
    Some(Update {
        field: Field::ReaderPlace,
        value: place.into(),
        quote: (*statement).into(),
        mode: UpdateMode::Supply,
    })
}

pub fn need_key(field: &str, case: &Consultation) -> Result<RequirementKey, String> {
    match field {
        "chart_place" => Ok(RequirementKey::ChartPlace),
        "chart_moment" => Ok(RequirementKey::ChartMoment),
        "ownership" => Ok(RequirementKey::Owner),
        "subject_relationship" => case.subject.resolved().filter(|s|!s.owner_id.is_empty() && s.owner_id!="querent").map(|s|RequirementKey::PersonRelationship(s.owner_id.clone())).ok_or_else(||"A relationship request must identify the actual participant.".into()),
        "scope" => Ok(RequirementKey::Field(Field::Horizon)),
        _ => serde_json::from_value::<Field>(Value::String(field.into())).map(RequirementKey::Field).map_err(|_|"Request a named factual input from the catalogue; internal chart data is the controller's responsibility.".into()),
    }
}

impl Consultation {
    pub fn require_information(&mut self, key: RequirementKey, reason: String) {
        if matches!(
            key,
            RequirementKey::ChartPlace | RequirementKey::ChartMoment
        ) {
            self.requested = Some(key);
            return;
        }
        if let RequirementKey::Field(field) = key {
            // An already known general context is not automatically the new
            // detail requested by the reader. Preserve it while reopening it.
            if let Some(slot) = self.facts.get_mut(&field) {
                if let Slot::Resolved { observation } = slot {
                    *slot = Slot::Proposed {
                        observation: observation.clone(),
                    };
                }
            }
        }
        if !self.additional.iter().any(|n| n.key == key) {
            self.additional.push(InformationNeed {
                key: key.clone(),
                reason,
                question: None,
            });
        }
        self.requested = Some(key);
    }
}

#[cfg(test)]
pub fn documentation() -> String {
    let mut out = include_str!("reading_contract_overview.md").replace("{VERSION}", VERSION);
    for card in CATALOGUE {
        out.push_str(&format!("### {} (`{}`)\n\nFrawley, *The Horary Textbook*, printed pp. {}. Coverage: `{:?}`. Owner requirement: `{:?}`. Allowed facets: {}.\n\n{}\n\n{}\n\n",card.title,card.method.name(),card.printed_pages,card.coverage,card.owner,card.facets.iter().map(|f|f.name()).collect::<Vec<_>>().join(", "),card.roles,card.judgment));
        if !card.requirements.is_empty() {
            out.push_str("| Input | Applicability | Exact elicitation | Accepted labels |\n|---|---|---|---|\n");
            for r in card.requirements {
                out.push_str(&format!(
                    "| `{}` | `{:?}` | {} | {} |\n",
                    r.field.name(),
                    r.guard,
                    field_prompt_for(Some(card.method), r.field),
                    allowed_values(r.field).join(", ")
                ));
            }
            out.push('\n');
        }
    }
    out.push_str("## All factual fields and response bank\n\n| Field | Exact question | Accepted labels (empty means contextual text) |\n|---|---|---|\n");
    for field in Field::ALL {
        out.push_str(&format!(
            "| `{}` | {} | {} |\n",
            field.name(),
            field_prompt(*field),
            allowed_values(*field).join(", ")
        ));
    }
    out.push_str("\n## Actual first-turn recognition prompt\n\n```text\n");
    out.push_str(&recognition_guide_for(None, "classify_question"));
    out.push_str("```\n\n## Actual selected recognition lessons\n\nA newly selected concrete method verifies its tentative classification and completes fact extraction from the same words before a user inquiry. A changed method receives its own lesson before supplying observations. The focused completion lesson and subsequent conversational update lesson have distinct output instructions. Both are exported below from the same functions used by model dispatch. Wish and Unclassified remain with the classifier until a concrete concern is identified; they do not receive a focused extraction lesson.\n\n");
    for card in CATALOGUE {
        let selected = Consultation {
            frame: Slot::Resolved {
                observation: Observation {
                    value: Frame {
                        method: card.method,
                        facet: card.facets.first().copied().unwrap_or(Facet::Unknown),
                    },
                    evidence: Evidence::Migration {
                        detail: "Documentation selects a method; no user facts supplied".into(),
                    },
                },
            },
            ..Consultation::default()
        };
        let phases: &[&str] = if matches!(card.method, Method::Wish | Method::Unclassified) {
            &["classify_question"]
        } else {
            &["complete_selected_program", "update_selected_program"]
        };
        for phase in phases {
            out.push_str(&format!(
                "### {}: `{phase}`\n\n```text\n{}\n```\n\n",
                card.title,
                recognition_guide_for(Some(&selected), phase)
            ));
        }
    }
    out.push_str("## Shared recognition patch vocabulary\n\nThis base vocabulary is further constrained by the actual recognition phase. The exact phase schemas follow it. Native acceptance also checks source quotes, bindings and authorized frame changes.\n\n```json\n");
    out.push_str(
        &serde_json::to_string_pretty(&turn_schema(None)).expect("Schema is serializable"),
    );
    out.push_str("\n```\n");
    let empty = Consultation::default();
    let selected = Consultation {
        frame: Slot::Resolved {
            observation: Observation {
                value: Frame {
                    method: Method::NewJob,
                    facet: Facet::Event,
                },
                evidence: Evidence::Migration {
                    detail: "Documentation selects a provisional job method".into(),
                },
            },
        },
        ..Consultation::default()
    };
    for (phase, case) in [
        ("classify_question", &empty),
        ("complete_selected_program", &selected),
        ("update_selected_program", &selected),
    ] {
        let input = json!({"recognition_phase": phase, "consultation": case});
        let schema = crate::horary_step::response_schema_for(
            crate::horary_lessons::Stage::Intake,
            crate::horary_lessons::Matter::Other,
            &input,
            &[],
        );
        out.push_str(&format!(
            "\n### Actual `{phase}` schema\n\n```json\n{}\n```\n",
            serde_json::to_string_pretty(&schema).expect("Phase schema is serializable")
        ));
    }
    out
}

/// Disjoint alternatives bind each closed field to its canonical labels.
/// Emit mode before value so the constrained decoder chooses supplied data
/// versus an unavailable reason before it can emit unconstrained text.
fn update_item_schema() -> Value {
    let item = |fields: Vec<&str>, modes: &[&str], value: Value| {
        json!({"type":"object","properties":{
            "field":{"type":"string","enum":fields},
            "mode":{"type":"string","enum":modes},
            "value":value,
            "quote":{"type":"string","maxLength":700}
        },"required":["field","mode","value","quote"],"additionalProperties":false})
    };
    let available_modes: Vec<_> = UpdateMode::ALL
        .iter()
        .filter(|mode| **mode != UpdateMode::Unavailable)
        .map(|mode| mode.name())
        .collect();
    let text_value = json!({"type":"string","maxLength":700});
    let mut alternatives = Vec::new();
    let mut open_fields = Vec::new();
    for field in Field::ALL {
        let labels = allowed_values(*field);
        if labels.is_empty() {
            open_fields.push(field.name());
        } else {
            alternatives.push(item(
                vec![field.name()],
                &available_modes,
                json!({"type":"string","enum":labels}),
            ));
        }
    }
    alternatives.push(item(open_fields, &available_modes, text_value.clone()));
    // An explicit inability remains a free-text reason, not a fabricated label.
    alternatives.push(item(
        Field::ALL.iter().map(|field| field.name()).collect(),
        &[UpdateMode::Unavailable.name()],
        text_value,
    ));
    json!({"oneOf":alternatives})
}

fn canonical_labels_guide(case: Option<&Consultation>) -> String {
    let mut fields =
        std::collections::BTreeSet::from([Field::PrincipalMode, Field::TimeOccurrence]);
    if let Some(method) = case.and_then(Consultation::method) {
        fields.extend(contract(method).requirements.iter().map(|need| need.field));
    }
    if let Some(case) = case {
        if let Some(RequirementKey::Field(field)) = case.requested.as_ref() {
            fields.insert(*field);
        }
        fields.extend(case.facts.keys().copied());
        fields.extend(case.additional.iter().filter_map(|need| match &need.key {
            RequirementKey::Field(field) => Some(*field),
            _ => None,
        }));
    }
    let mut out = String::from("CANONICAL UPDATE LABELS. Return each update in field, mode, value, quote order. Supplied/corrected/proposed closed fields use exactly these value labels; spoken synonyms belong unchanged in quote. Unavailable is only an explicit inability to answer a known pending need and uses a reason, not a guessed label. Open contextual fields remain text.\n");
    for field in fields {
        let labels = allowed_values(field);
        if !labels.is_empty() {
            out.push_str(&format!("{}: {}\n", field.name(), labels.join(" | ")));
        }
    }
    out.push_str("For a repeated civil clock only, first or earlier maps to value=earlier; second or later maps to value=later. Preserve the exact utterance in quote. Never discard a genuinely supplied occurrence merely because its spoken word is not the canonical label. An event happening this morning or earlier is not an occurrence choice.\n\n");
    out
}

/// The shared update vocabulary permits corrections/reclassification in one
/// turn. The selected program supplies instructions and applicable requirements.
pub fn turn_schema(_case: Option<&Consultation>) -> Value {
    // All first-turn applicable fields are available when a turn reclassifies.
    // The model still has an explicitly selected, much smaller instruction card.
    let enum_strings = |names: Vec<&str>| json!({"type":"string","enum":names});
    let person = json!({"type":"object","properties":{"id":{"type":"string","maxLength":40},"label":{"type":"string","maxLength":80},"relationship":{"type":"string","enum":["unknown","partner","child","sibling","friend","mother","father","employer","employee","other_party","neighbor","querent"]},"source_quote":{"type":"string","maxLength":240}},"required":["id","label","relationship","source_quote"],"additionalProperties":false});
    let subject = json!({"type":"object","properties":{"name":{"type":"string","maxLength":80,"description":"Preserve the actual target. An unnamed Relationship prospective partner may be named Prospective partner; person_role uses exactly Future marriage partner."},"kind":{"type":"string","enum":["person","person_role","movable","money","property","job","small_animal","large_animal","animal","other"],"description":"person is an identified target or an unnamed prospective partner in Relationship. person_role is only the explicitly unnamed future marriage partner in PersonDescription."},"owner_id":{"type":"string","maxLength":40,"description":"For an identified person use that target's ID. An unnamed Relationship prospective partner has an empty owner_id: no partner identity is known. For person_role this binds the principal whose future spouse is described, never an invented spouse ID."},"source_quote":{"type":"string","maxLength":240}},"required":["name","kind","owner_id","source_quote"],"additionalProperties":false});
    json!({"type":"object","properties":{
        "intent":enum_strings(Intent::ALL.iter().map(|i|i.name()).collect()),
        "question":{"type":["string","null"],"maxLength":500},
        "frame":{"oneOf":[{"type":"null"},{"type":"object","properties":{"method":enum_strings(Method::ALL.iter().map(|m|m.name()).collect()),"facet":enum_strings(Facet::ALL.iter().map(|f|f.name()).collect())},"required":["method","facet"],"additionalProperties":false}]},
        "people":{"type":"array","items":person,"maxItems":3},
        "subject":{"oneOf":[{"type":"null"},subject]},
        "updates":{"type":"array","maxItems":16,"items":update_item_schema()},
        "heard":{"type":"string","maxLength":1000},"unavailable_quote":{"type":"string","maxLength":240},"focus":{"type":"string","enum":["roles","condition","reception","contacts","location","timing","judgment","place","moment"]},
        "restore_revision":{"type":["integer","null"],"minimum":1}
    },"required":["intent","question","frame","people","subject","updates","heard","unavailable_quote","focus","restore_revision"],"additionalProperties":false})
}

/// Compatibility entry point for the normal selected-program update lesson.
/// Actual dispatch supplies its recognition phase explicitly.
pub fn recognition_guide(case: Option<&Consultation>) -> String {
    recognition_guide_for(case, "update_selected_program")
}

pub fn recognition_guide_for(case: Option<&Consultation>, phase: &str) -> String {
    let focused = phase == "complete_selected_program";
    let method = case
        .and_then(Consultation::method)
        .filter(|method| !matches!(method, Method::Wish | Method::Unclassified));
    if phase == "classify_question" || !focused && method.is_none() {
        return classification_guide();
    }
    let mut text = String::from("You are a private extraction worker supporting a conversational horary reader. Return only the supplied Turn JSON. You do not speak to the person, ask them questions, choose coordinates, cast a chart, assign planets, or judge an outcome. Rust keeps the fact record; a separate conversational reader speaks naturally about any genuine gap.\n\n");
    if focused {
        text.push_str("PHASE: complete_selected_program. Classification has already run. Your task is to VERIFY its tentative frame and extract this selected program's inputs from the SAME actual words.\n1. Return intent=clarify, question=null, heard='', unavailable_quote='', restore_revision=null. Do not return read, correct, new_question or another control command in this phase.\n2. The stored frame is a tentative classification, not an accepted user fact. Check it against the actual requested predicate. If correct, return frame=null. A same-method facet refinement may accompany facts without requiring the person to correct your classification. WILL an event happen within a period is event plus horizon; WHEN it happens is timing. Preserve an exact numerical goal as quantity.\n3. If the METHOD is wrong, return its corrected frame ONLY: subject=null, people=[], updates=[]. The controller must run that method's lesson before you can supply its facts. A changed method is not permission to change the original question.\n4. For the correct selected method, extract the actual subject, people and supplied observations now. Accepted factual slots remain authoritative; the tentative frame does not make missing facts present. subject=null means the accepted subject is unchanged. When the subject slot is missing and the words identify the target, supply it. Do not leave known facts for the conversational reader to ask again.\n5. Leave genuinely absent inputs missing, with no invented quote or default. Examples below are separate hypothetical inputs, never evidence about the actual person. An unstated relationship baseline remains absent. There is no request_input action in this Turn schema; missing facts become native reminders for the conversational reader.\n\n");
    } else {
        text.push_str("PHASE: update_selected_program. Read the retained consultation, latest_words, pending_requirement and last_reader_question. Return only new observations or explicitly corrected facts. question/frame/subject are null when accepted values are unchanged; people and updates are empty when unchanged. Never reconstruct the whole brief. A missing subject is not an unchanged subject: supply a clear target or newly supplied identity.\nUse clarify for new information on the same concern; correct only for an explicit factual correction or a clearly accepted single reframing; explain for a request to explain; resume to continue; pause to stop; use_device to request actual device location. A different matter uses new_question with only question/frame and subject=null, people=[], updates=[]; its own lesson then gathers its facts. Do not interpret explaining or resuming as a finished reading.\nIf last_reader_question offered ONE concrete reframing and latest_words clearly accept it, return correct with that agreed question/frame. Preserve unchanged people, subject and observations. An unaccepted suggestion has no authority; ambiguous yes or several offered alternatives require conversational clarification. Do not infer personal capacity or ownership from yes.\nAn explicit inability to answer the pending requirement uses unavailable_quote copied exactly from latest_words. Otherwise leave it empty. For direct audio heard is a faithful short meaning summary preserving negation, numbers, names, place/time and uncertainty, not a claimed transcript. Typed input uses heard=''.\n\n");
    }
    text.push_str("SOURCE AUTHORITY. Supply exact quotes from actual current words or the retained ORIGINAL question when an omitted observation is being recovered. Never quote another matter, an editorial example or a rejected worksheet. In repair, original_input holds the accepted consultation and actual words; previous_worksheet was REJECTED and none of its proposed changes were saved. Fix the native error using actual source evidence, not the rejected answer as a fact. A name alone has personal relationship=unknown. Seller is a task role, not ownership or personal relationship. Actor fields principal_id, deal_actor, deal_beneficiary, seller, deal_party and a relative-money sender contain known person IDs or querent, not prose. Leave an unspecified deal_party absent; Rust supplies the generic counterparty.\n\nCHART ANCHOR. The reader's place and the moment of understanding anchor the chart (Frawley printed pp. 7–8). Ordinary questions use supplied device coordinates and receipt UTC. CURRENT explicit reader-location or historical-consultation instructions must be extracted as reader_place/question_time; include both supplied components. A market, destination or appointment belongs in event_place/event_time and must not replace the chart anchor. Here/use my device supplies no guessed city. Preserve exact place/time source phrases; native tools own geocoding, time zones and civil-time validation. Calendar values can normalize a future Friday/tomorrow against current_local_clock/current_timezone, while their quotes remain literal. A natural horizon stays a horizon. TimeOccurrence means an explicit first/second occurrence of an ambiguous civil clock, never this morning or earlier/later event chronology.\n\n");
    text.push_str(&canonical_labels_guide(case));
    if let Some(method) = method {
        text.push_str(&crate::recognition_programs::guide(method));
    } else {
        // This is a defensive recovery lesson for a stale or absent selected
        // method. No method-specific observations may be established here.
        text.push_str("No concrete selected method exists. Verify the actual concern and return only a tentative frame, with subject=null, people=[], updates=[]. The native controller must select its concrete lesson before collecting facts.\nCLASSIFICATION CATALOGUE:\n");
        for card in CATALOGUE {
            text.push_str(&format!(
                "{}: {}. {}\n",
                card.method.name(),
                card.title,
                card.roles
            ));
        }
    }
    text.push_str("\nReturn compact JSON only. Editorial examples teach extraction; their words are never source evidence for the actual input.\n");
    text
}

fn classification_guide() -> String {
    let mut text = String::from("You are the private question classifier supporting a conversational horary reader. Return the supplied Turn JSON only. You do not speak to the person, cast a chart, assign planets, or judge an outcome.\n\nWork in this order:\n1. Read latest_words and any retained question. Identify the actual concern and current intent. A fresh substantive concern uses read; a different matter uses new_question. Clarify adds information, correct changes something explicitly, explain asks why, resume continues, pause stops, and use_device requests the actual device location. Do not mistake a symptom, destination, quotation or incidental job mention for the requested outcome.\n2. Preserve the person's literal goal in question. Choose method by the requested outcome and circumstances using the catalogue below, not a keyword alone. If the core concern is missing, use unclassified/unknown (or leave frame null), rather than inventing appearance, romance or danger. 'What about Morgan?' does not tell us what to predict.\n3. Choose facet: WILL it happen=event; HOW things are/feel=situation; WHERE=location; WHEN=timing; WHICH=choice; physical appearance=description; whether a claim is true=truth; financial benefit=profit; safety=safety; HOW MANY or HOW MUCH as an exact tally=quantity. 'Will it happen within a year?' remains event, with a stated horizon for the focused extractor. An exact count must not become event/profit without the person's agreement.\n4. Your first job is classification. Return subject=null, people=[], updates=[]; the selected program will extract its particular facts from the SAME words before any user inquiry. Do not attempt every horary recipe here. A missing subject in this first patch does not authorize a reading: the native contract still requires the focused extractor's actual subject and situational facts.\n5. For direct audio, heard is a short faithful meaning summary retaining negation, numbers, names, place, time and uncertainty, not a claimed transcript. Typed input uses heard=''. For a new matter, don't copy facts from a previous one. No invented context. question/frame are null if unchanged; unavailable_quote is empty unless answering a known pending requirement with an explicit inability.\n6. The chart uses the reader's place and the moment the question is understood (Frawley printed pp. 7–8). A market venue or starting time is event context, not a chart anchor. Native tools own coordinates and civil-time validation. The focused program gets the actual clock/device context; do not make up either.\n7. Return compact JSON, no prose or pretty-printing. In a repair, the earlier assistant object was REJECTED: none of its proposals were saved. Correct the specific native error using original_input, not the rejected proposal as evidence.\n\nCLASSIFICATION CATALOGUE (the selected program teaches the detailed recipe):\n");
    for card in CATALOGUE {
        text.push_str(&format!(
            "{}: {} — printed pp. {}. {}\n",
            card.method.name(),
            card.title,
            card.printed_pages,
            card.roles
        ));
    }
    text.push_str("\nContrasts: a job not yet obtained is new_job; keeping the current post is existing_job; returning to a former post is return_to_job; assessing a job already offered is job_offer. Weather at a wedding is weather, not relationship. A literal parcel is parcel; hearing from someone is contact. Tax paid to the government is tax, not money received from it. A question about an existing relationship's feelings is relationship/situation; a wedding going ahead is relationship/event. A sale's exact unit count stays movable_deal/quantity; undertaking/profit concerns the benefit of an activity rather than tallying its sales.\n\nExamples are editorial classification instructions, not textbook quotations:\n");
    for (words, method, facet) in [
        ("What about Morgan?", Method::Unclassified, Facet::Unknown),
        (
            "They offered me the position; would its hours suit me?",
            Method::JobOffer,
            Facet::Situation,
        ),
        (
            "Will it rain at the picnic next week?",
            Method::Weather,
            Facet::Event,
        ),
        (
            "How many candles will Ren sell at the stall?",
            Method::MovableDeal,
            Facet::Quantity,
        ),
        (
            "I don't want a job; I want to know where my missing passport is.",
            Method::LostObject,
            Facet::Location,
        ),
        (
            "My partner Jamie and I share a home, but things feel distant. How are things between us?",
            Method::Relationship,
            Facet::Situation,
        ),
    ] {
        let mut patch = control(Intent::Read);
        patch.question = Some(words.into());
        patch.frame = Some(Frame { method, facet });
        text.push_str(&format!(
            "INPUT: {words}\nOUTPUT: {}\n",
            serde_json::to_string(&patch).expect("Classifier example serializes")
        ));
    }
    text
}

pub fn method_guide(method: Method) -> String {
    let c = contract(method);
    format!("\n<executable_reading_contract version=\"{VERSION}\" method=\"{}\">\nFrawley, The Horary Textbook, printed pp. {}.\n{}\n{}\nThe supplied reading_request is an immutable, native-checked handoff. Answer its original question and facet using its circumstances. A factual gap is request_input with a catalogue field ID; do not ask for chart data the controller owns. Model judgment is still a proposal, not proof that the source method was followed. Unknown event coverage is not a negative verdict; exact timing/counts need their own verified method.\n</executable_reading_contract>\n", method.name(), c.printed_pages, c.roles, c.judgment)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn selected_examples_obey_the_focused_output_contract() {
        for card in CATALOGUE
            .iter()
            .filter(|card| !matches!(card.method, Method::Wish | Method::Unclassified))
        {
            let case = Consultation {
                frame: Slot::Resolved {
                    observation: Observation {
                        value: Frame {
                            method: card.method,
                            facet: card.facets[0],
                        },
                        evidence: Evidence::Migration {
                            detail: "Prompt regression fixture".into(),
                        },
                    },
                },
                ..Consultation::default()
            };
            let lesson = recognition_guide_for(Some(&case), "complete_selected_program");
            let input =
                json!({"recognition_phase":"complete_selected_program","consultation":case});
            let schema = crate::horary_step::response_schema_for(
                crate::horary_lessons::Stage::Intake,
                crate::horary_lessons::Matter::Other,
                &input,
                &[],
            );
            let mut examples = 0;
            for output in lesson
                .lines()
                .filter_map(|line| line.strip_prefix("OUTPUT: "))
            {
                let value: Value = serde_json::from_str(output).expect("Example must parse");
                crate::horary_contract::validate_shape(&value, &schema).unwrap_or_else(|error| {
                    panic!(
                        "{} lesson contradicts its focused contract: {error}",
                        card.method.name()
                    )
                });
                examples += 1;
            }
            assert!(
                examples > 0,
                "The selected lesson must include checked examples"
            );
            assert_eq!(lesson.matches("SELECTED EXTRACTION PROGRAM:").count(), 1);
        }
    }
    fn turn() -> Turn {
        Turn {
            intent: Intent::Read,
            question: Some("Will I get this job?".into()),
            frame: Some(Frame {
                method: Method::NewJob,
                facet: Facet::Event,
            }),
            people: vec![],
            subject: Some(Subject {
                name: "job".into(),
                kind: "job".into(),
                owner_id: "querent".into(),
                source_quote: "Will I get this job?".into(),
            }),
            updates: vec![],
            heard: String::new(),
            unavailable_quote: String::new(),
            focus: "judgment".into(),
            restore_revision: None,
        }
    }
    fn anchor() -> Anchor {
        Anchor {
            timestamp_ms: 1789387200000.,
            latitude: 38.657,
            longitude: -77.249,
            timezone: "America/New_York".into(),
        }
    }
    #[test]
    fn omitted_facts_survive_and_unsupported_quantity_cannot_become_yes_no() {
        let mut case = Consultation::default();
        let mut first = turn();
        first.frame = Some(Frame {
            method: Method::MovableDeal,
            facet: Facet::Quantity,
        });
        first.subject.as_mut().unwrap().kind = "movable".into();
        case.apply(&first, 1, "Will I get this job?", false)
            .unwrap();
        let mut reply = turn();
        reply.question = None;
        reply.frame = None;
        reply.subject = None;
        case.apply(&reply, 2, "The fair is in Bozeman", false)
            .unwrap();
        assert_eq!(case.question.resolved().unwrap(), "Will I get this job?");
        assert_eq!(
            case.plan(Some(&anchor())).limitation.unwrap().code,
            "unsupported_facet"
        );
        assert!(ReadyReading::prepare(&case, anchor()).is_err());
    }
    #[test]
    fn unknown_guard_is_not_false_and_unavailable_is_not_completed() {
        let mut case = Consultation::default();
        let mut first = turn();
        first.frame = Some(Frame {
            method: Method::Money,
            facet: Facet::Event,
        });
        first.subject.as_mut().unwrap().kind = "money".into();
        case.apply(&first, 1, "Will I get this job?", false)
            .unwrap();
        let plan = case.plan(Some(&anchor()));
        assert!(plan
            .needs
            .iter()
            .any(|n| n.key == RequirementKey::Field(Field::MoneySource)));
        let mut reply = turn();
        reply.question = None;
        reply.frame = None;
        reply.subject = None;
        reply.updates = vec![Update {
            field: Field::MoneySource,
            value: "government".into(),
            quote: "government".into(),
            mode: UpdateMode::Supply,
        }];
        case.apply(&reply, 2, "It is from government", false)
            .unwrap();
        assert!(case
            .plan(Some(&anchor()))
            .needs
            .iter()
            .any(|n| n.key == RequirementKey::Field(Field::Discretionary)));
        reply.updates = vec![Update {
            field: Field::Discretionary,
            value: "I don't know".into(),
            quote: "I don't know".into(),
            mode: UpdateMode::Unavailable,
        }];
        case.apply(&reply, 3, "I don't know", false).unwrap();
        assert_eq!(case.plan(Some(&anchor())).needs[0].state, "unavailable");
        assert!(ReadyReading::prepare(&case, anchor()).is_err());
    }
    #[test]
    fn rejected_patch_is_atomic_and_audio_rejection_precedes_completion() {
        let mut case = Consultation::default();
        let first = turn();
        let before = serde_json::to_value(&case).unwrap();
        assert!(case.apply(&first, 1, "Will I get this job?", true).is_err());
        assert_eq!(serde_json::to_value(&case).unwrap(), before);
        let mut bad = first;
        bad.updates = vec![Update {
            field: Field::EventPlace,
            value: "Bozeman".into(),
            quote: "invented words".into(),
            mode: UpdateMode::Supply,
        }];
        assert!(case.apply(&bad, 1, "Will I get this job?", false).is_err());
        assert_eq!(serde_json::to_value(&case).unwrap(), before);
    }
    #[test]
    fn every_enum_card_and_field_has_executable_documentation() {
        assert_eq!(CATALOGUE.len(), Method::ALL.len());
        for method in Method::ALL {
            assert_eq!(CATALOGUE.iter().filter(|c| c.method == *method).count(), 1);
            assert!(!contract(*method).printed_pages.is_empty());
        }
        for field in Field::ALL {
            assert!(!field_prompt(*field).is_empty());
        }
    }
    #[test]
    fn a_ready_permit_is_invalidated_by_later_context_without_losing_the_question() {
        let mut case = Consultation::default();
        case.apply(&turn(), 1, "Will I get this job?", false)
            .unwrap();
        let ready = ReadyReading::prepare(&case, anchor()).unwrap();
        let mut patch = turn();
        patch.question = None;
        patch.frame = None;
        patch.subject = None;
        patch.updates = vec![Update {
            field: Field::EventTime,
            value: "tomorrow".into(),
            quote: "tomorrow".into(),
            mode: UpdateMode::Supply,
        }];
        case.apply(&patch, 2, "The interview is tomorrow", false)
            .unwrap();
        assert!(ready.validate_current(&case).is_err());
        assert_eq!(
            case.question.resolved(),
            Some(&"Will I get this job?".into())
        );
    }
}

#[cfg(test)]
#[path = "reading_contract_tests.rs"]
mod domain_regressions;
