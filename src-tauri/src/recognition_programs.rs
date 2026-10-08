//! Focused, source-bound recognition programs for a selected reading contract.
//! These teach extraction into the private clipboard, not client-facing replies.
#![forbid(unsafe_code)]

use crate::reading_contracts::{self, Guard, Method};
use std::fmt::Write;

pub(crate) fn guide(method: Method) -> String {
    let card = reading_contracts::contract(method);
    let mut text = String::from(COMMON);
    let _ = writeln!(
        text,
        "\nSELECTED EXTRACTION PROGRAM: {} ({})\nSource: Frawley, The Horary Textbook, printed pp. {}.\nRole distinctions: {}\nMethod distinctions: {}\n",
        card.title,
        method.name(),
        card.printed_pages,
        card.roles,
        card.judgment
    );
    let _ = writeln!(text, "Permitted subject kinds: {:?}.", card.subject_kinds);
    let _ = writeln!(
        text,
        "Judgment-supported facets: {}. FIRST verify the tentative classifier label against the actual words and this lesson. A wrong tentative label is not a user fact or an instruction to preserve. Only after that verification preserve a genuinely unsupported requested goal, especially an explicit exact quantity; never rewrite the person's actual question merely to fit this list.",
        card.facets
            .iter()
            .map(|facet| facet.name())
            .collect::<Vec<_>>()
            .join(", ")
    );
    text.push_str("The executable contract requires these facts when applicable:\n");
    for requirement in card.requirements {
        let condition = match requirement.guard {
            Guard::Always => "always".into(),
            Guard::Equals { field, value } => {
                format!("only if {}={value}", field.name())
            }
            Guard::PrincipalOwnsSubject => {
                "only if the principal owns the subject; unknown ownership is unresolved".into()
            }
        };
        let labels = reading_contracts::allowed_values(requirement.field);
        let _ = writeln!(
            text,
            "- {}: {condition}. {} Accepted labels: {}.",
            requirement.field.name(),
            reading_contracts::field_prompt(requirement.field),
            if labels.is_empty() {
                "source-backed concise text".into()
            } else {
                labels.join(" | ")
            }
        );
    }
    text.push_str("A requirement already resolved in consultation needs no repeated update. A fact supplied in the words must be extracted now; do not leave it missing for the conversational reader to ask again. Conditional facts are required only in the named branch. A missing controlling fact remains missing, never false by assumption.\n");
    text.push_str(program(method));
    text.push_str("\n</recognition_program>\n");
    text
}

const COMMON: &str = r#"<recognition_program>
You are a private extraction worker supporting a horary reader. Produce the supplied Turn schema only. The person never sees this worksheet. You do not converse, cast a chart, select a planet, derive a house, or announce that a reading is ready. Rust retains the clipboard and checks the handoff.

Work in this order:
1. Read the original current words, the retained consultation, last_reader_question, and pending requirement. In recognition_phase=complete_selected_program the method/facet is a TENTATIVE classifier proposal, even if the stored frame is labelled resolved. It is not a fact supplied by the person. Verify it against these SAME words and the selected lesson. Use intent=clarify and question=null. Return frame=null only if the proposal is correct; return the corrected frame if its facet is wrong, alongside the subject and facts. This internal verification does NOT require the person to correct anything. If the METHOD is wrong, return only the corrected frame with subject=null, people=[], updates=[]; the controller runs the new lesson before accepting facts. Preserve the actual requested goal, not a mistaken classifier label. WILL something happen within a period is event plus horizon; WHEN it happens is timing. An explicit numerical tally stays quantity. In update_selected_program an actual user correction or accepted reframing may change the established concern. If no actual concern has been disclosed, keep method=unclassified; a person's name cannot supply one.
2. Identify the thing, person, or role about which that goal is asked. Set a clear first-turn subject now. subject=null means the accepted subject is unchanged, not that you forgot to extract one. For a named person as the subject, subject.kind=person and owner_id must bind that person's actual ID; it does not mean someone owns that person. For a job, exam, illness or journey, the binding identifies the affected person. For goods and property it identifies title ownership, which selling, using or seeing the object does not establish. For the speaker's own concern use the reserved querent binding where the words justify it. Never put querent in people. The person_description lesson has a specific person_role for an explicitly unnamed future marriage partner; its owner_id binds the principal whose partner is described. Do not invent a spouse ID or apply this role to a named person or another method. A relationship-formation question can retain an unnamed prospective-partner subject without asserting an existing partner.
3. Extract relevant people and their operative capacity to the speaker, not merely their occupation. Use stable lowercase IDs; actor fields reference those IDs or querent. Partner, child, sibling, mother, father, friend, neighbor, employer, employee and an identified deal counterparty are different. A name alone is unknown. Everyday paraphrases can establish a capacity: living immediately next door to the speaker implies neighbor; explicitly sharing parents with the speaker implies sibling. Quote the full relational statement verbatim. Being in an adjacent queue is not being a neighbor; sharing a surname is not siblinghood. Respect negation, reported uncertainty, and statements that the person is asking on their own behalf. A relayed question uses principal_mode=relay and principal_id of the actual questioner; a speaker's own concern about someone else is not automatically a relay.
4. Extract the applicable facts named by this selected program. Give each an exact quote from CURRENT words, or the retained ORIGINAL question when allowed. The value may be a learned enum label or a normalized date; the quote must remain what the person actually said. Do not paraphrase a quote, invent context, or copy a rejected worksheet as evidence. Fields omitted from an atomic patch are retained. Mark a real ambiguity proposed rather than resolving it by preference. Unknown facts stay absent; an explicit inability to answer the pending requirement uses unavailable_quote.
5. Resolve relative calendar expressions against the supplied current_local_clock and current_timezone, not your training date or a future chart. Retain their exact spoken quote. Example only: with local clock 2030-04-03T10:00, a future event on Friday has value 2030-04-05 and quote Friday; tomorrow means 2030-04-04. The same Friday after that local date can mean the next Friday. If past/future usage or an hour is genuinely ambiguous, do not invent an occurrence or AM/PM. For supplied ISO dates preserve them. event_time, target_period and action_window may carry these resolved civil dates; none silently changes question_time. A natural horizon such as next year remains a horizon unless the question genuinely supplies an event date.
6. Keep chart anchors separate from story context. Ordinary questions use device place and their understood receipt moment (printed pp. 7–8). Only a CURRENT explicit reader-place or earlier-question-moment instruction supplies reader_place/question_time. A destination, market, dream, symptom onset, appointment, hearing or departure belongs to the story. Here can use the available device place for a genuinely local target, but it cannot locate an unspecified remote event. A bare nearby place name may be qualified by the actual device region and stated hints when no contrary context exists; explicit state/country, distant-trip context and ambiguity override that bias. Preserve the place words and qualification without fabricating coordinates. Geocoding and clock-to-UTC calculation are native tools.
7. Return only new or corrected facts. In complete_selected_program always use clarify/question=null; refining a tentative frame is permitted verification, not a new reading or an invented user correction. In update_selected_program use clarify for same-concern information, correct for an actual explicit correction or clearly accepted single reframing, and new_question only for a fresh matter. Do not replace an established concern merely to make a more convenient recipe. Explain is a request for explanation, not permission to erase the retained chart or question. The native plan, not this worker, decides whether to proceed or remind the conversational reader of a gap.

