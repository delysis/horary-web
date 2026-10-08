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

pub const VERSION: &str = "frawley-reading-contracts-2026-10-07.1";

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
    Profit => "profit", Safety => "safety"
});
names!(Field {
    PrincipalMode => "principal_mode", PrincipalId => "principal_id", Context => "context",
    ReaderPlace => "reader_place", QuestionTime => "question_time", TimeOccurrence => "time_occurrence", EventPlace => "event_place",
    EventTime => "event_time", Horizon => "horizon", Baseline => "baseline",
    Description => "description", AnimalKind => "animal_kind", TheftRaised => "theft_raised",
    SearchContext => "search_context", DealCapacity => "deal_capacity", DealParty => "deal_party", Seller => "seller",
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
    User { turn: usize, quote: String },
    Convention { rule: String },
    Migration { detail: String },
    RetainedQuestion { quote: String },
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
    #[serde(default)]
    pub unavailable: Vec<UnavailableNeed>,
}
impl Default for Consultation {
    fn default() -> Self {
        Self { catalogue_version: VERSION.into(), revision: 0, question: Slot::Missing,
            frame: Slot::Missing, people: BTreeMap::new(), subject: Slot::Missing,
            facts: BTreeMap::from([(Field::PrincipalMode, Slot::Resolved { observation: Observation {
                value: "self".into(), evidence: Evidence::Convention { rule: "The speaker is the principal unless relay or ambiguity is stated; Frawley pp. 137–138.".into() }
            }})]), changes: Vec::new(), requested: None, additional: Vec::new(), unavailable:Vec::new() }
    }
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

#[derive(Clone, Copy, Debug, Serialize)]
pub enum OwnerRule {
    None,
    Required,
    Principal,
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
        | Method::Trust
        | Method::PersonDescription => &["person"],
        Method::LostObject | Method::MovableDeal => &["movable"],
        Method::Money | Method::Investment | Method::Bet => &["money", "movable"],
        Method::Property | Method::Rental | Method::BusinessProperty => &["property"],
        Method::LostAnimal => &["small_animal", "large_animal"],
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
    card!(MovableDeal, "Sale or purchase of movable goods", "156–161, 167–172", Required,
        &[required(Field::DealCapacity), when(Field::Seller, Field::DealCapacity, "sell")], DEAL, Implemented,
        "Goods are the relevant owner's second. Seller and owner are distinct. For completion use seller/buyer, not goods/buyer: an unspecified counterparty is the deal actor's seventh, while an identified relative keeps their own operative house. Potential possessions can be second-house goods.",
        "Preserve deal, quality or profit as asked. Non-property opposition may complete with regret. This recipe provides no exact count of books/tulips sold. A venue/date is event context, never the chart anchor."),
    card!(Money, "Payment, debt, gift or grant", "156–161", Required,
        &[required(Field::MoneySource), when(Field::Discretionary, Field::MoneySource, "government"), when(Field::Sender, Field::MoneySource, "relative")],
        &[Facet::Event, Facet::Situation, Facet::Timing, Facet::Profit], Implemented,
        "Customers/spouse: eighth; job or government money: eleventh; known relative's money: their turned second. Preserve entitlement versus discretionary gift.",
        "Arrival requires its own testimony. Amount/quality does not universally require an arrival aspect. Do not turn signs/degrees into an invented exact currency amount."),
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
        "Job and its pay have distinct roles. An offer already available is not a new acquisition.", "Judge quality and stated priorities. Ask priorities only if the answer truly depends on what 'good' means."),
    card!(WorkPerson, "Boss, colleague or subordinate", "224–225", None,
        &[required(Field::WorkCapacity)], EVENT_STATE, Implemented,
        "Co-worker seventh, subordinate sixth, boss tenth when directly asked about. Job/boss collisions need a justified contextual allocation.", "Address the actual work relationship; do not reclassify a friendly colleague as eleventh by habit."),
    card!(Property, "Buying or selling property", "167–171", Required,
        &[required(Field::DealCapacity)], DEAL, Implemented,
        "Ordinary parties first/seventh; specific relative may take their own house. Property fourth, price tenth. Profit is distinct.", "Assess condition, price and completion separately. Opposition may complete a sale; don't apply this exception to an ongoing rental."),
    card!(Rental, "Rental agreement", "170", Required,
        &[required(Field::DealCapacity)], DEAL, Implemented,
        "Modern tenant/landlord deal: first/seventh, not an automatic sixth-house servant.", "An opposition can imply regret in this ongoing relationship. Distinguish an available tenancy's quality from finding one."),
    card!(BusinessProperty, "Property used for business", "170–171", Required,
        &[required(Field::DealCapacity)], DEAL, ExpertReview,
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
    card!(PersonDescription, "Description of a person", "143–145", Required, &[],
        &[Facet::Description, Facet::Situation], ExpertReview,
        "Person's operative relationship fixes their ruler.", "Broad comparative description, not exact height, ethnicity or invented marks. Dedicated descriptive program needs review."),
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
        Field::DealCapacity => "Are you buying, selling, renting, or asking about money from the deal?",
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
        Field::DealCapacity => &["buy", "sell", "rent", "profit", "quality"],
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

fn field_prompt_for(method: Option<Method>, field: Field) -> &'static str {
    match (method, field) {
        (Some(Method::Money), Field::Sender) => {
            "Who is this relative sending the money, and who are they to you?"
        }
        _ => field_prompt(field),
    }
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
            self.facts
        ]);
        if turn.intent == Intent::NewQuestion {
            *self = Self::default();
        }
        if turn.intent == Intent::UseDevice {
            self.facts.remove(&Field::ReaderPlace);
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
            if person.relationship != "unknown" {
                evidence(&person.source_quote)?;
                let lower = person.source_quote.to_lowercase();
                if !crate::horary_role_options::relation_words(&person.relationship)
                    .iter()
                    .any(|w| lower.contains(w))
                {
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
                    "movable" | "property" | "small_animal" | "large_animal"
                )
                && subject.source_quote.split_whitespace().count() == 1
                && subject
                    .source_quote
                    .trim()
                    .eq_ignore_ascii_case(subject.name.trim())
                && self
                    .subject
                    .resolved()
                    .is_none_or(|old| old.owner_id != subject.owner_id)
            {
                return Err("The object's name alone does not establish its owner. Leave owner_id empty unless ownership is explicitly stated; being the seller is not ownership.".into());
            }
            if subject.name.trim().is_empty() {
                return Err("A subject needs a name.".into());
            }
            evidence(&subject.source_quote)?;
            if !subject.owner_id.is_empty()
                && subject.owner_id != "querent"
                && !self.people.contains_key(&subject.owner_id)
            {
                return Err(
                    "Owner must refer to a known participant, never an invented ID.".into(),
                );
            }
            let old = self.subject.resolved();
            let compatible = old.is_none_or(|old| {
                old.name.trim().eq_ignore_ascii_case(subject.name.trim())
                    && old.kind == subject.kind
                    && (old.owner_id.is_empty() || old.owner_id == subject.owner_id)
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
            let proof = evidence(&update.quote)?;
            if matches!(update.field, Field::ReaderPlace | Field::QuestionTime)
                && matches!(proof, Evidence::RetainedQuestion { .. })
            {
                return Err("A reader_place or question_time override must be stated in the CURRENT words, or answer the pending chart-anchor question. Remove this override: quoting a venue or event time from the retained question cannot change the chart anchor during an unrelated clarification.".into());
            }
            if update.value.trim().is_empty() || update.value.len() > 700 {
                return Err("A fact update needs a nonempty bounded value.".into());
            }
            let slot = self.facts.entry(update.field).or_default();
            if update.mode == UpdateMode::Unavailable {
                *slot = Slot::Unavailable {
                    reason: update.value.clone(),
                    evidence: proof,
                };
            } else {
                let allowed = allowed_values(update.field);
                if !allowed.is_empty() && !allowed.contains(&update.value.as_str()) {
                    return Err(format!(
                        "{} must use one of {}",
                        update.field.name(),
                        allowed.join(", ")
                    ));
                }
                let mut value = update.value.clone();
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
        if before
            != json!([
                self.question,
                self.frame,
                self.people,
                self.subject,
                self.facts
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
            Field::Seller,
            Field::Sender,
            Field::DealParty,
        ] {
            if (field == Field::Sender && frame.method != Method::Money)
                || (matches!(field, Field::Seller | Field::DealParty)
                    && !matches!(
                        frame.method,
                        Method::MovableDeal
                            | Method::Property
                            | Method::Rental
                            | Method::BusinessProperty
                    ))
            {
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
                if matches!(card.owner, OwnerRule::Required) && subject.owner_id.is_empty() {
                    need(
                        RequirementKey::Owner,
                        "missing",
                        "This method's role assignment depends on whose subject this is.",
                    );
                }
                if !subject.owner_id.is_empty()
                    && subject.owner_id != "querent"
                    && frame.method != Method::WorkPerson
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
            if self.text(field).is_none() {
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
                RequirementKey::Field(field) => self.text(*field).is_some(),
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
        let principal = if principal_mode == Some("relay") {
            self.text(Field::PrincipalId)
        } else {
            Some("querent")
        };
        let other_owner = self.subject.resolved().is_some_and(|subject| {
            !subject.owner_id.is_empty() && Some(subject.owner_id.as_str()) != principal
        });
        if plan.limitation.is_none()
            && other_owner
            && (matches!(frame.method, Method::Property | Method::Rental)
                || (frame.method == Method::MovableDeal
                    && self.text(Field::DealCapacity) == Some("buy")))
        {
            plan.limitation = Some(Limitation {
                code:"deal_party_frame_needs_review".into(),
                message:"I have kept the people, ownership and question. This deal needs a reviewed distinction between the owner and the person making the purchase or agreement before I can interpret it.".into(),
                printed_pages:card.printed_pages.into(),
            });
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
        Method::MovableDeal | Method::Property | Method::Rental => {
            matches!(field, Seller | DealParty | Priorities)
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

/// A standalone declaration needs no neural inference. Story venues and
/// ambiguous multi-clause utterances deliberately remain with recognition.
pub fn reader_place_statement(words: &str) -> Option<Update> {
    let words = words.trim();
    let lower = words.to_ascii_lowercase();
    let prefix = ["i'm asking from ", "i am asking from ", "i’m asking from "]
        .iter()
        .find(|prefix| lower.starts_with(**prefix))?;
    let place = words[prefix.len()..].trim().trim_end_matches(['.', '!']);
    if place.is_empty() || place.len() > 700 || place.contains(['?', ';', '\n']) {
        return None;
    }
    Some(Update {
        field: Field::ReaderPlace,
        value: place.into(),
        quote: words.into(),
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
    out.push_str(&recognition_guide(None));
    out.push_str("```\n\n## Actual recognition response schema\n\n```json\n");
    out.push_str(
        &serde_json::to_string_pretty(&turn_schema(None)).expect("Schema is serializable"),
    );
    out.push_str("\n```\n");
    out
}

/// The shared update vocabulary permits corrections/reclassification in one
/// turn. The selected program supplies instructions and applicable requirements.
pub fn turn_schema(_case: Option<&Consultation>) -> Value {
    // All first-turn applicable fields are available when a turn reclassifies.
    // The model still has an explicitly selected, much smaller instruction card.
    let field_names: Vec<_> = Field::ALL.iter().map(|f| f.name()).collect();
    let enum_strings = |names: Vec<&str>| json!({"type":"string","enum":names});
    let person = json!({"type":"object","properties":{"id":{"type":"string","maxLength":40},"label":{"type":"string","maxLength":80},"relationship":{"type":"string","enum":["unknown","partner","child","sibling","friend","mother","father","employer","employee","other_party","neighbor","querent"]},"source_quote":{"type":"string","maxLength":240}},"required":["id","label","relationship","source_quote"],"additionalProperties":false});
    let subject = json!({"type":"object","properties":{"name":{"type":"string","maxLength":80},"kind":{"type":"string","enum":["person","movable","money","property","job","small_animal","large_animal","other"]},"owner_id":{"type":"string","maxLength":40},"source_quote":{"type":"string","maxLength":240}},"required":["name","kind","owner_id","source_quote"],"additionalProperties":false});
    json!({"type":"object","properties":{
        "intent":enum_strings(Intent::ALL.iter().map(|i|i.name()).collect()),
        "question":{"type":["string","null"],"maxLength":500},
        "frame":{"oneOf":[{"type":"null"},{"type":"object","properties":{"method":enum_strings(Method::ALL.iter().map(|m|m.name()).collect()),"facet":enum_strings(Facet::ALL.iter().map(|f|f.name()).collect())},"required":["method","facet"],"additionalProperties":false}]},
        "people":{"type":"array","items":person,"maxItems":3},
        "subject":{"oneOf":[{"type":"null"},subject]},
        "updates":{"type":"array","maxItems":16,"items":{"type":"object","properties":{"field":enum_strings(field_names),"value":{"type":"string","maxLength":700},"quote":{"type":"string","maxLength":700},"mode":enum_strings(UpdateMode::ALL.iter().map(|m|m.name()).collect())},"required":["field","value","quote","mode"],"additionalProperties":false}},
        "heard":{"type":"string","maxLength":1000},"unavailable_quote":{"type":"string","maxLength":240},"focus":{"type":"string","enum":["roles","condition","reception","contacts","location","timing","judgment","place","moment"]},
        "restore_revision":{"type":["integer","null"],"minimum":1}
    },"required":["intent","question","frame","people","subject","updates","heard","unavailable_quote","focus","restore_revision"],"additionalProperties":false})
}

pub fn recognition_guide(case: Option<&Consultation>) -> String {
    let method = case.and_then(Consultation::method);
    let teaches = |methods: &[Method]| method.is_none_or(|method| methods.contains(&method));
    let mut text = String::from("You recognise this person's current conversational intent and propose fact updates. You never speak about the person in the third person, invent circumstances, choose coordinates, cast a chart, or improvise a horary method. You supply a private fact patch to the reader’s clipboard; a separate conversational model decides how to speak and inquire.\nRead consultation and pending_requirement first. question/frame/subject are null when unchanged; people and updates are empty when unchanged. Never reconstruct the whole brief. Supply exact source quotes from the current words or the retained ORIGINAL question for new people, subject or facts. A fresh question cannot quote a prior matter. A name does not identify a relationship; a seller does not establish an owner. Correct only when the person corrects a fact. Mark ambiguity with mode=propose and ignorance with mode=unavailable; never guess.\nA fresh matter supplies question, frame, subject, and any stated facts. Keep the literal goal and its facet: quantity is not event. Every reply can add multiple facts and interrupt with why, pause, device acquisition, correction, resume, or a fresh question. Explain is not completion. A known chart is not a finished reading.\nThe reader's coordinates and the moment of understanding are the chart anchor (Frawley printed pp. 7–8). Ordinary questions use device place and receipt UTC; reader_place/question_time are only EXPLICIT overrides. A city's name in an event story is event_place; an event start is event_time. 'Here'/'use my device' means intent=use_device, never a guessed city.\nFor direct audio heard is a faithful short meaning summary retaining negation/numbers/place/time and uncertainty; it is not claimed to be a transcript. Empty heard is rejected before completion.\n");
    text.push_str("When the person cannot answer pending_requirement, unavailable_quote is the exact current phrase such as 'I don't know'. Otherwise it is empty. Do not keep interrogating someone who already said this. Before the core concern is understood, clarification may refine the tentative question; after understanding, preserve it unless explicitly corrected. reader_place and question_time overrides require CURRENT words stating the reader location/question moment or answering its pending anchor inquiry. Never replay the original story's venue or event time as a chart override during a later reply. Actor fields principal_id, seller, deal_party (and sender in a relative-money question) contain a known person's ID or querent, not prose. Leave an unspecified deal_party absent: Rust supplies the generic counterparty. Never invent an identified customer.\n");
    text.push_str("Read last_reader_question when interpreting a short answer. If it proposes ONE concrete reframing and the person's current words clearly accept it, that acceptance is an explicit correction: intent=correct, question=the agreed concern, frame=its matching method/facet. Preserve subject, people and observations not changed; do not invent a relationship or ownership from 'yes'. An unaccepted suggestion has no authority. If the reader offered several choices or 'yes' could answer a different question, leave question/frame unchanged and let the conversational reader clarify.\n");
    text.push_str("In a repair request, original_input contains the actual consultation and current words. previous_worksheet is REJECTED and has no authority: nothing in it was accepted or saved as a fact. Null means unchanged only when the original consultation already has that fact. Preserve the initial question and identify its subject. For a name alone, such as Bob, relationship MUST be unknown; neither seller nor other_party is their personal relationship to the person asking.\n");
    text.push_str("Identify the subject as the thing or role asked about, even when no person is named. Existing people are identified separately. Do not leave a clear subject null on the first turn. 'Will ... within a year?' has facet=event plus horizon; facet=timing means 'WHEN will ...?', and quantity means 'HOW MANY ...?'.\n");
    if teaches(&[Method::Relationship]) {
        text.push_str("In 'Will I marry?', the quesited is a prospective partner: name='prospective partner', kind='person', owner_id='', source_quote='marry'. This is a role, not an invented person. The relationship question's baseline labels are hoped_for (formation), ongoing (an existing relationship's situation), arranged_wedding (a wedding already arranged). Infer only from stated circumstances: an unspecified baseline remains absent and the reader will inquire.\n");
    } else {
        text.push_str("baseline is not an input of this selected question type. Someone's personal capacity (husband, friend, boss) is recorded in people, not baseline.\n");
    }
    if teaches(&[
        Method::MovableDeal,
        Method::Property,
        Method::Rental,
        Method::BusinessProperty,
    ]) {
        text.push_str("Deal labels: deal_capacity=buy|sell|rent|profit|quality. Worked extraction: 'How many fish will Bob sell at the market on Friday?' -> movable_deal/quantity, Bob relationship unknown, Fish kind movable, owner_id EMPTY (seller is not proof of ownership), seller=bob, deal_capacity=sell, event_time=Friday, event_place=the market, unit=fish. A husband mentioned in a sale supplies a person capacity, never a relationship baseline or ongoing business.\n");
        text.push_str("Reframing example: retained question='How many fish will Bob sell at Friday's market?'; last_reader_question='Would you like to look at whether Friday’s market will be worthwhile for Bob?'; latest_words='Yes, that is what I want to know.' -> intent=correct; question='Will Friday’s market be worthwhile for Bob?'; frame=movable_deal/profit; subject=null, people=[], updates=[]. The current affirmation accepts the reader's one proposal, not a numerical prediction. Bob's relationship and fish ownership remain whatever the consultation actually records.\n");
    }
    if teaches(&[Method::Money]) {
        text.push_str(
            "Money labels: money_source=customer|partner|job|government|relative|other.\n",
        );
    }
    if teaches(&[Method::WorkPerson]) {
        text.push_str("Work labels: work_capacity=boss|colleague|subordinate.\n");
    }
    if teaches(&[Method::LostAnimal]) {
        text.push_str("Animal labels: animal_kind=small_kind|large_kind (species, not size).\n");
    }
    if let Some(method) = method {
        let card = contract(method);
        text.push_str(&format!("\nSELECTED PROGRAM: {} ({}) — Frawley printed pp. {}.\nRole distinctions: {}\nJudgment distinctions: {}\nRelevant facts:\n", card.title, method.name(), card.printed_pages, card.roles, card.judgment));
        for r in card.requirements {
            text.push_str(&format!(
                "{}: {:?}; accepted labels {:?}. {}\n",
                r.field.name(),
                r.guard,
                allowed_values(r.field),
                field_prompt_for(Some(method), r.field)
            ));
        }
        text.push_str("If the person changes the matter, reclassify explicitly with intent=new_question (fresh issue) or correct (same issue). Unmentioned prior facts survive.\n");
    } else {
        text.push_str(
            "\nChoose the method matching the substantive concern, not a keyword alone:\n",
        );
        for card in CATALOGUE {
            text.push_str(&format!(
                "{}: {} (pp. {}); subject kinds {:?}\n",
                card.method.name(),
                card.title,
                card.printed_pages,
                card.subject_kinds
            ));
        }
    }
    text.push_str("\nExamples (output is a patch):\n'The fair is in Bozeman tomorrow at three' -> event_place/event_time only; question/frame/subject null; never chart overrides.\n'Why that moment?' -> explain/moment; no invented factual updates. 'Continue' -> resume; don't answer a pending factual question for the person.\n'I do not know who owns it' -> owner remains unresolved; never write querent. 'Actually it is my sister's watch' -> correct; update person and subject ownership; preserve original chart.\n'My friend asked me to ask her own question' -> principal_mode=relay; identify principal_id.\n");
    for (example_method, words, output) in recognition_examples() {
        if method.is_some() && example_method.is_some() && method != example_method {
            continue;
        }
        text.push_str(&format!("\nINPUT: {words}\nOUTPUT: {}\n", output));
    }
    text
}

fn recognition_examples() -> Vec<(Option<Method>, &'static str, Value)> {
    let mut marriage = control(Intent::Read);
    marriage.question = Some("I'm single. Will I get married in the next year?".into());
    marriage.frame = Some(Frame {
        method: Method::Relationship,
        facet: Facet::Event,
    });
    marriage.subject = Some(Subject {
        name: "Prospective partner".into(),
        kind: "person".into(),
        owner_id: String::new(),
        source_quote: "get married".into(),
    });
    marriage.updates = vec![
        Update {
            field: Field::Baseline,
            value: "hoped_for".into(),
            quote: "I'm single".into(),
            mode: UpdateMode::Supply,
        },
        Update {
            field: Field::Horizon,
            value: "in the next year".into(),
            quote: "in the next year".into(),
            mode: UpdateMode::Supply,
        },
    ];
    let mut event = control(Intent::Clarify);
    event.updates = vec![
        Update {
            field: Field::EventPlace,
            value: "Bozeman, Montana".into(),
            quote: "Bozeman, Montana".into(),
            mode: UpdateMode::Supply,
        },
        Update {
            field: Field::EventTime,
            value: "tomorrow at three".into(),
            quote: "tomorrow at three".into(),
            mode: UpdateMode::Supply,
        },
    ];
    let mut unknown = control(Intent::Clarify);
    unknown.unavailable_quote = "I don't know".into();
    let mut sale = control(Intent::Read);
    sale.question = Some("Will Bob sell his books at the fair?".into());
    sale.frame = Some(Frame {
        method: Method::MovableDeal,
        facet: Facet::Event,
    });
    sale.people.push(Person {
        id: "bob".into(),
        label: "Bob".into(),
        relationship: "unknown".into(),
        source_quote: "Bob".into(),
    });
    sale.subject = Some(Subject {
        name: "Books".into(),
        kind: "movable".into(),
        owner_id: "bob".into(),
        source_quote: "his books".into(),
    });
    sale.updates = vec![
        Update {
            field: Field::DealCapacity,
            value: "sell".into(),
            quote: "sell".into(),
            mode: UpdateMode::Supply,
        },
        Update {
            field: Field::Seller,
            value: "bob".into(),
            quote: "Bob".into(),
            mode: UpdateMode::Supply,
        },
    ];
    let mut husband = control(Intent::Clarify);
    husband.people = vec![Person {
        id: "bob".into(),
        label: "Bob".into(),
        relationship: "partner".into(),
        source_quote: "Bob is my husband".into(),
    }];
    husband.subject = Some(Subject {
        name: "fish".into(),
        kind: "movable".into(),
        owner_id: "bob".into(),
        source_quote: "They are his fish".into(),
    });
    vec![
        (
            Some(Method::Relationship),
            "I'm single. Will I get married in the next year?",
            serde_json::to_value(marriage).expect("example"),
        ),
        (
            Some(Method::MovableDeal),
            "Will Bob sell his books at the fair?",
            serde_json::to_value(sale).expect("example"),
        ),
        (
            Some(Method::MovableDeal),
            "Bob is my husband. They are his fish. (reply within the retained fish sale)",
            serde_json::to_value(husband).expect("example"),
        ),
        (
            None,
            "The fair is in Bozeman, Montana tomorrow at three",
            serde_json::to_value(event).expect("example"),
        ),
        (
            None,
            "I don't know (reply to a pending ownership question)",
            serde_json::to_value(unknown).expect("example"),
        ),
    ]
}

pub fn method_guide(method: Method) -> String {
    let c = contract(method);
    format!("\n<executable_reading_contract version=\"{VERSION}\" method=\"{}\">\nFrawley, The Horary Textbook, printed pp. {}.\n{}\n{}\nThe supplied reading_request is an immutable, native-checked handoff. Answer its original question and facet using its circumstances. A factual gap is request_input with a catalogue field ID; do not ask for chart data the controller owns. Model judgment is still a proposal, not proof that the source method was followed. Unknown event coverage is not a negative verdict; exact timing/counts need their own verified method.\n</executable_reading_contract>\n", method.name(), c.printed_pages, c.roles, c.judgment)
}

#[cfg(test)]
mod tests {
    use super::*;
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
