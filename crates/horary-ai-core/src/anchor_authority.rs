//! Source authority for the reader's chart anchor, separate from event context.
//! These conservative declarations do not parse time, geocode, or judge a matter.

fn words(text: &str) -> Vec<(usize, &str)> {
    let mut result = Vec::new();
    let mut start = None;
    for (at, ch) in text.char_indices() {
        if ch.is_alphanumeric() || matches!(ch, '\'' | '’') {
            start.get_or_insert(at);
        } else if let Some(start) = start.take() {
            result.push((start, &text[start..at]));
        }
    }
    if let Some(start) = start {
        result.push((start, &text[start..]));
    }
    result
}

fn possessive_stem(word: &str) -> &str {
    word.strip_suffix("'s")
        .or_else(|| word.strip_suffix("’s"))
        .unwrap_or(word)
}

/// Whole-word matching keeps a name or capacity from matching a substring.
fn mentions(text: &str, phrase: &str) -> bool {
    let text = text.to_ascii_lowercase();
    let phrase = phrase.to_ascii_lowercase();
    let text = words(&text);
    let phrase = words(&phrase);
    !phrase.is_empty()
        && text.windows(phrase.len()).any(|window| {
            window
                .iter()
                .zip(&phrase)
                .all(|(left, right)| possessive_stem(left.1) == possessive_stem(right.1))
        })
}

/// A standalone first-person declaration may follow the actual question.
fn unquoted_sentence_starts(words: &str) -> Vec<usize> {
    let mut starts = vec![0];
    let mut quoted_until = None;
    for (at, ch) in words.char_indices() {
        let previous = words[..at].chars().next_back();
        let next = words[at + ch.len_utf8()..].chars().next();
        let apostrophe =
            previous.is_some_and(char::is_alphanumeric) && next.is_some_and(char::is_alphanumeric);
        if let Some(end) = quoted_until {
            if ch == end && !(matches!(ch, '\'' | '’') && apostrophe) {
                quoted_until = None;
            }
            continue;
        }
        match ch {
            '"' => quoted_until = Some('"'),
            '“' => quoted_until = Some('”'),
            '‘' => quoted_until = Some('’'),
            '\'' if !apostrophe && !previous.is_some_and(char::is_alphanumeric) => {
                quoted_until = Some('\'')
            }
            '.' | '?' | '!' | '\n' => starts.push(at + ch.len_utf8()),
            _ => {}
        }
    }
    starts
}

/// A narrow source-defined obligation, independent of the model's worksheet.
/// Direct question-anchor instructions differ from event places and dates.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HistoricalAnchorRequest<'a> {
    pub clause: &'a str,
    pub supplied_place: bool,
    pub supplied_time: bool,
}