ANCHOR DISTINCTION (printed pp. 7–8). These are UPDATE examples: the actual question and subject are already retained. They are not templates for leaving a missing first-turn subject null. On a first focused pass, collect the actual subject using its selected program as well as any explicit anchor observations. These patches change only supplied observations; they do not recast anything themselves.
Historical chart input: with example current_local_clock=2030-04-03T10:00, latest_words='Use the place and moment of my original question: London, United Kingdom, on January 14, 2030 at 14:30.'
OUTPUT: {"intent":"clarify","question":null,"frame":null,"people":[],"subject":null,"updates":[{"field":"reader_place","value":"London, United Kingdom","quote":"London, United Kingdom","mode":"supply"},{"field":"question_time","value":"2030-01-14T14:30","quote":"January 14, 2030 at 14:30","mode":"supply"}],"heard":"","unavailable_quote":"","focus":"moment","restore_revision":null}
Event-only input: with that same example current clock, latest_words='The fair is in Brighton, United Kingdom tomorrow at 3 PM.'
OUTPUT: {"intent":"clarify","question":null,"frame":null,"people":[],"subject":null,"updates":[{"field":"event_place","value":"Brighton, United Kingdom","quote":"Brighton, United Kingdom","mode":"supply"},{"field":"event_time","value":"2030-04-04T15:00","quote":"tomorrow at 3 PM","mode":"supply"}],"heard":"","unavailable_quote":"","focus":"judgment","restore_revision":null}
The historical input expressly identifies the original question's place/moment. The fair's place/start describes an event and supplies neither chart override. Never copy an example city or date into actual input.

The numbered steps and examples are editorial extraction instructions applying the cited text. They are not additional quotations or proof that a prediction is empirically accurate.
"#;

