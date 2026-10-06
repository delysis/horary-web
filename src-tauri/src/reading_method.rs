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
    pub event: Option<EventCandidate>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EventCandidate {
    pub within_current_signs: Option<bool>,
    pub astronomical_hours: f64,
}

pub fn facts(chart: Option<&Value>) -> Vec<Fact> {
    let mut out = Vec::new();
    let Some(chart) = chart else { return out };
    let mut add = |kind: &str, label: String, detail: String, planets: Vec<String>| {
        out.push(Fact {
            id: format!("e{}", out.len()),
            kind: kind.into(),
            label,
            detail,
            planets,
            event: None,
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
        );
    }
    // Own condition and directed reception are different evidence. Keeping
    // them in distinct facts prevents conflating a planet's own detriment
    // with dislike of its dispositor.
    for detail in horary_ai_core::book_method::evidence(chart) {
        if detail.contains(" in ") && detail.contains("'s ") {
            continue;
        }
        let planets = [
            "Sun", "Moon", "Mercury", "Venus", "Mars", "Jupiter", "Saturn",
        ]
        .into_iter()
        .filter(|p| detail.contains(p))
        .map(str::to_string)
        .collect();
        add("condition", "Calculated testimony".into(), detail, planets);
    }
    let mut pairs: std::collections::BTreeMap<(String, String), Vec<String>> =
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
            let meaning = match dignity {
                "domicile" => "positive reception by domicile (major)",
                "exaltation" => "positive reception by exaltation (major, potentially exaggerated)",
                "triplicityRuler" => "positive reception by triplicity",
                "termRuler" => "positive reception by term (minor)",
                "faceRuler" => "positive reception by face (minor)",
                "detriment" => "negative reception by detriment (major)",
                "fall" => "negative reception by fall (major)",
                _ => continue,
            };
            pairs
                .entry((guest.into(), host.into()))
                .or_default()
                .push(meaning.into());
        }
    }
    for ((guest, host), meanings) in pairs {
        add("reception",format!("{guest} → {host}"),format!("{guest} regards {host}: {}. This describes {guest}'s regard for {host}, not the reverse.",meanings.join("; ")),vec![guest,host]);
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
                event: Some(EventCandidate { within_current_signs: event["withinCurrentSigns"].as_bool(), astronomical_hours: hours }),
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
            event: None,
        });
    };
    let moon = &chart["derived"]["voidOfCourseMoon"];
    let detail = match moon["isVoid"].as_bool() {
        Some(true)=>"Hourly samples found no major lunar contact before sign exit. This is strict void-of-course testimony within this calculation, not an automatic verdict.",
        Some(false)=>"Hourly samples found a lunar contact before sign exit. The Moon is not strictly void of course in this calculation; the contact still needs contextual judgment.",
        None=>"The search does not establish the Moon's void-of-course status. Unknown coverage is not evidence of no contact.",
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
    fn planet(self) -> &'static str {
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

pub fn assign(chart: &Value, choices: Vec<RoleChoice>) -> Result<Vec<Role>, String> {
    if choices.is_empty() || choices.len() > 5 {
        return Err("Identify one to five relevant roles first.".into());
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
                let house = chart["houses"]
                    .as_array()
                    .and_then(|hs| {
                        hs.iter()
                            .find(|h| h["number"].as_u64() == Some(u64::from(number)))
                    })
                    .ok_or("Use a calculated house from 1 to 12.")?;
                ruler(house["sign"].as_str().unwrap_or(""))
                    .ok_or("The house's traditional ruler is unavailable.")?
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
