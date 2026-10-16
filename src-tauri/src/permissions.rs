//! Process-local startup acquisition, never a persisted denial preference.
#![forbid(unsafe_code)]
use serde::Serialize;
use std::sync::OnceLock;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
#[allow(dead_code)] // Platform-specific native enum cases share one IPC contract.
pub enum Permission {
    Granted,
    Denied,
    Restricted,
    Pending,
    Unavailable,
    PlatformManaged,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Permissions {
    microphone: Permission,
    speech: Permission,
    location: Permission,
    voice_available: bool,
}

#[derive(Default)]
pub struct PermissionState(OnceLock<()>);

impl Permissions {
    fn new(microphone: Permission, speech: Permission, location: Permission, device: bool) -> Self {
        Self {
            microphone,
            speech,
            location,
            voice_available: device
                && matches!(
                    microphone,
                    Permission::Granted | Permission::PlatformManaged
                ),
        }
    }
}

#[cfg(target_os = "macos")]
fn microphone_status(status: i32) -> Permission {
    match status {
        3 => Permission::Granted,
        2 => Permission::Denied,
        1 => Permission::Restricted,
        0 => Permission::Pending,
        _ => Permission::Unavailable,
    }
}
#[cfg(target_os = "macos")]
fn location_status(status: i32) -> Permission {
    match status {
        3 | 4 => Permission::Granted,
        2 => Permission::Denied,
        1 => Permission::Restricted,
        0 => Permission::Pending,
        _ => Permission::Unavailable,
    }
}

fn acquire(request: bool) -> Permissions {
    use cpal::traits::HostTrait;
    #[cfg(target_os = "macos")]
    let (microphone, speech, location) = {
        use speech::{
            async_api::AsyncSpeechRecognizer, error::AuthorizationStatus,
            recognizer::SpeechRecognizer,
        };
        // OS dialogs are separate. Sequence consent requests once at launch;
        // no waveform is captured just to acquire permission.
        let microphone =
            microphone_status(crate::native_location::permission_status(true, request));
        let status = if request {
            tauri::async_runtime::block_on(async {
                tokio::time::timeout(
                    std::time::Duration::from_secs(120),
                    AsyncSpeechRecognizer::request_authorization(),
                )
                .await
                .ok()
                .and_then(Result::ok)
            })
            .unwrap_or_else(SpeechRecognizer::authorization_status)
        } else {
            SpeechRecognizer::authorization_status()
        };
        let speech = match status {
            AuthorizationStatus::Authorized => Permission::Granted,
            AuthorizationStatus::Denied => Permission::Denied,
            AuthorizationStatus::Restricted => Permission::Restricted,
            AuthorizationStatus::NotDetermined => Permission::Pending,
            _ => Permission::Unavailable,
        };
        let location = location_status(crate::native_location::permission_status(false, request));
        (microphone, speech, location)
    };
    #[cfg(not(target_os = "macos"))]
    let (microphone, speech, location) = {
        let _ = request;
        (
            Permission::PlatformManaged,
            Permission::Unavailable,
            Permission::Unavailable,
        )
    };
    let report = Permissions::new(
        microphone,
        speech,
        location,
        cpal::default_host().default_input_device().is_some(),
    );
    log::info!(
        "horary permissions microphone={:?} speech={:?} location={:?} voice_available={}",
        report.microphone,
        report.speech,
        report.location,
        report.voice_available
    );
    report
}

#[tauri::command]
pub async fn startup_permissions(app: tauri::AppHandle) -> Result<Permissions, String> {
    use tauri::Manager;
    tauri::async_runtime::spawn_blocking(move || {
        app.state::<PermissionState>().0.get_or_init(|| {
            acquire(true);
        });
        acquire(false)
    })
    .await
    .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn permission_status() -> Result<Permissions, String> {
    tauri::async_runtime::spawn_blocking(|| acquire(false))
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn open_microphone_permissions() -> Result<(), String> {
    #[cfg(target_os = "macos")]
    return tauri::async_runtime::spawn_blocking(|| {
        let status = std::process::Command::new("/usr/bin/open")
            .arg("x-apple.systempreferences:com.apple.preference.security?Privacy_Microphone")
            .status()
            .map_err(|e| e.to_string())?;
        if status.success() {
            Ok(())
        } else {
            Err("Microphone settings could not be opened.".into())
        }
    })
    .await
    .map_err(|e| e.to_string())?;
    #[cfg(not(target_os = "macos"))]
    Err("Microphone permissions are managed by this platform.".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn denied_microphone_needs_text_but_denied_location_or_speech_does_not() {
        assert!(
            !Permissions::new(
                Permission::Denied,
                Permission::Granted,
                Permission::Granted,
                true
            )
            .voice_available
        );
        assert!(
            Permissions::new(
                Permission::Granted,
                Permission::Denied,
                Permission::Denied,
                true
            )
            .voice_available
        );
        assert!(
            !Permissions::new(
                Permission::Granted,
                Permission::Granted,
                Permission::Granted,
                false
            )
            .voice_available
        );
        assert!(
            !Permissions::new(
                Permission::Pending,
                Permission::Granted,
                Permission::Granted,
                true
            )
            .voice_available
        );
    }
    #[test]
    fn launch_state_does_not_carry_a_denial_to_the_next_process() {
        let previous = PermissionState::default();
        previous.0.set(()).unwrap();
        assert!(PermissionState::default().0.get().is_none());
    }
}