fn program(method: Method) -> &'static str {
    match method {
        Method::Relationship => {
            r#"
RELATIONSHIP (printed pp. 140, 191–200)
1. Determine whether the concern is forming a relationship, an existing relationship's feelings/state, or an already arranged wedding. Set baseline=hoped_for, ongoing, or arranged_wedding ONLY from supplied actual circumstances. Unspecified status stays absent. 'Will I get married?' does not establish being single, having a partner, engagement or a booked ceremony. An example's 'I am single' is not evidence about this person.
2. For a named person preserve their supplied capacity. For an unidentified prospective partner in this relationship program, subject.kind=person, name='prospective partner', owner_id='', and people=[] unless actual people were supplied. Empty owner_id means an unidentified partner; never bind that partner to querent. Do NOT use person_role here: that type belongs only to the physical-description program. No partner name, gender or birth date is required. For a named existing partner, bind the actual person. Do not turn a neighbour into an existing lover merely because feelings are asked about.
3. Whether something will happen has facet=event, even with within six months; when it will happen has timing; present feelings are situation. Extract a supplied horizon. Never infer which person has which traditional sex-based natural role from a name or voice.
PAIRED FOCUSED EXAMPLES. The same goal differs only in the actually supplied status. Both complete the selected program with clarify/question=null, not an initial read patch.
A. Latest words: 'Will I get married within the next year?' Tentative frame: relationship/timing. The period is a horizon, not a WHEN question. There is NO supplied relationship status, and no 'I am single' quote.
OUTPUT: {"intent":"clarify","question":null,"frame":{"method":"relationship","facet":"event"},"people":[],"subject":{"name":"prospective partner","kind":"person","owner_id":"","source_quote":"Will I get married within the next year?"},"updates":[{"field":"horizon","value":"within the next year","quote":"within the next year","mode":"supply"}],"heard":"","unavailable_quote":"","focus":"judgment","restore_revision":null}
baseline is absent in A. The scaffold will remind the reader to learn the circumstances. Do not fill that gap from another example.
B. Latest words: 'I am single and there is no partner or wedding planned. Will I get married within the next year?' Tentative frame: relationship/event. Here the person's actual words supply the status.
OUTPUT: {"intent":"clarify","question":null,"frame":null,"people":[],"subject":{"name":"prospective partner","kind":"person","owner_id":"","source_quote":"Will I get married within the next year?"},"updates":[{"field":"baseline","value":"hoped_for","quote":"I am single","mode":"supply"},{"field":"horizon","value":"within the next year","quote":"within the next year","mode":"supply"}],"heard":"","unavailable_quote":"","focus":"judgment","restore_revision":null}
The baseline quote in B exists in B ONLY. In A it would be fabricated and rejected.
C. Latest words: 'Our wedding ceremony is booked for next month; will it go ahead?' Tentative frame: relationship/event.
OUTPUT: {"intent":"clarify","question":null,"frame":null,"people":[],"subject":{"name":"prospective partner","kind":"person","owner_id":"","source_quote":"Our wedding ceremony is booked"},"updates":[{"field":"baseline","value":"arranged_wedding","quote":"Our wedding ceremony is booked","mode":"supply"},{"field":"horizon","value":"next month","quote":"next month","mode":"supply"}],"heard":"","unavailable_quote":"","focus":"judgment","restore_revision":null}
C uses the booked-event default, not the formation default. No unnamed partner is invented as a participant or identified as the speaker.
"#
        }
        Method::LostObject => {
            r#"
LOST INANIMATE POSSESSION (printed pp. 146–153, 244)
1. Identify the actual missing thing and its title owner. Own keyring, a sibling's bracelet, and a borrowed tool have different ownership. Find/use/sell does not establish ownership. Bind a non-speaker owner to a known person and their capacity.
2. A location question has location; whether it returns has event; when it returns has timing. Preserve which was asked rather than reducing all searches to will I recover it.
3. When the principal owns the lost object, extract description from supplied colour, material or shape for the book's ruler comparison. If it belongs to someone else, do not make the own-object description question mandatory. Record prior search results as search_context. Theft is a reported concern only when raised.
Example: 'Where is my copper ring, with a green stone?' -> movable subject owned by querent, location, description='copper ring, with a green stone'. No ownership or appearance question remains.
Example: 'Where is my uncle's briefcase?' -> uncle as an actual participant if his operative family capacity can be represented, briefcase owner=uncle; do not call it the speaker's possession. If the capacity needs an unsupported family distinction, preserve it for clarification/review rather than claiming an unrelated house.
"#
        }
        Method::LostAnimal => {
            r#"
LOST ANIMAL (printed pp. 1–3, 146–153)
1. Identify the animal species and requested recovery or location facet. animal_kind=small_kind for dog/cat kinds; large_kind for horse/cattle kinds. This is a species distinction, not a height or weight threshold. A very large dog remains small_kind; a tiny horse remains large_kind.
2. Retain the actual owner's stated capacity if given. 'My pet' or 'my cat' supplies owner_id=querent even though ownership is not a universal blocking requirement for this program. Do not omit that supplied binding. If ownership is absent, do not invent it or require a universal title-owner interview. Recording ownership does not automatically turn an animal as a movable possession: preserve the book's sixth/twelfth species distinction.
3. Retain a supplied animal name and species in the subject. A previously unidentified pet can become the named cat or dog when that identity is supplied; this enriches the same concern. Put actual appearance in description, escape/last-seen circumstances in context or search_context, and prior searches in search_context. Escape is not an appearance. Do not invent a thief, exact coordinates or a requirement to know its birth time.
FOCUSED EXTRACTION EXAMPLES. Preserve the accepted question and use clarify/question=null.
A. Latest words: 'My pet has escaped. Where is it?' Tentative frame: lost_animal/location; no accepted subject yet. Pet does not identify a species.
OUTPUT: {"intent":"clarify","question":null,"frame":null,"people":[],"subject":{"name":"pet","kind":"animal","owner_id":"querent","source_quote":"My pet"},"updates":[{"field":"context","value":"escaped","quote":"escaped","mode":"supply"}],"heard":"","unavailable_quote":"","focus":"location","restore_revision":null}
animal_kind is absent in A. Escape is what happened, not the pet's appearance; do NOT put escaped in description. The reader will learn the species.
B. Pending fact: animal_kind. Accepted subject is the same unidentified pet. Latest words: 'My cat Moss is a grey tabby.' This identifies that pet, not a new question.
OUTPUT: {"intent":"clarify","question":null,"frame":null,"people":[],"subject":{"name":"Moss","kind":"small_animal","owner_id":"querent","source_quote":"My cat Moss"},"updates":[{"field":"animal_kind","value":"small_kind","quote":"cat","mode":"supply"},{"field":"description","value":"grey tabby","quote":"grey tabby","mode":"supply"}],"heard":"","unavailable_quote":"","focus":"location","restore_revision":null}
Preserve the actual supplied name Moss, its ownership, species and appearance. The earlier escape context remains; no fabricated search result or changed chart is needed.
C. Latest words: 'Where is my huge dog Cedar?' -> a named small_animal subject bound to querent; animal_kind=small_kind from dog, with huge as supplied description. Huge does not change dog into the horse/cattle large_kind. A miniature pony remains large_kind. Never copy these example names into another person's missing-animal question.
"#
        }
        Method::MissingPerson => {
            r#"
MISSING PERSON (printed pp. 146–153)
1. The subject is the actual person missing, kind=person, owner_id bound to that person's ID. Extract their operative relationship to the speaker. A name alone cannot make them a friend, child, or generic seventh-house person.
2. Distinguish whereabouts (location), whether they will be found/contacted (event), and timing. Preserve stated circumstances as context or search_context, including uncertainty about whether they are actually missing.
3. An absent person is not a movable lost possession. Do not collect an object description as a compulsory field, convert a police report into a certainty, or invent a search outcome.
Example: 'Where is my father Luis? He has not come home from his walk.' -> Luis father, person subject bound to luis, location, the walk retained as context. 'Will Robin be found?' requires Robin's actual capacity unless already known.
"#
        }
        Method::MovableDeal => {
            r#"
SALE OR PURCHASE OF MOVABLE GOODS (printed pp. 156–161, 167–172)
1. Identify the goods, title owner, and deal actor separately. Extract deal_capacity=buy|sell|profit|quality as appropriate. Selling a thing or working a stall does not prove ownership. If sell, extract seller as the actual person's ID or querent. Leave an unnamed counterparty absent; native rules provide a generic deal partner.
2. A named seller/buyer/owner whose relation is unknown remains unknown. A partner's goods and the speaker's goods sold by that partner differ. Potential possessions bought for the speaker can bind the principal where the words establish this prospective acquisition; do not identify a current third-party title owner as the buyer merely because they own the goods.
3. Will the transaction complete is event; is the item sound is situation; is it worthwhile is profit; which offered item is preferable is choice. How many units will sell stays quantity, with unit, even though the generic program cannot judge that exact count. Do not silently change the concern.
4. Record a supplied fair/venue/date as event_place/event_time. It is not the reader's city or chart moment. baseline is not a sale input; a husband belongs in people.
Example: 'My partner Noor is selling a piano that I own; will the sale complete?' -> piano owner=querent, seller=noor, Noor partner, deal_capacity=sell, event. 'Will Noor sell the piano?' does not prove Noor owns it.
"#
        }
        Method::Money => {
            r#"
PAYMENT, DEBT, GIFT OR GRANT (printed pp. 156–161)
1. Identify whose money/claim is being asked about and the source: money_source=customer|partner|job|government|relative|other. Do not assume a source from the word money. A tax charge sent to the government is tax, not money received from government.
2. For government money extract discretionary=owed or discretionary: a contractual entitlement differs from a gift/grant chosen by officials. An unspecified grant's entitlement stays unresolved. For relative money extract sender as an actual known person ID and their capacity; free prose is not an actor ID.
3. Arrival is event or timing; quality or benefit is situation/profit. No exact currency amount is invented from the source. Existing money and an investment's value are not payment-arrival by default.
Example: 'Will the municipality release the benefit it already owes me?' -> own money claim, money_source=government, discretionary=owed, event. 'Will my mother send me the gift?' -> sender=mother's ID, relationship=mother, money_source=relative; preserve gift versus debt.
"#
        }
        Method::Investment => {
            r#"
SHARES OR INVESTMENTS (printed pp. 156–161)
1. Identify the particular investment and whose holding or proposed holding it is. Bind ownership from actual my shares, her fund, or buying for myself evidence, not from a mentioned broker's name.
2. Distinguish condition/value (situation), gain (profit), and comparison (choice). An investment's return is not automatically the arrival of someone else's money. Record a stated comparison, holding period and risk concern as context/priorities.
3. Do not request a payment sender by habit or turn an exact-return request into a different concern. Quantity is preserved as a method limit, never fabricated.
Example: 'Would the bond fund I own be profitable to keep?' -> investment/profit, money subject owner=querent. 'Will the shares perform well?' leaves the affected holder genuinely unclear.
"#
        }
        Method::NewJob => {
            r#"
GETTING A NEW EXTERNAL JOB (printed pp. 222–224)
1. This program asks whether a not-yet-available job will be obtained. Identify the job/application and the actual job seeker as its subject binding. An interview is not an offer; a job already available is job_offer; continuing an existing job is existing_job.
2. If the speaker asks about an acquaintance, record the acquaintance's actual capacity. If explicitly relaying that person's own question, also record principal_mode=relay and principal_id. Do not confuse own concern about my friend with the friend's relayed concern.
3. Whether it happens is event; when is timing. Extract a provided employer/interview/deadline as job_context, event_time or horizon; do not demand employer coordinates or a universal salary preference survey.
Example: 'Will my friend Sora get the analyst post she has applied for?' -> new_job/event, Sora friend, job bound to sora. 'They have offered me the post; is it suitable?' is job_offer, not acquisition.
"#
        }
        Method::ExistingJob => {
            r#"
KEEPING A JOB OR EXISTING CAREER (printed pp. 224–226)
1. Establish the already-held job/career and affected worker. Current employer or career continuity is not an acquisition question. The subject job binds its worker, not automatically the boss or company owner.
2. Preserve the event/situation/timing concern: dismissal, stability, progression or what happens next are distinct. Supplied restructuring or supervisor context belongs in job_context/context.
3. A person's colleague is not their boss; a question primarily about that person's work relationship may be work_person. No demand for an application date or an offer when the role is already held.
Example: 'I have worked at the museum for five years; will I keep this position through the reorganisation?' -> existing_job/event, job owner binding=querent, restructuring context. 'Will I get the museum vacancy?' is new_job.
"#
        }
        Method::ReturnToJob => {
            r#"
RETURNING TO AN OLD JOB (printed pp. 225–226)
1. Identify the former job and person returning. Preserve the old-job re-entry context; it is neither already-held continuity nor an arbitrary new external position.
2. Whether re-entry occurs is event; when is timing. Record any actual offer already made, employer discussion or reason for leaving without assuming approval or an intermediary.
3. Bind the returning worker from supplied evidence. If the name alone is given, keep their capacity unresolved.
Example: 'Will I be taken back at the bakery where I used to work?' -> return_to_job/event, job bound to querent, prior employment retained as job_context. 'Will I keep the bakery job I still hold?' is existing_job.
"#
        }
        Method::JobOffer => {
            r#"
ASSESSING AN AVAILABLE JOB (printed pp. 224–226)
1. Confirm that a particular job can already be taken. Signed offer, concrete invitation to start, or an accepted available position establishes availability; interview or hope does not.
2. Bind the person considering the offer. Is the offer good is situation; comparison is choice; financial suitability can be profit. Extract stated pay, schedule, work conditions or priorities. Do not invent an acquisition question or ask preferences already supplied.
3. Good without any material standard may need priorities later, but a narrow specific concern such as stable hours need not become a general values survey. Keep job and pay distinct in context.
Example: 'I have been offered the archivist position; would its regular hours suit me better than shift work?' -> job_offer/situation, own job, priorities=regular hours. 'I interviewed there; will they hire me?' is new_job.
"#
        }
        Method::WorkPerson => {
            r#"
BOSS, COLLEAGUE OR SUBORDINATE (printed pp. 224–225)
1. Identify the actual person as kind=person. Extract work_capacity=boss|colleague|subordinate from the operative work relation, rather than treating every friendly workmate as a friend or every manager as the speaker's employer.
2. The work_capacity field carries this specialist distinction. Do not erase a named person just because ordinary personal-relationship labels do not fit colleague. Preserve any explicitly supplied personal capacity separately, and leave an unsupported one unknown.
3. Retain whether the question concerns present feelings, a future interaction or timing. A question about the person's job or wages may belong to a job or money program instead.
Example: 'Will my colleague Dev support my proposal?' -> work_person/event, Dev person, work_capacity=colleague. 'Will my boss give me the raise?' may instead be a money-from-job concern; identify the actual asked result.
"#
        }
        Method::Property => {
            r#"
BUYING OR SELLING PROPERTY (printed pp. 167–171)
1. Identify the property, person making the deal, and actual title ownership where supplied. deal_capacity=buy|sell|profit|quality. House/property, its price, and the parties are separate; a relative selling for the principal is not automatically the title owner.
2. For a purchase for the speaker, keep the principal's intended acquisition as the binding and an identified current seller as a separate party. Do not turn the buyer into the seller by copying title ownership alone.
3. Deal completion is event; property condition or price suitability is situation; profit is profit; finite alternatives can be choice. Preserve a supplied horizon or location, but do not use the property's town as the reader's chart location.
Example: 'Will I sell the apartment that I own?' -> property/event, owner=querent, deal_capacity=sell. 'Will my mother sell my apartment for me?' preserves mother as deal actor and speaker as owner.
"#
        }
        Method::Rental => {
            r#"
RENTAL AGREEMENT (printed p. 170)
1. Identify the tenancy/property and who is considering or holding the rental agreement. Extract deal_capacity=rent for the ordinary tenant/landlord arrangement. Owning a building, being a tenant, and arranging a lease for someone else are different capacities.
2. Distinguish finding/completing a tenancy (event), an already available tenancy's suitability (situation), comparison (choice), and return (profit). Keep an identified landlord/tenant as a separate actual party if supplied.
3. Bind the affected tenant/principal for a tenancy inquiry; preserve another title owner separately rather than treating their house as the buyer's frame. Modern tenant does not imply servant or employee. Renting for business use may need business_property distinctions.
Example: 'Would taking this available lease for my own home be a good arrangement?' -> rental/situation, affected tenant=querent, deal_capacity=rent. 'Will I buy that house?' is property, not rental.
"#
        }
        Method::BusinessProperty => {
            r#"
PROPERTY USED FOR BUSINESS (printed pp. 170–171)
1. Identify the premises and actual business use: working there, farming it, or operating a workshop differs from simply living in an ordinary home. Bind the relevant owner or prospective deal actor from supplied evidence, keeping an identified seller separate.
2. Extract deal_capacity=buy|rent|sell|profit|quality and preserve the precise benefit/completion/condition concern. Business use and potential profit must remain visible as context, not disappear into an ordinary house-sale question.
3. This is a specialist intake; complete facts do not establish its unreviewed profit roles or permit a judgment. An unnamed title owner is not automatically the speaker.
Example: 'Would renting the mill as premises for my own pottery business pay off?' -> business_property/profit, business-premises subject, deal_capacity=rent, business context. 'Would this apartment be a pleasant home?' is ordinary property/rental quality.
"#
        }
        Method::Choice => {
            r#"
STAY, CHANGE OR COMPARE ALTERNATIVES (printed pp. 201–203)
1. Extract current_option and a finite alternatives description. Staying in the current situation, moving to a changed situation, and several unrelated options are different. Do not infer the current option from one alternative alone.
2. Preserve stated priorities and relevant capacity. For home ambiguity record home_meaning=current_home or homeland only when the person explains which. Specific school, job or investment comparisons may use their underlying program.
3. The subject is the actual decision/situation; do not invent a person's ownership. If the alternatives are not disclosed, leave that field missing; a bare should-I-change is not enough.
Example: 'Should I stay in my present town or relocate to the coast? I care most about being close to my parents.' -> choice/choice, current_option=present town, alternatives=relocate to coast, priorities=close to parents. 'Should I leave?' lacks both the concrete current situation and alternative.
"#
        }
        Method::Hiring => {
            r#"
HIRING STAFF (printed pp. 189–190)
1. Identify that the concern is hiring an employee, rather than obtaining the speaker's own job. Extract candidates as the actual finite shortlist, with brief distinguishing descriptions if provided.
2. Preserve what the person wants from the employee and any actual employment capacity. A candidate's name does not assign a planet or mean friend. Do not infer an offer must first be acquired when candidates are available.
3. Choice among staff is choice; suitability of a named candidate is situation. Missing candidates or an unbounded list needs clarification; this worker does not rank them or invent their skills.
Example: 'For my shop should I hire Iris, experienced but cautious, or Owen, energetic but new?' -> hiring/choice, candidates with both supplied descriptions. 'Who should I hire?' has no shortlist yet.
"#
        }
        Method::Contact => {
            r#"
CONTACT WITH SOMEONE (printed pp. 165–166)
1. Identify the actual person and their operative capacity to the speaker; bind the person subject to that ID. An incoming call from a parent is contact with that parent, not automatically a literal letter/parcels recipe.
2. Whether contact occurs is event; when is timing. Preserve whether contact is already expected or a hoped-for reunion as expected/context when stated, without assuming it from a name.
3. If the actual concern is receipt of an ordered object or a mailed packet, use parcel instead. No street address or biography is universally needed for contact.
Example: 'When will my mother telephone as she said she would this evening?' -> contact/timing, mother capacity, expected=yes if extracted, evening context. 'When will the catalogue she mailed arrive?' is parcel.
"#
        }
        Method::Parcel => {
            r#"
ARRIVAL OF A LETTER OR PARCEL (printed pp. 165–166)
1. Identify the particular item not yet received. Extract sender and their capacity: a seller, parent or friend sending it differs from the receiver. Use a source-backed actor ID where available, with the person also recorded, rather than a disconnected name.
2. The actual goal is physical receipt (event/timing), not merely hearing from the sender. Before receipt do not silently treat the letter as already the receiver's possession. Keep any announced mailing, expected delivery or tracking report as reports/context.
3. Unknown sender stays missing; a universally demanded title owner or exact post-office address is not a substitute. Do not choose a house or infer arrival from contact with the seller.
Example: 'When will the parcel my friend Mina mailed arrive?' -> parcel/timing, sender=mina, Mina friend, parcel subject. 'When will Mina call?' is contact, not parcel.
"#
        }
        Method::Visit => {
            r#"
EXPECTED VISIT (printed p. 166)
1. Identify visitor and operative capacity, and extract expected=yes|no from whether the visit is actually agreed/anticipated or merely hoped for. A plumber, a friend and a long-lost relative have different contextual roles.
2. visitor references the supplied person or an explicitly described role; preserve the matching capacity in people/context. Whether arrival occurs is event; when is timing. The assumed afternoon/appointment window is meaningful event context.
3. Being expected cannot be inferred solely from 'will they come'. No birth time, full street address or transport itinerary is a universal prerequisite.
Example: 'The electrician I hired is due this afternoon; when will he arrive?' -> visit/timing, visitor=identified hired electrician, expected=yes. 'Will my friend ever visit?' has expected=no only if the absence of an arrangement is actually stated.
"#
        }
        Method::Contest => {
            r#"
SPORTING MATCH OR CHAMPIONSHIP (printed pp. 203–208)
1. Extract competition=match|season|championship from the actual contest scope. Identify the competing sides and any supplied particular fixture. One game differs from the whole league season.
2. Extract affiliation as the person's actual supported side or declared indifference, not automatically the home side, first name or favourite in the media. If they ask whether their bet pays, use bet instead of assigning allegiance.
3. The subject is the actual contest, not a purchased possession. A supporter asking will we win is not an ordinary relayed question; preserve real allegiance. No player-by-player biography is mandatory.
Example: 'Will the Falcons beat the Otters in tonight's final? I support the Falcons.' -> contest/event, competition=match, affiliation=Falcons. 'Who wins?' with neither fixture nor allegiance/indifference disclosed retains the gaps.
"#
        }
        Method::Bet => {
            r#"
PROFIT FROM A BET (printed pp. 156–161, 203–204)
1. Identify the principal's wager/money and the actual financial outcome asked about. A bet on a team is not by itself fandom or a us-versus-them match question.
2. Use profit or situation for the financial benefit. Preserve stated stake, odds and contest as context without inventing an exact payout or transferring a match winner recipe.
3. A third-party bettor requires their actual capacity or a genuine relay; do not silently attribute their money to the speaker because this contract normally concerns the principal.
Example: 'Would my wager on the Otters be profitable?' -> bet/profit, wager bound to querent; supporting the Falcons is irrelevant unless separately stated. 'Will the Otters win their final?' is contest unless the person actually asks about the bet's gains.
"#
        }
        Method::CourtCase => {
            r#"
CIVIL TRIAL OR LEGAL DISPUTE (printed pp. 208–209)
1. Identify the actual party whose concern is being asked and the civil dispute. Extract claim as the specific issue and requested outcome; do not interpret a name or this-case as a known claim without an antecedent.
2. Bind a named litigant's subject to that person's ID and supplied capacity. If a lawyer expressly relays a client's genuine question, keep principal_mode=relay/principal_id. A lawyer's own separate curiosity is not the same declaration.
3. Verdict/settlement/completion is event, existing position can be situation, timing is when. Record a supplied hearing date as event_time; it does not set the chart clock. A criminal custody/release concern may need custody, not a generic civil trial.
Example: 'As her lawyer I am passing on my client Lena's own question: will she win the boundary dispute against her neighbour?' -> court_case/event, claim=boundary dispute, relay principal Lena. 'Will they win the case?' leaves both the party and dispute unresolved.
"#
        }
        Method::Vehicle => {
            r#"
VEHICLE OR JOURNEY SAFETY (printed pp. 142–143)
1. Identify the vehicle or actual journey and whose relevant undertaking it is. In this capacity the ship/car is a vehicle carrying out the person's journey, not automatically a second-house possession. They need not already be aboard or own the vehicle.
2. Safety, situation and event concern the specified journey; a car sale is movable_deal and a trip's profit may be undertaking. Bind an identified non-speaker traveller to their known capacity, never assume a named traveller is a child.
3. Departure and destination are event context only. Do not replace the question moment with a ticket time or demand vessel ownership when the concern is safety.
Example: 'Will my flight to Lisbon land without trouble? I leave next Tuesday.' -> vehicle/safety, own journey, next-Tuesday event date resolved with the actual local clock. 'Will someone buy my aeroplane?' is a sale of a possession instead.
"#
        }
        Method::PersonDescription => {
            r#"
DESCRIPTION OF A PERSON (printed pp. 143–145)
1. Use this method only when broad physical appearance/description is actually asked for. A bare 'What about Jules?' or a question about feelings is not an appearance request. If the concern is unknown, use unclassified; do not invent it.
2. Identify the person and operative capacity. Bind a named person subject to their ID. For an explicitly unnamed future spouse or marriage partner use subject.kind=person_role, name=Future marriage partner and owner_id of the principal whose spouse is asked about (querent for my future spouse; the identified participant for their future spouse). Quote the full clause establishing that principal and future marriage role. Do not invent an existing spouse, name, gender or relationship participant. Other unspecified future people remain unresolved; this narrowly supported role is not a generic future-person fallback.
3. Use description, or situation only when that is the actual concern. Do not infer race, exact height, scars, tattoos or gender from names, planets, or silence. Known description/context is retained as a report, not an established chart finding.
Example: 'Could you describe the general appearance of my half-sister Nia, whom I have not met?' -> person_description/description, Nia sibling from the explicitly stated capacity. 'What about Nia?' -> unclassified, not a physical-description program.
Example: 'What will my future spouse look like?' -> person_description/description, subject kind=person_role, name=Future marriage partner, owner_id=querent, exact whole-question quote; no spouse name or present relationship status is required. The method still awaits expert review; complete inputs do not authorize a completed judgment.
"#
        }
        Method::Information => {
            r#"
WHETHER INFORMATION IS TRUE (printed pp. 164–165)
1. Extract claim as the precise reported assertion whose veracity is asked. There must be an actual claim, not merely 'is it true?' without an antecedent. Keep a rumour as a rumour, not an established event.
2. Choose truth. Superficial 'is it true I will get the job/marry' normally uses the underlying new-job/relationship program. Prophetic dream/prediction veracity has its distinct scope; a person's trustworthiness is trust.
3. Do not convert future occurrence, a named person's character, or ordinary relationship testimony into a universal third-house truth recipe. Event place/time mentioned inside a report remains context.
Example: 'The town notice claims the footpath is already closed permanently; is that notice accurate?' -> information/truth, claim=reported permanent closure. 'Is it true that my application will succeed?' -> underlying acquisition concern, not automatic information.
"#
        }
        Method::Trust => {
            r#"
TRUSTWORTHINESS IN A CAPACITY (printed p. 165)
1. The subject is the PERSON whose trustworthiness is questioned, not keys, cash, a document or another thing entrusted to them. subject.kind=person; for a named person bind owner_id to their actual participant ID. Record the concrete concern as context when supplied.
2. Determine that person's operative relationship to the speaker. Literal neighbour and a clear lives-immediately-next-door statement can supply neighbor; unrelated nearby strangers cannot. A named person without capacity stays unknown.
3. Use truth or situation for the person's reliability/trustworthiness. 'Safely entrust' describes why reliability matters; it does NOT make the requested predicate vehicle/journey safety. A tentative trust/safety label is a classifier mistake to refine now, not a genuine unsupported question to hand back to the person. Mentioning keys/documents does not make this lost_object. Do not invent an accusation or establish dishonesty.
FOCUSED CORRECTION EXAMPLE. Latest words: 'Can I safely entrust my documents to Ezra? Ezra lives next door to me.' Tentative frame: trust/safety. No user correction is needed; this worker verifies the meaning.
OUTPUT: {"intent":"clarify","question":null,"frame":{"method":"trust","facet":"situation"},"people":[{"id":"ezra","label":"Ezra","relationship":"neighbor","source_quote":"Ezra lives next door to me"}],"subject":{"name":"Ezra","kind":"person","owner_id":"ezra","source_quote":"entrust my documents to Ezra"},"updates":[{"field":"context","value":"entrusting documents","quote":"entrust my documents","mode":"supply"}],"heard":"","unavailable_quote":"","focus":"judgment","restore_revision":null}
The original question is unchanged. The corrected facet expresses the already requested reliability inquiry; it is not a proposal to ask a different question. If latest_words is only 'Can I trust Ezra?', retain Ezra as a person with relationship=unknown and let the scaffold request that missing capacity, without inventing neighbor.
"#
        }
        Method::Pregnancy => {
            r#"
CURRENT PREGNANCY (printed pp. 173–174)
1. Identify whose PRESENT pregnancy is asked about, person/animal capacity and known parenthood context. Bind the affected person or animal from the source. Already expecting is current state; hoping to become pregnant is fertility.
2. Use situation. Do not infer that the speaker is a potential father, or assume paternity/sex from a name or partner term. Parenthood context belongs in parenthood when actually supplied and may matter to later turning.
3. Current pregnancy needs neither a universal future deadline nor an exact time of conception. Do not announce a clinical result; this extraction cannot bypass specialist review.
Example: 'Is my own dog pregnant already?' -> pregnancy/situation, dog subject with the speaker's supplied ownership, current state. 'Will my partner become pregnant this cycle?' -> fertility, not current pregnancy.
"#
        }
        Method::Fertility => {
            r#"
CONCEPTION AND FERTILITY (printed pp. 174–177)
1. Identify the affected prospective parent and actual scope. fertility_scope=conception for a particular attempt/near-term conception; carrying_to_term for completing an existing or specified pregnancy; lifetime for ever having children. Generic fertility leaves scope unresolved.
2. Use event/timing for will/when conception occurs, situation for potential, and quantity only when the actual question concerns the source-specific coarse number of children. Do not transfer a childbirth-count recipe to sales tallies.
3. Preserve explicitly supplied parenthood, age/life context, existing children and treatment context without guessing or requiring birth dates from everyone. A fertility-treatment attempt is this method, not automatically medical with medical_task=treatment and a tenth-house treatment role.
Example: 'I am beginning fertility treatment; will this attempt result in conception?' -> fertility/event, fertility_scope=conception. 'Will I ever have children?' -> lifetime, preserving known age/context if supplied. 'Tell me about my fertility' does not select one scope.
"#
        }
        Method::Adoption => {
            r#"
ADOPTION (printed pp. 177–178)
1. Extract adoption_state=prospective for an uncompleted adoption with no identified birth-parent capacity; prospective_known_parent when the identified birth parent changes turning; completed only when adoption has actually taken place.
2. For prospective_known_parent extract birth_parent and their source-backed operative relationship. An explicitly known sibling's child is not a generic stranger's child. Unknown parent details are not a mandatory field in the generic prospective branch.
3. Before completion never call the child already the querent's child. To bind whose adoption inquiry this is without falsely claiming title to a prospective child, a subject such as 'my prospective adoption', kind=other, can bind the adopting principal; retain the child/birth-parent facts separately. For a completed child's subsequent concern preserve actual child capacity and permit the underlying relevant program.
Example: 'Will I be permitted to adopt my cousin's child? The adoption has not happened.' -> prospective_known_parent, known birth-parent relationship preserved for clarification/review if unsupported by the current capacity vocabulary. 'The agency has proposed a child; can I adopt them?' -> prospective, not completed.
"#
        }
        Method::Medical => {
            r#"
ILLNESS OR TREATMENT (printed pp. 179–189)
1. Identify the patient and source-backed capacity; bind the illness/treatment subject to that patient, not to a doctor by default. medical_task=diagnosis for what is wrong, prognosis for how known illness develops, treatment for the effects/comparison of a specified intervention.
2. If treatment, extract treatment from the actual named option(s). 'The treatment' without an antecedent is missing. For prognosis/diagnosis no intervention field is made mandatory merely because the person is ill.
3. Preserve what is known as reported context; do not infer a diagnosis, declare a clinician wrong, or equate illness automatically with Lord 6. Symptom onset is context, not a replacement chart anchor. This specialist intake is not medical advice or clinical qualification.
Example: 'How will my mother's already diagnosed migraine develop?' -> method=medical, facet=situation, medical_task=prognosis, mother patient, no compulsory treatment inquiry. 'Will acupuncture help my own migraine?' -> method=medical, facet=event, medical_task=treatment, treatment=acupuncture. 'Will that help?' without treatment/context needs clarification.
"#
        }
        Method::Politics => {
            r#"
POLITICAL ELECTION (printed pp. 212–214)
1. Extract office as the actual office/election/jurisdiction, election_state=incumbent or open, and political_capacity=supporter|impartial_citizen|foreign_observer. A candidate seeking another term is incumbent; a stated vacant office without a sitting contender is open. Do not guess from a title alone if the situation is unclear.
2. Preserve candidates, affiliation and the person’s actual observation capacity. Being physically in a country does not establish citizenship or allegiance. Supporting the challenger differs from supporting the incumbent; an impartial citizen differs from a foreign observer.
3. The subject is the actual political contest. Whether a candidate wins is event; current position or removal concerns may be situation/event as asked. A personal relation to a candidate is recorded only if stated, not because of a shared name.
Example: 'Who wins the presidency: sitting president Vale or challenger Hart? I am an impartial citizen of that country.' -> incumbent, impartial_citizen, actual presidential office. 'Will Hart win?' with no office/status/capacity keeps those fields missing.
"#
        }
        Method::Knowledge => {
            r#"
KNOWLEDGE AND ITS EARNINGS (printed pp. 216–218)
1. Identify the actual knowledge used by the principal. knowledge_task=quality for soundness/ability; profit for income from applying knowledge independently. A new employed position is a job; wages from it are job money.
2. Use situation for knowledge quality and profit for its earning power. A received fee/invoice can be money rather than abstract earning capacity. Record supplied customer constraints or ability to express the skill as context, not an invented finding.
3. If a different person's knowledge is asked about, keep their supplied capacity rather than silently assigning the speaker's knowledge. No obligatory payment-arrival aspect or exact earnings estimate is implied by extraction.
Example: 'Could my independent tutoring knowledge support me financially?' -> knowledge/profit, knowledge_task=profit, principal's own skill. 'Is my grasp of philosophy sound?' -> quality. 'Will I get a salaried tutoring vacancy?' -> new_job.
"#
        }
        Method::Exam => {
            r#"
EXAMINATION (printed p. 218)
1. Identify the particular examination and examinee. Bind it to the affected person with actual capacity. exam_task=passing for successful exam/result; admission when the actual goal is being admitted through it and that is what the person asks.
2. Whether it happens is event; when is timing; an existing performance/state can be situation. A grade needed to pass identifies passing without requiring the word exam if a final paper is explicitly described.
3. Preserve a supplied date as event_time, never the question moment. The institution's quality/enjoyment is education, not automatically an exam; a named examinee is not automatically the speaker's child.
Example: 'Will my son Amir get through the driving theory test?' -> exam/event, exam_task=passing, Amir child, exam bound to amir. 'What about Amir's test?' without an actual desired result may still need the task.
"#
        }
        Method::Undertaking => {
            r#"
VOYAGE, COURSE OR TRADE-SHOW BENEFIT (printed p. 219)
1. Identify the actual undertaking and whose benefit is being considered. An unidentified 'that trip' cannot be invented from no antecedent. Use an undertaking subject, not automatically goods sold, a school application or a vehicle.
2. Quality/suitability is situation; whether it improves earnings or pays off is profit. Attendance to learn/make contacts differs from selling objects at the event (movable_deal), from journey safety (vehicle), and from enrollment/admission (education).
3. Preserve destination, dates, cost and actual benefit priority as context/event fields when supplied. No universal ownership interview is required for an explicitly personal undertaking, and no inferred event coordinates may replace the device chart place.
Example: 'Would attending the ceramics convention improve my independent earning power? I will attend, not sell there.' -> undertaking/profit, the convention is the subject. 'Will I benefit from it?' with no antecedent leaves the undertaking/concern unresolved.
"#
        }
        Method::Dream => {
            r#"
DREAM MEANING OR PROPHETIC TRUTH (printed p. 219; pp. 164–165)
1. Extract dream_account as what actually happened in the dream and dream_task=meaning or prophetic_truth. An omen/prediction concern is veracity; 'what was that about?' asks meaning. A request to interpret with no account leaves dream_account missing.
2. Use situation for meaning and truth for prophetic veracity. Preserve known character capacities/context from the account without claiming dreamed facts happened in waking life. A dream about a concrete future marriage may be the ordinary relationship concern if that is what is actually asked.
3. Charting uses the question's understood time, not a guessed dream timestamp. The whole meaning inquiry is not automatically confined to the ninth house. No need to elicit the exact time the person fell asleep.
Example: 'I dreamed my mother was crossing a bridge and I could not reach her; what does it mean?' -> dream/situation, meaning, account retained. 'Does my dream foretell the bridge collapsing?' -> prophetic_truth, dream account retained as a dream.
"#
        }
        Method::Education => {
            r#"
SCHOOL OR UNIVERSITY (printed pp. 219–220)
1. Identify the affected student and institution. Bind the education subject to the student and supplied capacity. school_level=school or university from actual level; an ambiguous academy/college name is not enough by itself. An explicit undergraduate course establishes university.
2. school_task=admission|enjoyment|quality. Already enrolled and asking whether they will be happy is enjoyment, not an application. Institution quality differs from its fit for this particular student; preserve supplied priorities.
3. Comparison requires actual finite options and brief distinguishing descriptions, kept as alternatives/context; do not invent a ranking of unnamed schools. The institution's native house remains radical rather than being turned mechanically from a child.
Example: 'My child has a place at the primary school; will she enjoy it?' -> method=education, facet=situation, school_level=school, school_task=enjoyment, child binding. 'Is that university a good educational institution?' -> method=education, facet=situation, school_level=university, school_task=quality. 'Will she be admitted?' -> method=education, facet=event, school_task=admission, not enjoyment.
"#
        }
        Method::Wish => {
            r#"
UNSPECIFIED WISH (printed p. 231; pp. 164–165)
1. A wish is not a universal eleventh-house prediction. If the person discloses the actual matter, select that concrete program and preserve its goal. Wishing for a job, wedding or recovery does not make the concern unclassifiable.
2. If the matter is deliberately undisclosed, keep method=wish and the core concern unresolved. Do not invent a marriage, employment or a generic yes/no answer. An explicit refusal to disclose the pending concern uses unavailable_quote; do not repeatedly demand it.
3. Retain voluntary context and invite later disclosure through the native reminder. This worker never writes that invitation to the person itself. No chart finding substitutes for the actual question.
Example: 'My wish is that my already booked ceremony goes ahead' -> underlying relationship, arranged_wedding. 'Will the thing I will not identify happen?' -> wish, no manufactured subject/outcome. 'I can tell you now: I mean my old job taking me back' permits routing to return_to_job.
"#
        }
        Method::Tax => {
            r#"
TAX OR ASSESSMENT (printed pp. 231–232)
1. Identify whose government assessment and money are involved. An explicitly personal bill binds the principal; 'the savings' with no holder does not prove whose money they are. Another person's tax concern needs their capacity/relay preserved for review rather than silently substituting the speaker.
2. Distinguish assessment/burden/ability to survive (situation/profit as asked) from an exact numeric amount. Preserve quantity if an exact amount is demanded; do not invent a currency figure or quietly change it to affordability.
3. A payment demanded by the government is not a government gift arriving to the person. Record known bill, fiscal period or burden concern as context; no birth chart or full financial ledger is automatically required.
Example: 'Can my own savings bear the revenue assessment?' -> tax/situation, own-money binding. 'How much exactly will they assess?' preserves the requested amount under the method limit instead of manufacturing a sum.
"#
        }
        Method::Allegation => {
            r#"
REPORTED HARMFUL PRACTICE (printed pp. 233–234)
1. Extract claim as the actual reported concern: suspected harmful practice, occult assault or relevant self-harm/vice context, without declaring that a suspected assault is established. Generic harmful allegation with no content leaves claim missing.
2. Keep any supplied accused person's capacity only when stated and material. An unknown alleged attacker is not automatically a neighbour or ex-partner. Truth/situation follow the actual concern; do not invent a future act from a past allegation.
3. Anxiety about a reported act is not proof of that act. No unprompted accusation, threat, or claimed supernatural finding belongs in a fact patch. A future applying aspect cannot prove past harm; judgment remains under specialist review.
Example: 'I fear that someone has cursed my farm, but I do not know who or whether it happened' -> allegation/truth, reported claim with uncertainty retained. 'Is my drinking harming me?' retains the stated vice concern rather than inventing an assailant.
"#
        }
        Method::Custody => {
            r#"
IMPRISONMENT OR RELEASE (printed pp. 234–237)
1. Identify the person and actual capacity, with person subject bound to their ID. custody_state=already_held for currently jailed/detained/on remand; not_held for someone currently free facing possible entry. Being accused or having a future hearing alone does not establish custody.
2. Entry and release are opposite event directions. Preserve whether the person asks will be sent down, will be released, or what happens to an already remanded person's condition. 'Coming out' in explicit custody context means release.
3. Criminal prison concerns are distinct from an ordinary civil case verdict. Keep current bail/remand/sentence facts as reported context. No inference of innocence, guilt or jail from a name.
Example: 'My brother Theo has been refused bail and is on remand; will he get out?' -> custody/event, already_held, Theo sibling. 'He is at home awaiting sentencing; will he be sent to prison?' -> not_held. 'What happens about prison?' needs the current state.
"#
        }
        Method::Weather => {
            r#"
WEATHER AT A PLACE OR EVENT (printed pp. 238–240)
1. Identify the event or general locality as the subject, kind=other. Asking whether rain will spoil a picnic/party is a weather EVENT concern; the event is already identified. Do not set subject=null and ask what is concerning about rain when the concern is explicit.
2. Extract weather_scope=event for a stated specific event, general for regional/day/season weather. Extract target_place and target_period from the supplied event/locality and date/season. Those are the operative Weather fields, even if event_place/event_time are also retained; generic event metadata alone must not leave target inputs missing.
3. Normalize a relative target day from current_local_clock with its exact quote; qualify a nearby named target only from actual device region and stated hints. Target location differs from reader_place, and target period differs from question_time. Here/local means the device target only if the person genuinely asks about this area; an unspecified wedding elsewhere cannot be located by the phone.
4. Whether rain occurs has event; what conditions are like has situation; when a rainy spell ends has timing. A remote or unnamed target date/place remains a real gap. Do not invent weather observations or claim a meteorological forecast.
Example: 'Will rain spoil my garden party in Ashford this coming Tuesday? I mean the Ashford near our current town.' -> weather/event, subject='garden party' kind other, weather_scope=event, target_place qualified by actual nearby context, target_period=current-clock resolved Tuesday. Preserve source quotes and keep the original chart anchor.
Example: 'What will the weather be like here this winter?' -> general, target_place from actual local-device context if available, target_period=specified winter. 'Will it rain at the reunion?' without locality/day retains those target gaps, not the available reader-place gap.
"#
        }
        Method::Election => {
            r#"
CHOOSING WHEN TO ACT BY HORARY (printed pp. 241–242)
1. Extract action as the particular act whose favourable time is sought, and action_window as its feasible dates/time range. Supplied practical restrictions belong in constraints. Unknown action or an unbounded best-day request leaves the corresponding field missing.
2. Selecting among dates is choice; seeking the suitable time in a known window is timing. A prediction of when an event will happen is a different underlying matter, not automatically election. A political election is politics.
3. Resolve relative allowed days from the actual local clock, retaining exact phrases, without inventing a future chart or minute-perfect election. This horary method uses the original question chart and does not require universal natal data.
Example: 'I can send my poetry collection either Tuesday or Thursday before the office closes; which day is best?' -> election/choice, action=submitting collection, action_window=both actual-clock dates, constraints=office closing. 'When should I submit it?' without antecedent/action/window remains incomplete.
"#
        }
        Method::Unclassified => {
            r#"
ACTUAL CONCERN NOT YET IDENTIFIED (printed p. 14; pp. 26, 137–140)
1. Retain the person's words and named participants without assigning an unsupported matter. A bare name, 'what about her', 'will it work out', or an unnamed this/that is not a physical-description request, romance, employment, or missing-object concern by default.
2. Keep method=unclassified when the actual core is absent; do not manufacture a goal, owner, or requested outcome to fit a familiar program. The native frame reminder will let the conversational reader ask what they want to know.
3. If a later reply supplies the actual concern, select its concrete program, subject and situational inputs from that reply. This is a clarification of a tentative core, not automatically a new chart or correction of an already understood matter. Exact chart-moment acquisition remains native.
Example: 'And Leila?' with no antecedent -> unclassified, named Leila can be recorded unknown, no imagined appearance/feelings. 'I mean whether my sister Leila will get the vacancy she applied for' -> new_job/event, Leila sibling, job bound to leila. 'They have offered me a post; is it worth taking?' can directly route to job_offer without a generic topic interview.
"#
        }
    }
}