pub fn historical_anchor_request(words: &str) -> Option<HistoricalAnchorRequest<'_>> {
    let starts = unquoted_sentence_starts(words);
    for (index, at) in starts.iter().enumerate() {
        let before = &words[..*at];
        let header = before
            .rsplit("\n\n")
            .next()
            .unwrap_or(before)
            .lines()
            .rev()
            .find(|line| !line.trim().is_empty())
            .unwrap_or("")
            .trim();
        if header.ends_with(':')
            && [
                "said", "says", "asked", "asks", "wrote", "writes", "told", "reported",
            ]
            .iter()
            .any(|cue| mentions(header, cue))
        {
            continue;
        }
        let end = starts.get(index + 1).copied().unwrap_or(words.len());
        let clause = words[*at..end].trim();
        let lower = clause.to_ascii_lowercase();
        let imperative = lower.strip_prefix("please ").unwrap_or(&lower);
        let prefixes = [
            "use my earlier consultation",
            "use the earlier consultation",
            "use my original consultation",
            "use the question i understood",
            "use the question i first understood",
        ];
        let Some(prefix) = prefixes.iter().find(|prefix| {
            imperative.starts_with(**prefix)
                && imperative[prefix.len()..]
                    .chars()
                    .next()
                    .is_none_or(|ch| !ch.is_alphanumeric())
        }) else {
            continue;
        };
        let tail = &imperative[prefix.len()..];
        let months = [
            "january",
            "february",
            "march",
            "april",
            "may",
            "june",
            "july",
            "august",
            "september",
            "october",
            "november",
            "december",
        ];
        let supplied_place = [" in ", " from "].iter().any(|cue| {
            tail.split_once(*cue).is_some_and(|(_, rest)| {
                let rest = rest.trim();
                let period = rest.trim_end_matches(['.', '?', '!']);
                let date_words = period.split_whitespace().collect::<Vec<_>>();
                let temporal_tail = date_words.first().is_some_and(|word|
                    ["yesterday", "today", "tomorrow"].contains(word))
                    || matches!(date_words.as_slice(), [month, year, ..]
                        if months.contains(month) && year.trim_matches(|ch: char| !ch.is_ascii_digit()).len() == 4
                            && year.trim_matches(|ch: char| !ch.is_ascii_digit()).chars().all(|ch| ch.is_ascii_digit()));
                !temporal_tail
                    && !rest.is_empty()
                    && !rest.starts_with(char::is_numeric)
                    && ![
                        "the morning",
                        "the afternoon",
                        "the evening",
                        "morning",
                        "afternoon",
                        "evening",
                    ]
                    .iter()
                    .any(|period| rest.starts_with(period))
            })
        });
        let has_clock = tail.split_whitespace().any(|word| {
            let clock = word.trim_matches(|ch: char| !ch.is_ascii_digit() && ch != ':');
            clock.split_once(':').is_some_and(|(hour, minute)| {
                !hour.is_empty()
                    && !minute.is_empty()
                    && hour.chars().all(|ch| ch.is_ascii_digit())
                    && minute.chars().all(|ch| ch.is_ascii_digit())
            })
        });
        let tokens: Vec<_> = tail
            .split(|ch: char| !ch.is_ascii_alphanumeric())
            .filter(|word| !word.is_empty())
            .collect();
        let has_year = tokens
            .iter()
            .any(|part| part.len() == 4 && part.chars().all(|ch| ch.is_ascii_digit()));

        let has_month_day = tokens.iter().enumerate().any(|(index, word)| {
            months.contains(word)
                && [index.checked_sub(1), index.checked_add(1)]
                    .into_iter()
                    .flatten()
                    .any(|next| {
                        tokens.get(next).is_some_and(|day| {
                            day.parse::<u8>().is_ok_and(|day| (1..=31).contains(&day))
                        })
                    })
        });
        let has_iso_date = tail.split(|ch: char| !ch.is_ascii_digit() && ch != '-').any(|word| {
            let parts: Vec<_> = word.split('-').collect();
            matches!(parts.as_slice(), [year, month, day] if year.len() == 4 && month.len() == 2 && day.len() == 2
                && parts.iter().all(|part| part.chars().all(|ch| ch.is_ascii_digit())))
        });
        return Some(HistoricalAnchorRequest {
            clause,
            supplied_place,
            supplied_time: has_clock && ((has_year && has_month_day) || has_iso_date),
        });
    }
    None
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReaderPlaceDeclaration<'a> {
    pub value: &'a str,
    pub quote: &'a str,
}

/// Only these factual fields can replace the reader's current device anchor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AnchorField {
    ReaderPlace,
    QuestionTime,
}

/// Check declaration provenance, not geocoding, civil-time validity, or the
/// semantic equivalence of an extracted value. Device defaults do not need a
/// user declaration; this boundary is for source-authorized overrides.
pub fn validate_source_quote(words: &str, field: AnchorField, quote: &str) -> Result<(), String> {
    if quote.trim().is_empty() || !words.contains(quote) {
        return Err("An anchor override needs a nonempty exact source quote.".into());
    }
    if field == AnchorField::ReaderPlace
        && reader_place_statement(words)
            .is_some_and(|declaration| declaration.quote.contains(quote))
    {
        return Ok(());
    }
    if historical_anchor_request(words).is_some_and(|request| {
        let supplied = match field {
            AnchorField::ReaderPlace => request.supplied_place,
            AnchorField::QuestionTime => request.supplied_time,
        };
        supplied && request.clause.contains(quote)
    }) {
        return Ok(());
    }
    Err("The source quote is not within a supplied reader declaration or explicit historical question-anchor instruction. An event venue, event date, negation, or reported statement does not authorize an override.".into())
}

