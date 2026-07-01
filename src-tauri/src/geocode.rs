use serde::{Deserialize, Serialize};
use std::{
    cmp::Ordering,
    collections::{HashMap, VecDeque},
    sync::{Mutex, OnceLock},
};

const CITIES_JSON: &str = include_str!("../../src/data/cities.json");
const PROVIDER_ID: &str = "bundled-cities";
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

    let results = search_city_records(&query, limit)?;
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

    let nearest = city_records()?
        .iter()
        .map(|city| {
            (
                city,
                distance_km(req.latitude, req.longitude, city.lat, city.lng),
            )
        })
        .min_by(|(_, a_distance), (_, b_distance)| {
            a_distance
                .partial_cmp(b_distance)
                .unwrap_or(Ordering::Equal)
        });

    Ok(nearest.and_then(|(city, distance)| {
        if distance > max_distance_km {
            return None;
        }
        let candidate = city_to_candidate(city);
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
    query.trim().to_lowercase()
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
