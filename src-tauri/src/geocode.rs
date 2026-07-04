use serde::{Deserialize, Serialize};
use std::{
    cmp::Ordering,
    collections::{HashMap, VecDeque},
    sync::{Mutex, OnceLock},
};

const CITIES_JSON: &str = include_str!("../../src/data/cities.json");
const US_LOCATIONS_JSON: &str = include_str!("../../src/data/us_locations.json");
const PROVIDER_ID: &str = "bundled-cities";
const US_POSTAL_PROVIDER_ID: &str = "bundled-us-postal";
const DEFAULT_LIMIT: usize = 8;
const MAX_LIMIT: usize = 50;
const CACHE_SIZE: usize = 50;
const DEFAULT_REVERSE_MAX_DISTANCE_KM: f64 = 50.0;
const EARTH_RADIUS_KM: f64 = 6371.0088;

#[derive(Debug, Serialize)]
pub struct GeocodeError {
    pub message: String,
}

impl From<serde_json::Error> for GeocodeError {
    fn from(value: serde_json::Error) -> Self {
        Self {
            message: value.to_string(),
        }
    }
}

pub type GeocodeResult<T> = Result<T, GeocodeError>;

#[derive(Debug, Clone, Deserialize)]
struct CityRecord {
    name: String,
    country: String,
    lat: f64,
    lng: f64,
    tz: String,
}

