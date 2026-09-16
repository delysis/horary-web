//! Frawley, The Horary Textbook (2005), printed pp. 44–83.
//! Rules over supplied astronomy: no ephemeris, predictive score, or actor selection.
use serde_json::{json, Value};

const SIGNS: [&str; 12] = [
    "Aries",
    "Taurus",
    "Gemini",
    "Cancer",
    "Leo",
    "Virgo",
    "Libra",
    "Scorpio",
    "Sagittarius",
    "Capricorn",
    "Aquarius",
    "Pisces",
];
const PLANETS: [&str; 7] = [
    "Sun", "Moon", "Mercury", "Venus", "Mars", "Jupiter", "Saturn",
];
const DOMICILE: [&str; 12] = [
    "Mars", "Venus", "Mercury", "Moon", "Sun", "Mercury", "Venus", "Mars", "Jupiter", "Saturn",
    "Saturn", "Jupiter",
];
const EXALTATION: [&str; 12] = [
    "Sun", "Moon", "", "Jupiter", "", "Mercury", "Saturn", "", "", "Mars", "", "Venus",
];
const FACES: [&str; 7] = [
    "Mars", "Sun", "Venus", "Mercury", "Moon", "Saturn", "Jupiter",
];
// Exclusive upper bounds, from the table on printed p. 72. Boundary 14 means
// the next ruler begins at 14°00′, not at 15°00′.
const TERMS: [[(u8, &str); 5]; 12] = [
    [
        (6, "Jupiter"),
        (14, "Venus"),
        (21, "Mercury"),
        (26, "Mars"),
        (30, "Saturn"),
    ],
    [
        (8, "Venus"),
        (15, "Mercury"),
        (22, "Jupiter"),
        (26, "Saturn"),
        (30, "Mars"),
    ],
    [
        (7, "Mercury"),
        (14, "Jupiter"),
        (21, "Venus"),
        (25, "Saturn"),
        (30, "Mars"),
    ],
    [
        (6, "Mars"),
        (13, "Jupiter"),
        (20, "Mercury"),
        (27, "Venus"),
        (30, "Saturn"),
    ],
    [
        (6, "Saturn"),
        (13, "Mercury"),
        (19, "Venus"),
        (25, "Jupiter"),
        (30, "Mars"),
    ],
    [
        (7, "Mercury"),
        (13, "Venus"),
        (18, "Jupiter"),
        (24, "Saturn"),
        (30, "Mars"),
    ],
    [
        (6, "Saturn"),
        (11, "Venus"),
        (19, "Jupiter"),
        (24, "Mercury"),
        (30, "Mars"),
    ],
    [
        (6, "Mars"),
        (14, "Jupiter"),
        (21, "Venus"),
        (27, "Mercury"),
        (30, "Saturn"),
    ],
    [
        (8, "Jupiter"),
        (14, "Venus"),
        (19, "Mercury"),
        (25, "Saturn"),
        (30, "Mars"),
    ],
    [
        (6, "Venus"),
        (12, "Mercury"),
        (19, "Jupiter"),
        (25, "Mars"),
        (30, "Saturn"),
    ],
    [
        (6, "Saturn"),
        (12, "Mercury"),
        (20, "Venus"),
        (25, "Jupiter"),
        (30, "Mars"),
    ],
    [
        (8, "Venus"),
        (14, "Jupiter"),
        (20, "Mercury"),
        (26, "Mars"),
        (30, "Saturn"),
    ],
];

fn position(body: &Value) -> Option<(usize, f64)> {
    let sign = SIGNS
        .iter()
        .position(|s| Some(*s) == body["sign"].as_str())?;
    let degree = body["degree"].as_f64()?;
    (degree.is_finite() && (0.0..30.0).contains(&degree)).then_some((sign, degree))
}

fn rulers(sign: usize, degree: f64, day: bool) -> [(&'static str, &'static str); 7] {
    let triplicity = match (sign % 4, day) {
        (0, true) => "Sun",
        (0, false) => "Jupiter",
        (1, true) => "Venus",
        (1, false) => "Moon",
        (2, true) => "Saturn",
        (2, false) => "Mercury",
        _ => "Mars",
    };
    let term = TERMS[sign]
        .iter()
        .find(|(end, _)| degree < f64::from(*end))
        .map_or("", |(_, ruler)| *ruler);
    [
        ("domicile", DOMICILE[sign]),
        ("exaltation", EXALTATION[sign]),
        ("triplicityRuler", triplicity),
        ("termRuler", term),
        (
            "faceRuler",
            FACES[(sign * 3 + (degree / 10.0) as usize) % 7],
        ),
        ("detriment", DOMICILE[(sign + 6) % 12]),
        ("fall", EXALTATION[(sign + 6) % 12]),
    ]
}

