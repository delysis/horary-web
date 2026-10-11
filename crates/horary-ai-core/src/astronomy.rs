//! Native chart tool. The inherited approximate ephemeris is kept explicit;
//! domain rules consume one set of positions, never model-supplied astronomy.
use crate::{
    book_method,
    ephemeris_coefficients::{ELEMENTS, MOON_TERMS},
};
use serde_json::{json, Value};

const NAMES: [&str; 7] = [
    "Sun", "Moon", "Mercury", "Venus", "Mars", "Jupiter", "Saturn",
];
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
fn sin(x: f64) -> f64 {
    x.to_radians().sin()
}
fn cos(x: f64) -> f64 {
    x.to_radians().cos()
}
fn norm(x: f64) -> f64 {
    x.rem_euclid(360.)
}
fn delta(a: f64, b: f64) -> f64 {
    norm(b - a + 180.) - 180.
}

fn helio(index: usize, t: f64) -> (f64, f64) {
    let el = ELEMENTS[index];
    let l = norm(el[0] + el[1] * t);
    let e = el[3] + el[4] * t;
    let i = el[5] + el[6] * t;
    let node = norm(el[7] + el[8] * t);
    let peri = norm(el[9] + el[10] * t);
    let m = norm(l - peri).to_radians();
    let mut eccentric = m;
    for _ in 0..30 {
        let change = (m - eccentric + e * eccentric.sin()) / (1. - e * eccentric.cos());
        eccentric += change;
        if change.abs() < 1e-12 {
            break;
        }
    }
    let x = el[2] * (eccentric.cos() - e);
    let y = el[2] * (1. - e * e).sqrt() * eccentric.sin();
    let w = peri - node;
    (
        (cos(node) * cos(w) - sin(node) * sin(w) * cos(i)) * x
            + (-cos(node) * sin(w) - sin(node) * cos(w) * cos(i)) * y,
        (sin(node) * cos(w) + cos(node) * sin(w) * cos(i)) * x
            + (-sin(node) * sin(w) + cos(node) * cos(w) * cos(i)) * y,
    )
}

pub fn positions(jd: f64) -> [f64; 7] {
    let t = (jd - 2451545.) / 36525.;
    let m = norm(357.52911 + 35999.05029 * t - 0.0001537 * t * t);
    let center = (1.914602 - 0.004817 * t - 0.000014 * t * t) * sin(m)
        + (0.019993 - 0.000101 * t) * sin(2. * m)
        + 0.000289 * sin(3. * m);
    let sun = norm(
        280.46646 + 36000.76983 * t + 0.0003032 * t * t + center
            - 0.00569
            - 0.00478 * sin(125.04 - 1934.136 * t),
    );
    let lp = norm(
        218.3164477 + 481267.88123421 * t - 0.0015786 * t * t + t.powi(3) / 538841.
            - t.powi(4) / 65194000.,
    );
    let d = norm(
        297.8501921 + 445267.1114034 * t - 0.0018819 * t * t + t.powi(3) / 545868.
            - t.powi(4) / 113065000.,
    );
    let m = norm(357.5291092 + 35999.0502909 * t - 0.0001536 * t * t + t.powi(3) / 24490000.);
    let mp = norm(
        134.9633964 + 477198.8675055 * t + 0.0087414 * t * t + t.powi(3) / 69699.
            - t.powi(4) / 14712000.,
    );
    let f = norm(
        93.2720950 + 483202.0175233 * t - 0.0036539 * t * t - t.powi(3) / 3526000.
            + t.powi(4) / 863310000.,
    );
    let e = 1. - 0.002516 * t - 0.0000074 * t * t;
    let sum: f64 = MOON_TERMS
        .iter()
        .map(|a| a[4] * e.powi(a[1].abs() as i32) * sin(a[0] * d + a[1] * m + a[2] * mp + a[3] * f))
        .sum();
    let moon = norm(
        lp + (sum
            + 3958. * sin(119.75 + 131.849 * t)
            + 1962. * sin(lp - f)
            + 318. * sin(53.09 + 479264.290 * t))
            / 1000000.,
    );
    let earth = helio(0, t);
    let mut out = [sun, moon, 0., 0., 0., 0., 0.];
    for index in 1..6 {
        let p = helio(index, t);
        out[index + 1] = norm((p.1 - earth.1).atan2(p.0 - earth.0).to_degrees());
    }
    out
}