#[derive(Debug, Clone, Deserialize)]
struct UsLocations {
    zips: Vec<UsZipRecord>,
    places: Vec<UsPlaceRecord>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct UsZipRecord {
    zip: String,
    city: String,
    state: String,
    lat: f64,
    lng: f64,
    tz: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct UsPlaceRecord {
    city: String,
    state: String,
    lat: f64,
    lng: f64,
    tz: String,
    zip_count: usize,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GeocodeRequest {
    pub query: String,
    #[serde(default)]
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReverseGeocodeRequest {
    pub latitude: f64,
    pub longitude: f64,
    #[serde(default)]
    pub max_distance_km: Option<f64>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LocationCandidate {
    pub id: String,
    pub label: String,
    pub name: String,
    pub country: String,
    pub latitude: f64,
    pub longitude: f64,
    pub timezone: String,
    pub provider: String,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LocationLabel {
    pub label: String,
    pub name: String,
    pub country: String,
    pub latitude: f64,
    pub longitude: f64,
    pub timezone: String,
    pub provider: String,
    pub distance_km: f64,
}

#[derive(Default)]
pub struct GeocodeState {
    cache: Mutex<GeocodeCache>,
}

#[derive(Default)]
struct GeocodeCache {
    entries: HashMap<String, Vec<LocationCandidate>>,
    order: VecDeque<String>,
}

pub fn geocode_with_cache(
    state: &GeocodeState,
    req: GeocodeRequest,
) -> GeocodeResult<Vec<LocationCandidate>> {
    let query = normalize_query(&req.query);
    if query.len() < 2 {
        return Ok(Vec::new());
    }

    let limit = normalize_limit(req.limit);
    let key = format!("{query}|{limit}");
    if let Some(cached) = state.cache.lock().unwrap().get(&key) {
        return Ok(cached);
    }

    let results = search_location_records(&query, limit)?;
    state.cache.lock().unwrap().insert(key, results.clone());
    Ok(results)
}

pub fn reverse_geocode_local_city(
    req: ReverseGeocodeRequest,
) -> GeocodeResult<Option<LocationLabel>> {
    validate_coordinates(req.latitude, req.longitude)?;
    let max_distance_km = req
        .max_distance_km
        .filter(|value| value.is_finite() && *value > 0.0)
        .unwrap_or(DEFAULT_REVERSE_MAX_DISTANCE_KM);

    let nearest_city = city_records()?
        .iter()
        .map(|city| {
            (
                city_to_candidate(city),
                distance_km(req.latitude, req.longitude, city.lat, city.lng),
            )
        })
        .min_by(compare_distance);

    let nearest_us_place = us_location_records()?
        .places
        .iter()
        .map(|place| {
            (
                us_place_to_candidate(place),
                distance_km(req.latitude, req.longitude, place.lat, place.lng),
            )
        })
        .min_by(compare_distance);

    let nearest = [nearest_city, nearest_us_place]
        .into_iter()
        .flatten()
        .min_by(compare_distance);

    Ok(nearest.and_then(|(candidate, distance)| {
        if distance > max_distance_km {
            return None;
        }
        Some(LocationLabel {
            label: candidate.label,
            name: candidate.name,
            country: candidate.country,
            latitude: candidate.latitude,
            longitude: candidate.longitude,
            timezone: candidate.timezone,
            provider: candidate.provider,
            distance_km: (distance * 1000.0).round() / 1000.0,
        })
    }))
}

fn search_location_records(query: &str, limit: usize) -> GeocodeResult<Vec<LocationCandidate>> {
    let mut scored: Vec<(u16, LocationCandidate)> = Vec::new();
    scored.extend(search_us_locations(query)?);
    scored.extend(
        search_city_records(query, limit)?
            .into_iter()
            .map(|candidate| {
                (
                    90 + city_match_rank(&candidate.name, query) as u16,
                    candidate,
                )
            }),
    );

    scored.sort_by(|(a_score, a), (b_score, b)| {
        a_score
            .cmp(b_score)
            .then_with(|| a.label.cmp(&b.label))
            .then_with(|| a.id.cmp(&b.id))
    });

    let mut seen = HashMap::new();
    let mut results = Vec::new();
    for (_, candidate) in scored {
        if seen.insert(candidate.id.clone(), ()).is_none() {
            results.push(candidate);
        }
        if results.len() >= limit {
            break;
        }
    }
    Ok(results)
}

fn search_us_locations(query: &str) -> GeocodeResult<Vec<(u16, LocationCandidate)>> {
    let parsed = parse_us_query(query);
    if parsed.state.is_none() && parsed.zip.is_none() {
        return Ok(Vec::new());
    }
    let records = us_location_records()?;
    let mut scored = Vec::new();

    if let Some(zip) = parsed.zip.as_deref() {
        for record in &records.zips {
            if record.zip == zip {
                scored.push((0, us_zip_to_candidate(record)));
            } else if zip.len() >= 3 && record.zip.starts_with(zip) {
                scored.push((
                    12 + (record.zip.len() - zip.len()) as u16,
                    us_zip_to_candidate(record),
                ));
            }
        }
    }

    let city_query = parsed.city_query.as_str();
    if !city_query.is_empty() {
        for place in &records.places {
            if let Some(state) = parsed.state.as_deref() {
                if place.state != state {
                    continue;
                }
            }

            let city = normalize_location_text(&place.city);
            let rank = if city == city_query {
                20
            } else if city.starts_with(city_query) {
                30
            } else if city.contains(city_query) {
                45
            } else {
                continue;
            };
            let state_bonus = if parsed.state.is_some() { 0 } else { 20 };
            let zip_count_penalty = place.zip_count.min(30) as u16;
            scored.push((
                rank + state_bonus + zip_count_penalty,
                us_place_to_candidate(place),
            ));
        }

        for zip_record in &records.zips {
            if let Some(state) = parsed.state.as_deref() {
                if zip_record.state != state {
                    continue;
                }
            }
            let city = normalize_location_text(&zip_record.city);
            let rank = if city == city_query {
                70
            } else if city.starts_with(city_query) {
                80
            } else {
                continue;
            };
            scored.push((rank, us_zip_to_candidate(zip_record)));
        }
    }

    Ok(scored)
}

fn search_city_records(query: &str, limit: usize) -> GeocodeResult<Vec<LocationCandidate>> {
    let mut matches: Vec<&CityRecord> = city_records()?
        .iter()
        .filter(|city| city.name.to_lowercase().contains(query))
        .collect();

    matches.sort_by(|a, b| {
        city_match_rank(&a.name, query)
            .cmp(&city_match_rank(&b.name, query))
            .then_with(|| a.name.cmp(&b.name))
    });

    Ok(matches
        .into_iter()
        .take(limit)
        .map(city_to_candidate)
        .collect())
}

fn city_records() -> GeocodeResult<&'static [CityRecord]> {
    static CITIES: OnceLock<Result<Vec<CityRecord>, String>> = OnceLock::new();
    match CITIES.get_or_init(|| {
        serde_json::from_str::<Vec<CityRecord>>(CITIES_JSON).map_err(|error| error.to_string())
    }) {
        Ok(cities) => Ok(cities.as_slice()),
        Err(message) => Err(GeocodeError {
            message: message.clone(),
        }),
    }
}

fn us_location_records() -> GeocodeResult<&'static UsLocations> {
    static US_LOCATIONS: OnceLock<Result<UsLocations, String>> = OnceLock::new();
    match US_LOCATIONS.get_or_init(|| {
        serde_json::from_str::<UsLocations>(US_LOCATIONS_JSON).map_err(|error| error.to_string())
    }) {
        Ok(locations) => Ok(locations),
        Err(message) => Err(GeocodeError {
            message: message.clone(),
        }),
    }
}

#[derive(Debug, PartialEq)]
struct ParsedUsQuery {
    city_query: String,
    state: Option<String>,
    zip: Option<String>,
}

fn parse_us_query(query: &str) -> ParsedUsQuery {
    let zip = query
        .split(|ch: char| !ch.is_ascii_digit())
        .find(|part| part.len() == 5)
        .map(str::to_string);

    let mut normalized = normalize_location_text(query);
    if let Some(zip) = zip.as_deref() {
        normalized = normalized.replace(zip, " ");
    }

    let mut state = None;
    for (abbr, name) in US_STATES {
        let normalized_name = normalize_location_text(name);
        let padded = format!(" {normalized} ");
        if padded.contains(&format!(" {} ", abbr.to_lowercase()))
            || padded.contains(&format!(" {normalized_name} "))
        {
            state = Some((*abbr).to_string());
            normalized = remove_word(&normalized, &abbr.to_lowercase());
            normalized = normalized.replace(&normalized_name, " ");
            break;
        }
    }

    ParsedUsQuery {
        city_query: normalize_location_text(&normalized),
        state,
        zip,
    }
}

fn city_match_rank(name: &str, query: &str) -> u8 {
    let normalized_name = name.to_lowercase();
    if normalized_name.starts_with(query) {
        return 0;
    }
    if normalized_name.contains(&format!(" {query}")) {
        return 1;
    }
    2
}

fn normalize_query(query: &str) -> String {
    normalize_location_text(query)
}

fn normalize_location_text(value: &str) -> String {
    value
        .to_lowercase()
        .chars()
        .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { ' ' })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn remove_word(value: &str, word: &str) -> String {
    value
        .split_whitespace()
        .filter(|part| *part != word)
        .collect::<Vec<_>>()
        .join(" ")
}

fn normalize_limit(limit: Option<usize>) -> usize {
    limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT)
}

fn city_to_candidate(city: &CityRecord) -> LocationCandidate {
    let label = format!("{}, {}", city.name, city.country);
    LocationCandidate {
        id: label.to_lowercase(),
        label,
        name: city.name.clone(),
        country: city.country.clone(),
        latitude: city.lat,
        longitude: city.lng,
        timezone: city.tz.clone(),
        provider: PROVIDER_ID.to_string(),
    }
}

fn us_zip_to_candidate(record: &UsZipRecord) -> LocationCandidate {
    let label = format!("{}, {} {}", record.city, record.state, record.zip);
    LocationCandidate {
        id: format!("us-zip-{}", record.zip),
        label,
        name: record.city.clone(),
        country: "US".to_string(),
        latitude: record.lat,
        longitude: record.lng,
        timezone: record.tz.clone(),
        provider: US_POSTAL_PROVIDER_ID.to_string(),
    }
}

fn us_place_to_candidate(place: &UsPlaceRecord) -> LocationCandidate {
    let label = format!("{}, {}", place.city, place.state);
    LocationCandidate {
        id: format!(
            "us-place-{}-{}",
            place.state.to_lowercase(),
            normalize_location_text(&place.city).replace(' ', "-")
        ),
        label,
        name: place.city.clone(),
        country: "US".to_string(),
        latitude: place.lat,
        longitude: place.lng,
        timezone: place.tz.clone(),
        provider: US_POSTAL_PROVIDER_ID.to_string(),
    }
}

fn compare_distance<T>((_, a_distance): &(T, f64), (_, b_distance): &(T, f64)) -> Ordering {
    a_distance
        .partial_cmp(b_distance)
        .unwrap_or(Ordering::Equal)
}

fn validate_coordinates(latitude: f64, longitude: f64) -> GeocodeResult<()> {
    if !latitude.is_finite() || !(-90.0..=90.0).contains(&latitude) {
        return Err(GeocodeError {
            message: "latitude must be between -90 and 90".to_string(),
        });
    }
    if !longitude.is_finite() || !(-180.0..=180.0).contains(&longitude) {
        return Err(GeocodeError {
            message: "longitude must be between -180 and 180".to_string(),
        });
    }
    Ok(())
}

const US_STATES: &[(&str, &str)] = &[
    ("AL", "Alabama"),
    ("AK", "Alaska"),
    ("AZ", "Arizona"),
    ("AR", "Arkansas"),
    ("CA", "California"),
    ("CO", "Colorado"),
    ("CT", "Connecticut"),
    ("DE", "Delaware"),
    ("DC", "District of Columbia"),
    ("FL", "Florida"),
    ("GA", "Georgia"),
    ("HI", "Hawaii"),
    ("ID", "Idaho"),
    ("IL", "Illinois"),
    ("IN", "Indiana"),
    ("IA", "Iowa"),
    ("KS", "Kansas"),
    ("KY", "Kentucky"),
    ("LA", "Louisiana"),
    ("ME", "Maine"),
    ("MD", "Maryland"),
    ("MA", "Massachusetts"),
    ("MI", "Michigan"),
    ("MN", "Minnesota"),
    ("MS", "Mississippi"),
    ("MO", "Missouri"),
    ("MT", "Montana"),
    ("NE", "Nebraska"),
    ("NV", "Nevada"),
    ("NH", "New Hampshire"),
    ("NJ", "New Jersey"),
    ("NM", "New Mexico"),
    ("NY", "New York"),
    ("NC", "North Carolina"),
    ("ND", "North Dakota"),
    ("OH", "Ohio"),
    ("OK", "Oklahoma"),
    ("OR", "Oregon"),
    ("PA", "Pennsylvania"),
    ("RI", "Rhode Island"),
    ("SC", "South Carolina"),
    ("SD", "South Dakota"),
    ("TN", "Tennessee"),
    ("TX", "Texas"),
    ("UT", "Utah"),
    ("VT", "Vermont"),
    ("VA", "Virginia"),
    ("WA", "Washington"),
    ("WV", "West Virginia"),
    ("WI", "Wisconsin"),
    ("WY", "Wyoming"),
];

fn distance_km(lat1: f64, lon1: f64, lat2: f64, lon2: f64) -> f64 {
    let d_lat = (lat2 - lat1).to_radians();
    let d_lon = (lon2 - lon1).to_radians();
    let lat1 = lat1.to_radians();
    let lat2 = lat2.to_radians();
    let a = (d_lat / 2.0).sin().powi(2) + lat1.cos() * lat2.cos() * (d_lon / 2.0).sin().powi(2);
    let c = 2.0 * a.sqrt().asin();
    EARTH_RADIUS_KM * c
}

impl GeocodeCache {
    fn get(&mut self, key: &str) -> Option<Vec<LocationCandidate>> {
        let cached = self.entries.get(key).cloned();
        if cached.is_some() {
            self.touch(key);
        }
        cached
    }

    fn insert(&mut self, key: String, value: Vec<LocationCandidate>) {
        self.entries.insert(key.clone(), value);
        self.touch(&key);
        while self.order.len() > CACHE_SIZE {
            if let Some(oldest) = self.order.pop_front() {
                self.entries.remove(&oldest);
            }
        }
    }

    fn touch(&mut self, key: &str) {
        self.order.retain(|existing| existing != key);
        self.order.push_back(key.to_string());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn geocodes_bundled_city_records() {
        let state = GeocodeState::default();
        let results = geocode_with_cache(
            &state,
            GeocodeRequest {
                query: "lon".to_string(),
                limit: Some(8),
            },
        )
        .unwrap();

        assert_eq!(results[0].label, "London, GB");
        assert_eq!(results[0].latitude, 51.5074);
        assert_eq!(results[0].longitude, -0.1278);
        assert_eq!(results[0].timezone, "Europe/London");
        assert_eq!(results[0].provider, PROVIDER_ID);
    }

    #[test]
    fn geocode_uses_normalized_bounded_cache_keys() {
        let state = GeocodeState::default();
        let first = geocode_with_cache(
            &state,
            GeocodeRequest {
                query: " London ".to_string(),
                limit: Some(999),
            },
        )
        .unwrap();
        let second = geocode_with_cache(
            &state,
            GeocodeRequest {
                query: "london".to_string(),
                limit: Some(50),
            },
        )
        .unwrap();

        assert_eq!(first, second);
        let cache = state.cache.lock().unwrap();
        assert_eq!(cache.entries.len(), 1);
        assert!(cache.entries.contains_key("london|50"));
    }

    #[test]
    fn geocode_ignores_short_queries() {
        let state = GeocodeState::default();

        assert!(geocode_with_cache(
            &state,
            GeocodeRequest {
                query: "l".to_string(),
                limit: None,
            },
        )
        .unwrap()
        .is_empty());
    }

    #[test]
    fn geocodes_us_city_state_zip_offline() {
        let state = GeocodeState::default();
        let results = geocode_with_cache(
            &state,
            GeocodeRequest {
                query: "Alexandria, VA 22301".to_string(),
                limit: Some(8),
            },
        )
        .unwrap();

        assert_eq!(results[0].label, "Alexandria, VA 22301");
        assert_eq!(results[0].latitude, 38.82);
        assert_eq!(results[0].longitude, -77.0589);
        assert_eq!(results[0].timezone, "America/New_York");
        assert_eq!(results[0].provider, US_POSTAL_PROVIDER_ID);
    }

    #[test]
    fn geocodes_us_city_state_without_zip_offline() {
        let state = GeocodeState::default();
        let results = geocode_with_cache(
            &state,
            GeocodeRequest {
                query: "Alexandria VA".to_string(),
                limit: Some(3),
            },
        )
        .unwrap();

        assert_eq!(results[0].label, "Alexandria, VA");
        assert_eq!(results[0].timezone, "America/New_York");
    }

    #[test]
    fn parses_us_location_queries() {
        assert_eq!(
            parse_us_query("Alexandria, VA 22301"),
            ParsedUsQuery {
                city_query: "alexandria".to_string(),
                state: Some("VA".to_string()),
                zip: Some("22301".to_string()),
            }
        );
        assert_eq!(
            parse_us_query("Alexandria Virginia"),
            ParsedUsQuery {
                city_query: "alexandria".to_string(),
                state: Some("VA".to_string()),
                zip: None,
            }
        );
    }

    #[test]
    fn reverse_geocodes_nearby_bundled_city() {
        let label = reverse_geocode_local_city(ReverseGeocodeRequest {
            latitude: 51.5,
            longitude: -0.12,
            max_distance_km: Some(20.0),
        })
        .unwrap()
        .unwrap();

        assert_eq!(label.label, "London, GB");
        assert!(label.distance_km < 2.0);
    }

    #[test]
    fn reverse_geocode_rejects_invalid_coordinates() {
        let error = reverse_geocode_local_city(ReverseGeocodeRequest {
            latitude: 91.0,
            longitude: 0.0,
            max_distance_km: None,
        })
        .unwrap_err();

        assert!(error.message.contains("latitude"));
    }

    #[test]
    fn reverse_geocode_returns_none_beyond_threshold() {
        let result = reverse_geocode_local_city(ReverseGeocodeRequest {
            latitude: 0.0,
            longitude: 0.0,
            max_distance_km: Some(1.0),
        })
        .unwrap();

        assert!(result.is_none());
    }
}
