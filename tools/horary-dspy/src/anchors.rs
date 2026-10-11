//! The application's actual offline gazetteer and civil-time tools own anchors.
use crate::{
    catalogue::{Device, Fact},
    native_geocode::{self, GeocodeRequest, GeocodeState, LocationCandidate},
};
use anyhow::{Context, Result, ensure};
use chrono::{DateTime, FixedOffset};
use serde_json::{Value, json};

pub struct Anchor {
    pub at: DateTime<FixedOffset>,
    pub place: LocationCandidate,
    pub events: Vec<Value>,
}
fn value<'a>(facts: &'a [Fact], name: &str) -> Option<&'a str> {
    facts
        .iter()
        .find(|f| f.field == name)
        .map(|f| f.value.as_str())
}
fn lookup(query: &str, device: &LocationCandidate) -> Result<LocationCandidate> {
    let matches = native_geocode::geocode_with_cache(
        &GeocodeState::default(),
        GeocodeRequest {
            query: query.into(),
            limit: Some(50),
        },
    )
    .map_err(|e| anyhow::anyhow!(e.message))?;
    native_geocode::uniquely_qualified_city(query, &matches)
        .or_else(|| (matches.len() == 1).then(|| matches[0].clone()))
        .or_else(|| native_geocode::nearby_unique_city(&matches, device))
        .context("Stated place did not resolve to a unique qualified or nearby municipality")
}
pub fn resolve(question: &str, device: &Device, facts: &[Fact]) -> Result<Anchor> {
    for (path, field) in [
        (
            "reader_place",
            horary_ai_core::anchor_authority::AnchorField::ReaderPlace,
        ),
        (
            "question_time",
            horary_ai_core::anchor_authority::AnchorField::QuestionTime,
        ),
    ] {
        if let Some(fact) = facts.iter().find(|f| f.field == path) {
            horary_ai_core::anchor_authority::validate_source_quote(question, field, &fact.quote)
                .map_err(|reason| {
                anyhow::anyhow!("{path}: {reason}; event context cannot override the device anchor")
            })?;
        }
    }
    let initial = LocationCandidate {
        id: "device".into(),
        label: device.place.name.clone(),
        name: device.place.name.clone(),
        country: String::new(),
        latitude: device.place.latitude,
        longitude: device.place.longitude,
        timezone: device.time_zone.clone(),
        provider: "device".into(),
    };
    ensure!(
        initial.latitude.is_finite()
            && initial.latitude.abs() < 90.
            && initial.longitude.is_finite()
            && initial.longitude.abs() <= 180.,
        "Invalid actual device coordinates"
    );
    let place = match value(facts, "reader_place") {
        None => initial.clone(),
        Some(p)
            if ["device", "device-location", "here", "current location"]
                .contains(&p.to_lowercase().as_str())
                || p.eq_ignore_ascii_case(&device.place.name) =>
        {
            initial.clone()
        }
        Some(p) => lookup(p, &initial)?,
    };
    let at = match value(facts, "question_time") {
        None => DateTime::parse_from_rfc3339(&device.observed_at)?,
        Some(time) => {
            if let Ok(at) = DateTime::parse_from_rfc3339(time) {
                at
            } else {
                let zone = value(facts, "question_timezone").unwrap_or(&place.timezone);
                let occurrence = value(facts, "time_occurrence").unwrap_or("");
                let ms = horary_ai_core::chart_input::resolve_chart_time(time, zone, occurrence)
                    .map_err(anyhow::Error::msg)?;
                DateTime::from_timestamp_millis(ms as i64)
                    .context("Invalid resolved chart instant")?
                    .fixed_offset()
            }
        }
    };
    let mut events = Vec::new();
    if let Some(fact) = facts.iter().find(|f| f.field == "event_place") {
        match lookup(&fact.value,&initial){
            Ok(located)=>events.push(json!({"field":"event_place","source":fact,"resolved":located,"authority":"Native offline geocoder; event context only"})),
            Err(error)=>events.push(json!({"field":"event_place","source":fact,"geocoder_limit":error.to_string(),"authority":"The stated event place is retained. An unavailable gazetteer match does not erase it or change the reader's chart."})),
        }
    }
    if let Some(fact) = facts.iter().find(|f| f.field == "event_time") {
        let receipt = DateTime::parse_from_rfc3339(&device.observed_at)?.timestamp_millis() as f64;
        let day = horary_ai_core::chart_input::resolve_event_day(
            &fact.quote,
            question,
            question,
            false,
            receipt,
            &device.time_zone,
        )
        .map_err(anyhow::Error::msg)?;
        if let Some(day) = &day {
            ensure!(
                fact.value.contains(&day.date),
                "Model's event date disagrees with the actual native clock"
            );
        }
        events.push(json!({"field":"event_time","source":fact,"native_day":day,"authority":"Event context, never the chart instant"}));
    }
    Ok(Anchor { at, place, events })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalogue::Place;
    fn device() -> Device {
        Device {
            observed_at: "2026-10-08T12:00:00-04:00".into(),
            time_zone: "America/New_York".into(),
            place: Place {
                name: "Woodbridge".into(),
                latitude: 38.6582,
                longitude: -77.2497,
            },
        }
    }
    #[test]
    fn event_place_and_friday_are_resolved_without_reanchoring() {
        let q = "Will I sell in Springfield on Friday?";
        let facts = vec![
            Fact {
                field: "event_place".into(),
                value: "Springfield".into(),
                quote: "Springfield".into(),
            },
            Fact {
                field: "event_time".into(),
                value: "2026-10-09".into(),
                quote: "Friday".into(),
            },
        ];
        let a = resolve(q, &device(), &facts).unwrap();
        assert_eq!(a.at.to_rfc3339(), device().observed_at);
        assert_eq!(a.place.latitude, device().place.latitude);
        assert!(a.events[0]["resolved"]["latitude"].as_f64().unwrap() > 38.);
    }
    #[test]
    fn fully_supplied_historical_civil_time_uses_the_stated_place() {
        let facts = vec![
            Fact {
                field: "reader_place".into(),
                value: "London, United Kingdom".into(),
                quote: "London".into(),
            },
            Fact {
                field: "question_time".into(),
                value: "2026-01-14T14:30".into(),
                quote: "14:30".into(),
            },
        ];
        let a = resolve(
            "Use the question I understood in London at 14:30 on 14 January 2026.",
            &device(),
            &facts,
        )
        .unwrap();
        assert_eq!(a.at.to_rfc3339(), "2026-01-14T14:30:00+00:00");
        assert_eq!(a.place.name, "London");
    }
    #[test]
    fn ambiguous_historical_clock_and_wrong_relative_day_are_withheld() {
        let time = Fact {
            field: "question_time".into(),
            value: "2026-11-01T01:30".into(),
            quote: "01:30".into(),
        };
        assert!(resolve("I asked at 01:30 on November 1.", &device(), &[time]).is_err());
        let day = Fact {
            field: "event_time".into(),
            value: "2026-10-16".into(),
            quote: "Friday".into(),
        };
        assert!(resolve("Will it sell Friday?", &device(), &[day]).is_err());
    }
}
