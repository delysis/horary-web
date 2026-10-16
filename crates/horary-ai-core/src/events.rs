//! Bracket major aspects from supplied ephemeris samples, independently of
//! drawing orbs. Interpolation is reported as an estimate, never exact timing.
use serde::{Deserialize, Serialize};
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

/// Both bodies move. These estimates deliberately do not choose the applying
/// planet, convert degrees to symbolic units, or certify a future event.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PerfectionMotion {
    pub estimate_method: MotionEstimateMethod,
    pub bracket_hours: [f64; 2],
    pub body_motion: [BodyMotion; 2],
    pub intra_interval_motion_resolved: bool,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MotionEstimateMethod {
    SampleIntervalLinearInterpolation,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BodyMotion {
    pub planet: String,
    pub chart_longitude_degrees: f64,
    pub estimated_perfection_longitude_degrees: f64,
    pub estimated_signed_travel_degrees: f64,
    pub estimated_path_distance_degrees: f64,
    pub initial_interval_mean_speed_degrees_per_day: f64,
    pub perfection_bracket_mean_speed_degrees_per_day: f64,
    pub sampled_motion_reversal_observed: bool,
    pub zero_mean_motion_interval_observed: bool,
}

#[derive(Clone, Copy, Default)]
struct MotionPath {
    signed_travel: f64,
    path_distance: f64,
    previous_direction: Option<bool>,
    sampled_reversal: bool,
    zero_mean_interval: bool,
}

impl MotionPath {
    fn advance(&mut self, displacement: f64) {
        self.signed_travel += displacement;
        self.path_distance += displacement.abs();
        if displacement == 0. {
            // Equal sampled endpoints are not a certified instantaneous station.
            self.zero_mean_interval = true;
        } else {
            let direction = displacement.is_sign_positive();
            self.sampled_reversal |= self
                .previous_direction
                .is_some_and(|previous| previous != direction);
            self.previous_direction = Some(direction);
        }
    }
}

fn body_motion(
    planet: &str,
    chart: &Sample,
    a: &Sample,
    b: &Sample,
    t: f64,
    initial_mean_speed: f64,
    mut path: MotionPath,
) -> BodyMotion {
    let displacement = delta(a.positions[planet], b.positions[planet]);
    path.advance(t * displacement);
    BodyMotion {
        planet: planet.into(),
        chart_longitude_degrees: chart.positions[planet].rem_euclid(360.),
        estimated_perfection_longitude_degrees: (a.positions[planet] + t * displacement)
            .rem_euclid(360.),
        estimated_signed_travel_degrees: path.signed_travel,
        estimated_path_distance_degrees: path.path_distance,
        initial_interval_mean_speed_degrees_per_day: initial_mean_speed,
        perfection_bracket_mean_speed_degrees_per_day: displacement / (b.hours - a.hours) * 24.,
        sampled_motion_reversal_observed: path.sampled_reversal,
        zero_mean_motion_interval_observed: path.zero_mean_interval,
    }
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
    let initial_mean_speed: [f64; 7] = std::array::from_fn(|i| {
        delta(
            samples[0].positions[PLANETS[i]],
            samples[1].positions[PLANETS[i]],
        ) / samples[1].hours
            * 24.
    });
    let mut motion_paths = [MotionPath::default(); 7];
    for w in samples.windows(2) {
        let a = &w[0];
        let b = &w[1];
        let displacement: [f64; 7] =
            std::array::from_fn(|i| delta(a.positions[PLANETS[i]], b.positions[PLANETS[i]]));
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
            for (j, planet2) in PLANETS.iter().enumerate().skip(i + 1) {
                let p = a.positions[*planet1];
                let q = a.positions[*planet2];
                let start = (p - q).rem_euclid(360.);
                let end = start + displacement[i] - displacement[j];
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
                            let perfection_motion = PerfectionMotion {
                                estimate_method:
                                    MotionEstimateMethod::SampleIntervalLinearInterpolation,
                                bracket_hours: [a.hours, b.hours],
                                body_motion: [
                                    body_motion(
                                        planet1,
                                        &samples[0],
                                        a,
                                        b,
                                        t,
                                        initial_mean_speed[i],
                                        motion_paths[i],
                                    ),
                                    body_motion(
                                        planet2,
                                        &samples[0],
                                        a,
                                        b,
                                        t,
                                        initial_mean_speed[j],
                                        motion_paths[j],
                                    ),
                                ],
                                intra_interval_motion_resolved: false,
                            };
                            events.push(json!({"type":"perfectionCandidate","planet1":planet1,"planet2":planet2,"aspectName":name,"estimatedPerfectsWithinHours":hours,"withinCurrentSigns":current_signs,"bracketHours":[a.hours,b.hours],"moonTravelDegrees":moon_travel,"perfectionMotion":perfection_motion}));
                        }
                    }
                }
            }
        }
        for (path, displacement) in motion_paths.iter_mut().zip(displacement) {
            path.advance(displacement);
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

    fn candidate(rows: Vec<Value>, first: &str, second: &str, aspect: &str) -> Value {
        derive(&json!(rows))["events"]
            .as_array()
            .unwrap()
            .iter()
            .find(|event| {
                event["planet1"] == first
                    && event["planet2"] == second
                    && event["aspectName"] == aspect
            })
            .expect("The authored sample path must cross this aspect")
            .clone()
    }

    fn motion(event: &Value, planet: &str) -> BodyMotion {
        let estimate: PerfectionMotion =
            serde_json::from_value(event["perfectionMotion"].clone()).unwrap();
        assert_eq!(
            estimate.estimate_method,
            MotionEstimateMethod::SampleIntervalLinearInterpolation
        );
        assert!(!estimate.intra_interval_motion_resolved);
        estimate
            .body_motion
            .into_iter()
            .find(|body| body.planet == planet)
            .unwrap()
    }

    #[test]
    fn moving_target_travel_is_to_estimated_perfection_not_the_initial_gap() {
        let rows = (0..=8)
            .map(|h| {
                let h = f64::from(h);
                let mut row = sample(h, 200., 40. + h);
                row["positions"]["Mars"] = json!(134. + h * 3. / 7.);
                row
            })
            .collect();
        let event = candidate(rows, "Sun", "Mars", "Square");
        let sun = motion(&event, "Sun");
        let mars = motion(&event, "Mars");
        assert!((event["estimatedPerfectsWithinHours"].as_f64().unwrap() - 7.).abs() < 1e-9);
        assert_eq!(sun.chart_longitude_degrees, 40.);
        assert_eq!(mars.chart_longitude_degrees, 134.);
        assert!((sun.estimated_perfection_longitude_degrees - 47.).abs() < 1e-9);
        assert!((mars.estimated_perfection_longitude_degrees - 137.).abs() < 1e-9);
        assert!((sun.estimated_signed_travel_degrees - 7.).abs() < 1e-9);
        assert!((sun.estimated_path_distance_degrees - 7.).abs() < 1e-9);
        assert!((mars.estimated_signed_travel_degrees - 3.).abs() < 1e-9);
        assert_eq!(sun.initial_interval_mean_speed_degrees_per_day, 24.);
        assert_eq!(sun.perfection_bracket_mean_speed_degrees_per_day, 24.);
        // Frawley pp. 128–129: the four-degree chart gap is not Sun's travel.
        assert!((sun.estimated_signed_travel_degrees - 4.).abs() > 2.9);
    }

    #[test]
    fn forward_wrap_preserves_unwrapped_travel_for_the_second_body() {
        let event = candidate(
            vec![
                sample(0., 350., 5.),
                sample(1., 359., 5.),
                sample(2., 5., 5.),
            ],
            "Sun",
            "Moon",
            "Conjunction",
        );
        let moon = motion(&event, "Moon");
        assert_eq!(moon.chart_longitude_degrees, 350.);
        assert_eq!(moon.estimated_perfection_longitude_degrees, 5.);
        assert_eq!(moon.estimated_signed_travel_degrees, 15.);
        assert_eq!(moon.estimated_path_distance_degrees, 15.);
        assert_eq!(moon.perfection_bracket_mean_speed_degrees_per_day, 144.);
        assert_eq!(motion(&event, "Sun").estimated_signed_travel_degrees, 0.);
        assert!(event.get("applicantTravelDegrees").is_none());
        assert!(event.get("applyingPlanet").is_none());
    }

    #[test]
    fn retrograde_wrap_retains_negative_displacement_and_positive_path_distance() {
        let rows = [5., 1., 359., 355.]
            .into_iter()
            .enumerate()
            .map(|(h, mercury)| {
                let mut row = sample(h as f64, 200., 355.);
                row["positions"]["Mercury"] = json!(mercury);
                row
            })
            .collect();
        let event = candidate(rows, "Sun", "Mercury", "Conjunction");
        let mercury = motion(&event, "Mercury");
        assert_eq!(mercury.estimated_perfection_longitude_degrees, 355.);
        assert_eq!(mercury.estimated_signed_travel_degrees, -10.);
        assert_eq!(mercury.estimated_path_distance_degrees, 10.);
        assert_eq!(mercury.initial_interval_mean_speed_degrees_per_day, -96.);
        assert!(!mercury.sampled_motion_reversal_observed);
    }

    #[test]
    fn sampled_reversal_and_zero_mean_interval_do_not_claim_an_exact_station() {
        let rows = [20., 22., 22., 20., 18.]
            .into_iter()
            .enumerate()
            .map(|(h, mercury)| {
                let mut row = sample(h as f64, 200., 18.);
                row["positions"]["Mercury"] = json!(mercury);
                row
            })
            .collect();
        let event = candidate(rows, "Sun", "Mercury", "Conjunction");
        let mercury = motion(&event, "Mercury");
        assert_eq!(mercury.estimated_signed_travel_degrees, -2.);
        assert_eq!(mercury.estimated_path_distance_degrees, 6.);
        assert_eq!(mercury.initial_interval_mean_speed_degrees_per_day, 48.);
        assert_eq!(mercury.perfection_bracket_mean_speed_degrees_per_day, -48.);
        assert!(mercury.sampled_motion_reversal_observed);
        assert!(mercury.zero_mean_motion_interval_observed);
        assert_eq!(event["perfectionMotion"]["bracketHours"], json!([3., 4.]));
    }

    #[test]
    fn opposing_motion_exposes_both_paths_without_assigning_the_applicant() {
        let rows = (0..=10)
            .map(|h| {
                let h = f64::from(h);
                let mut row = sample(h, 200., 20.);
                row["positions"]["Venus"] = json!(100. - h * 0.5);
                row["positions"]["Mars"] = json!(90. + h * 0.5);
                row
            })
            .collect();
        let event = candidate(rows, "Venus", "Mars", "Conjunction");
        assert_eq!(motion(&event, "Venus").estimated_signed_travel_degrees, -5.);
        assert_eq!(motion(&event, "Mars").estimated_signed_travel_degrees, 5.);
        assert!(event.get("applyingPlanet").is_none());
        assert!(event.get("symbolicTimingUnits").is_none());
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
