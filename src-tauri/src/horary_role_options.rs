//! Interpret a small, source-backed matter description into named house choices.
//! The model selects IDs. Rust binds the person/object, turns houses, and derives
//! rulers. A person cannot be relabeled as their possessions by a numeric slip.
#![forbid(unsafe_code)]
use crate::{
    horary_lessons::Matter,
    reading_method::{self, Fact, NaturalRole, Role, RoleChoice},
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Person {
    pub id: String,
    pub label: String,
    pub relationship: String,
    pub source_quote: String,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Subject {
    pub name: String,
    /// `person` names an identified target. `person_role` is currently only
    /// the canonical Future marriage partner role used by PersonDescription.
    pub kind: String,
    /// For `person`, this is the target's ID. For `person_role`, this is the
    /// principal whose future spouse is described, never an invented spouse ID.
    /// Other kinds retain their owner or relevant principal semantics.
    pub owner_id: String,
    pub source_quote: String,
}
pub const FUTURE_MARRIAGE_PARTNER: &str = "Future marriage partner";
impl Subject {
    pub fn is_empty(&self) -> bool {
        self.name.is_empty()
            && self.kind.is_empty()
            && self.owner_id.is_empty()
            && self.source_quote.is_empty()
    }

    pub fn is_future_marriage_partner(&self) -> bool {
        self.kind == "person_role" && self.name == FUTURE_MARRIAGE_PARTNER
    }
}

#[derive(Clone, Deserialize, Serialize)]
pub struct Choice {
    pub id: String,
    pub label: String,
    pub house: Option<u8>,
    pub natural: Option<NaturalRole>,
    pub basis: String,
}

#[derive(Clone, Deserialize, Serialize)]
pub struct ConditionalRole {
    pub choice_id: String,
    pub unless_house_claims: NaturalRole,
    pub basis: String,
}

#[derive(Clone, Deserialize, Serialize)]
pub struct Options {
    pub choices: Vec<Choice>,
    pub required_groups: Vec<Vec<String>>,
    #[serde(default)]
    pub conditional_roles: Vec<ConditionalRole>,
    pub missing: Vec<String>,
    pub compare: Vec<String>,
}

pub fn turn(base: u8, relative: u8) -> u8 {
    (base + relative - 2) % 12 + 1
}

pub fn relationship_house(relationship: &str) -> Option<u8> {
    match relationship {
        "partner" | "other_party" => Some(7),
        "child" => Some(5),
        "sibling" => Some(3),
        "friend" => Some(11),
        "mother" | "employer" => Some(10),
        "father" => Some(4),
        "employee" => Some(6),
        "querent" => Some(1),
        "neighbor" => Some(3),
        _ => None,
    }
}

/// This is a conservative English extraction check, not a proof of the full
/// meaning of a sentence. Quotes and classification remain reviewable.
pub fn relation_words(relationship: &str) -> &'static [&'static str] {
    match relationship {
        "partner" => &[
            "husband",
            "wife",
            "spouse",
            "partner",
            "boyfriend",
            "girlfriend",
            "lover",
            "fiance",
            "fiancé",
            "marry",
            "married",
            "marriage",
        ],
        "child" => &["daughter", "son", "child"],
        "sibling" => &["brother", "sister", "sibling"],
        "friend" => &["friend"],
        "mother" => &["mother", "mom", "mum"],
        "father" => &["father", "dad"],
        "employer" => &["employer", "boss"],
        "employee" => &["employee", "servant"],
        "neighbor" => &["neighbor", "neighbour"],
        "querent" => &[
            "my own question",
            "their own question",
            "her own question",
            "his own question",
        ],
        "other_party" => &[
            "client",
            "customer",
            "buyer",
            "seller",
            "opponent",
            "stranger",
            "other party",
        ],
        _ => &[],
    }
}

fn words(text: &str) -> Vec<(usize, &str)> {
    let mut result = Vec::new();
    let mut start = None;
    for (at, ch) in text.char_indices() {
        if ch.is_alphanumeric() || matches!(ch, '\'' | '’') {
            start.get_or_insert(at);
        } else if let Some(start) = start.take() {
            result.push((start, &text[start..at]));
        }
    }
    if let Some(start) = start {
        result.push((start, &text[start..]));
    }
    result
}

fn possessive_stem(word: &str) -> &str {
    word.strip_suffix("'s")
        .or_else(|| word.strip_suffix("’s"))
        .unwrap_or(word)
}

/// Whole-word matching keeps a name or capacity from matching a substring.
pub(crate) fn mentions(text: &str, phrase: &str) -> bool {
    let text = text.to_ascii_lowercase();
    let phrase = phrase.to_ascii_lowercase();
    let text = words(&text);
    let phrase = words(&phrase);
    !phrase.is_empty()
        && text.windows(phrase.len()).any(|window| {
            window
                .iter()
                .zip(&phrase)
                .all(|(left, right)| possessive_stem(left.1) == possessive_stem(right.1))
        })
}

fn negated_before(source: &str, cue: usize) -> bool {
    let preceding = &source[..cue];
    let clause = preceding
        .rsplit(['.', '!', '?', ';', ','])
        .next()
        .unwrap_or(preceding);
    for (_, word) in words(clause).iter().rev() {
        if *word == "only" {
            return false; // "not only my neighbour" affirms the capacity.
        }
        if matches!(
            *word,
            "not"
                | "no"
                | "never"
                | "isn't"
                | "isn’t"
                | "aren't"
                | "aren’t"
                | "wasn't"
                | "wasn’t"
                | "weren't"
                | "weren’t"
                | "don't"
                | "don’t"
                | "doesn't"
                | "doesn’t"
                | "didn't"
                | "didn’t"
        ) {
            return true;
        }
        if !matches!(
            *word,
            "my" | "our"
                | "your"
                | "his"
                | "her"
                | "their"
                | "a"
                | "an"
                | "the"
                | "really"
                | "actually"
                | "even"
                | "longer"
                | "next"
                | "door"
        ) {
            return false;
        }
    }
    false
}

fn positive_phrase(
    quote: &str,
    source: &str,
    phrase: &str,
    binding: impl Fn(&str, usize) -> bool,
) -> bool {
    let quote = quote.to_ascii_lowercase();
    let source = source.to_ascii_lowercase();
    let phrase = phrase.to_ascii_lowercase();
    let quote_words = words(&quote);
    let source_words = words(&source);
    let phrase_words = words(&phrase);
    if phrase_words.is_empty() {
        return false;
    }
    quote_words.windows(phrase_words.len()).any(|window| {
        window
            .iter()
            .zip(&phrase_words)
            .all(|(left, right)| possessive_stem(left.1) == possessive_stem(right.1))
            && source.match_indices(&quote).any(|(at, _)| {
                let cue = at + window[0].0;
                let Some(index) = source_words.iter().position(|word| word.0 == cue) else {
                    return false;
                };
                let source_match = source_words[index..].get(..phrase_words.len());
                source_match.is_some_and(|matched| {
                    matched
                        .iter()
                        .zip(&phrase_words)
                        .all(|(left, right)| possessive_stem(left.1) == possessive_stem(right.1))
                }) && !negated_before(&source, cue)
                    && binding(&source, cue)
            })
    })
}

fn capacity_modifier(word: &str) -> bool {
    matches!(
        word,
        "good"
            | "old"
            | "close"
            | "dear"
            | "adult"
            | "older"
            | "younger"
            | "eldest"
            | "youngest"
            | "next"
            | "door"
            | "new"
            | "former"
            | "prospective"
            | "potential"
            | "romantic"
            | "business"
            | "direct"
            | "only"
            | "biological"
            | "adopted"
            | "half"
            | "step"
            | "really"
            | "actually"
    )
}

/// A pending participant supplies an antecedent only for a bare capacity or an
/// anaphoric reply. An explicitly named actor still needs its own binding.
fn pending_reference(source: &str, cue: usize, predicate_end: usize) -> bool {
    let before = &source[..cue];
    let clause_before = before
        .rsplit(['.', '!', '?', ';', '\n'])
        .next()
        .unwrap_or(before);
    let mut prefix = words(clause_before);
    while prefix
        .last()
        .is_some_and(|(_, word)| capacity_modifier(word))
    {
        prefix.pop();
    }
    if prefix
        .last()
        .is_some_and(|(_, word)| matches!(*word, "my" | "our" | "a" | "an" | "the"))
    {
        prefix.pop();
    }
    let pronoun = |word: &str| matches!(word, "he" | "she" | "they" | "we");
    let anaphoric = match prefix.as_slice() {
        [] => true,
        [(_, word)] => {
            pronoun(word)
                || matches!(
                    *word,
                    "he's" | "he’s" | "she's" | "she’s" | "they're" | "they’re" | "we're" | "we’re"
                )
        }
        [(_, actor), (_, copula)] => {
            pronoun(actor) && matches!(*copula, "is" | "are" | "was" | "were")
                || *actor == "we" && *copula == "both"
        }
        _ => false,
    };
    if !anaphoric {
        return false;
    }
    let after = &source[predicate_end..];
    let clause_after = after
        .split(['.', '!', '?', ';', '\n'])
        .next()
        .unwrap_or(after);
    let suffix = words(clause_after);
    matches!(
        suffix.as_slice(),
        [] | [(_, "of"), (_, "mine")] | [(_, "to"), (_, "me")]
    ) || suffix
        .first()
        .is_some_and(|(_, word)| matches!(*word, "from" | "at" | "in" | "since"))
}

fn literal_capacity_binding(
    source: &str,
    cue: usize,
    relationship: &str,
    label: &str,
    pending: bool,
) -> bool {
    if relationship == "querent" {
        return true; // The explicit own-question role has a separate relay contract.
    }
    let tokens = words(source);
    let Some(index) = tokens.iter().position(|word| word.0 == cue) else {
        return false;
    };
    let cue_word = tokens[index].1;
    let before = &source[..cue];
    let clause_before = before
        .rsplit(['.', '!', '?', ';', '\n'])
        .next()
        .unwrap_or(before);
    let speaker_possessive = words(clause_before)
        .iter()
        .rev()
        .find(|(_, word)| !capacity_modifier(word))
        .is_some_and(|(_, word)| matches!(*word, "my" | "our"));
    let after = &source[cue + cue_word.len()..];
    let clause_after = after
        .split(['.', '!', '?', ';', '\n'])
        .next()
        .unwrap_or(after);
    let speaker_complement = clause_after.trim_start().starts_with("of mine")
        || clause_after.trim_start().starts_with("to me");
    let other_possessive = words(clause_before)
        .last()
        .is_some_and(|(_, word)| possessive_stem(word) != *word);
    let romantic_self = relationship == "partner"
        && matches!(possessive_stem(cue_word), "marry" | "married" | "marriage")
        && !other_possessive
        && (mentions(clause_before, "i")
            || mentions(clause_after, "me")
            || pending && pending_reference(source, cue, cue + cue_word.len()));
    // 'The buyer Jo' is an operative deal capacity. Another person's buyer
    // remains unbound when a possessive immediately owns that capacity.
    let other_party = relationship == "other_party" && !other_possessive;
    if !(speaker_possessive || speaker_complement || romantic_self || other_party) {
        return false;
    }
    let label = label.to_ascii_lowercase();
    let label_words = words(&label);
    let role_label = !label_words.is_empty()
        && label_words.iter().all(|(_, word)| {
            matches!(*word, "my" | "our")
                || relation_words(relationship)
                    .iter()
                    .any(|role| *role == possessive_stem(word))
        });
    if possessive_stem(cue_word) != cue_word && !role_label {
        return false; // Pat is my sister's FRIEND, not my sister.
    }
    let following = words(after);
    let label_after = !label_words.is_empty()
        && following.get(..label_words.len()).is_some_and(|window| {
            window
                .iter()
                .zip(&label_words)
                .all(|(left, right)| possessive_stem(left.1) == possessive_stem(right.1))
        });
    role_label
        || label_after
        || mentions(clause_before, &label)
        || pending && pending_reference(source, cue, cue + cue_word.len())
}

/// Narrow English evidence checks, not a general semantic proof. Both the exact
/// quote and its source context matter: trimming "not" must not create evidence.
pub(crate) fn relationship_evidence(
    relationship: &str,
    quote: &str,
    source: &str,
    label: &str,
    pending: bool,
) -> bool {
    if relation_words(relationship).iter().any(|cue| {
        positive_phrase(quote, source, cue, |source, at| {
            literal_capacity_binding(source, at, relationship, label, pending)
        })
    }) {
        return true;
    }
    match relationship {
        "neighbor" => ["lives next door to me", "lives next door to us"]
            .iter()
            .any(|cue| {
                positive_phrase(quote, source, cue, |source, at| {
                    pending && pending_reference(source, at, at + cue.len())
                        || mentions(
                            source[..at]
                                .rsplit(['.', '!', '?', ';', '\n'])
                                .next()
                                .unwrap_or(""),
                            label,
                        )
                })
            }),
        "sibling" => [
            "share the same parents",
            "have the same parents",
            "share both parents",
            "have the same mother and father",
            "share the same mother and father",
        ]
        .iter()
        .any(|cue| {
            positive_phrase(quote, source, cue, |source, at| {
                let before = &source[..at];
                let clause_before = before
                    .rsplit(['.', '!', '?', ';', '\n'])
                    .next()
                    .unwrap_or(before);
                let clause_after = source[at..]
                    .split(['.', '!', '?', ';', '\n', ','])
                    .next()
                    .unwrap_or("");
                (mentions(clause_before, "we")
                    || mentions(clause_before, "i")
                    || mentions(clause_after, "as me"))
                    && (pending && pending_reference(source, at, at + cue.len())
                        || mentions(clause_before, label)
                        || mentions(clause_after, label))
            })
        }),
        _ => false,
    }
}

/// Recognises a small set of explicit future-spouse phrases. The role's
/// principal and the absence of an attached identity are checked in the full
/// clause, even when the extractor quotes only its role words. This is a
/// conservative English guard, not a general semantic proof.
pub(crate) fn future_marriage_partner_evidence(
    quote: &str,
    source: &str,
    principal_id: &str,
    principal: Option<&Person>,
) -> bool {
    let binding = |source: &str, at: usize, cue: &str, marriage_clause: bool| {
        let tokens = words(source);
        let Some(index) = tokens.iter().position(|word| word.0 == at) else {
            return false;
        };
        let count = cue.split_whitespace().count();
        let Some(last) = tokens.get(index + count - 1) else {
            return false;
        };
        let end = last.0 + last.1.len();
        let after = source[end..]
            .split(['.', '!', '?', ';', '\n'])
            .next()
            .unwrap_or("");
        if words(after).first().is_some_and(|(_, word)| {
            !matches!(
                *word,
                "look"
                    | "looks"
                    | "appearance"
                    | "appear"
                    | "appears"
                    | "be"
                    | "have"
                    | "will"
                    | "would"
                    | "could"
                    | "might"
                    | "physical"
                    | "general"
                    | "broad"
                    | "like"
            )
        }) {
            return false; // 'my future husband Alex' supplies an identity.
        }

        let owner_start =
            if marriage_clause {
                if !matches!(principal_id, "" | "querent") {
                    return false; // 'I will marry' does not name Sam's future spouse.
                }
                if index > 0 && tokens[index - 1].1 == "the" {
                    index - 1
                } else {
                    index
                }
            } else {
                match principal_id {
                    "" => {
                        // Retain an unresolved principal, but inspect an attached
                        // possessive so 'Alex is my future husband' is still named.
                        if index > 0
                            && (tokens[index - 1].1 == "my"
                                || tokens[index - 1].1.ends_with("'s")
                                || tokens[index - 1].1.ends_with("’s"))
                        {
                            index - 1
                        } else {
                            index
                        }
                    }
                    "querent" if index > 0 && tokens[index - 1].1 == "my" => index - 1,
                    "querent" => return false,
                    _ => {
                        let Some(person) = principal else {
                            return false;
                        };
                        let label = person.label.to_ascii_lowercase();
                        let label_words = words(&label);
                        let Some(start) = index.checked_sub(label_words.len()) else {
                            return false;
                        };
                        if label_words.is_empty()
                            || !tokens[start..index].iter().zip(&label_words).all(
                                |(left, right)| possessive_stem(left.1) == possessive_stem(right.1),
                            )
                            || !tokens[index - 1].1.ends_with("'s")
                                && !tokens[index - 1].1.ends_with("’s")
                        {
                            return false;
                        }
                        start
                    }
                }
            };
        let before_owner = &source[..tokens[owner_start].0];
        let clause = before_owner
            .rsplit(['.', '!', '?', ';', '\n'])
            .next()
            .unwrap_or(before_owner);
        let prefix = clause.trim_end();
        let prefix_words = words(prefix);
        let excludes_role = [
            "don't mean",
            "don’t mean",
            "do not mean",
            "not asking about",
            "not referring to",
            "not talking about",
            "not a description of",
        ]
        .iter()
        .any(|negative| {
            let negative = words(negative);
            prefix_words
                .len()
                .checked_sub(negative.len())
                .is_some_and(|start| {
                    prefix_words[start..]
                        .iter()
                        .zip(&negative)
                        .all(|(left, right)| left.1 == right.1)
                })
        });
        if prefix.ends_with([',', ':'])
            || excludes_role
            || words(prefix).last().is_some_and(|(_, word)| {
                matches!(*word, "is" | "are" | "was" | "were" | "be" | "become")
            })
        {
            return false; // 'Alex, my future husband' / 'Alex is my future husband'.
        }
        true
    };
    [
        "future marriage partner",
        "future spouse",
        "future husband",
        "future wife",
    ]
    .iter()
    .any(|cue| {
        positive_phrase(quote, source, cue, |source, at| {
            binding(source, at, cue, false)
        })
    }) || [
        "person i will marry",
        "person who i will marry",
        "man i will marry",
        "woman i will marry",
    ]
    .iter()
    .any(|cue| {
        positive_phrase(quote, source, cue, |source, at| {
            binding(source, at, cue, true)
        })
    })
}

pub fn build(matter: Matter, people: &[Person], subject: &Subject) -> Options {
    let mut options = Options {
        choices: Vec::new(),
        required_groups: Vec::new(),
        conditional_roles: Vec::new(),
        missing: Vec::new(),
        compare: Vec::new(),
    };
    let mut add = |id: String, label: String, base: u8, relative: u8, basis: String| {
        options.choices.push(Choice {
            id,
            label,
            house: Some(turn(base, relative)),
            natural: None,
            basis,
        });
    };
    add(
        "querent.self".into(),
        "You".into(),
        1,
        1,
        "The ordinary first represents the person asking.".into(),
    );
    options.required_groups.push(vec!["querent.self".into()]);
    for person in people {
        if let Some(base) = relationship_house(&person.relationship) {
            add(
                format!("{}.self", person.id),
                person.label.clone(),
                base,
                1,
                format!(
                    "{} is classified as {}; their own house is {base}.",
                    person.label, person.relationship
                ),
            );
            options
                .required_groups
                .push(vec![format!("{}.self", person.id)]);
        } else {
            options.missing.push(format!(
                "{}: relationship to the person asking is unknown",
                person.label
            ));
        }
    }
    let owner = if subject.owner_id == "querent" {
        Some(1)
    } else {
        people
            .iter()
            .find(|person| person.id == subject.owner_id)
            .and_then(|person| relationship_house(&person.relationship))
    };
    match subject.kind.as_str() {
        "person_role" => {
            if !subject.is_future_marriage_partner() {
                options.missing.push(
                    "The person role is not a supported future marriage partner role.".into(),
                );
            } else if let Some(base) = owner {
                add(
                    "subject.primary".into(),
                    subject.name.clone(),
                    base,
                    7,
                    format!("Future marriage partner is seventh from the principal's house {base}; an unnamed spouse is a role, not an invented person. Frawley printed pp. 143, 191, 196."),
                );
                options.required_groups.push(vec!["subject.primary".into()]);
            } else {
                options.missing.push("The future marriage partner's principal or that principal's operative relationship is unresolved.".into());
            }
        }
        "person" => {
            if subject.owner_id != "querent"
                && !people.iter().any(|person| person.id == subject.owner_id)
            {
                options
                    .missing
                    .push("The person asked about has not been identified.".into());
            }
        }
        "movable" | "money" | "property" | "job" | "small_animal" | "large_animal" => {
            if let Some(base) = owner {
                let relative = match subject.kind.as_str() {
                    "property" => 4,
                    "job" => 10,
                    "small_animal" => 6,
                    "large_animal" => 12,
                    _ => 2,
                };
                add("subject.primary".into(),subject.name.clone(),base,relative,format!("{} belongs to {}; count relative house {relative} from owner house {base}. Rust computes {}.",subject.name,subject.owner_id,turn(base,relative)));
                options.required_groups.push(vec!["subject.primary".into()]);
                if matter == Matter::LostObject && base == 1 {
                    add("subject.alternative_fourth".into(),format!("{}: fourth-house candidate",subject.name),1,4,"Frawley's alternative fourth-house candidate for the querent's missing object; compare it with Lord 2.".into());
                    options
                        .required_groups
                        .last_mut()
                        .expect("The primary subject group exists")
                        .push("subject.alternative_fourth".into());
                    options.compare = vec![
                        "subject.primary".into(),
                        "subject.alternative_fourth".into(),
                    ];
                }
            } else {
                options.missing.push(format!(
                    "{}: whose matter or possession this is remains unknown",
                    subject.name
                ));
            }
        }
        _ => {
            // Unmapped topics still require contextual house judgment, but the
            // identity and the resulting ruler are bound to the selected ID.
            let ids: Vec<_> = (1..=12)
                .map(|house| format!("subject.ordinary_{house}"))
                .collect();
            for house in 1..=12 {
                add(format!("subject.ordinary_{house}"),subject.name.clone(),1,house,format!("Contextual choice of ordinary house {house}; the model must justify relevance from the lesson."));
            }
            options.required_groups.push(ids);
        }
    }
    options.choices.push(Choice {
        id: "moon.contextual".into(),
        label: "The Moon's contextual role".into(),
        house: None,
        natural: Some(NaturalRole::Moon),
        basis: "Optional contextual testimony; a claimed house ruler has first use of its planet."
            .into(),
    });
    options
}

/// Contract-specific capacities override the generic turning helper. Only
/// operative participants enter this program; names in background context do
/// not become mandatory astrological roles.
pub fn build_for(
    case: &crate::reading_contracts::Consultation,
    matter: Matter,
    people: &[Person],
    subject: &Subject,
) -> Options {
    use crate::reading_contracts::{Field, Method};
    let method = case.method();
    if subject.kind == "person_role" && method != Some(Method::PersonDescription) {
        let mut options = build(
            matter,
            &[],
            &Subject {
                kind: "person".into(),
                owner_id: "querent".into(),
                ..Default::default()
            },
        );
        options.missing.push("The future marriage partner role is only available to the person_description contract.".into());
        return options;
    }
    let relay = case.text(Field::PrincipalMode) == Some("relay");
    let principal = if relay {
        case.text(Field::PrincipalId).unwrap_or("querent")
    } else {
        "querent"
    };
    let mut relevant: Vec<_> = people
        .iter()
        .filter(|p| {
            (p.id == subject.owner_id && (!case.is_deal() || case.needs_title_owner()))
                || (matches!(
                    method,
                    Some(
                        Method::MovableDeal
                            | Method::Property
                            | Method::Rental
                            | Method::BusinessProperty
                    )
                ) && (Some(p.id.as_str()) == case.deal_actor()
                    || Some(p.id.as_str()) == case.text(Field::DealBeneficiary)
                    || Some(p.id.as_str()) == case.text(Field::Seller)
                    || Some(p.id.as_str()) == case.text(Field::DealParty)))
                || (method == Some(Method::Money)
                    && Some(p.id.as_str()) == case.text(Field::Sender))
        })
        .cloned()
        .collect();
    for person in &mut relevant {
        if person.id == principal {
            person.relationship = "querent".into();
        }
    }
    let mut chosen = subject.clone();
    if case.is_deal() {
        // This is a local astrological projection, not an assertion of title.
        // The immutable consultation keeps the actual owner, if supplied.
        if case.is_pure_deal_completion() {
            chosen.kind = "other".into();
        }
        if !case.needs_title_owner() {
            chosen.owner_id = case.deal_actor().unwrap_or("").into();
        }
    }
    if method == Some(Method::WorkPerson) {
        // The work capacity is already a resolved input. Do not require a
        // second personal relationship or bind the same person twice.
        relevant.retain(|p| p.id != subject.owner_id);
        chosen.kind = "other".into();
    }
    if matches!(method, Some(Method::LostAnimal)) {
        chosen.owner_id = "querent".into();
        chosen.kind = match case.text(Field::AnimalKind) {
            Some("large_kind") => "large_animal",
            Some("small_kind") => "small_animal",
            _ => {
                let mut options = build(Matter::Other, &[], &Subject::default());
                options
                    .choices
                    .retain(|choice| !choice.id.starts_with("subject."));
                options
                    .required_groups
                    .retain(|group| !group.iter().any(|id| id.starts_with("subject.")));
                options.missing.push(
                    "The animal's actual kind is unresolved; do not assume the sixth or twelfth."
                        .into(),
                );
                return options;
            }
        }
        .into();
        relevant.clear();
    }
    if method.is_some_and(|m| {
        matches!(
            crate::reading_contracts::contract(m).owner,
            crate::reading_contracts::OwnerRule::Principal
        )
    }) {
        chosen.owner_id = principal.into();
    }
    let mut options = build(matter, &relevant, &chosen);
    if case.is_pure_deal_completion() {
        // Frawley p. 168: completion is contact between the actual parties.
        // The native chart still supplies all planets for prohibition,
        // translation and collection; pruning an unused asset role does not
        // prune intervening-event evidence.
        options
            .choices
            .retain(|choice| !choice.id.starts_with("subject."));
        options
            .required_groups
            .retain(|group| !group.iter().any(|id| id.starts_with("subject.")));
        options.compare.clear();
    } else if case.is_deal() && !case.needs_title_owner() {
        for choice in options
            .choices
            .iter_mut()
            .filter(|choice| choice.id.starts_with("subject."))
        {
            choice.basis = format!("The potential acquisition or tenancy is framed from the contracting actor {}. This role assignment does not assert legal title ownership; Frawley printed pp. 167–171.", chosen.owner_id);
        }
    }
    let base = if chosen.owner_id == "querent" || chosen.owner_id == principal {
        1
    } else {
        relevant
            .iter()
            .find(|p| p.id == chosen.owner_id)
            .and_then(|p| relationship_house(&p.relationship))
            .unwrap_or(1)
    };
    let replacement = match method {
        Some(Method::NewJob | Method::JobOffer) => {
            Some(if base == 10 { turn(base, 10) } else { 10 })
        }
        Some(Method::WorkPerson) => Some(match case.text(Field::WorkCapacity) {
            Some("boss") => 10,
            Some("subordinate") => 6,
            _ => 7,
        }),
        Some(Method::Money) => match case.text(Field::MoneySource) {
            Some("customer" | "partner") => Some(turn(base, 8)),
            Some("job" | "government") => Some(turn(base, 11)),
            Some("relative") => case
                .text(Field::Sender)
                .and_then(|id| case.people.get(id))
                .and_then(|p| relationship_house(&p.relationship))
                .map(|house| turn(house, 2)),
            _ => None,
        },
        _ => None,
    };
    if let Some(house) = replacement {
        options.choices.retain(|c| !c.id.starts_with("subject."));
        options
            .required_groups
            .retain(|group| !group.iter().any(|id| id.starts_with("subject.")));
        options.compare.clear();
        options.choices.push(Choice{id:"subject.primary".into(),label:subject.name.clone(),house:Some(house),natural:None,basis:format!("The selected {} contract supplies house {house}; ordinary indiscriminate turning is not applied. Frawley printed pp. {}.",method.expect("Matched method").name(),crate::reading_contracts::contract(method.expect("Matched method")).printed_pages)});
        options.required_groups.push(vec!["subject.primary".into()]);
        options.missing.retain(|s| !s.starts_with(&subject.name));
    }
    if method == Some(Method::Money)
        && case.frame.resolved().is_some_and(|frame| {
            matches!(
                frame.facet,
                crate::reading_contracts::Facet::Event | crate::reading_contracts::Facet::Timing
            )
        })
    {
        options.choices.push(Choice {
            id: "money.recipient_pocket".into(),
            label: if chosen.owner_id == "querent" && !relay {
                "Your pocket or bank account"
            } else {
                "The recipient's pocket or bank account"
            }.into(),
            house: Some(turn(base, 2)),
            natural: None,
            basis: format!("For arrival, incoming money may contact the recipient or their pocket/bank account. The pocket is second from the recipient's house {base}, house {}; it is distinct from the incoming money (Frawley printed p. 158).",turn(base,2)),
        });
        options
            .required_groups
            .push(vec!["money.recipient_pocket".into()]);
        if chosen.owner_id == principal {
            options.conditional_roles.push(ConditionalRole {
                choice_id: "moon.contextual".into(),
                unless_house_claims: NaturalRole::Moon,
                basis: "For money arrival, the genuine querent is represented by Lord 1 or Moon, and the pocket by Lord 2. Select Moon unless a selected house ruler already claims it. Moon is not transferred to someone merely asked about (Frawley printed pp. 32, 158).".into(),
            });
            if let Some(moon) = options
                .choices
                .iter_mut()
                .find(|choice| choice.id == "moon.contextual")
            {
                moon.label = if relay {
                    "The principal's Moon as recipient"
                } else {
                    "Your Moon as recipient"
                }
                .into();
                moon.basis = "Required genuine-recipient testimony for arrival, unless a selected house ruler has first claim on Moon. Incoming money may contact Lord 1, Moon or the recipient's second-house pocket; this is not an amount-only aspect requirement (Frawley printed pp. 32, 158).".into();
            }
        }
    }
    if matches!(method, Some(Method::NewJob | Method::ReturnToJob))
        && case.frame.resolved().is_some_and(|frame| {
            matches!(
                frame.facet,
                crate::reading_contracts::Facet::Event | crate::reading_contracts::Facet::Timing
            )
        })
        && match case.text(Field::PrincipalMode) {
            Some("relay") => case.text(Field::PrincipalId) == Some(chosen.owner_id.as_str()),
            Some("self" | "concerning_other") => chosen.owner_id == "querent",
            _ => false,
        }
    {
        options.conditional_roles.push(ConditionalRole {
            choice_id: "moon.contextual".into(),
            unless_house_claims: NaturalRole::Moon,
            basis: "For acquiring or returning to a job, the genuine principal has Lord 1 and Moon. Select Moon unless a selected house ruler already claims it; do not transfer the speaker's Moon to another worker (Frawley printed pp. 32, 222, 225–226).".into(),
        });
        if let Some(moon) = options
            .choices
            .iter_mut()
            .find(|choice| choice.id == "moon.contextual")
        {
            moon.label = if relay {
                "The principal's Moon as worker"
            } else {
                "Your Moon as worker"
            }
            .into();
            moon.basis = "Required genuine-worker co-significator for acquiring or returning to a job, unless a selected house ruler has first claim on Moon. This preserves relevant testimony; it does not establish an event or its date (Frawley printed pp. 32, 222, 225–226).".into();
        }
    }
    if method == Some(Method::JobOffer) {
        if let Some(job_house) = options
            .choices
            .iter()
            .find(|choice| choice.id == "subject.primary")
            .and_then(|choice| choice.house)
        {
            options.choices.push(Choice {
                id: "job.wages".into(),
                label: "The offered job's pay".into(),
                house: Some(turn(job_house, 2)),
                natural: None,
                basis: "The job's money is its second house, distinct from the job and the worker's pocket. External job tenth/pay eleventh; tenth-house person's job seventh/pay eighth. A wages aspect is not acquiring the job (Frawley pp. 223–227).".into(),
            });
            if case
                .frame
                .resolved()
                .is_some_and(|frame| frame.facet == crate::reading_contracts::Facet::Profit)
            {
                options.required_groups.push(vec!["job.wages".into()]);
            }
        }
    }
    if method == Some(Method::Relationship)
        && (subject.owner_id.is_empty() || case.text(Field::Baseline) == Some("hoped_for"))
    {
        // A future partner is a role, not an invented biographical person.
        options = build(
            Matter::Other,
            &[],
            &Subject {
                name: subject.name.clone(),
                kind: "other".into(),
                ..Default::default()
            },
        );
        options.choices.retain(|c| !c.id.starts_with("subject."));
        let basis = if subject.owner_id.is_empty() {
            "Seventh for an unnamed partner role; no invented identity or gender (Frawley p. 191)."
        } else {
            "Seventh for the identified person as a prospective romantic partner; their supplied identity is retained (Frawley p. 191)."
        };
        options.choices.push(Choice {
            id: "subject.primary".into(),
            label: subject.name.clone(),
            house: Some(7),
            natural: None,
            basis: basis.into(),
        });
        options
            .required_groups
            .retain(|g| !g.iter().any(|id| id.starts_with("subject.")));
        options.required_groups.push(vec!["subject.primary".into()]);
    }
    if method == Some(Method::Relationship) {
        options.conditional_roles.push(ConditionalRole {
            choice_id: "moon.contextual".into(),
            unless_house_claims: NaturalRole::Moon,
            basis: "The querent has Lord 1 and the Moon as heart, including prospective or arranged relationships. A house ruler has first claim on Moon; if Moon already rules the querent, keep its emotional meaning on that role, and if it rules the enquired-about party do not also assign it to the querent (Frawley pp. 191–193).".into(),
        });
        if let Some(moon) = options
            .choices
            .iter_mut()
            .find(|c| c.id == "moon.contextual")
        {
            moon.label = "Your feelings".into();
            moon.basis = "Required querent-emotions testimony in this relationship question unless a selected main house ruler already claims Moon; do not infer gender (Frawley pp. 191–193).".into();
        }
    }
    if matches!(method, Some(Method::Property | Method::Rental)) && !case.is_pure_deal_completion()
    {
        options.choices.push(Choice{id:"deal.price".into(),label:"The price".into(),house:Some(turn(base,10)),natural:None,basis:"Property and its price are distinct: fourth/tenth in the relevant frame, Frawley pp. 167–170.".into()});
        options.required_groups.push(vec!["deal.price".into()]);
    }
    if matches!(method, Some(Method::Property | Method::Rental))
        && case
            .frame
            .resolved()
            .is_some_and(|frame| frame.facet == crate::reading_contracts::Facet::Profit)
    {
        options.choices.push(Choice {
            id: "deal.property_profit".into(),
            label: "Return from the property".into(),
            house: Some(turn(base, 5)),
            natural: None,
            basis: "Property bought to improve/resell or let has its profit in the second from the property's fourth, the relevant fifth. This is distinct from the purchase price and working a business on the premises; Frawley printed p. 170.".into(),
        });
        options
            .required_groups
            .push(vec!["deal.property_profit".into()]);
    }
    let actor = case.deal_actor().unwrap_or("");
    let participant_house = |id: &str| {
        if id == "querent" || id == principal {
            Some(1)
        } else {
            relevant
                .iter()
                .find(|p| p.id == id)
                .and_then(|p| relationship_house(&p.relationship))
        }
    };
    let actor_house = participant_house(actor);
    if matches!(
        method,
        Some(Method::MovableDeal | Method::Property | Method::Rental)
    ) && case.text(Field::DealParty).is_none()
    {
        if let Some(actor_house) = actor_house {
            let house = turn(actor_house, 7);
            options.choices.push(Choice {
                id: "deal.counterparty".into(),
                label: "The other party in the deal".into(),
                house: Some(house),
                natural: None,
                basis: format!("The unnamed other party is seventh from the deal actor's house {actor_house}; completion concerns the parties, not goods touching a buyer (Frawley pp. 168–172)."),
            });
            options
                .required_groups
                .push(vec!["deal.counterparty".into()]);
        } else {
            options
                .missing
                .push("The deal actor's operative capacity has not been resolved.".into());
        }
    }
    if method == Some(Method::MovableDeal)
        && case.text(Field::DealCapacity) == Some("sell")
        && case
            .frame
            .resolved()
            .is_some_and(|frame| frame.facet == crate::reading_contracts::Facet::Profit)
    {
        let customer_house = match case.text(Field::DealParty) {
            Some(id) if id == "querent" || id == principal => Some(1),
            Some(id) => relevant
                .iter()
                .find(|p| p.id == id)
                .and_then(|p| relationship_house(&p.relationship)),
            None => actor_house.map(|house| turn(house, 7)),
        };
        let beneficiary_house = case
            .text(Field::DealBeneficiary)
            .and_then(participant_house);
        if let (Some(beneficiary_house), Some(customer_house)) = (beneficiary_house, customer_house)
        {
            for (id, label, house, basis) in [
                ("deal.incoming_money", "The buyers' money", turn(customer_house, 2), "Customers' money is their second, ordinarily eighth from the recipient. Its condition concerns amount/quality; goods dignity alone cannot establish financial gain."),
                ("deal.pocket", "The recipient's money", turn(beneficiary_house, 2), "Use the separately sourced financial beneficiary, never an owner or contracting seller default. Keep their second-house pocket distinct from incoming money and the goods, even when rulers coincide. Whether money arrives and whether an amount is good are separate judgments."),
            ] {
                options.choices.push(Choice { id: id.into(), label: label.into(), house: Some(house), natural: None, basis: format!("{basis} Frawley printed pp. 156–158.") });
                options.required_groups.push(vec![id.into()]);
            }
        } else {
            options.missing.push("The actual recipient or identified buyer's capacity is unresolved; do not invent a house for their money.".into());
        }
    }
    // Some method overrides rebuild the choices. Apply the relay identity last.
    if relay {
        if method == Some(Method::Relationship) {
            if let Some(moon) = options
                .choices
                .iter_mut()
                .find(|c| c.id == "moon.contextual")
            {
                moon.label = case
                    .people
                    .get(principal)
                    .map(|p| format!("{}'s feelings", p.label))
                    .unwrap_or_else(|| "The principal's feelings".into());
            }
        }
        if let Some(querent) = options.choices.iter_mut().find(|c| c.id == "querent.self") {
            querent.label = case
                .people
                .get(principal)
                .map(|p| p.label.clone())
                .unwrap_or_else(|| "The person whose question is relayed".into());
            querent.basis = "The genuine principal receives first; the speaker is a mouthpiece (Frawley pp. 137–138).".into();
        }
        let redundant = format!("{principal}.self");
        options.choices.retain(|c| c.id != redundant);
        options.required_groups.retain(|g| !g.contains(&redundant));
    }
    options
}

pub fn contract(options: &Options) -> Value {
    let ids: Vec<_> = options
        .choices
        .iter()
        .map(|choice| choice.id.as_str())
        .collect();
    let mut contract = json!({"type":"object","properties":{
        "selections":{"type":"array","maxItems":8,"items":{"type":"object","properties":{"id":{"type":"string","enum":ids},"reason":{"type":"string","maxLength":240}},"required":["id","reason"],"additionalProperties":false}},
        "summary":{"type":"string","maxLength":350},"unknowns":{"type":"array","maxItems":3,"items":{"type":"string","maxLength":150}}
    },"required":["selections","summary","unknowns"],"additionalProperties":false});
    if !options.compare.is_empty() {
        contract["properties"]["comparison"] = json!({"type":"array","maxItems":2,"items":{"type":"object","properties":{"id":{"type":"string","enum":options.compare},"observation":{"type":"string","maxLength":240}},"required":["id","observation"],"additionalProperties":false}});
        contract["required"]
            .as_array_mut()
            .expect("Required is an array")
            .push(json!("comparison"));
    }
    contract
}

pub fn resolve(options: &Options, value: &Value, facts: &[Fact]) -> Result<Vec<Role>, String> {
    if !options.missing.is_empty() {
        return Err(
            "Resolve the listed missing person/ownership context before selecting roles.".into(),
        );
    }
    let selections = value["selections"]
        .as_array()
        .ok_or("Select native role options")?;
    let mut selected = std::collections::BTreeSet::new();
    let mut choices = Vec::new();
    for selection in selections {
        let id = selection["id"]
            .as_str()
            .ok_or("Use a supplied role option ID")?;
        if !selected.insert(id) {
            return Err("Select each role option only once.".into());
        }
        let choice = options
            .choices
            .iter()
            .find(|choice| choice.id == id)
            .ok_or("Use a supplied role option ID")?;
        choices.push(RoleChoice {
            label: choice.label.clone(),
            house: choice.house,
            natural: choice.natural,
            reason: selection["reason"].as_str().unwrap_or("").into(),
        });
    }
    for group in &options.required_groups {
        let count = group
            .iter()
            .filter(|id| selected.contains(id.as_str()))
            .count();
        if count == 0 {
            return Err(format!("Missing required role: select exactly one of [{}]. A person's own role is distinct from their possessions.", group.join(", ")));
        }
        if count > 1 {
            return Err(format!("Select exactly ONE alternative from [{}]; you selected {count}. Compare alternatives in comparison, then choose one in selections now. Do not select every alternative or postpone the choice.", group.join(", ")));
        }
    }
    if !options.compare.is_empty() {
        let comparison = value["comparison"]
            .as_array()
            .ok_or("Compare both own-object candidates before selecting one")?;
        if comparison.len() != options.compare.len()
            || options.compare.iter().any(|id| {
                comparison
                    .iter()
                    .filter(|entry| {
                        entry["id"] == id.as_str()
                            && entry["observation"]
                                .as_str()
                                .is_some_and(|s| s.trim().len() >= 12)
                    })
                    .count()
                    != 1
            })
        {
            return Err(
                "Explain the comparison of both Lords 2 and 4; choose one actual object role."
                    .into(),
            );
        }
    }
    let roles = reading_method::assign_from_facts(facts, choices)?;
    for requirement in &options.conditional_roles {
        let claimed = roles.iter().any(|role| {
            role.house.is_some() && role.planet == requirement.unless_house_claims.planet()
        });
        if !claimed && !selected.contains(requirement.choice_id.as_str()) {
            return Err(format!(
                "Missing conditional role {}: {} Select it now; the supplied main house rulers do not claim {}.",
                requirement.choice_id,
                requirement.basis,
                requirement.unless_house_claims.planet()
            ));
        }
    }
    Ok(roles)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reading_contracts::{
        Consultation, Evidence, Facet, Field, Frame, Method, Observation, Slot,
    };

    fn facts() -> Vec<Fact> {
        let chart = horary_ai_core::astronomy::chart(1789387200000., 38.657, -77.249).unwrap();
        reading_method::facts(Some(&chart))
    }

    fn resolved<T>(value: T) -> Slot<T> {
        Slot::Resolved {
            observation: Observation {
                value,
                evidence: Evidence::Migration {
                    detail: "Authored money role coverage".into(),
                },
            },
        }
    }

    fn money_person(id: &str, relationship: &str) -> Person {
        Person {
            id: id.into(),
            label: id.into(),
            relationship: relationship.into(),
            source_quote: "The supplied participant and their stated relationship.".into(),
        }
    }

    fn money_case(
        facet: Facet,
        recipient: &str,
        source: &str,
        people: &[Person],
    ) -> (Consultation, Subject) {
        let subject = Subject {
            name: "The incoming money".into(),
            kind: "money".into(),
            owner_id: recipient.into(),
            source_quote: "The stated recipient's incoming money.".into(),
        };
        let mut case = Consultation {
            frame: resolved(Frame {
                method: Method::Money,
                facet,
            }),
            subject: resolved(subject.clone()),
            people: people.iter().cloned().map(|p| (p.id.clone(), p)).collect(),
            ..Default::default()
        };
        case.facts
            .insert(Field::MoneySource, resolved(source.into()));
        (case, subject)
    }

    fn money_options(case: &Consultation, subject: &Subject) -> Options {
        let people: Vec<_> = case.people.values().cloned().collect();
        build_for(case, Matter::Other, &people, subject)
    }

    fn choice_house(options: &Options, id: &str) -> Option<u8> {
        options.choices.iter().find(|c| c.id == id).unwrap().house
    }

    fn money_worksheet(options: &Options, moon: bool) -> Value {
        let mut selections: Vec<_> = options
            .required_groups
            .iter()
            .map(|group| {
                assert_eq!(group.len(), 1, "Money roles have no rival-house candidates");
                json!({"id":group[0],"reason":"The participant's supplied native capacity."})
            })
            .collect();
        if moon {
            selections.push(json!({"id":"moon.contextual","reason":"The genuine recipient's Moon without a competing house claim."}));
        }
        json!({"selections":selections,"summary":"Incoming money and its recipient's pocket are distinct roles.","unknowns":[]})
    }

    fn money_house_facts() -> Vec<Fact> {
        // Authored ruler inputs exercise the native binding, not astronomy or
        // a reading verdict. Initially no house has prior claim on Moon.
        [
            "Jupiter", "Saturn", "Mars", "Mercury", "Venus", "Saturn", "Sun", "Mars", "Jupiter",
            "Mercury", "Venus", "Sun",
        ]
        .into_iter()
        .enumerate()
        .map(|(index, planet)| Fact {
            id: format!("house.{}", index + 1),
            kind: "house".into(),
            label: format!("House {}", index + 1),
            detail: "Authored house-ruler input for role coverage.".into(),
            planets: vec![planet.into()],
            condition_facet: None,
            event: None,
        })
        .collect()
    }

    #[test]
    fn money_arrival_requires_pocket_and_keeps_all_three_contact_endpoints() {
        for facet in [Facet::Event, Facet::Timing] {
            let (case, subject) = money_case(facet, "querent", "job", &[]);
            let options = money_options(&case, &subject);
            assert_eq!(choice_house(&options, "subject.primary"), Some(11));
            assert_eq!(choice_house(&options, "money.recipient_pocket"), Some(2));
            assert_eq!(options.conditional_roles.len(), 1);
            let native = money_house_facts();
            let mut worksheet = json!({"selections":[
                {"id":"querent.self","reason":"The genuine recipient of the wages."},
                {"id":"subject.primary","reason":"The employer's money, eleventh house."}
            ],"summary":"Arrival needs the recipient's actual routes.","unknowns":[]});
            let missing_pocket = resolve(&options, &worksheet, &native).unwrap_err();
            assert!(missing_pocket.contains("money.recipient_pocket"));
            worksheet["selections"].as_array_mut().unwrap().push(json!({
                "id":"money.recipient_pocket","reason":"The recipient's bank account, second house."
            }));
            let missing_moon = resolve(&options, &worksheet, &native).unwrap_err();
            assert!(missing_moon.contains("Missing conditional role moon.contextual"));
            worksheet["selections"].as_array_mut().unwrap().push(json!({
                "id":"moon.contextual","reason":"The genuine recipient's unclaimed co-significator."
            }));
            let roles = resolve(&options, &worksheet, &native).unwrap();
            assert_eq!(roles.len(), 4);
            let money = roles.iter().find(|r| r.house == Some(11)).unwrap();
            assert_eq!(money.planet, "Venus");
            // Each valid arrival route has both endpoints in the selected
            // role set. This makes no claim that an actual aspect perfects.
            for (house, planet) in [(Some(1), "Jupiter"), (None, "Moon"), (Some(2), "Saturn")] {
                assert!(roles.iter().any(|r| r.house == house && r.planet == planet));
            }
        }
    }

    #[test]
    fn money_arrival_house_claim_on_moon_overrides_its_contextual_role() {
        let (case, subject) = money_case(Facet::Timing, "querent", "job", &[]);
        let options = money_options(&case, &subject);
        for house in [1, 2, 11] {
            let mut native = money_house_facts();
            native
                .iter_mut()
                .find(|f| f.label == format!("House {house}"))
                .unwrap()
                .planets = vec!["Moon".into()];
            let roles = resolve(&options, &money_worksheet(&options, false), &native).unwrap();
            assert!(roles
                .iter()
                .any(|r| r.house == Some(house) && r.planet == "Moon"));
            assert!(!roles.iter().any(|r| r.house.is_none()));
            let duplicate =
                resolve(&options, &money_worksheet(&options, true), &native).unwrap_err();
            assert!(duplicate.contains("first claim"));
        }
    }

    #[test]
    fn money_arrival_relative_sender_retains_their_money_separately_from_the_recipient() {
        for (relationship, sender_house, money_house) in [("sibling", 3, 4), ("friend", 11, 12)] {
            let sender = money_person("debtor", relationship);
            let (mut case, subject) = money_case(Facet::Event, "querent", "relative", &[sender]);
            case.facts.insert(Field::Sender, resolved("debtor".into()));
            let options = money_options(&case, &subject);
            assert_eq!(choice_house(&options, "debtor.self"), Some(sender_house));
            assert_eq!(choice_house(&options, "subject.primary"), Some(money_house));
            assert_eq!(choice_house(&options, "money.recipient_pocket"), Some(2));
            let roles = resolve(
                &options,
                &money_worksheet(&options, true),
                &money_house_facts(),
            )
            .unwrap();
            assert_eq!(roles.len(), 5);
            assert!(roles.iter().any(|r| r.house == Some(money_house)));
            assert!(roles
                .iter()
                .any(|r| r.house.is_none() && r.planet == "Moon"));
        }
    }

    #[test]
    fn money_arrival_about_another_person_turns_their_pocket_without_giving_them_moon() {
        for (relationship, recipient_house, salary_house, pocket_house) in [
            ("child", 5, 3, 6),
            ("partner", 7, 5, 8),
            ("friend", 11, 9, 12),
        ] {
            let recipient = money_person("recipient", relationship);
            let (case, subject) = money_case(Facet::Event, "recipient", "job", &[recipient]);
            let options = money_options(&case, &subject);
            assert_eq!(
                choice_house(&options, "recipient.self"),
                Some(recipient_house)
            );
            assert_eq!(
                choice_house(&options, "subject.primary"),
                Some(salary_house)
            );
            assert_eq!(
                choice_house(&options, "money.recipient_pocket"),
                Some(pocket_house)
            );
            assert!(options.conditional_roles.is_empty());
            let roles = resolve(
                &options,
                &money_worksheet(&options, false),
                &money_house_facts(),
            )
            .unwrap();
            assert_eq!(roles.len(), 4);
            assert!(!roles.iter().any(|r| r.house.is_none()));
        }
    }

    #[test]
    fn money_arrival_genuine_relay_principal_has_first_house_pocket_and_moon() {
        let recipient = money_person("recipient", "child");
        let sender = money_person("debtor", "sibling");
        for source in ["job", "relative"] {
            let (mut case, subject) = money_case(
                Facet::Timing,
                "recipient",
                source,
                &[recipient.clone(), sender.clone()],
            );
            case.facts
                .insert(Field::PrincipalMode, resolved("relay".into()));
            case.facts
                .insert(Field::PrincipalId, resolved("recipient".into()));
            if source == "relative" {
                case.facts.insert(Field::Sender, resolved("debtor".into()));
            }
            let options = money_options(&case, &subject);
            assert_eq!(choice_house(&options, "querent.self"), Some(1));
            assert_eq!(
                options
                    .choices
                    .iter()
                    .find(|c| c.id == "querent.self")
                    .unwrap()
                    .label,
                "recipient"
            );
            assert!(!options.choices.iter().any(|c| c.id == "recipient.self"));
            assert_eq!(choice_house(&options, "money.recipient_pocket"), Some(2));
            // Relative sender turning is retained, not guessed or rebased
            // from the speaker's presumed relationship to the principal.
            assert_eq!(
                choice_house(&options, "subject.primary"),
                Some(if source == "job" { 11 } else { 4 })
            );
            assert_eq!(options.conditional_roles.len(), 1);
            let roles = resolve(
                &options,
                &money_worksheet(&options, true),
                &money_house_facts(),
            )
            .unwrap();
            assert!(roles
                .iter()
                .any(|r| r.house.is_none() && r.planet == "Moon"));
        }
    }

    #[test]
    fn money_arrival_relay_owner_alias_does_not_invent_principal_ownership_for_moon() {
        let principal = money_person("recipient", "child");
        let (mut case, subject) = money_case(Facet::Event, "querent", "job", &[principal]);
        case.facts
            .insert(Field::PrincipalMode, resolved("relay".into()));
        case.facts
            .insert(Field::PrincipalId, resolved("recipient".into()));
        let options = money_options(&case, &subject);
        // Existing base logic treats the literal querent alias as first. This
        // test does NOT qualify that ambiguous ownership/house frame; it only
        // protects the new Moon obligation from transferring to an unbound ID.
        assert!(options.conditional_roles.is_empty());
    }

    #[test]
    fn money_amount_and_quality_do_not_acquire_arrival_role_obligations() {
        for facet in [Facet::Profit, Facet::Situation, Facet::Quantity] {
            let (case, subject) = money_case(facet, "querent", "job", &[]);
            let options = money_options(&case, &subject);
            assert_eq!(choice_house(&options, "subject.primary"), Some(11));
            assert!(!options
                .choices
                .iter()
                .any(|c| c.id == "money.recipient_pocket"));
            assert!(options.conditional_roles.is_empty());
            let roles = resolve(
                &options,
                &money_worksheet(&options, false),
                &money_house_facts(),
            )
            .unwrap();
            assert_eq!(roles.len(), 2);
        }
    }

    fn job_case(
        method: Method,
        facet: Facet,
        worker: &str,
        people: &[Person],
    ) -> (Consultation, Subject) {
        let subject = Subject {
            name: "The stated job".into(),
            kind: "job".into(),
            owner_id: worker.into(),
            source_quote: "The stated worker's job.".into(),
        };
        let frame = Slot::Resolved {
            observation: Observation {
                value: Frame { method, facet },
                evidence: Evidence::Migration {
                    detail: "Authored job role coverage, not a model judgment.".into(),
                },
            },
        };
        (
            Consultation {
                frame,
                subject: resolved(subject.clone()),
                people: people.iter().cloned().map(|p| (p.id.clone(), p)).collect(),
                ..Default::default()
            },
            subject,
        )
    }

    fn job_options(case: &Consultation, subject: &Subject) -> Options {
        let people: Vec<_> = case.people.values().cloned().collect();
        build_for(case, Matter::Work, &people, subject)
    }

    fn job_worksheet(options: &Options, moon: bool) -> Value {
        let mut selections: Vec<_> = options
            .required_groups
            .iter()
            .map(|group| {
                assert_eq!(
                    group.len(),
                    1,
                    "These job roles are identified, not alternatives"
                );
                json!({"id":group[0],"reason":"The stated worker and job keep their native roles."})
            })
            .collect();
        if moon {
            selections.push(json!({"id":"moon.contextual","reason":"The genuine principal's unclaimed co-significator."}));
        }
        json!({"selections":selections,"summary":"Role coverage establishes no event or date.","unknowns":[]})
    }

    #[test]
    fn job_event_roles_require_the_genuine_workers_unclaimed_moon() {
        for method in [Method::NewJob, Method::ReturnToJob] {
            for facet in [Facet::Event, Facet::Timing] {
                let (case, subject) = job_case(method, facet, "querent", &[]);
                let options = job_options(&case, &subject);
                let native = money_house_facts();
                let omitted =
                    resolve(&options, &job_worksheet(&options, false), &native).unwrap_err();
                assert!(omitted.contains("Missing conditional role moon.contextual"));
                let roles = resolve(&options, &job_worksheet(&options, true), &native).unwrap();
                assert_eq!(roles.len(), 3);
                assert!(roles
                    .iter()
                    .any(|r| r.house == Some(1) && r.planet == "Jupiter"));
                assert!(roles
                    .iter()
                    .any(|r| r.house == Some(10) && r.planet == "Mercury"));
                assert!(roles
                    .iter()
                    .any(|r| r.house.is_none() && r.planet == "Moon"));
            }
        }
    }

    #[test]
    fn job_event_moon_keeps_selected_house_rulers_first_claim() {
        for method in [Method::NewJob, Method::ReturnToJob] {
            let (case, subject) = job_case(method, Facet::Timing, "querent", &[]);
            let options = job_options(&case, &subject);
            for house in [1, 10] {
                let mut native = money_house_facts();
                native
                    .iter_mut()
                    .find(|fact| fact.label == format!("House {house}"))
                    .unwrap()
                    .planets = vec!["Moon".into()];
                let roles = resolve(&options, &job_worksheet(&options, false), &native).unwrap();
                assert!(roles
                    .iter()
                    .any(|r| r.house == Some(house) && r.planet == "Moon"));
                assert!(!roles.iter().any(|r| r.house.is_none()));
                let duplicate =
                    resolve(&options, &job_worksheet(&options, true), &native).unwrap_err();
                assert!(duplicate.contains("first claim"));
            }
        }
    }

    #[test]
    fn job_event_moon_does_not_transfer_to_a_third_person_worker() {
        for method in [Method::NewJob, Method::ReturnToJob] {
            for (relationship, worker_house) in [("child", 5), ("partner", 7)] {
                let worker = money_person("worker", relationship);
                let (mut case, subject) = job_case(method, Facet::Event, "worker", &[worker]);
                case.facts
                    .insert(Field::PrincipalMode, resolved("concerning_other".into()));
                let options = job_options(&case, &subject);
                assert!(options.conditional_roles.is_empty());
                assert_eq!(choice_house(&options, "worker.self"), Some(worker_house));
                assert_eq!(
                    choice_house(&options, "subject.primary"),
                    Some(if method == Method::NewJob {
                        10
                    } else {
                        turn(worker_house, 10)
                    })
                );
                let roles = resolve(
                    &options,
                    &job_worksheet(&options, false),
                    &money_house_facts(),
                )
                .unwrap();
                assert!(!roles
                    .iter()
                    .any(|r| r.house.is_none() && r.planet == "Moon"));
            }
        }
    }

    #[test]
    fn job_event_relay_requires_exact_principal_binding_without_role_aliases() {
        for method in [Method::NewJob, Method::ReturnToJob] {
            let worker = money_person("worker", "unknown");
            let (mut case, subject) = job_case(method, Facet::Timing, "worker", &[worker]);
            case.facts
                .insert(Field::PrincipalMode, resolved("relay".into()));
            case.facts
                .insert(Field::PrincipalId, resolved("worker".into()));
            let options = job_options(&case, &subject);
            assert_eq!(options.conditional_roles.len(), 1);
            assert_eq!(choice_house(&options, "querent.self"), Some(1));
            assert_eq!(choice_house(&options, "subject.primary"), Some(10));
            assert!(!options
                .choices
                .iter()
                .any(|choice| choice.id == "worker.self"));
            assert!(!options
                .required_groups
                .iter()
                .flatten()
                .any(|id| id == "worker.self"));
            let roles = resolve(
                &options,
                &job_worksheet(&options, true),
                &money_house_facts(),
            )
            .unwrap();
            assert!(roles
                .iter()
                .any(|r| r.house == Some(1) && r.label == "worker"));
            assert!(roles
                .iter()
                .any(|r| r.house.is_none() && r.planet == "Moon"));
            let mut alias = job_worksheet(&options, true);
            alias["selections"][0]["id"] = json!("worker.self");
            assert!(resolve(&options, &alias, &money_house_facts())
                .unwrap_err()
                .contains("supplied role option ID"));

            // Literal querent cannot stand in for an identified relayed worker.
            // Nor does a missing PrincipalId make build_for's first-house
            // fallback a genuine principal. These incomplete input frames are
            // not qualified here; only the new Moon obligation is withheld.
            let mut unbound = subject.clone();
            unbound.owner_id = "querent".into();
            assert!(job_options(&case, &unbound).conditional_roles.is_empty());
            case.facts.remove(&Field::PrincipalId);
            assert!(job_options(&case, &unbound).conditional_roles.is_empty());
            assert!(job_options(&case, &subject).conditional_roles.is_empty());
        }
    }

    #[test]
    fn job_event_moon_does_not_broaden_methods_or_non_event_facets() {
        for method in [Method::NewJob, Method::ReturnToJob] {
            for facet in [
                Facet::Situation,
                Facet::Profit,
                Facet::Choice,
                Facet::Quantity,
            ] {
                let (case, subject) = job_case(method, facet, "querent", &[]);
                assert!(job_options(&case, &subject).conditional_roles.is_empty());
            }
        }
        for method in [Method::ExistingJob, Method::JobOffer] {
            for facet in [Facet::Event, Facet::Timing, Facet::Situation] {
                let (case, subject) = job_case(method, facet, "querent", &[]);
                assert!(job_options(&case, &subject).conditional_roles.is_empty());
            }
        }
    }

    #[test]
    fn relationship_requires_emotional_moon_without_competing_with_a_house_ruler() {
        use crate::reading_contracts::{
            Consultation, Evidence, Facet, Frame, Method, Observation, Slot,
        };
        for (relationship, target_house) in [("partner", 7), ("neighbor", 3)] {
            let case = Consultation {
                frame: Slot::Resolved {
                    observation: Observation {
                        value: Frame {
                            method: Method::Relationship,
                            facet: Facet::Situation,
                        },
                        evidence: Evidence::Migration {
                            detail: "Relationship Moon coverage".into(),
                        },
                    },
                },
                ..Default::default()
            };
            let person = Person {
                id: "alex".into(),
                label: "Alex".into(),
                relationship: relationship.into(),
                source_quote: "Alex's stated relationship".into(),
            };
            let subject = Subject {
                name: "Alex".into(),
                kind: "person".into(),
                owner_id: "alex".into(),
                source_quote: "Alex's stated relationship".into(),
            };
            let options = build_for(&case, Matter::Relationship, &[person], &subject);
            assert_eq!(options.conditional_roles.len(), 1);
            for claimed_house in [None, Some(1), Some(target_house)] {
                let mut native = facts();
                for fact in native.iter_mut().filter(|f| f.kind == "house") {
                    if fact.label == "House 1" {
                        fact.planets = vec!["Jupiter".into()];
                    }
                    if fact.label == format!("House {target_house}") {
                        fact.planets = vec!["Mercury".into()];
                    }
                    if claimed_house.is_some_and(|h| fact.label == format!("House {h}")) {
                        fact.planets = vec!["Moon".into()];
                    }
                }
                let mut worksheet = json!({"selections":[
                    {"id":"querent.self","reason":"The querent's considered position."},
                    {"id":"alex.self","reason":"The person in their actual capacity."}
                ],"summary":"Head and heart remain distinct.","unknowns":[]});
                let result = resolve(&options, &worksheet, &native);
                if claimed_house.is_none() {
                    assert!(result.unwrap_err().contains("moon.contextual"));
                } else {
                    assert!(result
                        .unwrap()
                        .iter()
                        .any(|r| r.planet == "Moon" && r.house == claimed_house));
                }
                worksheet["selections"].as_array_mut().unwrap().push(json!({"id":"moon.contextual","reason":"The querent's emotional facet, distinct from their primary ruler."}));
                let with_moon = resolve(&options, &worksheet, &native);
                if claimed_house.is_none() {
                    assert!(with_moon
                        .unwrap()
                        .iter()
                        .any(|r| r.planet == "Moon" && r.house.is_none()));
                } else {
                    assert!(with_moon.unwrap_err().contains("first claim"));
                }
            }
            // This obligation belongs to the relationship method, not to the
            // mere existence of a Moon option in every role table.
            assert!(build(Matter::Other, &[], &Subject::default())
                .conditional_roles
                .is_empty());
            assert!(build(
                Matter::LostObject,
                &[],
                &Subject {
                    name: "Watch".into(),
                    kind: "movable".into(),
                    owner_id: "querent".into(),
                    source_quote: "My watch".into()
                }
            )
            .conditional_roles
            .is_empty());
        }
    }

    #[test]
    fn competing_object_candidates_receive_exact_one_feedback_instead_of_missing_role_feedback() {
        let options = build(
            Matter::LostObject,
            &[],
            &Subject {
                name: "Watch".into(),
                kind: "movable".into(),
                owner_id: "querent".into(),
                source_quote: "My gold watch".into(),
            },
        );
        let mut worksheet = json!({"selections":[
            {"id":"querent.self","reason":"The querent owns the missing watch."},
            {"id":"subject.primary","reason":"The second-house candidate."},
            {"id":"subject.alternative_fourth","reason":"The fourth-house candidate."}
        ],"comparison":[
            {"id":"subject.primary","observation":"One candidate fits part of the supplied appearance."},
            {"id":"subject.alternative_fourth","observation":"The other fits the supplied appearance less closely."}
        ],"summary":"Compare candidates, then choose one now.","unknowns":[]});
        let error = resolve(&options, &worksheet, &facts()).unwrap_err();
        assert!(error.contains("exactly ONE"));
        assert!(error.contains("subject.primary"));
        assert!(error.contains("subject.alternative_fourth"));
        assert!(!error.contains("Missing required role"));
        worksheet["selections"].as_array_mut().unwrap().pop();
        assert_eq!(resolve(&options, &worksheet, &facts()).unwrap().len(), 2);
        worksheet["selections"].as_array_mut().unwrap().pop();
        let missing = resolve(&options, &worksheet, &facts()).unwrap_err();
        assert!(missing.contains("Missing required role"));
        assert!(missing.contains("select exactly one"));
    }

    fn stock(relationship: &str) -> Options {
        build(
            Matter::Other,
            &[Person {
                id: "bob".into(),
                label: "Bob".into(),
                relationship: relationship.into(),
                source_quote: "Bob is my husband. They are his books.".into(),
            }],
            &Subject {
                name: "Bob's books".into(),
                kind: "movable".into(),
                owner_id: "bob".into(),
                source_quote: "They are his books.".into(),
            },
        )
    }

    #[test]
    fn available_job_pay_is_bound_to_the_job_not_indiscriminately_to_the_worker() {
        use crate::reading_contracts::{
            Consultation, Evidence, Facet, Frame, Method, Observation, Slot,
        };
        for (relationship, job_house, pay_house) in
            [("querent", 10, 11), ("child", 10, 11), ("mother", 7, 8)]
        {
            let mut case = Consultation {
                frame: Slot::Resolved {
                    observation: Observation {
                        value: Frame {
                            method: Method::JobOffer,
                            facet: Facet::Profit,
                        },
                        evidence: Evidence::Migration {
                            detail: "Available-offer role regression".into(),
                        },
                    },
                },
                ..Default::default()
            };
            let person = Person {
                id: "worker".into(),
                label: "The worker".into(),
                relationship: relationship.into(),
                source_quote: "The worker has an available offer.".into(),
            };
            let owner = if relationship == "querent" {
                "querent"
            } else {
                "worker"
            };
            let subject = Subject {
                name: "Available job".into(),
                kind: "job".into(),
                owner_id: owner.into(),
                source_quote: "The worker has an available offer.".into(),
            };
            let people = if owner == "querent" {
                vec![]
            } else {
                vec![person]
            };
            let options = build_for(&case, Matter::Other, &people, &subject);
            assert_eq!(
                options
                    .choices
                    .iter()
                    .find(|c| c.id == "subject.primary")
                    .unwrap()
                    .house,
                Some(job_house)
            );
            assert_eq!(
                options
                    .choices
                    .iter()
                    .find(|c| c.id == "job.wages")
                    .unwrap()
                    .house,
                Some(pay_house)
            );
            let mut selections = vec![
                json!({"id":"querent.self","reason":"The principal."}),
                json!({"id":"subject.primary","reason":"The external available job."}),
            ];
            if owner != "querent" {
                selections.push(json!({"id":"worker.self","reason":"The identified worker."}));
            }
            let mut value = json!({"selections":selections,"summary":"Offer pay is distinct from job acquisition.","unknowns":[]});
            assert!(
                resolve(&options, &value, &facts()).is_err(),
                "A profit judgment cannot omit pay"
            );
            value["selections"].as_array_mut().unwrap().push(
                json!({"id":"job.wages","reason":"Job money rather than an acquisition aspect."}),
            );
            let roles = resolve(&options, &value, &facts()).unwrap();
            assert_eq!(
                roles
                    .iter()
                    .find(|role| role.label == "The offered job's pay")
                    .unwrap()
                    .house,
                Some(pay_house)
            );
            case.frame = Slot::Resolved {
                observation: Observation {
                    value: Frame {
                        method: Method::JobOffer,
                        facet: Facet::Situation,
                    },
                    evidence: Evidence::Migration {
                        detail: "Non-financial offer suitability".into(),
                    },
                },
            };
            let situation = build_for(&case, Matter::Other, &people, &subject);
            assert!(situation
                .choices
                .iter()
                .any(|choice| choice.id == "job.wages"));
            assert!(
                !situation
                    .required_groups
                    .iter()
                    .any(|group| group.iter().any(|id| id == "job.wages")),
                "An evenings-only concern must not compulsorily become a pay judgment"
            );
        }
    }

    #[test]
    fn selling_books_requires_bob_separately_from_his_stock() {
        let options = stock("partner");
        let mut value = json!({"selections":[
            {"id":"querent.self","reason":"The person asking."},
            {"id":"subject.primary","reason":"The husband's possessions."}
        ],"summary":"Authored role regression.","unknowns":[]});
        assert!(resolve(&options, &value, &facts()).is_err());
        value["selections"]
            .as_array_mut()
            .unwrap()
            .push(json!({"id":"bob.self","reason":"The stated husband."}));
        let roles = resolve(&options, &value, &facts()).unwrap();
        assert_eq!(
            roles.iter().find(|r| r.label == "Bob").unwrap().house,
            Some(7)
        );
        assert_eq!(
            roles
                .iter()
                .find(|r| r.label == "Bob's books")
                .unwrap()
                .house,
            Some(8)
        );
    }

    #[test]
    fn unnamed_relationship_cannot_be_completed_as_an_assumed_other_party() {
        let options = stock("unknown");
        let value = json!({"selections":[
            {"id":"querent.self","reason":"The person asking."},
            {"id":"moon.contextual","reason":"Attempted substitute for missing context."}
        ],"summary":"Authored incomplete role proposal.","unknowns":[]});
        assert!(!options.missing.is_empty());
        assert!(resolve(&options, &value, &facts()).is_err());
    }

    #[test]
    fn lost_object_comparison_selects_one_role_only_after_comparing_both() {
        let options = build(
            Matter::LostObject,
            &[],
            &Subject {
                name: "Ring".into(),
                kind: "movable".into(),
                owner_id: "querent".into(),
                source_quote: "Where is my ring?".into(),
            },
        );
        let mut value = json!({"selections":[
            {"id":"querent.self","reason":"The person asking."},
            {"id":"subject.primary","reason":"The supplied second-house candidate."}
        ],"summary":"Authored lost-object regression.","unknowns":[]});
        assert!(resolve(&options, &value, &facts()).is_err());
        value["comparison"] = json!([
            {"id":"subject.primary","observation":"Authored comparison of the supplied Lord 2 facts."},
            {"id":"subject.alternative_fourth","observation":"Authored comparison of the supplied Lord 4 facts."}
        ]);
        assert!(resolve(&options, &value, &facts()).is_ok());
        value["selections"].as_array_mut().unwrap().push(
            json!({"id":"subject.alternative_fourth","reason":"Attempted second object role."}),
        );
        assert!(resolve(&options, &value, &facts()).is_err());
    }
}
