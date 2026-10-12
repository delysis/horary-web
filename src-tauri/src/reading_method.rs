//! Inspectable editorial rules and calculated evidence; never model-made astronomy.
#![forbid(unsafe_code)]
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Step {
    Significators,
    Testimony,
    Judgment,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BookRule {
    pub id: String,
    pub title: String,
    pub explanation: String,
    pub pages: String,
    #[serde(default)]
    pub quoted: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Fact {
    pub id: String,
    pub kind: String,
    pub label: String,
    pub detail: String,
    pub planets: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub condition_facet: Option<horary_ai_core::book_method::ConditionFacet>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub event: Option<EventCandidate>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reception: Option<DirectedReception>,
}

/// The calculator's directed rows, without a strength sum or assigned meaning.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DirectedReception {
    pub guest_planet: String,
    pub host_planet: String,
    pub qualities: Vec<ReceptionQuality>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ReceptionQuality {
    pub dignity: ReceptionDignity,
    pub polarity: ReceptionPolarity,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ReceptionDignity {
    Domicile,
    Exaltation,
    TriplicityRuler,
    TermRuler,
    FaceRuler,
    Detriment,
    Fall,
}

impl ReceptionDignity {
    pub fn native_name(self) -> &'static str {
        match self {
            Self::Domicile => "domicile",
            Self::Exaltation => "exaltation",
            Self::TriplicityRuler => "triplicityRuler",
            Self::TermRuler => "termRuler",
            Self::FaceRuler => "faceRuler",
            Self::Detriment => "detriment",
            Self::Fall => "fall",
        }
    }

    fn from_native(value: &str) -> Option<Self> {
        Some(match value {
            "domicile" => Self::Domicile,
            "exaltation" => Self::Exaltation,
            "triplicityRuler" => Self::TriplicityRuler,
            "termRuler" => Self::TermRuler,
            "faceRuler" => Self::FaceRuler,
            "detriment" => Self::Detriment,
            "fall" => Self::Fall,
            _ => return None,
        })
    }

    fn meaning(self) -> &'static str {
        match self {
            Self::Domicile => "positive reception by domicile (major)",
            Self::Exaltation => "positive reception by exaltation (major, potentially exaggerated)",
            Self::TriplicityRuler => "positive reception by triplicity",
            Self::TermRuler => "positive reception by term (minor)",
            Self::FaceRuler => "positive reception by face (minor)",
            Self::Detriment => "negative reception by detriment (major)",
            Self::Fall => "negative reception by fall (major)",
        }
    }

    fn polarity(self) -> ReceptionPolarity {
        match self {
            Self::Detriment | Self::Fall => ReceptionPolarity::Negative,
            _ => ReceptionPolarity::Positive,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ReceptionPolarity {
    Positive,
    Negative,
}

/// Only the identity fields of the already accepted role clipboard are needed
/// for this mechanical check. Labels and role/house choices remain untouched.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ReceptionRoleIdentity {
    pub selected_id: String,
    pub planet: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ReceptionInterpretation {
    pub edge_id: String,
    pub regarding_actor_ids: Vec<String>,
    pub regarded_actor_ids: Vec<String>,
    pub positive_qualities: Vec<String>,
    pub negative_qualities: Vec<String>,
    pub interpretation: String,
    pub limitations: String,
}

/// Invalid supplied identity is a source error. Received model mismatches are
/// observed rejections. Neither result grades the truth of interpretation prose.
/// DSPy can use this same pure guard after parsing its string-array outputs.
pub fn reception_interpretation_failures(
    roles: &[ReceptionRoleIdentity],
    facts: &[Fact],
    rows: &[ReceptionInterpretation],
) -> Result<Vec<String>, String> {
    use std::collections::{BTreeMap, BTreeSet};
    let mut role_ids = BTreeSet::new();
    if roles.len() > 8
        || roles.iter().any(|role| {
            role.selected_id.is_empty()
                || role.planet.is_empty()
                || !role_ids.insert(role.selected_id.as_str())
        })
    {
        return Err(
            "The native reception clipboard has ambiguous or missing accepted role IDs.".into(),
        );
    }
    let mut edges = BTreeMap::new();
    for fact in facts.iter().filter(|fact| fact.kind == "reception") {
        let Some(edge) = fact.reception.as_ref() else {
            continue; // Legacy prose cannot mint a typed directed edge.
        };
        if fact.id.is_empty()
            || fact.planets != [edge.guest_planet.as_str(), edge.host_planet.as_str()]
            || edges.insert(fact.id.as_str(), edge).is_some()
        {
            return Err("The supplied native reception edge identity is inconsistent.".into());
        }
    }
    if !edges.is_empty() && roles.is_empty() {
        return Err("Native reception edges require the accepted role clipboard.".into());
    }
    let ids_for = |planet: &str| {
        roles
            .iter()
            .filter(|role| role.planet == planet)
            .map(|role| role.selected_id.as_str())
            .collect::<BTreeSet<_>>()
    };
    let qualities_for = |edge: &DirectedReception, polarity: ReceptionPolarity| {
        edge.qualities
            .iter()
            .filter(|quality| quality.polarity == polarity)
            .map(|quality| quality.dignity.native_name())
            .collect::<BTreeSet<_>>()
    };
    let mut failures = Vec::new();
    let mut seen = BTreeSet::new();
    for (index, row) in rows.iter().enumerate() {
        let path = format!("$.edge_interpretations[{index}]");
        if !seen.insert(row.edge_id.as_str()) {
            failures.push(format!("{path}.edge_id: duplicate {}.", row.edge_id));
        }
        let Some(edge) = edges.get(row.edge_id.as_str()) else {
            failures.push(format!(
                "{path}.edge_id: {} is not a supplied typed reception edge.",
                row.edge_id
            ));
            continue;
        };
        for (field, actual, expected) in [
            (
                "regarding_actor_ids",
                &row.regarding_actor_ids,
                ids_for(&edge.guest_planet),
            ),
            (
                "regarded_actor_ids",
                &row.regarded_actor_ids,
                ids_for(&edge.host_planet),
            ),
            (
                "positive_qualities",
                &row.positive_qualities,
                qualities_for(edge, ReceptionPolarity::Positive),
            ),
            (
                "negative_qualities",
                &row.negative_qualities,
                qualities_for(edge, ReceptionPolarity::Negative),
            ),
        ] {
            let actual_set: BTreeSet<_> = actual.iter().map(String::as_str).collect();
            if actual_set.len() != actual.len() || actual_set != expected {
                failures.push(format!("{path}.{field}: use exactly {expected:?} for the supplied {} → {} edge; retain every accepted alias and quality without duplicates.", edge.guest_planet, edge.host_planet));
            }
        }
        for (field, text) in [
            ("interpretation", &row.interpretation),
            ("limitations", &row.limitations),
        ] {
            if text.trim().is_empty() {
                failures.push(format!("{path}.{field}: a short contextual proposal or explicit limitation is required."));
            }
        }
    }
    for id in edges.keys().filter(|id| !seen.contains(**id)) {
        failures.push(format!(
            "$.edge_interpretations: missing supplied typed edge {id}."
        ));
    }
    Ok(failures)
}

struct ReceptionRow {
    dignity: ReceptionDignity,
    quality: Option<ReceptionQuality>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EventCandidate {
    pub within_current_signs: Option<bool>,
    pub astronomical_hours: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub perfection_motion: Option<horary_ai_core::events::PerfectionMotion>,
}

pub fn facts(chart: Option<&Value>) -> Vec<Fact> {
    let mut out = Vec::new();
    let Some(chart) = chart else { return out };
    let mut add = |kind: &str,
                   label: String,
                   detail: String,
                   planets: Vec<String>,
                   condition_facet,
                   reception| {
        out.push(Fact {
            id: format!("e{}", out.len()),
            kind: kind.into(),
            label,
            detail,
            planets,
            condition_facet,
            event: None,
            reception,
        });
    };
    for body in chart["bodies"].as_array().into_iter().flatten() {
        let Some(name) = body["name"].as_str() else {
            continue;
        };
        let Some(degree) = body["degree"].as_f64().filter(|v| v.is_finite()) else {
            continue;
        };
        let Some(sign) = body["sign"].as_str().filter(|s| ruler(s).is_some()) else {
            continue;
        };
        let Some(house) = body["house"].as_u64().filter(|n| (1..=12).contains(n)) else {
            continue;
        };
        let motion = if body["retrograde"] == true {
            "retrograde"
        } else {
            "direct"
        };
        add(
            "position",
            name.into(),
            format!("{name} at {degree:.3}° {sign}, house {house}, {motion}."),
            vec![name.into()],
            None,
            None,
        );
    }
    for house in chart["houses"].as_array().into_iter().flatten() {
        let Some(sign) = house["sign"].as_str() else {
            continue;
        };
        let Some(ruler) = ruler(sign) else { continue };
        let Some(degree) = house["degree"].as_f64().filter(|v| v.is_finite()) else {
            continue;
        };
        add(
            "house",
            format!("House {}", house["number"]),
            format!(
                "Cusp {:.3}° {sign}; its traditional ruler is {ruler}.",
                degree
            ),
            vec![ruler.into()],
            None,
            None,
        );
    }
    // Own condition and directed reception are different evidence. Keeping
    // them in distinct facts prevents conflating a planet's own detriment
    // with dislike of its dispositor.
    for testimony in horary_ai_core::book_method::condition_evidence(chart) {
        add(
            "condition",
            "Calculated testimony".into(),
            testimony.detail,
            vec![testimony.planet],
            Some(testimony.facet),
            None,
        );
    }
    if let Some(detail) = horary_ai_core::book_method::moon_gap_evidence(chart) {
        add(
            "condition",
            "Calculated testimony".into(),
            detail,
            vec!["Moon".into()],
            None,
            None,
        );
    }
    let mut pairs: std::collections::BTreeMap<(String, String), Vec<ReceptionRow>> =
        std::collections::BTreeMap::new();
    for entry in chart["derived"]["receptions"]
        .as_array()
        .into_iter()
        .flatten()
    {
        if let (Some(guest), Some(host), Some(dignity)) = (
            entry["guestPlanet"].as_str(),
            entry["hostPlanet"].as_str(),
            entry["dignity"].as_str(),
        ) {
            let Some(dignity) = ReceptionDignity::from_native(dignity) else {
                continue;
            };
            // Historical chart rows may lack polarity. Preserve their original
            // prose, but only publish a typed pair when every native row agrees.
            let polarity = match entry["polarity"].as_str() {
                Some("positive") => Some(ReceptionPolarity::Positive),
                Some("negative") => Some(ReceptionPolarity::Negative),
                _ => None,
            };
            let quality = polarity
                .filter(|polarity| *polarity == dignity.polarity())
                .map(|polarity| ReceptionQuality { dignity, polarity });
            pairs
                .entry((guest.into(), host.into()))
                .or_default()
                .push(ReceptionRow { dignity, quality });
        }
    }
    for ((guest, host), rows) in pairs {
        let meanings: Vec<_> = rows.iter().map(|row| row.dignity.meaning()).collect();
        let reception = rows
            .into_iter()
            .map(|row| row.quality)
            .collect::<Option<Vec<_>>>()
            .map(|qualities| DirectedReception {
                guest_planet: guest.clone(),
                host_planet: host.clone(),
                qualities,
            });
        add(
            "reception",
            format!("{guest} → {host}"),
            format!(
                "{guest} regards {host}: {}. This describes {guest}'s regard for {host}, not the reverse.",
                meanings.join("; ")
            ),
            vec![guest, host],
            None,
            reception,
        );
    }
    // Bounded, compact event summaries avoid dumping the raw ephemeris into every prompt.
    // Attach typed sign-change data so the worksheet need not guess from prose.
    if let Some(events) = chart["derived"]["eventSearch"]["events"].as_array() {
        for event in events {
            let a = event["planet1"].as_str().unwrap_or("unknown");
            let b = event["planet2"].as_str().unwrap_or("unknown");
            let aspect = event["aspectName"].as_str().unwrap_or("contact");
            let signs = match event["withinCurrentSigns"].as_bool() {
                Some(true) => "before either changes sign",
                Some(false) => "after a change of sign",
                None => "sign-change status unestablished",
            };
            let Some(hours) = event["estimatedPerfectsWithinHours"]
                .as_f64()
                .filter(|n| n.is_finite() && *n >= 0.)
            else {
                continue;
            };
            let position = |name: &str| {
                chart["bodies"]
                    .as_array()?
                    .iter()
                    .find(|body| body["name"] == name)?["longitude"]
                    .as_f64()
            };
            let angle = match aspect {
                "Conjunction" => Some(0.),
                "Sextile" => Some(60.),
                "Square" => Some(90.),
                "Trine" => Some(120.),
                "Opposition" => Some(180.),
                _ => None,
            };
            let distance = position(a)
                .zip(position(b))
                .zip(angle)
                .map(|((x, y), target)| {
                    let separation = (x - y + 180.).rem_euclid(360.) - 180.;
                    format!(
                        " Current angular distance from the aspect: {:.3}°.",
                        (separation.abs() - target).abs()
                    )
                })
                .unwrap_or_default();
            out.push(Fact {
                id: format!("e{}", out.len()),
                kind: "event".into(),
                label: format!("{a} · {b}"),
                detail: format!("{aspect} candidate in approximately {hours:.2} astronomical hours, {signs}.{distance} This is not the predicted timing of the earthly event."),
                planets: vec![a.into(),b.into()],
                condition_facet: None,
                event: Some(EventCandidate {
                    within_current_signs: event["withinCurrentSigns"].as_bool(),
                    astronomical_hours: hours,
                    perfection_motion: event.get("perfectionMotion")
                        .and_then(|value| serde_json::from_value::<horary_ai_core::events::PerfectionMotion>(value.clone()).ok())
                        .filter(|motion| motion.body_motion[0].planet == a && motion.body_motion[1].planet == b),
                }),
                reception: None,
            });
        }
    }
    let mut add = |kind: &str, label: String, detail: String, planets: Vec<String>| {
        out.push(Fact {
            id: format!("e{}", out.len()),
            kind: kind.into(),
            label,
            detail,
            planets,
            condition_facet: None,
            event: None,
            reception: None,
        });
    };
    let moon = &chart["derived"]["voidOfCourseMoon"];
    let detail = match moon["isVoid"].as_bool() {
        Some(true) => {
            "Hourly samples found no major lunar contact before sign exit. This is strict void-of-course testimony within this calculation, not an automatic verdict."
        }
        Some(false) => {
            "Hourly samples found a lunar contact before sign exit. The Moon is not strictly void of course in this calculation; the contact still needs contextual judgment."
        }
        None => {
            "The search does not establish the Moon's void-of-course status. Unknown coverage is not evidence of no contact."
        }
    };
    add(
        "moon",
        "The Moon's next contact".into(),
        detail.into(),
        vec!["Moon".into()],
    );
    add("boundary", "What this chart can establish".into(), "Approximate planetary positions; hourly contact brackets cover seven days. Fine event order, stations between samples, fixed stars and antiscia are not certified. A missing candidate is not proof that an event cannot happen.".into(), Vec::new());
    out
}

pub fn ruler(sign: &str) -> Option<&'static str> {
    match sign {
        "Aries" | "Scorpio" => Some("Mars"),
        "Taurus" | "Libra" => Some("Venus"),
        "Gemini" | "Virgo" => Some("Mercury"),
        "Cancer" => Some("Moon"),
        "Leo" => Some("Sun"),
        "Sagittarius" | "Pisces" => Some("Jupiter"),
        "Capricorn" | "Aquarius" => Some("Saturn"),
        _ => None,
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RoleChoice {
    pub label: String,
    pub house: Option<u8>,
    pub natural: Option<NaturalRole>,
    pub reason: String,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
pub enum NaturalRole {
    Moon,
    Sun,
    Venus,
}

impl NaturalRole {
    pub(crate) fn planet(self) -> &'static str {
        match self {
            Self::Moon => "Moon",
            Self::Sun => "Sun",
            Self::Venus => "Venus",
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Role {
    pub label: String,
    pub house: Option<u8>,
    pub planet: String,
    pub reason: String,
}

pub fn turn(base: u8, relative: u8) -> u8 {
    (base + relative - 2) % 12 + 1
}

pub fn relationship_house(relationship: &str) -> Option<u8> {
    match relationship {
        "partner" | "other_party" => Some(7),
        "child" => Some(5),
        "sibling" | "neighbor" => Some(3),
        "friend" => Some(11),
        "mother" | "employer" => Some(10),
        "father" => Some(4),
        "employee" => Some(6),
        "querent" => Some(1),
        _ => None,
    }
}

/// Exact accepted selection values, not roles reconstructed from a model label.
#[derive(Clone, Serialize)]
pub struct ContactAcceptedRole {
    pub selected_id: String,
    pub label: String,
    pub house: Option<u8>,
    pub planet: String,
}

/// A read-only view of an accepted consultation projection and calculated facts.
/// The same view serves the component reader and the application's judgment.
/// It neither chooses a contact route nor interprets reception or an event.
pub fn contact_role_reference(
    consultation: &Value,
    native: &[Fact],
    accepted_roles: Option<&[ContactAcceptedRole]>,
) -> Value {
    use serde_json::json;
    use std::collections::{BTreeMap, BTreeSet};
    let mut unknowns = Vec::new();
    let mut projection_valid = true;
    let mut fields: BTreeMap<&str, Vec<&Value>> = BTreeMap::new();
    if let Some(facts) = consultation["facts"].as_array() {
        for fact in facts {
            if let (Some(field), Some(_), Some(_)) = (
                fact["field"].as_str(),
                fact["value"].as_str(),
                fact["quote"].as_str(),
            ) {
                fields.entry(field).or_default().push(fact);
            } else {
                projection_valid = false;
                unknowns.push("An accepted fact projection is malformed.".into());
            }
        }
    } else {
        unknowns.push("The accepted fact projection is unavailable.".into());
    }
    let unavailable: BTreeSet<_> = consultation["unknown"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect();
    let value = |field: &str| {
        (!unavailable.contains(field))
            .then(|| fields.get(field))
            .flatten()
            .filter(|entries| entries.len() == 1)
            .and_then(|entries| entries[0]["value"].as_str())
            .filter(|value| !value.is_empty())
    };
    for (field, entries) in &fields {
        if entries.len() != 1 || unavailable.contains(field) {
            unknowns.push(format!("{field} is conflicting or unresolved."));
        }
    }
    let principal = match value("principal_mode") {
        Some("relay") => value("principal_id"),
        Some("self" | "concerning_other") => Some("querent"),
        None if projection_valid
            && consultation["facts"].is_array()
            && !fields.contains_key("principal_mode")
            && !unavailable.contains("principal_mode") =>
        {
            Some("querent")
        }
        _ => None,
    };
    let target = value("subject.owner_id");
    if principal.is_none() {
        unknowns.push(
            "The effective principal is unresolved; no first-house actor is invented.".into(),
        );
    }
    if target.is_none() {
        unknowns.push("The contact target's accepted person ID is unresolved.".into());
    }
    let label = |id: &str| {
        value(&format!("people.{id}.label"))
            .or_else(|| value(&format!("people.{id}.name")))
            .unwrap_or(id)
            .to_owned()
    };
    let relationship_field = target.map(|id| format!("people.{id}.relationship"));
    let capacity_field = target.map(|id| format!("people.{id}.capacity"));
    let target_house = target.and_then(|id| {
        if Some(id) == principal {
            Some(1)
        } else {
            if [relationship_field.as_deref(), capacity_field.as_deref()]
                .into_iter()
                .flatten()
                .any(|field| {
                    (fields.contains_key(field) || unavailable.contains(field))
                        && value(field).is_none()
                })
            {
                return None;
            }
            let relationship = relationship_field.as_deref().and_then(value);
            let capacity = capacity_field.as_deref().and_then(value);
            // Both are canonical operative inputs, not an occupation inferred
            // from prose. Conflicting allocations require actual clarification.
            match (relationship, capacity) {
                (Some(relationship), Some(capacity)) if relationship != capacity => None,
                (Some(relationship), _) => relationship_house(relationship),
                (_, Some(capacity)) => relationship_house(capacity),
                _ => None,
            }
        }
    });
    if target.is_some() && target_house.is_none() {
        unknowns.push("The target's operative relationship is absent or unsupported; no seventh-house default or relationship chain is inferred.".into());
    }
    let mut rows = Vec::new();
    let mut add = |id: &str,
                   actor: Option<&str>,
                   house: Option<u8>,
                   optional: bool,
                   sources: Vec<String>| {
        let selection_id = match id {
            "principal.person" => principal.map(|_| "querent.self".to_owned()),
            "target.person" => target.map(|actor| {
                if Some(actor) == principal {
                    "querent.self".to_owned()
                } else {
                    format!("{actor}.self")
                }
            }),
            _ => None,
        };
        let selections: Vec<_> = accepted_roles
            .into_iter()
            .flatten()
            .filter(|role| selection_id.as_deref() == Some(role.selected_id.as_str()))
            .collect();
        let selection = (selections.len() == 1).then(|| selections[0]);
        // The final fact allowlist can omit cusps after native role acceptance.
        // Preserve that accepted binding; missing reference IDs stay absent.
        let accepted_row = accepted_roles.is_some() && !optional;
        let house = if accepted_row {
            selection.and_then(|role| role.house)
        } else {
            house
        };
        let house_facts: Vec<_> = native
            .iter()
            .filter(|fact| {
                fact.kind == "house"
                    && house.is_some_and(|house| fact.label == format!("House {house}"))
            })
            .collect();
        let cusp = (house_facts.len() == 1)
            .then(|| house_facts[0])
            .filter(|fact| selection.is_none_or(|role| fact.planets == [role.planet.as_str()]));
        let ruler = if accepted_row {
            selection.map(|role| role.planet.as_str())
        } else {
            cusp.and_then(|fact| (fact.planets.len() == 1).then(|| fact.planets[0].as_str()))
        };
        if accepted_row && selection.is_none() {
            unknowns.push(format!(
                "{id}: the unique accepted role selection is unavailable."
            ));
        } else if !accepted_row && house.is_some() && ruler.is_none() {
            unknowns.push(format!(
                "{id}: the unique native cusp/ruler is unavailable."
            ));
        }
        let positions: Vec<_> = native
            .iter()
            .filter(|fact| {
                fact.kind == "position"
                    && ruler.is_some_and(|ruler| fact.planets.iter().any(|planet| planet == ruler))
            })
            .map(|fact| fact.id.as_str())
            .collect();
        rows.push(json!({"role_id":id,"actor_id":actor,"label":selection.map(|role|role.label.clone()).or_else(||actor.map(label)),
            "significator_house":house,"cusp_fact_id":cusp.map(|fact|&fact.id),
            "ruler_planet":ruler,"position_fact_ids":positions,"optional":optional,"source_fields":sources,
            "authority":if accepted_row{"native_accepted_selection"}else{"calculated_reference"},
            "selected_id":selection.map(|role|&role.selected_id)}));
    };
    add(
        "principal.person",
        principal,
        principal.map(|_| 1),
        false,
        ["principal_mode", "principal_id"]
            .into_iter()
            .filter(|field| fields.contains_key(field))
            .map(str::to_owned)
            .collect(),
    );
    let mut target_sources = vec!["subject.owner_id".into()];
    if let Some(field) = relationship_field {
        target_sources.push(field);
    }
    if let Some(field) = capacity_field.filter(|field| fields.contains_key(field.as_str())) {
        target_sources.push(field);
    }
    add(
        "target.person",
        target,
        target_house,
        false,
        target_sources.clone(),
    );
    // Frawley p. 166: the person's own contact is sufficient. This additional
    // sender-third reference is optional, never a required message actor.
    add(
        "target.message",
        target,
        target_house.map(|house| turn(house, 3)),
        true,
        target_sources,
    );

    let mut aliases: Vec<Value> = match accepted_roles {
        Some(roles) => roles.iter().map(|role| {
            let actor = if role.selected_id == "querent.self" || role.selected_id == "moon.contextual" {
                principal
            } else {
                target.filter(|id| role.selected_id == format!("{id}.self"))
            };
            json!({"role_id":role.selected_id,"actor_id":actor,"label":role.label,
                "significator_house":role.house,"planet":role.planet})
        }).collect(),
        None => rows.iter().filter(|row| row["ruler_planet"].is_string()).map(|row| {
            json!({"role_id":row["role_id"],"actor_id":row["actor_id"],"label":row["label"],
                "significator_house":row["significator_house"],"planet":row["ruler_planet"],"optional":row["optional"]})
        }).collect(),
    };
    let moon_claims: Vec<_> = aliases
        .iter()
        .filter(|role| {
            role["planet"] == "Moon"
                && role["significator_house"].is_number()
                && role["optional"] != true
        })
        .cloned()
        .collect();
    let conditional_moon_claims: Vec<_> = rows
        .iter()
        .filter(|row| row["ruler_planet"] == "Moon" && row["optional"] == true)
        .map(|row| row["role_id"].clone())
        .collect();
    let moon_available = principal.is_some()
        && moon_claims.is_empty()
        && match accepted_roles {
            Some(roles) => roles.iter().any(|role| {
                role.selected_id == "moon.contextual"
                    && role.planet == "Moon"
                    && role.house.is_none()
            }),
            None => rows
                .iter()
                .filter(|row| row["optional"] == false)
                .all(|row| row["ruler_planet"].is_string()),
        };
    if accepted_roles.is_none() && moon_available {
        aliases.push(json!({"role_id":"principal.moon","actor_id":principal,
            "label":"The principal's contextual Moon","significator_house":null,"planet":"Moon","optional":true}));
    }
    let matching = |planet: &str| {
        aliases
            .iter()
            .filter(|role| role["planet"] == planet)
            .collect::<Vec<_>>()
    };
    let reception: Vec<_> = native
        .iter()
        .filter_map(|fact| {
            let edge = fact
                .reception
                .as_ref()
                .filter(|_| fact.kind == "reception")?;
            let guest_roles = matching(&edge.guest_planet);
            let host_roles = matching(&edge.host_planet);
            if guest_roles.is_empty() || host_roles.is_empty() {
                return None;
            }
            Some(json!({"fact_id":fact.id,"guest_planet":edge.guest_planet,
            "guest_roles":guest_roles,"host_planet":edge.host_planet,
            "host_roles":host_roles,"qualities":edge.qualities}))
        })
        .collect();
    let events: Vec<_> = native
        .iter()
        .filter_map(|fact| {
            let event = fact
                .event
                .as_ref()
                .filter(|_| fact.kind == "event" && fact.planets.len() == 2)?;
            if fact
                .planets
                .iter()
                .all(|planet| matching(planet).is_empty())
            {
                return None;
            }
            Some(
                json!({"fact_id":fact.id,"endpoints":fact.planets.iter().map(|planet| {
            json!({"planet":planet,"roles":matching(planet)})
        }).collect::<Vec<_>>(),"candidate":event}),
            )
        })
        .collect();
    let sources: Vec<_> = fields
        .iter()
        .filter(|(field, _)| {
            matches!(
                **field,
                "principal_mode" | "principal_id" | "subject.owner_id"
            ) || target.is_some_and(|id| field.starts_with(&format!("people.{id}.")))
                || principal.is_some_and(|id| field.starts_with(&format!("people.{id}.")))
        })
        .flat_map(|(_, entries)| entries.iter().copied())
        .collect();
    json!({"version":"contact-role-reference-1","source_identity":consultation["source_identity"],
        "source_scope":consultation["source_scope"],"accepted_fact_references":sources,
        "principal_basis":if principal.is_none(){"unresolved"}else if fields.contains_key("principal_mode"){"accepted_principal_mode"}else{"default_querent_no_relay_asserted"},
        "role_rows":rows,"alias_authority":if accepted_roles.is_some(){"native_accepted_selections"}else{"calculated_role_references"},
        "aliases":aliases,"moon":{"available_as_contextual_reference":moon_available,"house_claims":moon_claims,
            "conditional_house_claims_if_optional_role_used":conditional_moon_claims},
        "directed_reception":reception,"event_endpoints":events,"unknowns":unknowns,
        "limitations":"Significator houses assign roles; they are not the bodies' occupied houses. The sender-third row is optional. Reception arrows retain guest-to-host direction, not an event or emotional verdict. Event endpoints do not assign sender or applying planet.",
        "printed_pages":"25–27, 30–32, 165–166"})
}

#[cfg(test)]
pub fn assign(chart: &Value, choices: Vec<RoleChoice>) -> Result<Vec<Role>, String> {
    assign_with(choices, |number| {
        let house = chart["houses"]
            .as_array()
            .and_then(|hs| {
                hs.iter()
                    .find(|h| h["number"].as_u64() == Some(u64::from(number)))
            })
            .ok_or("Use a calculated house from 1 to 12.")?;
        ruler(house["sign"].as_str().unwrap_or(""))
            .ok_or("The house's traditional ruler is unavailable.")
    })
}

/// Resolve from the same native facts supplied to the role task. Validation and
/// derivation are one operation; no fallible assignment remains after completion.
pub fn assign_from_facts(facts: &[Fact], choices: Vec<RoleChoice>) -> Result<Vec<Role>, String> {
    assign_with(choices, |number| {
        facts
            .iter()
            .find(|f| f.kind == "house" && f.label == format!("House {number}"))
            .and_then(|f| f.planets.first())
            .map(String::as_str)
            .ok_or("Use a supplied calculated house from 1 to 12.")
    })
}

fn assign_with<'a>(
    choices: Vec<RoleChoice>,
    house_ruler: impl Fn(u8) -> Result<&'a str, &'static str>,
) -> Result<Vec<Role>, String> {
    if choices.is_empty() || choices.len() > 8 {
        return Err("Identify one to eight relevant roles first.".into());
    }
    let roles: Vec<Role> = choices
        .into_iter()
        .map(|c| {
            if c.label.trim().is_empty()
                || c.label.chars().count() > 80
                || c.reason.trim().chars().count() < 12
                || c.reason.chars().count() > 240
            {
                return Err(
                    "Each role needs a short name and a contextual reason for its house.".into(),
                );
            }
            let planet = if let (Some(natural), None) = (c.natural, c.house) {
                natural.planet()
            } else if let (Some(number), None) = (c.house, c.natural) {
                house_ruler(number)?
            } else {
                return Err("Choose a house or an explicit natural role, not both.".into());
            };
            Ok(Role {
                label: c.label,
                house: c.house,
                planet: planet.into(),
                reason: c.reason,
            })
        })
        .collect::<Result<_, String>>()?;
    for role in roles.iter().filter(|r| r.house.is_none()) {
        if roles
            .iter()
            .any(|r| r.house.is_some() && r.planet == role.planet)
        {
            return Err(
                "A house ruler has first claim; do not assign it a competing natural role.".into(),
            );
        }
    }
    Ok(roles)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn contact_projection() -> Value {
        serde_json::json!({"source_scope":"synthetic accepted projection","source_identity":"accepted-input-test",
            "facts":[{"field":"subject.owner_id","value":"alma","quote":"my mother Alma"},
                {"field":"people.alma.label","value":"Alma","quote":"Alma"},
                {"field":"people.alma.relationship","value":"mother","quote":"my mother Alma"}],"unknown":[]})
    }

    fn contact_native_facts() -> Vec<Fact> {
        let mut native: Vec<_> = [(1, "Saturn"), (10, "Mars"), (12, "Venus")]
            .into_iter()
            .enumerate()
            .map(|(index, (house, planet))| Fact {
                id: format!("e{index}"),
                kind: "house".into(),
                label: format!("House {house}"),
                detail: "Synthetic calculated cusp/ruler regression.".into(),
                planets: vec![planet.into()],
                condition_facet: None,
                event: None,
                reception: None,
            })
            .collect();
        native.push(Fact {
            id: "e3".into(),
            kind: "position".into(),
            label: "Saturn".into(),
            detail: "Saturn occupies physical house 4, not its first-house role.".into(),
            planets: vec!["Saturn".into()],
            condition_facet: None,
            event: None,
            reception: None,
        });
        native
    }

    fn reference_row<'a>(view: &'a Value, id: &str) -> &'a Value {
        view["role_rows"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["role_id"] == id)
            .unwrap()
    }

    #[test]
    fn contact_reference_binds_known_actor_and_optional_turned_message_to_cusps() {
        let projection = contact_projection();
        let native = contact_native_facts();
        let view = contact_role_reference(&projection, &native, None);
        let principal = reference_row(&view, "principal.person");
        assert_eq!(principal["actor_id"], "querent");
        assert_eq!(principal["significator_house"], 1);
        assert_eq!(principal["position_fact_ids"], serde_json::json!(["e3"]));
        assert!(principal.get("physical_house").is_none());
        assert_eq!(
            reference_row(&view, "target.person")["significator_house"],
            10
        );
        let message = reference_row(&view, "target.message");
        assert_eq!(message["significator_house"], 12);
        assert_eq!(message["cusp_fact_id"], "e2");
        assert_eq!(message["ruler_planet"], "Venus");
        assert_eq!(message["optional"], true);
        assert_eq!(view["source_identity"], "accepted-input-test");
        assert_eq!(
            view["accepted_fact_references"][2]["quote"],
            "my mother Alma"
        );
        assert_eq!(turn(7, 3), 9);
        assert_eq!(turn(11, 3), 1);
    }

    #[test]
    fn contact_reference_keeps_unknown_conflicting_and_unsupported_inputs_unknown() {
        let native = contact_native_facts();
        let mut projection = contact_projection();
        projection["facts"][2]["value"] = serde_json::json!("mother_of_neighbor");
        let view = contact_role_reference(&projection, &native, None);
        assert!(reference_row(&view, "target.person")["significator_house"].is_null());
        assert!(reference_row(&view, "target.message")["ruler_planet"].is_null());
        projection["facts"][2]["value"] = serde_json::json!("mother");
        projection["facts"]
            .as_array_mut()
            .unwrap()
            .push(serde_json::json!({
            "field":"people.alma.relationship","value":"friend","quote":"Alma is my friend"}));
        let view = contact_role_reference(&projection, &native, None);
        assert!(reference_row(&view, "target.person")["significator_house"].is_null());
        assert_eq!(
            view["accepted_fact_references"].as_array().unwrap().len(),
            4
        );
        let mut missing = contact_projection();
        missing["unknown"] = serde_json::json!(["people.alma.relationship"]);
        assert!(reference_row(
            &contact_role_reference(&missing, &native, None),
            "target.person"
        )["ruler_planet"]
            .is_null());
        let mut capacity = contact_projection();
        capacity["facts"][2]["field"] = serde_json::json!("people.alma.capacity");
        capacity["facts"][2]["value"] = serde_json::json!("employer");
        assert_eq!(
            reference_row(
                &contact_role_reference(&capacity, &native, None),
                "target.person"
            )["significator_house"],
            10
        );
        let empty = contact_role_reference(&serde_json::json!({}), &native, None);
        assert!(reference_row(&empty, "principal.person")["actor_id"].is_null());
    }

    #[test]
    fn contact_reference_requires_actual_relay_binding_and_keeps_other_person_ordinary() {
        let native = contact_native_facts();
        let mut projection = contact_projection();
        projection["facts"]
            .as_array_mut()
            .unwrap()
            .push(serde_json::json!({
            "field":"principal_mode","value":"relay","quote":"Alma asked her own question"}));
        let view = contact_role_reference(&projection, &native, None);
        assert!(reference_row(&view, "principal.person")["actor_id"].is_null());
        projection["facts"]
            .as_array_mut()
            .unwrap()
            .push(serde_json::json!({
            "field":"principal_id","value":"alma","quote":"Alma asked her own question"}));
        let view = contact_role_reference(&projection, &native, None);
        assert_eq!(reference_row(&view, "principal.person")["actor_id"], "alma");
        assert_eq!(
            reference_row(&view, "target.person")["significator_house"],
            1
        );
        projection["facts"][3]["value"] = serde_json::json!("concerning_other");
        let view = contact_role_reference(&projection, &native, None);
        assert_eq!(
            reference_row(&view, "principal.person")["actor_id"],
            "querent"
        );
        assert_eq!(
            reference_row(&view, "target.person")["significator_house"],
            10
        );
    }

    #[test]
    fn contact_reference_preserves_house_claims_shared_aliases_and_reception_direction() {
        use serde_json::json;
        let projection = contact_projection();
        let mut native = contact_native_facts();
        native[0].planets = vec!["Moon".into()];
        native[1].planets = vec!["Moon".into()];
        native.push(Fact {
            id: "e4".into(),
            kind: "reception".into(),
            label: "Moon toward Venus".into(),
            detail: "Synthetic directed qualities.".into(),
            planets: vec!["Moon".into(), "Venus".into()],
            condition_facet: None,
            event: None,
            reception: Some(DirectedReception {
                guest_planet: "Moon".into(),
                host_planet: "Venus".into(),
                qualities: vec![
                    ReceptionQuality {
                        dignity: ReceptionDignity::FaceRuler,
                        polarity: ReceptionPolarity::Positive,
                    },
                    ReceptionQuality {
                        dignity: ReceptionDignity::Detriment,
                        polarity: ReceptionPolarity::Negative,
                    },
                ],
            }),
        });
        native.push(Fact {
            id: "e5".into(),
            kind: "event".into(),
            label: "Synthetic candidate".into(),
            detail: "No earthly verdict.".into(),
            planets: vec!["Moon".into(), "Venus".into()],
            condition_facet: None,
            reception: None,
            event: Some(EventCandidate {
                within_current_signs: Some(false),
                astronomical_hours: 25.,
                perfection_motion: None,
            }),
        });
        let before = serde_json::to_value(&native).unwrap();
        let view = contact_role_reference(&projection, &native, None);
        assert_eq!(view["moon"]["house_claims"].as_array().unwrap().len(), 2);
        assert_eq!(view["moon"]["available_as_contextual_reference"], false);
        let edge = &view["directed_reception"][0];
        assert_eq!(
            edge["guest_roles"]
                .as_array()
                .unwrap()
                .iter()
                .map(|role| role["role_id"].clone())
                .collect::<Vec<_>>(),
            vec![json!("principal.person"), json!("target.person")]
        );
        assert_eq!(edge["host_roles"][0]["role_id"], "target.message");
        assert_eq!(edge["guest_roles"][1]["actor_id"], "alma");
        assert_eq!(
            edge["qualities"],
            serde_json::to_value(&native[4].reception.as_ref().unwrap().qualities).unwrap()
        );
        assert_eq!(edge["fact_id"], "e4");
        assert_eq!(
            view["event_endpoints"][0]["candidate"]["withinCurrentSigns"],
            false
        );
        assert!(view["event_endpoints"][0].get("applying_planet").is_none());
        assert_eq!(serde_json::to_value(&native).unwrap(), before);
        let selected = [
            ContactAcceptedRole {
                selected_id: "querent.self".into(),
                label: "Actual first role".into(),
                house: Some(1),
                planet: "Moon".into(),
            },
            ContactAcceptedRole {
                selected_id: "alma.self".into(),
                label: "Actual target role".into(),
                house: Some(10),
                planet: "Moon".into(),
            },
        ];
        let view = contact_role_reference(&projection, &native, Some(&selected));
        assert_eq!(view["alias_authority"], "native_accepted_selections");
        assert_eq!(view["aliases"][0]["role_id"], "querent.self");
        assert_eq!(view["aliases"][1]["label"], "Actual target role");
        assert!(
            view["directed_reception"].as_array().unwrap().is_empty(),
            "An unselected optional message role cannot become an accepted reception alias"
        );
    }

    #[test]
    fn contact_accepted_reference_keeps_roles_and_moon_when_final_facts_omit_cusps() {
        let native: Vec<_> = contact_native_facts()
            .into_iter()
            .filter(|fact| {
                matches!(
                    fact.kind.as_str(),
                    "condition" | "reception" | "event" | "position" | "moon" | "boundary"
                )
            })
            .collect();
        let mut selected = vec![
            ContactAcceptedRole {
                selected_id: "querent.self".into(),
                label: "Accepted principal".into(),
                house: Some(1),
                planet: "Saturn".into(),
            },
            ContactAcceptedRole {
                selected_id: "alma.self".into(),
                label: "Accepted Alma".into(),
                house: Some(10),
                planet: "Mars".into(),
            },
            ContactAcceptedRole {
                selected_id: "moon.contextual".into(),
                label: "Accepted contextual Moon".into(),
                house: None,
                planet: "Moon".into(),
            },
        ];
        let view = contact_role_reference(&contact_projection(), &native, Some(&selected));
        for (row, role) in view["role_rows"].as_array().unwrap()[..2]
            .iter()
            .zip(&selected)
        {
            assert_eq!(row["authority"], "native_accepted_selection");
            assert_eq!(row["selected_id"], role.selected_id);
            assert_eq!(row["label"], role.label);
            assert_eq!(row["significator_house"], json!(role.house));
            assert_eq!(row["ruler_planet"], role.planet);
            assert!(row["cusp_fact_id"].is_null());
        }
        assert_eq!(view["role_rows"][0]["position_fact_ids"], json!(["e3"]));
        assert_eq!(view["role_rows"][1]["position_fact_ids"], json!([]));
        assert_eq!(view["moon"]["available_as_contextual_reference"], true);
        assert_eq!(view["aliases"][2]["role_id"], "moon.contextual");
        assert!(view["role_rows"][2]["ruler_planet"].is_null());
        assert_eq!(view["role_rows"][2]["authority"], "calculated_reference");
        let missing_target = [selected[0].clone(), selected[2].clone()];
        let missing = contact_role_reference(&contact_projection(), &native, Some(&missing_target));
        assert!(missing["role_rows"][1]["selected_id"].is_null());
        assert!(missing["role_rows"][1]["significator_house"].is_null());
        assert!(missing["role_rows"][1]["ruler_planet"].is_null());
        assert!(missing["unknowns"]
            .as_array()
            .unwrap()
            .iter()
            .any(|unknown| {
                unknown.as_str().is_some_and(|text| {
                    text.contains(
                        "target.person: the unique accepted role selection is unavailable",
                    )
                })
            }));
        let component = contact_role_reference(&contact_projection(), &native, None);
        assert!(component["role_rows"]
            .as_array()
            .unwrap()
            .iter()
            .all(|row| row["ruler_planet"].is_null()));
        assert_eq!(
            component["moon"]["available_as_contextual_reference"],
            false
        );
        selected.pop();
        selected[1].planet = "Moon".into();
        let claimed = contact_role_reference(&contact_projection(), &native, Some(&selected));
        assert_eq!(claimed["role_rows"][1]["ruler_planet"], "Moon");
        assert_eq!(claimed["moon"]["house_claims"][0]["role_id"], "alma.self");
        assert_eq!(claimed["moon"]["available_as_contextual_reference"], false);
    }

    #[test]
    fn reception_rows_preserve_direction_shared_aliases_and_mixed_qualities() {
        let facts = facts(Some(&json!({"derived":{"receptions":[
            {"guestPlanet":"Mars","hostPlanet":"Saturn","dignity":"termRuler","polarity":"positive"},
            {"guestPlanet":"Mars","hostPlanet":"Saturn","dignity":"fall","polarity":"negative"}
        ]}})));
        let roles = vec![
            ReceptionRoleIdentity {
                selected_id: "alma.self".into(),
                planet: "Mars".into(),
            },
            ReceptionRoleIdentity {
                selected_id: "querent.self".into(),
                planet: "Saturn".into(),
            },
            ReceptionRoleIdentity {
                selected_id: "subject.primary".into(),
                planet: "Saturn".into(),
            },
        ];
        let row = ReceptionInterpretation {
            edge_id: facts[0].id.clone(),
            regarding_actor_ids: vec!["alma.self".into()],
            regarded_actor_ids: vec!["querent.self".into(), "subject.primary".into()],
            positive_qualities: vec!["termRuler".into()],
            negative_qualities: vec!["fall".into()],
            interpretation: "Mechanical fixture only; contextual meaning is unqualified.".into(),
            limitations: "No reverse regard, event or timing follows from this row alone.".into(),
        };
        assert!(
            reception_interpretation_failures(&roles, &facts, std::slice::from_ref(&row))
                .unwrap()
                .is_empty()
        );
        for (field, mutate) in [
            ("regarding_actor_ids", 0),
            ("regarded_actor_ids", 1),
            ("positive_qualities", 2),
            ("negative_qualities", 3),
        ] {
            let mut rejected = row.clone();
            match mutate {
                0 => rejected.regarding_actor_ids = row.regarded_actor_ids.clone(),
                1 => {
                    rejected.regarded_actor_ids.pop();
                }
                2 => rejected.positive_qualities.push("termRuler".into()),
                _ => rejected.negative_qualities.clear(),
            }
            let errors = reception_interpretation_failures(&roles, &facts, &[rejected]).unwrap();
            assert!(
                errors.iter().any(|error| error.contains(field)),
                "{errors:?}"
            );
        }
        let mut foreign = row.clone();
        foreign.edge_id = "not_supplied".into();
        let errors = reception_interpretation_failures(&roles, &facts, &[foreign]).unwrap();
        assert!(errors
            .iter()
            .any(|error| error.contains("not a supplied typed")));
        assert!(errors
            .iter()
            .any(|error| error.contains("missing supplied typed")));
        assert!(
            reception_interpretation_failures(&roles, &facts, &[row.clone(), row])
                .unwrap()
                .iter()
                .any(|error| error.contains("duplicate"))
        );
        assert!(reception_interpretation_failures(&roles, &facts, &[])
            .unwrap()
            .iter()
            .any(|error| error.contains("missing supplied typed")));
    }

    #[test]
    fn reception_rows_distinguish_unavailable_native_payload_from_bad_source() {
        let mut native = facts(Some(&json!({"derived":{"receptions":[
            {"guestPlanet":"Moon","hostPlanet":"Mars","dignity":"domicile","polarity":"positive"}
        ]}})));
        let roles = [ReceptionRoleIdentity {
            selected_id: "moon.contextual".into(),
            planet: "Moon".into(),
        }];
        let before = serde_json::to_value(&native).unwrap();
        assert!(reception_interpretation_failures(&[], &native, &[]).is_err());
        assert_eq!(serde_json::to_value(&native).unwrap(), before);
        native[0].reception = None;
        assert!(
            reception_interpretation_failures(&roles, &native, &[])
                .unwrap()
                .is_empty(),
            "Legacy prose cannot silently become typed reception testimony"
        );
        assert!(reception_interpretation_failures(
            &[roles[0].clone(), roles[0].clone()],
            &native,
            &[]
        )
        .is_err());
    }

    #[test]
    fn typed_reception_preserves_native_direction_qualities_ids_and_prose() {
        let chart = serde_json::json!({"derived":{"receptions":[
            {"guestPlanet":"Venus","hostPlanet":"Mars","dignity":"exaltation","polarity":"positive"},
            {"guestPlanet":"Mars","hostPlanet":"Venus","dignity":"detriment","polarity":"negative"},
            {"guestPlanet":"Mars","hostPlanet":"Venus","dignity":"termRuler","polarity":"positive"},
            {"guestPlanet":"Jupiter","hostPlanet":"Saturn","dignity":"legacyScore","polarity":"positive"}
        ]}});
        let generated = facts(Some(&chart));
        assert_eq!(generated.len(), 4);
        assert_eq!(generated[0].id, "e0");
        assert_eq!(generated[0].label, "Mars → Venus");
        assert_eq!(generated[0].planets, ["Mars", "Venus"]);
        assert_eq!(
            generated[0].detail,
            "Mars regards Venus: negative reception by detriment (major); positive reception by term (minor). This describes Mars's regard for Venus, not the reverse."
        );
        assert_eq!(
            serde_json::to_value(generated[0].reception.as_ref().unwrap()).unwrap(),
            serde_json::json!({"guestPlanet":"Mars","hostPlanet":"Venus","qualities":[
                {"dignity":"detriment","polarity":"negative"},
                {"dignity":"termRuler","polarity":"positive"}
            ]})
        );
        assert_eq!(generated[1].id, "e1");
        assert_eq!(
            generated[1].detail,
            "Venus regards Mars: positive reception by exaltation (major, potentially exaggerated). This describes Venus's regard for Mars, not the reverse."
        );
        assert_eq!(
            generated[1].reception.as_ref().unwrap().guest_planet,
            "Venus"
        );
        assert_eq!(generated[2].id, "e2");
        assert_eq!(generated[2].kind, "moon");
        assert_eq!(generated[3].id, "e3");
        assert_eq!(generated[3].kind, "boundary");
        assert!(generated[2..].iter().all(|fact| fact.reception.is_none()));
    }

    #[test]
    fn legacy_reception_facts_round_trip_without_invented_payload() {
        let legacy = serde_json::json!({"id":"e7","kind":"reception","label":"Venus → Mars",
            "detail":"Historical prose is preserved.","planets":["Venus","Mars"]});
        let decoded: Fact = serde_json::from_value(legacy.clone()).unwrap();
        assert!(decoded.reception.is_none());
        assert_eq!(serde_json::to_value(decoded).unwrap(), legacy);

        let mut current = legacy.clone();
        current["reception"] = serde_json::json!({"guestPlanet":"Venus","hostPlanet":"Mars",
            "qualities":[{"dignity":"faceRuler","polarity":"positive"}]});
        let decoded: Fact = serde_json::from_value(current.clone()).unwrap();
        assert_eq!(serde_json::to_value(decoded).unwrap(), current);
        current["reception"]["qualities"][0]["dignity"] = serde_json::json!("score");
        assert!(serde_json::from_value::<Fact>(current).is_err());
    }

    #[test]
    fn incomplete_native_rows_keep_prose_without_claiming_a_complete_typed_pair() {
        for polarity in [serde_json::Value::Null, serde_json::json!("negative")] {
            let chart = serde_json::json!({"derived":{"receptions":[
                {"guestPlanet":"Venus","hostPlanet":"Mars","dignity":"domicile","polarity":"positive"},
                {"guestPlanet":"Venus","hostPlanet":"Mars","dignity":"faceRuler","polarity":polarity}
            ]}});
            let generated = facts(Some(&chart));
            assert_eq!(
                generated[0].detail,
                "Venus regards Mars: positive reception by domicile (major); positive reception by face (minor). This describes Venus's regard for Mars, not the reverse."
            );
            assert!(generated[0].reception.is_none());
            assert!(serde_json::to_value(&generated[0])
                .unwrap()
                .get("reception")
                .is_none());
        }
    }

    #[test]
    fn condition_facets_are_native_and_legacy_prose_cannot_supply_them() {
        use horary_ai_core::book_method::ConditionFacet;
        let chart = horary_ai_core::astronomy::chart(1789387200000., 38.657, -77.249).unwrap();
        let generated = facts(Some(&chart));
        for facet in [ConditionFacet::Essential, ConditionFacet::HouseCapacity] {
            let fact = generated
                .iter()
                .find(|fact| fact.condition_facet == Some(facet) && fact.planets == ["Mercury"])
                .unwrap();
            let mut legacy = serde_json::to_value(fact).unwrap();
            legacy.as_object_mut().unwrap().remove("condition_facet");
            let decoded: Fact = serde_json::from_value(legacy.clone()).unwrap();
            assert_eq!(decoded.condition_facet, None);
            assert_eq!(serde_json::to_value(decoded).unwrap(), legacy);
        }
        assert!(generated
            .iter()
            .filter(|fact| fact.kind != "condition")
            .all(|fact| fact.condition_facet.is_none()));
    }

    #[test]
    fn per_body_perfection_motion_reaches_facts_without_retrofitting_old_events() {
        let chart = horary_ai_core::astronomy::chart(1789387200000., 38.657, -77.249).unwrap();
        let raw = chart["derived"]["eventSearch"]["events"]
            .as_array()
            .unwrap()
            .first()
            .unwrap();
        let generated = facts(Some(&chart));
        let event = generated
            .iter()
            .find_map(|fact| fact.event.as_ref())
            .unwrap();
        assert_eq!(
            serde_json::to_value(event.perfection_motion.as_ref().unwrap()).unwrap(),
            raw["perfectionMotion"]
        );
        for body in &event.perfection_motion.as_ref().unwrap().body_motion {
            let initial = chart["bodies"]
                .as_array()
                .unwrap()
                .iter()
                .find(|position| position["name"] == body.planet)
                .unwrap();
            assert_eq!(
                body.chart_longitude_degrees,
                initial["longitude"].as_f64().unwrap()
            );
        }
        let legacy = json!({"withinCurrentSigns":false,"astronomicalHours":25.});
        let decoded: EventCandidate = serde_json::from_value(legacy.clone()).unwrap();
        assert!(decoded.perfection_motion.is_none());
        assert_eq!(serde_json::to_value(decoded).unwrap(), legacy);
        let mut foreign = chart.clone();
        foreign["derived"]["eventSearch"]["events"][0]["perfectionMotion"]["bodyMotion"][0]
            ["planet"] = json!("Foreign body");
        let generated = facts(Some(&foreign));
        assert!(
            generated
                .iter()
                .find_map(|fact| fact.event.as_ref())
                .unwrap()
                .perfection_motion
                .is_none(),
            "A mismatched body must not acquire native motion authority"
        );
    }
    use serde_json::json;
    #[test]
    fn natural_relationship_roles_cannot_displace_house_rulers() {
        let chart = json!({"houses":[{"number":1,"sign":"Leo"},{"number":7,"sign":"Cancer"}]});
        let choices=serde_json::from_value::<Vec<RoleChoice>>(json!([
            {"label":"You","house":1,"reason":"The first house signifies the querent."},
            {"label":"Partner","house":7,"reason":"The seventh house signifies the prospective partner."},
            {"label":"Your feelings","natural":"Moon","reason":"The Moon would otherwise signify the querent's feelings."}
        ])).unwrap();
        assert!(assign(&chart, choices).is_err());
        let choices=serde_json::from_value::<Vec<RoleChoice>>(json!([
            {"label":"You","house":1,"reason":"The first house signifies the querent."},
            {"label":"Your natural role","natural":"Sun","reason":"An explicitly established natural role."}
        ])).unwrap();
        assert!(assign(&chart, choices).is_err());
    }
    #[test]
    fn model_selects_context_but_cannot_supply_or_invent_house_rulers() {
        let chart =
            json!({"houses":[{"number":2,"sign":"Sagittarius"},{"number":4,"sign":"Pisces"}]});
        let roles = assign(
            &chart,
            vec![RoleChoice {
                label: "The ring".into(),
                house: Some(2),
                natural: None,
                reason: "Your own movable possession.".into(),
            }],
        )
        .unwrap();
        assert_eq!(roles[0].planet, "Jupiter");
        assert!(assign(
            &chart,
            vec![RoleChoice {
                label: "Ring".into(),
                house: Some(99),
                natural: None,
                reason: "A made up assignment.".into()
            }]
        )
        .is_err());
        assert!(serde_json::from_value::<RoleChoice>(
            json!({"label":"Ring","house":2,"reason":"Your possession","planet":"Venus"})
        )
        .is_err());
    }
    #[test]
    fn displayed_facts_match_the_same_chart_used_for_inference() {
        let chart = horary_ai_core::astronomy::chart(1789387200000., 51.5074, -0.1278).unwrap();
        let facts = facts(Some(&chart));
        assert!(facts
            .iter()
            .any(|f| f.label == "Jupiter" && f.detail.contains("16.116° Leo")));
        assert!(facts
            .iter()
            .any(|f| f.label == "House 2" && f.detail.contains("Jupiter")));
        assert!(facts.iter().all(|f| !f.detail.contains("timestampMs")));
        assert!(facts.iter().any(|f| f.kind == "reception"
            && f.label == "Venus → Mars"
            && f.detail.contains("positive reception by domicile")));
        assert!(facts.iter().any(|f| f.kind == "reception"
            && f.label == "Mars → Venus"
            && f.detail.contains("term (minor)")
            && !f.detail.contains("domicile")));
        for fact in facts.iter().filter(|fact| fact.kind == "reception") {
            let reception = fact.reception.as_ref().unwrap();
            let qualities = chart["derived"]["receptions"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|row| {
                    row["guestPlanet"] == reception.guest_planet
                        && row["hostPlanet"] == reception.host_planet
                })
                .map(|row| json!({"dignity":row["dignity"],"polarity":row["polarity"]}))
                .collect::<Vec<_>>();
            assert_eq!(
                serde_json::to_value(reception).unwrap()["qualities"],
                json!(qualities)
            );
            assert_eq!(
                fact.planets,
                [
                    reception.guest_planet.clone(),
                    reception.host_planet.clone()
                ]
            );
        }
        assert!(facts
            .iter()
            .any(|f| f.kind == "event" && f.detail.contains("angular distance")));
        let missing = super::facts(Some(
            &json!({"bodies":[{"name":"Jupiter"}],"houses":[{"number":1}]}),
        ));
        assert!(missing
            .iter()
            .all(|f| f.kind != "position" && f.kind != "house"));
    }
}