fn solar_condition(sign: usize, degree: f64, sun: (usize, f64)) -> Value {
    let distance = ((sign as f64 * 30.0 + degree - sun.0 as f64 * 30.0 - sun.1 + 180.0)
        .rem_euclid(360.0)
        - 180.0)
        .abs();
    let same_sign = sign == sun.0;
    let condition = if same_sign && distance <= 17.5 / 60.0 {
        "cazimi"
    } else if same_sign && distance <= 8.5 {
        "combust"
    } else if distance <= 17.5 {
        "underBeams"
    } else {
        "clear"
    };
    json!({"condition":condition,"separationDegrees":distance,"sameSignAsSun":same_sign})
}

/// Rebuild method-dependent facts from positions, including on repeat calls.
/// Unknown/missing input stays unknown; legacy scores are never evidence.
pub fn apply(chart: &Value) -> Value {
    let mut output = chart.clone();
    if !output.is_object() {
        return output;
    }
    let day = match chart["derived"]["chartSect"].as_str() {
        Some("day") => Some(true),
        Some("night") => Some(false),
        _ => None,
    };
    let bodies = chart["bodies"].as_array().cloned().unwrap_or_default();
    let sun = bodies
        .iter()
        .find(|b| b["name"] == "Sun")
        .and_then(position);
    let houses = chart["houses"].as_array().cloned().unwrap_or_default();
    let mut receptions = Vec::new();
    let mut solar_conditions = Vec::new();
    let mut normalized = Vec::with_capacity(bodies.len());
    for mut body in bodies.clone() {
        let name = body["name"].as_str().unwrap_or("").to_owned();
        if !PLANETS.contains(&name.as_str()) {
            if let Some(fields) = body.as_object_mut() {
                // The traditional table does not assign dignity or peregrine
                // status to outer planets. Discard legacy synthetic scores.
                fields.remove("dignity");
                fields.remove("accidentalDignity");
            }
            normalized.push(body);
            continue;
        }
        let Some((sign, degree)) = position(&body) else {
            body["dignity"] = Value::Null;
            body["accidentalDignity"] = Value::Null;
            normalized.push(body);
            continue;
        };
        let mut dignity = json!({});
        let mut any_condition = false;
        for (kind, host) in rulers(sign, degree, day.unwrap_or(true)) {
            if kind == "triplicityRuler" && day.is_none() {
                dignity[kind] = Value::Null;
                continue;
            }
            let own = host == name;
            dignity[kind] = json!(own);
            any_condition |= own;
            if !host.is_empty() && host != name && bodies.iter().any(|b| b["name"] == host) {
                receptions.push(json!({"guestPlanet":name,"hostPlanet":host,"dignity":kind,
                    "polarity":if kind == "detriment" || kind == "fall" {"negative"} else {"positive"}}));
            }
        }
        dignity["peregrine"] = if any_condition {
            json!(false)
        } else if day.is_some() {
            json!(true)
        } else {
            Value::Null
        };
        body["dignity"] = dignity;
        let geometric = body.get("geometricHouse").unwrap_or(&body["house"]).clone();
        body["geometricHouse"] = geometric.clone();
        body["house"] = geometric.clone();
        body["cuspAdvance"] = Value::Null;
        if let Some(house) = geometric.as_u64().filter(|h| (1..=12).contains(h)) {
            let next = house % 12 + 1;
            if let Some((cusp_sign, cusp_degree)) = houses
                .iter()
                .find(|h| h["number"] == next)
                .and_then(position)
            {
                let before = cusp_degree - degree;
                if sign == cusp_sign && (0.0..=5.0).contains(&before) {
                    body["house"] = json!(next);
                    body["cuspAdvance"] = json!({"degreesBeforeCusp":before,"printedPage":56});
                }
            }
        }
        let house = body["house"].as_u64();
        let capacity = match house {
            Some(1 | 4 | 7 | 10) => Some("strong"),
            Some(6 | 8 | 12) => Some("weak"),
            Some(2 | 3 | 5 | 9 | 11) => Some("neutral"),
            _ => None,
        };
        let angularity = match house {
            Some(1 | 4 | 7 | 10) => Some("angular"),
            Some(2 | 5 | 8 | 11) => Some("succedent"),
            Some(3 | 6 | 9 | 12) => Some("cadent"),
            _ => None,
        };
        let solar = sun
            .filter(|_| name != "Sun")
            .map(|sun| solar_condition(sign, degree, sun));
        if let Some(solar) = &solar {
            let mut entry = solar.clone();
            entry["planet"] = json!(name);
            solar_conditions.push(entry);
        }
        let joy_house = match name.as_str() {
            "Mercury" => 1,
            "Moon" => 3,
            "Venus" => 5,
            "Mars" => 6,
            "Sun" => 9,
            "Jupiter" => 11,
            _ => 12,
        };
        body["accidentalDignity"] = json!({"house":house,"angularity":angularity,"houseCapacity":capacity,
            "retrograde":body["retrograde"],"solarCondition":solar.map(|s| s["condition"].clone()),
            "inJoy":house.map(|h|h == joy_house),"joyHouse":joy_house});
        normalized.push(body);
    }
    output["bodies"] = json!(normalized);
    if !output["derived"].is_object() {
        output["derived"] = json!({});
    }
    output["derived"]["receptions"] = json!(receptions);
    output["derived"]["solarConditions"] = json!(solar_conditions);
    if chart["motionSamples"].is_array() {
        let search = crate::events::derive(&chart["motionSamples"]);
        output["derived"]["voidOfCourseMoon"] = search["voidOfCourseMoon"].clone();
        output["derived"]["timingPatterns"] = search["events"].clone();
        output["derived"]["eventSearch"] = search;
    }
    if let Some(object) = output.as_object_mut() {
        object.remove("motionSamples");
    }
    output["bookMethod"] = json!({
        "profile":"frawley-2005-v1", "printedPages":[44,56,58,59,71,72,73,74,75],
        "receptionDirection":"The guest occupies a dignity or debility of the host: read the guest's inclination toward the host. Opposite directions may differ; negative and positive testimony may coexist.",
        "context":"Essential condition is distinct from capacity to act. House capacity, retrogradation and combustion require question context; no summed strength score or automatic veto.",
        "cuspRule":"Within approximately five degrees before the next cusp and in its sign, use that next house for judgement; geometricHouse preserves the unadjusted placement.",
        "missingSect":day.is_none()
    });
    output
}

