//! Resolve a civil chart time once, before either calculation or interpretation.
use chrono::{Datelike, LocalResult, NaiveDate, NaiveDateTime, TimeZone};
use chrono_tz::Tz;

/// A date-only calculation from this turn's actual native clock. It neither
/// chooses an event hour nor converts an event's civil time into an instant.
#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct EventDay {
    pub date: String,
    pub source_day: String,
    pub receipt_timestamp_ms: f64,
    pub local_clock: String,
    pub timezone: String,
}

/// Resolve only a literal current-turn day with one unambiguous interpretation.
/// Retained questions need their original clock and are deliberately not used
/// as a new turn's calendar source. Absolute dates remain user authority.
pub fn resolve_event_day(
    quote: &str,
    words: &str,
    question: &str,
    pending_event_time: bool,
    receipt_timestamp_ms: f64,
    timezone: &str,
) -> Result<Option<EventDay>, String> {
    if quote.trim().is_empty() {
        return Ok(None);
    }
    let occurrences: Vec<_> = words.match_indices(quote).collect();
    if occurrences.len() != 1 {
        return Ok(None);
    }
    let days: Vec<_> = word_spans(quote)
        .into_iter()
        .filter(|(_, _, word)| day_word(word).is_some())
        .collect();
    let [(start, _, source_day)] = days.as_slice() else {
        return Ok(None);
    };
    let day = day_word(source_day).expect("Selected day word");
    // Reported/historical speech cannot borrow today's clock. In particular,
    // a model cannot omit 'next' from its quote and turn it into bare Friday.
    let source_tokens: Vec<_> = word_spans(words)
        .into_iter()
        .chain(word_spans(question))
        .map(|(_, _, word)| word.to_ascii_lowercase())
        .collect();
    if words.contains(['"', '“', '”'])
        || question.contains(['"', '“', '”'])
        || source_tokens.iter().any(|word| {
            matches!(
                word.as_str(),
                "next" | "last" | "previous" | "following" | "historical" | "original" | "ago"
            )
        })
    {
        return Ok(None);
    }
    let source_offset = occurrences[0].0 + *start;
    let clause_start = words[..source_offset]
        .rfind(['.', ';', '?', '!'])
        .map_or(0, |index| index + 1);
    let clause_end = words[source_offset..]
        .find(['.', ';', '?', '!'])
        .map_or(words.len(), |index| source_offset + index);
    let clause = &words[clause_start..clause_end];
    if word_spans(clause)
        .iter()
        .filter(|(_, _, word)| day_word(word).is_some())
        .count()
        != 1
        || has_absolute_date(clause)
        || word_spans(clause).iter().any(|(_, _, word)| {
            matches!(
                word.to_ascii_lowercase().as_str(),
                "asked" | "said" | "told"
            )
        })
    {
        return Ok(None);
    }
    if let DayWord::Weekday(_) = day {
        let past = source_tokens.iter().any(|word| {
            matches!(
                word.as_str(),
                "was"
                    | "were"
                    | "did"
                    | "had"
                    | "sold"
                    | "bought"
                    | "visited"
                    | "arrived"
                    | "went"
                    | "happened"
                    | "spent"
                    | "yesterday"
                    | "earlier"
            )
        });
        let modified_day = source_tokens
            .windows(2)
            .any(|pair| pair[0] == "this" && pair[1].eq_ignore_ascii_case(source_day));
        let future_question = future_event_question(clause)
            || question
                .split(['.', ';', '?', '!'])
                .any(|part| part.contains(source_day) && future_event_question(part));
        let supplied_future_day = pending_event_time && future_event_question(question);
        let scheduled = ["scheduled for", "planned for", "booked for"]
            .iter()
            .any(|phrase| {
                clause
                    .to_ascii_lowercase()
                    .contains(&format!("{phrase} {}", source_day.to_ascii_lowercase()))
            });
        if past || modified_day || !(future_question || supplied_future_day || scheduled) {
            return Ok(None);
        }
    }
    let clock = local_clock(receipt_timestamp_ms, timezone)?;
    let today = NaiveDate::parse_from_str(&clock[..10], "%Y-%m-%d")
        .map_err(|_| "The native clock's date is invalid.")?;
    let offset = match day {
        DayWord::Offset(offset) => offset,
        DayWord::Weekday(weekday) => {
            let offset =
                (weekday - i64::from(today.weekday().num_days_from_monday())).rem_euclid(7);
            if offset == 0 {
                return Ok(None); // Bare Friday today might mean today or next week.
            }
            offset
        }
    };
    let date = today
        .checked_add_signed(chrono::Duration::days(offset))
        .ok_or("The event date is outside the supported calendar.")?;
    Ok(Some(EventDay {
        date: date.format("%Y-%m-%d").to_string(),
        source_day: (*source_day).into(),
        receipt_timestamp_ms,
        local_clock: clock,
        timezone: timezone.into(),
    }))
}

