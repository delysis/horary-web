//! Resolve a civil chart time once, before either calculation or interpretation.
use chrono::{Datelike, LocalResult, NaiveDateTime, TimeZone};
use chrono_tz::Tz;

pub fn nudge_chart_time(local: &str, unit: &str, direction: i32) -> Result<String, String> {
    let civil = NaiveDateTime::parse_from_str(local, "%Y-%m-%dT%H:%M")
        .map_err(|_| "Enter a valid calendar date and time.".to_string())?;
    if !matches!(direction, -1 | 1) {
        return Err("Time nudges must be forward or backward one step.".into());
    }
    let next = match unit {
        "month" | "year" => {
            let months = chrono::Months::new(if unit == "year" { 12 } else { 1 });
            if direction > 0 {
                civil.checked_add_months(months)
            } else {
                civil.checked_sub_months(months)
            }
        }
        "minute" | "hour" | "day" | "week" => {
            let minutes = match unit {
                "minute" => 1,
                "hour" => 60,
                "day" => 1440,
                _ => 10080,
            };
            civil.checked_add_signed(chrono::Duration::minutes(minutes * i64::from(direction)))
        }
        _ => return Err("Unknown time increment.".into()),
    }
    .ok_or("Time increment is outside the supported calendar.")?;
    if !(1600..=2399).contains(&next.year()) {
        return Err("Chart year must be between 1600 and 2399.".into());
    }
    Ok(next.format("%Y-%m-%dT%H:%M").to_string())
}

pub fn resolve_chart_time(local: &str, timezone: &str, occurrence: &str) -> Result<f64, String> {
    let civil = NaiveDateTime::parse_from_str(local, "%Y-%m-%dT%H:%M")
        .map_err(|_| "Enter a valid calendar date and time.".to_string())?;
    if !(1600..=2399).contains(&civil.year()) {
        return Err("Chart year must be between 1600 and 2399.".into());
    }
    let zone: Tz = timezone.parse().map_err(|_| {
        "Choose a valid timezone, such as America/New_York or Europe/London.".to_string()
    })?;
    let instant = match zone.from_local_datetime(&civil) {
        LocalResult::Single(value) => value,
        LocalResult::None => {
            return Err("This local time does not exist because the clocks moved forward. Choose a time before or after the clock change.".into());
        }
        LocalResult::Ambiguous(first, second) => match occurrence {
            "earlier" => first.min(second),
            "later" => first.max(second),
            _ => return Err("This local time occurs twice because the clocks moved back. Choose the first or second occurrence.".into()),
        },
    };
    Ok(instant.timestamp_millis() as f64)
}

pub fn validate_dms(degrees: f64, minutes: f64, seconds: f64, sign: &str) -> Result<f64, String> {
    let (limit, multiplier) = match sign {
        "N" => (90.0, 1.0),
        "S" => (90.0, -1.0),
        "E" => (180.0, 1.0),
        "W" => (180.0, -1.0),
        _ => return Err("Choose N/S for latitude or E/W for longitude.".into()),
    };
    if !degrees.is_finite()
        || !minutes.is_finite()
        || !seconds.is_finite()
        || !(0.0..=limit).contains(&degrees)
        || !(0.0..60.0).contains(&minutes)
        || !(0.0..60.0).contains(&seconds)
    {
        return Err(
            "Coordinates need nonnegative degrees, minutes below 60, and seconds below 60.".into(),
        );
    }
    let value = degrees + minutes / 60.0 + seconds / 3600.0;
    if value > limit {
        return Err(format!("Coordinate exceeds {limit} degrees."));
    }
    Ok(value * multiplier)
}

#[cfg(feature = "wasm")]
mod wasm {
    use wasm_bindgen::prelude::*;

    #[wasm_bindgen(js_name = nudge_chart_time)]
    pub fn nudge(local: &str, unit: &str, direction: i32) -> Result<String, JsValue> {
        super::nudge_chart_time(local, unit, direction).map_err(|e| JsValue::from_str(&e))
    }

    #[wasm_bindgen(js_name = resolve_chart_time)]
    pub fn resolve(local: &str, timezone: &str, occurrence: &str) -> Result<f64, JsValue> {
        super::resolve_chart_time(local, timezone, occurrence).map_err(|e| JsValue::from_str(&e))
    }

    #[wasm_bindgen(js_name = validate_dms)]
    pub fn dms(degrees: f64, minutes: f64, seconds: f64, sign: &str) -> Result<f64, JsValue> {
        super::validate_dms(degrees, minutes, seconds, sign).map_err(|e| JsValue::from_str(&e))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn calendar_nudges_clamp_month_ends_and_preserve_civil_time() {
        assert_eq!(
            nudge_chart_time("2024-01-31T12:15", "month", 1).unwrap(),
            "2024-02-29T12:15"
        );
        assert_eq!(
            nudge_chart_time("2024-02-29T12:15", "year", 1).unwrap(),
            "2025-02-28T12:15"
        );
        assert_eq!(
            nudge_chart_time("2024-03-10T01:30", "hour", 1).unwrap(),
            "2024-03-10T02:30"
        );
        assert!(nudge_chart_time("1600-01-01T12:15", "year", -1).is_err());
    }

    #[test]
    fn selected_timezone_controls_the_instant() {
        assert_eq!(
            resolve_chart_time("2024-06-21T13:00", "Europe/London", "").unwrap(),
            1_718_971_200_000.0
        );
        assert_eq!(
            resolve_chart_time("2024-06-21T08:00", "America/New_York", "").unwrap(),
            1_718_971_200_000.0
        );
        assert_eq!(
            resolve_chart_time("2024-06-21T17:45", "Asia/Kathmandu", "").unwrap(),
            1_718_971_200_000.0
        );
    }

    #[test]
    fn rejects_invalid_civil_time_and_zone() {
        for local in [
            "2024-02-30T12:00",
            "2024-01-01T24:00",
            "2024-01-01T12:60",
            "1599-01-01T12:00",
        ] {
            assert!(resolve_chart_time(local, "UTC", "").is_err(), "{local}");
        }
        assert!(resolve_chart_time("2024-01-01T12:00", "unknown", "").is_err());
    }

    #[test]
    fn clock_changes_require_a_real_unambiguous_instant() {
        assert!(
            resolve_chart_time("2024-03-10T02:30", "America/New_York", "")
                .unwrap_err()
                .contains("does not exist")
        );
        assert!(
            resolve_chart_time("2024-11-03T01:30", "America/New_York", "")
                .unwrap_err()
                .contains("occurs twice")
        );
        let first = resolve_chart_time("2024-11-03T01:30", "America/New_York", "earlier").unwrap();
        let second = resolve_chart_time("2024-11-03T01:30", "America/New_York", "later").unwrap();
        assert_eq!(second - first, 3_600_000.0);
    }

    #[test]
    fn coordinate_components_cannot_overflow_or_cancel_each_other() {
        assert_eq!(validate_dms(74.0, 30.0, 0.0, "W").unwrap(), -74.5);
        for (d, m, s) in [
            (-1.0, 0.0, 0.0),
            (90.0, 0.0, 1.0),
            (40.0, 60.0, 0.0),
            (40.0, 0.0, 60.0),
            (f64::NAN, 0.0, 0.0),
        ] {
            assert!(validate_dms(d, m, s, "N").is_err());
        }
    }
}
