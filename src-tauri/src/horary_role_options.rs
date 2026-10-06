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

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Person {
    pub id: String,
    pub label: String,
    pub relationship: String,
    pub source_quote: String,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Subject {
    pub name: String,
    pub kind: String,
    pub owner_id: String,
    pub source_quote: String,
}
impl Subject {
    pub fn is_empty(&self) -> bool {
        self.name.is_empty()
            && self.kind.is_empty()
            && self.owner_id.is_empty()
            && self.source_quote.is_empty()
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
pub struct Options {
    pub choices: Vec<Choice>,
    pub required_groups: Vec<Vec<String>>,
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

pub fn build(matter: Matter, people: &[Person], subject: &Subject) -> Options {
    let mut options = Options {
        choices: Vec::new(),
        required_groups: Vec::new(),
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
    if options.required_groups.iter().any(|group| {
        group
            .iter()
            .filter(|id| selected.contains(id.as_str()))
            .count()
            != 1
    }) {
        return Err("Supply every required person/object role. A person's own house is distinct from the house of their possessions.".into());
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
    reading_method::assign_from_facts(facts, choices)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn facts() -> Vec<Fact> {
        let chart = horary_ai_core::astronomy::chart(1789387200000., 38.657, -77.249).unwrap();
        reading_method::facts(Some(&chart))
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