/// Replace only the date component, retaining every other byte of the proposed
/// event description, including its time of day. Two date components are not a
/// license to discard either one. The caller keeps the original value in audit.
pub fn event_day_value(value: &str, day: &EventDay) -> Option<String> {
    let dates = iso_dates(value);
    let spans: Vec<_> = word_spans(value)
        .into_iter()
        .filter(|(_, _, word)| word.eq_ignore_ascii_case(&day.source_day))
        .collect();
    let (start, end) = match (dates.as_slice(), spans.as_slice()) {
        ([(start, end)], _) => (*start, *end),
        ([], [(start, end, _)]) => (*start, *end),
        _ => return None,
    };
    Some(format!("{}{}{}", &value[..start], day.date, &value[end..]))
}

#[derive(Clone, Copy)]
enum DayWord {
    Offset(i64),
    Weekday(i64),
}

fn day_word(word: &str) -> Option<DayWord> {
    Some(match word.to_ascii_lowercase().as_str() {
        "today" => DayWord::Offset(0),
        "tomorrow" => DayWord::Offset(1),
        "yesterday" => DayWord::Offset(-1),
        "monday" => DayWord::Weekday(0),
        "tuesday" => DayWord::Weekday(1),
        "wednesday" => DayWord::Weekday(2),
        "thursday" => DayWord::Weekday(3),
        "friday" => DayWord::Weekday(4),
        "saturday" => DayWord::Weekday(5),
        "sunday" => DayWord::Weekday(6),
        _ => return None,
    })
}

fn word_spans(text: &str) -> Vec<(usize, usize, &str)> {
    let mut spans = Vec::new();
    let mut start = None;
    for (index, character) in text.char_indices() {
        if character.is_alphanumeric() {
            start.get_or_insert(index);
        } else if let Some(start) = start.take() {
            spans.push((start, index, &text[start..index]));
        }
    }
    if let Some(start) = start {
        spans.push((start, text.len(), &text[start..]));
    }
    spans
}

fn iso_dates(text: &str) -> Vec<(usize, usize)> {
    text.char_indices()
        .filter_map(|(index, _)| {
            let part = text.get(index..index + 10)?;
            (NaiveDate::parse_from_str(part, "%Y-%m-%d").is_ok()
                && part.as_bytes()[4] == b'-'
                && part.as_bytes()[7] == b'-')
                .then_some((index, index + 10))
        })
        .collect()
}

fn has_absolute_date(text: &str) -> bool {
    if !iso_dates(text).is_empty()
        || text.as_bytes().windows(3).any(|part| {
            part[0].is_ascii_digit()
                && matches!(part[1], b'/' | b'-' | b'.')
                && part[2].is_ascii_digit()
        })
    {
        return true;
    }
    let words = word_spans(text);
    words.iter().enumerate().any(|(index, (_, _, word))| {
        let month = matches!(
            word.to_ascii_lowercase().as_str(),
            "jan"
                | "january"
                | "feb"
                | "february"
                | "mar"
                | "march"
                | "apr"
                | "april"
                | "may"
                | "jun"
                | "june"
                | "jul"
                | "july"
                | "aug"
                | "august"
                | "sep"
                | "sept"
                | "september"
                | "oct"
                | "october"
                | "nov"
                | "november"
                | "dec"
                | "december"
        );
        month
            && [index.checked_sub(1), Some(index + 1)]
                .into_iter()
                .flatten()
                .filter_map(|adjacent| words.get(adjacent))
                .any(|(_, _, word)| word.chars().next().is_some_and(|c| c.is_ascii_digit()))
    })
}

fn future_event_question(text: &str) -> bool {
    let text = text.trim().to_ascii_lowercase();
    text.starts_with("will ")
        || text.starts_with("when will ")
        || (text.starts_with("how many ") || text.starts_with("how much "))
            && text.contains(" will ")
}

