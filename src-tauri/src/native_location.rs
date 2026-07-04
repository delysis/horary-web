use serde::{Deserialize, Serialize};
use std::sync::{
    atomic::{AtomicBool, AtomicU64, Ordering},
    Arc, Mutex,
};
use tauri::Emitter;

const DEFAULT_TIMEOUT_MS: u64 = 6_000;
const MIN_TIMEOUT_MS: u64 = 1_000;
const MAX_TIMEOUT_MS: u64 = 15_000;

pub const CURRENT_LOCATION_DETECTED_EVENT: &str = "current-location-detected";
pub const CURRENT_LOCATION_ERROR_EVENT: &str = "current-location-error";
pub const CURRENT_LOCATION_CANCELLED_EVENT: &str = "current-location-cancelled";

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CurrentLocationRequest {
    #[serde(default)]
    pub timeout_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CurrentLocation {
    pub latitude: f64,
    pub longitude: f64,
    pub accuracy_meters: Option<f64>,
    pub provider: String,
    pub authorization_status: i32,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct CurrentLocationError {
    pub message: String,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CurrentLocationDetectionStarted {
    pub detection_id: String,
    pub timeout_ms: u64,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CurrentLocationDetected {
    pub detection_id: String,
    pub location: CurrentLocation,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CurrentLocationMessage {
    pub detection_id: String,
    pub message: String,
}

#[derive(Debug, Default)]
pub struct CurrentLocationDetectionState {
    registry: Arc<LocationDetectionRegistry>,
}

#[derive(Debug, Default)]
struct LocationDetectionRegistry {
    active: Mutex<Option<ActiveLocationDetection>>,
    next_id: AtomicU64,
}

#[derive(Debug)]
struct ActiveLocationDetection {
    detection_id: String,
    cancel: Arc<AtomicBool>,
}

impl CurrentLocationDetectionState {
    fn registry(&self) -> Arc<LocationDetectionRegistry> {
        Arc::clone(&self.registry)
    }
}

impl LocationDetectionRegistry {
    fn start_detection(
        &self,
        timeout_ms: u64,
    ) -> Result<(CurrentLocationDetectionStarted, Arc<AtomicBool>), CurrentLocationError> {
        let mut active = self.active.lock().map_err(|_| CurrentLocationError {
            message: "Location detection state lock poisoned.".to_string(),
        })?;
        if active.is_some() {
            return Err(CurrentLocationError {
                message: "Location detection is already running.".to_string(),
            });
        }

        let serial = self.next_id.fetch_add(1, Ordering::Relaxed) + 1;
        let detection_id = format!("current-location-{serial}");
        let cancel = Arc::new(AtomicBool::new(false));
        *active = Some(ActiveLocationDetection {
            detection_id: detection_id.clone(),
            cancel: Arc::clone(&cancel),
        });

        Ok((
            CurrentLocationDetectionStarted {
                detection_id,
                timeout_ms,
            },
            cancel,
        ))
    }

    fn cancel_detection(&self, detection_id: &str) -> Result<bool, CurrentLocationError> {
        let mut active = self.active.lock().map_err(|_| CurrentLocationError {
            message: "Location detection state lock poisoned.".to_string(),
        })?;
        let Some(current) = active.as_ref() else {
            return Ok(false);
        };
        if current.detection_id != detection_id {
            return Ok(false);
        }
        current.cancel.store(true, Ordering::Relaxed);
        *active = None;
        Ok(true)
    }

    fn finish_detection(&self, detection_id: &str) -> bool {
        let Ok(mut active) = self.active.lock() else {
            return false;
        };
        let Some(current) = active.as_ref() else {
            return false;
        };
        if current.detection_id != detection_id || current.cancel.load(Ordering::Relaxed) {
            return false;
        }
        *active = None;
        true
    }
}

pub async fn get_current_location_native(
    req: CurrentLocationRequest,
) -> Result<CurrentLocation, CurrentLocationError> {
    let timeout_ms = normalize_timeout_ms(req.timeout_ms);
    tauri::async_runtime::spawn_blocking(move || request_current_location(timeout_ms))
        .await
        .map_err(|error| CurrentLocationError {
            message: format!("Location worker failed: {error}"),
        })?
}

pub fn start_current_location_detection_from_state(
    app: tauri::AppHandle,
    state: &CurrentLocationDetectionState,
    req: CurrentLocationRequest,
) -> Result<CurrentLocationDetectionStarted, CurrentLocationError> {
    let timeout_ms = normalize_timeout_ms(req.timeout_ms);
    let registry = state.registry();
    let (started, cancel) = registry.start_detection(timeout_ms)?;
    let detection_id = started.detection_id.clone();

    log::info!("starting current location detection: {detection_id}");
    tauri::async_runtime::spawn(async move {
        let result = get_current_location_native(CurrentLocationRequest {
            timeout_ms: Some(timeout_ms),
        })
        .await;

        if cancel.load(Ordering::Relaxed) || !registry.finish_detection(&detection_id) {
            log::info!("discarding cancelled/stale current location detection: {detection_id}");
            return;
        }

        match result {
            Ok(location) => {
                log::info!("current location detection completed: {detection_id}");
                let _ = app.emit(
                    CURRENT_LOCATION_DETECTED_EVENT,
                    CurrentLocationDetected {
                        detection_id,
                        location,
                    },
                );
            }
            Err(error) => {
                log::info!(
                    "current location detection failed: {detection_id}: {}",
                    error.message
                );
                let _ = app.emit(
                    CURRENT_LOCATION_ERROR_EVENT,
                    CurrentLocationMessage {
                        detection_id,
                        message: error.message,
                    },
                );
            }
        }
    });

    Ok(started)
}

pub fn cancel_current_location_detection_from_state(
    app: tauri::AppHandle,
    state: &CurrentLocationDetectionState,
    detection_id: String,
) -> Result<bool, CurrentLocationError> {
    let cancelled = state.registry.cancel_detection(&detection_id)?;
    if cancelled {
        log::info!("current location detection cancelled: {detection_id}");
        let _ = app.emit(
            CURRENT_LOCATION_CANCELLED_EVENT,
            CurrentLocationMessage {
                detection_id,
                message: "Location detection cancelled.".to_string(),
            },
        );
    }
    Ok(cancelled)
}

fn normalize_timeout_ms(timeout_ms: Option<u64>) -> u64 {
    timeout_ms
        .unwrap_or(DEFAULT_TIMEOUT_MS)
        .clamp(MIN_TIMEOUT_MS, MAX_TIMEOUT_MS)
}

#[cfg(target_os = "macos")]
fn request_current_location(timeout_ms: u64) -> Result<CurrentLocation, CurrentLocationError> {
    use std::ffi::CStr;
    use std::os::raw::{c_char, c_double, c_int};

    #[repr(C)]
    #[derive(Debug, Copy, Clone)]
    struct HoraryNativeLocationResult {
        latitude: c_double,
        longitude: c_double,
        horizontal_accuracy: c_double,
        authorization_status: c_int,
    }

    extern "C" {
        fn horary_request_current_location(
            timeout_seconds: c_double,
            out_result: *mut HoraryNativeLocationResult,
            error_buffer: *mut c_char,
            error_buffer_len: usize,
        ) -> c_int;
    }

    let mut native_result = HoraryNativeLocationResult {
        latitude: 0.0,
        longitude: 0.0,
        horizontal_accuracy: 0.0,
        authorization_status: 0,
    };
    let mut error_buffer = vec![0 as c_char; 512];
    let code = unsafe {
        horary_request_current_location(
            timeout_ms as c_double / 1000.0,
            &mut native_result,
            error_buffer.as_mut_ptr(),
            error_buffer.len(),
        )
    };

    if code != 0 {
        let native_message = unsafe { CStr::from_ptr(error_buffer.as_ptr()) }
            .to_string_lossy()
            .trim()
            .to_string();
        let message = if native_message.is_empty() {
            "Could not detect location with macOS Location Services.".to_string()
        } else {
            native_message
        };
        return Err(CurrentLocationError { message });
    }

    validate_coordinate(native_result.latitude, -90.0, 90.0, "latitude")?;
    validate_coordinate(native_result.longitude, -180.0, 180.0, "longitude")?;
    let accuracy_meters = if native_result.horizontal_accuracy.is_finite()
        && native_result.horizontal_accuracy >= 0.0
    {
        Some(native_result.horizontal_accuracy)
    } else {
        None
    };

    Ok(CurrentLocation {
        latitude: native_result.latitude,
        longitude: native_result.longitude,
        accuracy_meters,
        provider: "macos-core-location".to_string(),
        authorization_status: native_result.authorization_status,
    })
}

#[cfg(not(target_os = "macos"))]
fn request_current_location(_timeout_ms: u64) -> Result<CurrentLocation, CurrentLocationError> {
    Err(CurrentLocationError {
        message: "Native location detection is currently available on macOS only.".to_string(),
    })
}

fn validate_coordinate(
    value: f64,
    min: f64,
    max: f64,
    label: &str,
) -> Result<(), CurrentLocationError> {
    if value.is_finite() && (min..=max).contains(&value) {
        Ok(())
    } else {
        Err(CurrentLocationError {
            message: format!("CoreLocation returned invalid {label}: {value}."),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timeout_is_bounded_for_ui_responsiveness() {
        assert_eq!(normalize_timeout_ms(None), DEFAULT_TIMEOUT_MS);
        assert_eq!(normalize_timeout_ms(Some(100)), MIN_TIMEOUT_MS);
        assert_eq!(normalize_timeout_ms(Some(9_000)), 9_000);
        assert_eq!(normalize_timeout_ms(Some(60_000)), MAX_TIMEOUT_MS);
    }

    #[test]
    fn coordinate_validation_rejects_non_finite_and_out_of_range_values() {
        assert!(validate_coordinate(40.0, -90.0, 90.0, "latitude").is_ok());
        assert!(validate_coordinate(f64::NAN, -90.0, 90.0, "latitude").is_err());
        assert!(validate_coordinate(90.1, -90.0, 90.0, "latitude").is_err());
    }

    #[test]
    fn detection_registry_allows_one_active_request() {
        let registry = LocationDetectionRegistry::default();
        let (started, _cancel) = registry.start_detection(6_000).unwrap();

        assert_eq!(started.detection_id, "current-location-1");
        assert!(registry.start_detection(6_000).is_err());
        assert!(registry.finish_detection(&started.detection_id));
        assert!(registry.start_detection(6_000).is_ok());
    }

    #[test]
    fn detection_registry_cancel_prevents_completion() {
        let registry = LocationDetectionRegistry::default();
        let (started, cancel) = registry.start_detection(6_000).unwrap();

        assert!(registry.cancel_detection(&started.detection_id).unwrap());
        assert!(cancel.load(Ordering::Relaxed));
        assert!(!registry.finish_detection(&started.detection_id));
        assert!(!registry.cancel_detection(&started.detection_id).unwrap());
    }
}
