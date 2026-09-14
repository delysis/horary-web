//! Bracket major aspects from supplied ephemeris samples, independently of
//! drawing orbs. Interpolation is reported as an estimate, never exact timing.
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::BTreeMap;
const PLANETS: [&str; 7] = [
    "Sun", "Moon", "Mercury", "Venus", "Mars", "Jupiter", "Saturn",
];
const ASPECTS: [(f64, &str); 8] = [
    (0., "Conjunction"),
    (60., "Sextile"),
    (90., "Square"),
    (120., "Trine"),
    (180., "Opposition"),
    (240., "Trine"),
    (270., "Square"),
    (300., "Sextile"),
];
#[derive(Deserialize)]
struct Sample {
    hours: f64,
    positions: BTreeMap<String, f64>,
}
fn delta(a: f64, b: f64) -> f64 {
    (b - a + 180.).rem_euclid(360.) - 180.
}
fn sign(lon: f64) -> u8 {
    (lon.rem_euclid(360.) / 30.).floor() as u8
}
fn crossing(start: f64, end: f64, target: f64) -> Option<f64> {
    let movement = end - start;
    if movement.abs() < 1e-12 {
        return None;
    }
    let t = (target - start) / movement;
    (t > 0. && t <= 1.).then_some(t)
}

pub fn derive(samples: &Value) -> Value {
    let Ok(samples) = serde_json::from_value::<Vec<Sample>>(samples.clone()) else {
        return unavailable("Invalid motion samples");
    };
    if samples.len() < 2
        || samples.len() > 1000
        || samples[0].hours != 0.
        || samples.windows(2).any(|w| {
            !w[1].hours.is_finite() || w[1].hours <= w[0].hours || w[1].hours - w[0].hours > 1.01
        })
        || samples.iter().any(|s| {
            PLANETS
                .iter()
                .any(|p| s.positions.get(*p).is_none_or(|v| !v.is_finite()))
        })
    {
        return unavailable(
            "Continuous hourly positions for all seven classical planets are required",
        );
    }
    let mut events = Vec::new();
    let mut ingresses = Vec::new();
    let mut moon_exit = None;
    for w in samples.windows(2) {
        let a = &w[0];
        let b = &w[1];
        for planet in PLANETS {
            let start = a.positions[planet].rem_euclid(360.);
            let end = start + delta(start, b.positions[planet]);
            if sign(start) != sign(end) {
                let boundary = if end > start {
                    (start / 30.).floor() * 30. + 30.
                } else {
                    (start / 30.).floor() * 30.
                };
                // A retrograde body exactly on a cusp leaves the sign at t=0.
                // Aspect crossings exclude t=0, but ingresses must retain it.
                let ingress = if start == boundary {
                    Some(0.)
                } else {
                    crossing(start, end, boundary)
                };
                if let Some(t) = ingress {
                    let hours = a.hours + t * (b.hours - a.hours);
                    if planet == "Moon" && moon_exit.is_none() {
                        moon_exit = Some(hours);
                    }
                    ingresses.push(json!({"planet":planet,"estimatedHours":hours,"fromSign":sign(start),"toSign":sign(end)}));
                }
            }
        }
        for (i, planet1) in PLANETS.iter().enumerate() {
            for planet2 in &PLANETS[i + 1..] {
                let p = a.positions[*planet1];
                let q = a.positions[*planet2];
                let start = (p - q).rem_euclid(360.);
                let end = start + delta(p, b.positions[*planet1]) - delta(q, b.positions[*planet2]);
                for (angle, name) in ASPECTS {
                    for target in [angle - 360., angle, angle + 360.] {
                        if let Some(t) = crossing(start, end, target) {
                            let hours = a.hours + t * (b.hours - a.hours);
                            let current_signs = ingresses.iter().all(|entry| {
                                !(entry["planet"] == *planet1 || entry["planet"] == *planet2)
                                    || entry["estimatedHours"].as_f64().is_none_or(|h| h > hours)
                            });
                            let moon_travel = if *planet1 == "Moon" || *planet2 == "Moon" {
                                let longitude = a.positions["Moon"]
                                    + t * delta(a.positions["Moon"], b.positions["Moon"]);
                                Some((longitude - samples[0].positions["Moon"]).rem_euclid(360.))
                            } else {
                                None
                            };
                            events.push(json!({"type":"perfectionCandidate","planet1":planet1,"planet2":planet2,"aspectName":name,"estimatedPerfectsWithinHours":hours,"withinCurrentSigns":current_signs,"bracketHours":[a.hours,b.hours],"moonTravelDegrees":moon_travel}));
                        }
                    }
                }
            }
        }
    }
    events.sort_by(|a, b| {
        a["estimatedPerfectsWithinHours"]
            .as_f64()
            .unwrap_or(0.)
            .total_cmp(&b["estimatedPerfectsWithinHours"].as_f64().unwrap_or(0.))
    });
    let moon_event = events.iter().find(|e| {
        (e["planet1"] == "Moon" || e["planet2"] == "Moon")
            && moon_exit.is_none_or(|exit| {
                e["estimatedPerfectsWithinHours"]
                    .as_f64()
                    .is_some_and(|h| h < exit)
            })
    });
    let void = if moon_event.is_some() {
        Some(false)
    } else {
        moon_exit.map(|_| true)
    };
    let hours = samples.last().map_or(0., |s| s.hours);
    let long_gap = moon_event
        .and_then(|e| e["moonTravelDegrees"].as_f64())
        .map(|d| d >= 15.);
    json!({"available":true,"checkedUntilHours":hours,"events":events,"ingresses":ingresses,
  "method":"Major aspect crossings in continuous hourly ephemeris samples; estimated by interpolation. No display-orb gate. A reversal within a sample can remain unresolved.",
  "timingCaution":"Astronomical hours are not a predicted date for the question. Interpret symbolic timing only with question context and a plausible timeframe.",
  "voidOfCourseMoon":{"available":void.is_some(),"isVoid":void,"checkedUntilHours":moon_exit.unwrap_or(hours),"nextApplyingAspect":moon_event,"longGapNeedsReview":long_gap,"context":"isVoid reports absence of an aspect before sign exit. Frawley also treats a long gap, roughly 15 degrees or more, as possible void-like stagnation; this requires contextual judgment (printed p. 65).","reason":if void.is_none(){Some("Moon sign exit is outside search coverage")}else{None}}})
}
fn unavailable(reason: &str) -> Value {
    json!({"available":false,"reason":reason,"events":[],"voidOfCourseMoon":{"available":false,"isVoid":null,"reason":reason}})
}