fn cusps(jd: f64, lat: f64, lon: f64) -> [f64; 12] {
    let t = (jd - 2451545.) / 36525.;
    let u = t / 100.;
    let mean = 23.439291111
        + u * (-1.300258333
            + u * (-0.000430556
                + u * (0.555347222
                    + u * (-0.014272222
                        + u * (-0.069352778
                            + u * (-0.010847222
                                + u * (0.001977778
                                    + u * (0.007741667
                                        + u * (0.001608333 + u * (-0.000680556))))))))));
    let node = 125.04452 - 1934.136261 * t + 0.0020708 * t * t + t.powi(3) / 450000.;
    let mp = 134.96298 + 477198.867398 * t + 0.0086972 * t * t + t.powi(3) / 56250.;
    let eps = mean
        + (9.20 * cos(node) + 0.57 * cos(2. * (280.4665 + 36000.7698 * t)) + 0.10 * cos(2. * mp)
            - 0.09 * cos(2. * node))
            / 3600.;
    let ramc = norm(
        280.46061837 + 360.98564736629 * (jd - 2451545.) + 0.000387933 * t * t
            - t.powi(3) / 38710000.
            + lon,
    );
    let cusp = |angle: f64, pole: f64| {
        norm(
            cos(angle)
                .atan2(-(sin(angle) * cos(eps) + pole.tan() * sin(eps)))
                .to_degrees(),
        )
    };
    let p30 = (lat.to_radians().tan() * sin(30.)).atan();
    let p60 = (lat.to_radians().tan() * sin(60.)).atan();
    let mut c = [0.; 12];
    c[9] = norm(sin(ramc).atan2(cos(ramc) * cos(eps)).to_degrees());
    c[10] = cusp(ramc - 60., p30);
    c[11] = cusp(ramc - 30., p60);
    c[0] = cusp(ramc, lat.to_radians());
    c[1] = cusp(ramc + 30., p60);
    c[2] = cusp(ramc + 60., p30);
    for i in [9, 10, 11, 0, 1, 2] {
        c[(i + 6) % 12] = norm(c[i] + 180.);
    }
    c
}

pub fn chart(timestamp_ms: f64, lat: f64, lon: f64) -> Result<Value, String> {
    if !timestamp_ms.is_finite()
        || !lat.is_finite()
        || !lon.is_finite()
        || lat.abs() >= 90.
        || lon.abs() > 180.
    {
        return Err(
            "A valid moment and coordinates below the geographic poles are required.".into(),
        );
    }
    let jd = timestamp_ms / 86400000. + 2440587.5;
    if !(2305447.5..=2597641.5).contains(&jd) {
        return Err("Supported chart years are 1600–2399.".into());
    }
    let p = positions(jd);
    let future = positions(jd + 1. / 24.);
    let c = cusps(jd, lat, lon);
    let houses:Vec<Value>=c.iter().enumerate().map(|(i,x)|json!({"number":i+1,"longitude":x,"sign":SIGNS[(*x/30.) as usize],"degree":x%30.})).collect();
    let bodies:Vec<Value>=p.iter().enumerate().map(|(i,x)|{
        let house=(0..12).find(|h|norm(*x-c[*h])<norm(c[(*h+1)%12]-c[*h])).map(|h|h+1);
        let speed=delta(*x,future[i])*24.;
        json!({"name":NAMES[i],"longitude":x,"sign":SIGNS[(*x/30.) as usize],"degree":x%30.,"house":house,"speed":speed,"retrograde":speed<0.})
    }).collect();
    let samples: Vec<Value> = (0..=168)
        .map(|hours| {
            let p = positions(jd + f64::from(hours) / 24.);
            let map: serde_json::Map<String, Value> = NAMES
                .iter()
                .zip(p)
                .map(|(name, x)| (name.to_string(), json!(x)))
                .collect();
            json!({"hours":hours,"positions":map})
        })
        .collect();
    let mut aspects = Vec::new();
    for i in 0..7 {
        for j in i + 1..7 {
            for (angle, name) in [
                (0., "Conjunction"),
                (60., "Sextile"),
                (90., "Square"),
                (120., "Trine"),
                (180., "Opposition"),
            ] {
                let orb = (delta(p[i], p[j]).abs() - angle).abs();
                if orb <= 5. {
                    aspects.push(json!({"planet1":NAMES[i],"planet2":NAMES[j],"aspect":name,"orb":orb,"applying":(delta(future[i],future[j]).abs()-angle).abs()<orb}));
                }
            }
        }
    }
    Ok(book_method::apply(
        &json!({"timestampMs":timestamp_ms,"latitude":lat,"longitude":lon,"houseSystem":"Regiomontanus","zodiac":"tropical","bodies":bodies,"houses":houses,"aspects":aspects,"derived":{"chartSect":if norm(p[0]-c[0])>=180. {"day"} else {"night"}},"motionSamples":samples,"calculationNote":"Inherited approximate Meeus/orbital-elements ephemeris. Hourly event brackets cover seven days; they are astronomical estimates, not predicted calendar timing."}),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_positions_match_existing_calculator_receipt() {
        let fixture: Value =
            serde_json::from_str(include_str!("../tests/astronomy-parity.json")).unwrap();
        for case in fixture.as_array().unwrap() {
            let actual = chart(
                case["timestampMs"].as_f64().unwrap(),
                case["lat"].as_f64().unwrap(),
                case["lon"].as_f64().unwrap(),
            )
            .unwrap();
            for (i, expected) in case["positions"].as_array().unwrap().iter().enumerate() {
                assert!(
                    delta(
                        actual["bodies"][i]["longitude"].as_f64().unwrap(),
                        expected.as_f64().unwrap()
                    )
                    .abs()
                        < 1e-7
                );
            }
            for (i, expected) in case["cusps"].as_array().unwrap().iter().enumerate() {
                assert!(
                    delta(
                        actual["houses"][i]["longitude"].as_f64().unwrap(),
                        expected.as_f64().unwrap()
                    )
                    .abs()
                        < 1e-7
                );
            }
        }
    }
    #[test]
    fn invalid_coordinates_do_not_become_charts() {
        assert!(chart(0., 90., 0.).is_err());
        assert!(chart(0., f64::NAN, 0.).is_err());
        assert!(chart(f64::INFINITY, 0., 0.).is_err());
    }
}