/// Quoted speech and ambiguous compound declarations remain with recognition.
pub fn reader_place_statement(words: &str) -> Option<ReaderPlaceDeclaration<'_>> {
    let words = words.trim();
    let starts = unquoted_sentence_starts(words);
    let prefixes = ["i'm asking from ", "i am asking from ", "i’m asking from "];
    let declarations: Vec<_> = starts
        .iter()
        .filter_map(|at| {
            let before = &words[..*at];
            let paragraph = before.rsplit("\n\n").next().unwrap_or(before);
            let header = paragraph.lines().next().unwrap_or("").trim();
            if header.ends_with(':')
                && [
                    "said", "says", "asked", "asks", "wrote", "writes", "told", "reported",
                ]
                .iter()
                .any(|cue| mentions(header, cue))
            {
                return None; // A colon-led report is somebody's words, not this reader's declaration.
            }
            let statement = words[*at..].trim();
            let lower = statement.to_ascii_lowercase();
            prefixes
                .iter()
                .find(|prefix| lower.starts_with(**prefix))
                .map(|prefix| (statement, prefix.len()))
        })
        .collect();
    let [(statement, prefix_len)] = declarations.as_slice() else {
        return None;
    };
    let place = statement[*prefix_len..].trim().trim_end_matches(['.', '!']);
    let lower = place.to_ascii_lowercase();
    // Complete deictic phrases request the current device anchor, not a city
    // named "here, now". Keep explicit names such as Hereford or Here, Kansas.
    let deictic_words: Vec<_> = lower
        .split(|ch: char| ch.is_whitespace() || ch == ',')
        .filter(|word| !word.is_empty())
        .collect();
    let device = matches!(
        deictic_words.as_slice(),
        ["here"]
            | ["here", "now"]
            | ["right", "here"]
            | ["right", "here", "now"]
            | ["right", "here", "right", "now"]
            | ["here", "at", "the", "moment"]
            | ["where", "i", "am"]
            | ["where", "i", "am", "now"]
            | ["where", "i", "am", "right", "now"]
            | ["where", "i", "currently", "am"]
            | ["my", "present", "location"]
    ) || mentions(place, "my device")
        || mentions(place, "this device")
        || mentions(place, "current location");
    let extra_sentence = place.match_indices('.').any(|(at, _)| {
        let suffix = &place[at + 1..];
        let prefix_word = place[..at].split_whitespace().next_back().unwrap_or("");
        !suffix.trim().is_empty()
            && suffix.starts_with(char::is_whitespace)
            && !matches!(
                prefix_word.to_ascii_lowercase().as_str(),
                "st" | "mt" | "ft" | "u" | "s"
            )
    });
    if place.is_empty()
        || place.len() > 700
        || place.contains(['?', ';', '\n', '"', '“', '”', '‘'])
        || place.ends_with(['\'', '’'])
        || device
        || extra_sentence
    {
        return None;
    }
    Some(ReaderPlaceDeclaration {
        value: place,
        quote: statement,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn current_reader_declaration_keeps_exact_value_and_quote() {
        let words =
            "Will my parents' business sell in Paris? I’m asking from D’Iberville, Mississippi.";
        let declaration = reader_place_statement(words).unwrap();
        assert_eq!(declaration.value, "D’Iberville, Mississippi");
        assert_eq!(
            declaration.quote,
            "I’m asking from D’Iberville, Mississippi."
        );
        validate_source_quote(words, AnchorField::ReaderPlace, "D’Iberville, Mississippi").unwrap();
        assert!(validate_source_quote(words, AnchorField::ReaderPlace, "Paris").is_err());
        assert!(validate_source_quote(words, AnchorField::QuestionTime, "D’Iberville").is_err());
    }

    #[test]
    fn historical_components_are_bound_to_the_directive_not_the_event() {
        let words = "Will the fair in Paris on Friday succeed? Use the question I understood in London on January 14, 2026 at 14:30.";
        let request = historical_anchor_request(words).unwrap();
        assert_eq!(
            request.clause,
            "Use the question I understood in London on January 14, 2026 at 14:30."
        );
        assert!(request.supplied_place && request.supplied_time);
        validate_source_quote(words, AnchorField::ReaderPlace, "London").unwrap();
        validate_source_quote(
            words,
            AnchorField::QuestionTime,
            "January 14, 2026 at 14:30",
        )
        .unwrap();
        assert!(validate_source_quote(words, AnchorField::ReaderPlace, "Paris").is_err());
        assert!(validate_source_quote(words, AnchorField::QuestionTime, "Friday").is_err());
    }

    #[test]
    fn event_negation_and_global_chart_words_do_not_authorize_an_anchor() {
        for words in [
            "Will I sell the book in London on Friday?",
            "I asked whether the fair in London at 14:30 on January 14, 2026 succeeds.",
            "The chart is for my question about the event in London on January 14, 2026 at 14:30.",
            "Do not use my earlier consultation in London on January 14, 2026 at 14:30.",
            "I am not asking from London; that is where the auction is.",
        ] {
            assert!(reader_place_statement(words).is_none(), "{words}");
            assert!(historical_anchor_request(words).is_none(), "{words}");
            assert!(
                validate_source_quote(words, AnchorField::ReaderPlace, "London").is_err(),
                "{words}"
            );
            assert!(
                validate_source_quote(words, AnchorField::QuestionTime, "14:30").is_err(),
                "{words}"
            );
        }
    }

    #[test]
    fn quoted_and_colon_led_reports_do_not_become_reader_authority() {
        for words in [
            "Mira said, \"I'm asking from London.\"",
            "Mira said, 'I'm asking from London.'",
            "Mira said, ‘I’m asking from London.’",
            "Mira said:\nI'm asking from London.",
            "Mira wrote:\nUse my earlier consultation in London on January 14, 2026 at 14:30.",
            "Mira reported: \"Use my earlier consultation in London on January 14, 2026 at 14:30.\"",
        ] {
            assert!(reader_place_statement(words).is_none(), "{words}");
            assert!(historical_anchor_request(words).is_none(), "{words}");
            assert!(validate_source_quote(words, AnchorField::ReaderPlace, "London").is_err(), "{words}");
        }
    }

    #[test]
    fn unsupplied_historical_components_stay_unsupplied() {
        for (words, place, time) in [
            ("Use my earlier consultation.", false, false),
            ("Use my earlier consultation in London.", true, false),
            (
                "Use my earlier consultation from January 2026 at 14:30.",
                false,
                false,
            ),
            (
                "Use my earlier consultation from yesterday at 3:30 PM.",
                false,
                false,
            ),
            (
                "Use my earlier consultation on 2026-01-14 at 14:30.",
                false,
                true,
            ),
        ] {
            let request = historical_anchor_request(words).unwrap();
            assert_eq!(request.supplied_place, place, "{words}");
            assert_eq!(request.supplied_time, time, "{words}");
            if !place {
                assert!(
                    validate_source_quote(words, AnchorField::ReaderPlace, request.clause).is_err()
                );
            }
            if !time {
                assert!(
                    validate_source_quote(words, AnchorField::QuestionTime, request.clause)
                        .is_err()
                );
            }
        }
    }

    #[test]
    fn ambiguous_or_device_declarations_are_not_explicit_city_overrides() {
        for words in [
            "I'm asking from London. I'm asking from Paris.",
            "I'm asking from London; the event is in Paris.",
            "I'm asking from here, now.",
            "I'm asking from my device's current location.",
        ] {
            assert!(reader_place_statement(words).is_none(), "{words}");
            assert!(
                validate_source_quote(words, AnchorField::ReaderPlace, "I'm asking from").is_err(),
                "{words}"
            );
        }
        let declaration = reader_place_statement("I'm asking from St. Louis, Missouri.").unwrap();
        assert_eq!(declaration.value, "St. Louis, Missouri");
        assert_eq!(
            reader_place_statement("I'm asking from Hereford.")
                .unwrap()
                .value,
            "Hereford"
        );
    }

    #[test]
    fn source_proof_cannot_be_empty_or_normalized_from_different_bytes() {
        let words = "I’m asking from London.";
        for quote in ["", " ", "I'm asking from London.", "Paris"] {
            assert!(validate_source_quote(words, AnchorField::ReaderPlace, quote).is_err());
        }
        validate_source_quote(words, AnchorField::ReaderPlace, "I’m asking from London.").unwrap();
    }
}