#[cfg(feature = "wasm")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn apply_book_method_json(chart: &str) -> Result<String, wasm_bindgen::JsValue> {
    let chart: Value =
        serde_json::from_str(chart).map_err(|e| wasm_bindgen::JsValue::from_str(&e.to_string()))?;
    serde_json::to_string(&apply(&chart))
        .map_err(|e| wasm_bindgen::JsValue::from_str(&e.to_string()))
}

pub fn evidence(chart: &Value) -> Vec<String> {
    let mut facts = Vec::new();
    for body in chart["bodies"].as_array().into_iter().flatten() {
        let Some(name) = body["name"].as_str().filter(|name| PLANETS.contains(name)) else {
            continue;
        };
        let conditions = body["dignity"]
            .as_object()
            .into_iter()
            .flatten()
            .filter(|(_, value)| value.as_bool() == Some(true))
            .map(|(key, _)| key.as_str())
            .collect::<Vec<_>>();
        if !conditions.is_empty() {
            facts.push(format!(
                "{name} essential conditions: {}",
                conditions.join(", ")
            ));
        }
        if let Some(capacity) = body["accidentalDignity"]["houseCapacity"].as_str() {
            facts.push(format!("{name} house capacity {capacity}"));
        }
        if let Some(condition) = body["accidentalDignity"]["solarCondition"].as_str() {
            facts.push(format!("{name} solar condition {condition}"));
        }
        if let Some(before) = body["cuspAdvance"]["degreesBeforeCusp"].as_f64() {
            facts.push(format!(
                "{name} {before:.2} degrees before next cusp in same sign; judgement house {}",
                body["house"]
            ));
        }
    }
    for reception in chart["derived"]["receptions"]
        .as_array()
        .into_iter()
        .flatten()
    {
        if let (Some(guest), Some(host), Some(kind)) = (
            reception["guestPlanet"].as_str(),
            reception["hostPlanet"].as_str(),
            reception["dignity"].as_str(),
        ) {
            facts.push(format!("{guest} in {host}'s {kind}"));
        }
    }
    if chart["derived"]["voidOfCourseMoon"]["longGapNeedsReview"] == true {
        facts.push("Moon travels at least 15 degrees before next major contact; review possible stagnation".into());
    }
    facts
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn outer_planets_do_not_inherit_synthetic_traditional_dignities() {
        let result = apply(
            &json!({"bodies":[{"name":"Uranus","sign":"Gemini","degree":6.,"house":12,"dignity":{"peregrine":true,"score":0}}]}),
        );
        assert!(result["bodies"][0].get("dignity").is_none());
        assert_eq!(result["bodies"][0]["house"], 12);
    }
    #[test]
    fn printed_table_boundaries_faces_and_water_sect() {
        assert_eq!(rulers(0, 13.99, true)[3].1, "Venus");
        assert_eq!(rulers(0, 14.0, true)[3].1, "Mercury");
        assert_eq!(rulers(0, 0.0, true)[4].1, "Mars");
        assert_eq!(rulers(0, 10.0, true)[4].1, "Sun");
        for sign in [3, 7, 11] {
            for day in [true, false] {
                assert_eq!(rulers(sign, 1.0, day)[2].1, "Mars");
            }
        }
        for sign in 0..12 {
            for (end, host) in TERMS[sign] {
                assert_eq!(rulers(sign, f64::from(end) - 0.001, true)[3].1, host);
            }
        }
    }
    #[test]
    fn solar_conditions_respect_sign_boundary_and_wrap() {
        assert_eq!(
            solar_condition(0, 0.1, (11, 29.9))["condition"],
            "underBeams"
        );
        assert_eq!(solar_condition(0, 0.29, (0, 0.0))["condition"], "cazimi");
        assert_eq!(solar_condition(0, 8.5, (0, 0.0))["condition"], "combust");
        assert_eq!(
            solar_condition(0, 17.4, (0, 0.0))["condition"],
            "underBeams"
        );
    }
    #[test]
    fn signed_receptions_peregrine_and_cusp_placement_are_not_scores() {
        let chart = json!({"bodies":[{"name":"Venus","sign":"Aries","degree":13.0,"house":8},
            {"name":"Mars","sign":"Libra","degree":1.0,"house":2}],
            "houses":[{"number":9,"sign":"Aries","degree":17.0},{"number":3,"sign":"Scorpio","degree":2.0}],
            "derived":{"chartSect":"day"}});
        let facts = apply(&chart);
        assert_eq!(facts["bodies"][0]["house"], 9);
        assert_eq!(facts["bodies"][0]["geometricHouse"], 8);
        assert_eq!(
            facts["bodies"][0]["accidentalDignity"]["houseCapacity"],
            "neutral"
        );
        assert_eq!(facts["bodies"][1]["house"], 2);
        assert_eq!(facts["bodies"][1]["dignity"]["peregrine"], false);
        assert!(facts["bodies"][0]["dignity"].get("score").is_none());
        let receptions = facts["derived"]["receptions"].as_array().unwrap();
        assert!(receptions.iter().any(|r| r["guestPlanet"] == "Mars"
            && r["hostPlanet"] == "Venus"
            && r["dignity"] == "domicile"));
        assert!(receptions.iter().any(|r| r["guestPlanet"] == "Venus"
            && r["hostPlanet"] == "Mars"
            && r["dignity"] == "domicile"));
        assert_eq!(apply(&facts), facts);
        let negative = apply(
            &json!({"bodies":[{"name":"Moon","sign":"Aries","degree":3.0},{"name":"Venus","sign":"Taurus","degree":1.0}],"derived":{"chartSect":"day"}}),
        );
        assert!(negative["derived"]["receptions"]
            .as_array()
            .unwrap()
            .iter()
            .any(|r| r["guestPlanet"] == "Moon"
                && r["hostPlanet"] == "Venus"
                && r["polarity"] == "negative"));
    }
    #[test]
    fn missing_sect_does_not_invent_triplicity_or_peregrine() {
        let facts = apply(&json!({"bodies":[{"name":"Sun","sign":"Gemini","degree":1.0}]}));
        assert!(facts["bodies"][0]["dignity"]["triplicityRuler"].is_null());
        assert!(facts["bodies"][0]["dignity"]["peregrine"].is_null());
    }
}