#[cfg(test)]
mod tests {
    use super::*;
    fn sample(hour: f64, moon: f64, sun: f64) -> Value {
        let mut positions = BTreeMap::from([
            ("Sun", sun),
            ("Moon", moon),
            ("Mercury", 47.),
            ("Venus", 81.),
            ("Mars", 143.),
            ("Jupiter", 214.),
            ("Saturn", 268.),
        ]);
        positions.insert("Moon", moon);
        json!({"hours":hour,"positions":positions})
    }
    #[test]
    fn last_fraction_of_sign_is_searched_before_declaring_void() {
        let value = derive(&json!([sample(0., 29.5, 29.8), sample(1., 30.5, 29.8)]));
        assert_eq!(value["voidOfCourseMoon"]["isVoid"], false);
        let event = &value["voidOfCourseMoon"]["nextApplyingAspect"];
        assert!((event["estimatedPerfectsWithinHours"].as_f64().unwrap() - 0.3).abs() < 1e-8);
    }
    #[test]
    fn an_aspect_after_sign_exit_does_not_end_void() {
        let value = derive(&json!([sample(0., 29.5, 30.2), sample(1., 30.5, 30.2)]));
        assert_eq!(value["voidOfCourseMoon"]["isVoid"], true);
        assert!(!value["events"].as_array().unwrap().is_empty());
    }
    #[test]
    fn future_perfection_is_not_limited_by_display_orbs() {
        let rows = (0..=15)
            .map(|h| sample(f64::from(h), f64::from(h), 10.))
            .collect::<Vec<_>>();
        let value = derive(&json!(rows));
        assert!(value["events"]
            .as_array()
            .unwrap()
            .iter()
            .any(|e| e["planet1"] == "Sun"
                && e["planet2"] == "Moon"
                && e["estimatedPerfectsWithinHours"] == 10.));
    }
    #[test]
    fn gaps_and_short_coverage_do_not_fabricate_void() {
        assert_eq!(
            derive(&json!([sample(0., 1., 20.), sample(1., 2., 20.)]))["voidOfCourseMoon"]
                ["isVoid"],
            Value::Null
        );
        assert_eq!(
            derive(&json!([sample(0., 1., 20.), sample(3., 4., 20.)]))["available"],
            false
        );
    }

    #[test]
    fn retrograde_ingress_on_the_starting_cusp_is_not_lost() {
        let mut a = sample(0., 1., 20.);
        let mut b = sample(1., 2., 20.);
        a["positions"]["Mercury"] = json!(30.);
        b["positions"]["Mercury"] = json!(29.9);
        let result = derive(&json!([a, b]));
        assert!(result["ingresses"]
            .as_array()
            .unwrap()
            .iter()
            .any(|i| i["planet"] == "Mercury" && i["estimatedHours"] == 0.));
    }

    #[test]
    fn long_gap_is_visible_even_when_a_later_contact_ends_strict_void() {
        let rows = (0..=20)
            .map(|h| {
                let positions = PLANETS
                    .into_iter()
                    .map(|p| (p, if p == "Moon" { 1. + f64::from(h) } else { 20. }))
                    .collect::<BTreeMap<_, _>>();
                json!({"hours":h,"positions":positions})
            })
            .collect::<Vec<_>>();
        let value = derive(&json!(rows));
        assert_eq!(value["voidOfCourseMoon"]["isVoid"], false);
        assert_eq!(value["voidOfCourseMoon"]["longGapNeedsReview"], true);
    }
}