/// Native clock data for a question-moment task; never a model-invented date.
pub fn local_clock(timestamp_ms: f64, timezone: &str) -> Result<String, String> {
    if !timestamp_ms.is_finite() || timestamp_ms.abs() > 8.64e15 {
        return Err("The question's clock is invalid.".into());
    }
    let zone: Tz = timezone
        .parse()
        .map_err(|_| "The clock's timezone is invalid.")?;
    let instant = chrono::DateTime::from_timestamp_millis(timestamp_ms as i64)
        .ok_or("The question's clock is outside the calendar.")?;
    Ok(instant
        .with_timezone(&zone)
        .format("%Y-%m-%dT%H:%M")
        .to_string())
}

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

    fn event_day(quote: &str, words: &str, question: &str, pending: bool) -> Option<EventDay> {
        resolve_event_day(
            quote,
            words,
            question,
            pending,
            resolve_chart_time("2032-03-02T10:00", "Europe/London", "").unwrap(),
            "Europe/London",
        )
        .unwrap()
    }

    #[test]
    fn event_days_use_native_calendar_arithmetic_and_preserve_every_other_value_byte() {
        let words = "Will the boat sell on Friday at 3 PM?";
        let day = event_day("Friday", words, words, false).unwrap();
        assert_eq!(day.date, "2032-03-05");
        assert_eq!(day.local_clock, "2032-03-02T10:00");
        assert_eq!(
            event_day_value("Friday at 3 PM", &day).unwrap(),
            "2032-03-05 at 3 PM"
        );
        assert_eq!(
            event_day_value("2032-03-04 at 3 PM", &day).unwrap(),
            "2032-03-05 at 3 PM"
        );
        assert_eq!(
            event_day_value("évent: Friday, at 3 PM; indoors", &day).unwrap(),
            "évent: 2032-03-05, at 3 PM; indoors"
        );
        assert!(event_day_value("2032-03-04 or 2032-03-06", &day).is_none());
        assert_eq!(
            event_day("Friday at 3 PM", words, words, false).unwrap(),
            day
        );
        assert!(event_day("Friday…", words, words, false).is_none());
    }

    #[test]
    fn future_chronology_needs_the_actual_event_question_or_a_bound_supplying_turn() {
        assert!(event_day("Friday", "Friday at 3 PM", "Will the boat sell?", true).is_some());
        assert!(event_day("Friday", "Friday at 3 PM", "Will the boat sell?", false).is_none());
        assert!(event_day(
            "Friday",
            "Will this boat sell Friday? This question is asked here now.",
            "Will this boat sell Friday?",
            false
        )
        .is_some());
        for words in [
            "I sold the boat Friday.",
            "Will the boat sell? I sold my bicycle Friday.",
            "My friend will travel. The market is Friday.",
            "Will the boat sell Friday? It was advertised Monday.",
            "Will the boat sell next Friday?",
            "Will the boat sell this Friday?",
            "He asked: \"Will the boat sell Friday?\"",
            "Will the boat sell Friday or Saturday?",
            "Will the boat sell Friday, March 12?",
            "Will the boat sell Friday, Mar 12?",
            "Will the boat sell Friday, 2032-03-12?",
            "Will the boat sell Friday, 3/12?",
        ] {
            assert!(
                event_day("Friday", words, words, false).is_none(),
                "{words}"
            );
        }
    }

    #[test]
    fn relative_days_use_the_turn_zone_without_choosing_a_dst_hour_or_occurrence() {
        let instant = resolve_chart_time("2032-03-01T00:15", "Pacific/Auckland", "").unwrap();
        let auckland = resolve_event_day(
            "yesterday",
            "It arrived yesterday.",
            "",
            false,
            instant,
            "Pacific/Auckland",
        )
        .unwrap()
        .unwrap();
        let london = resolve_event_day(
            "yesterday",
            "It arrived yesterday.",
            "",
            false,
            instant,
            "Europe/London",
        )
        .unwrap()
        .unwrap();
        assert_eq!(auckland.date, "2032-02-29");
        assert_eq!(london.date, "2032-02-28");
        let instant = resolve_chart_time("2030-03-09T10:00", "America/New_York", "").unwrap();
        let day = resolve_event_day(
            "tomorrow at 2:30 AM",
            "The event is tomorrow at 2:30 AM.",
            "",
            false,
            instant,
            "America/New_York",
        )
        .unwrap()
        .unwrap();
        assert_eq!(
            event_day_value("tomorrow at 2:30 AM", &day).unwrap(),
            "2030-03-10 at 2:30 AM"
        );
        assert!(
            resolve_chart_time("2030-03-10T02:30", "America/New_York", "").is_err(),
            "A date calculation must not resolve a skipped event hour"
        );
        assert!(resolve_event_day("tomorrow", "tomorrow", "", false, instant, "unknown").is_err());
        assert!(resolve_event_day(
            "Friday",
            "Will it sell Friday?",
            "",
            false,
            resolve_chart_time("2032-03-05T10:00", "Europe/London", "").unwrap(),
            "Europe/London"
        )
        .unwrap()
        .is_none());
        assert!(
            event_day(
                "tomorrow",
                "The current reply says only yes.",
                "Will it sell tomorrow?",
                false
            )
            .is_none(),
            "A retained phrase cannot borrow a new receipt clock"
        );
    }

    #[test]
    fn native_clock_uses_the_selected_place_zone_and_rejects_invalid_input() {
        let instant = resolve_chart_time("2026-01-14T14:30", "Europe/London", "").unwrap();
        assert_eq!(
            local_clock(instant, "Europe/London").unwrap(),
            "2026-01-14T14:30"
        );
        assert_eq!(
            local_clock(instant, "America/New_York").unwrap(),
            "2026-01-14T09:30"
        );
        assert!(local_clock(f64::NAN, "UTC").is_err());
        assert!(local_clock(instant, "not-a-zone").is_err());
    }

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
