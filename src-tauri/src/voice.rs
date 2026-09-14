//! Explicit local microphone capture and optional macOS installed-voice speech.
#![forbid(unsafe_code)]
use crate::microphone_capture::NativeMicrophoneCapture;
use std::sync::Mutex;
use tauri::Manager;

pub struct VoiceState {
    capture: NativeMicrophoneCapture,
    speech: Mutex<Option<std::process::Child>>,
}
impl Default for VoiceState {
    fn default() -> Self {
        Self {
            capture: NativeMicrophoneCapture::new(),
            speech: Mutex::new(None),
        }
    }
}
impl VoiceState {
    pub fn stop_speaking(&self) -> Result<(), String> {
        if let Some(mut child) = self.speech.lock().map_err(|_| "Voice unavailable")?.take() {
            if child.try_wait().map_err(|e| e.to_string())?.is_none() {
                child.kill().map_err(|e| e.to_string())?;
            }
            child.wait().map_err(|e| e.to_string())?;
        }
        Ok(())
    }
    pub fn shutdown(&self) {
        let _ = self.capture.shutdown();
        let _ = self.stop_speaking();
    }
}
#[tauri::command]
pub async fn voice_start(app: tauri::AppHandle) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<VoiceState>();
        state.stop_speaking()?;
        state
            .capture
            .start("horary-voice".into())
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}
#[tauri::command]
pub async fn voice_finish(app: tauri::AppHandle) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let bytes = app
            .state::<VoiceState>()
            .capture
            .stop("horary-voice".into())
            .map_err(|e| e.to_string())?;
        if !has_sound(&bytes) {
            return Err("No audible words were recorded.".into());
        }
        crate::conversation::transcribe(&app, bytes)
    })
    .await
    .map_err(|e| e.to_string())?
}

// The owned recorder emits exactly this PCM16 mono WAV layout. Reject silence
// before asking a generative model to transcribe it; this is not speech detection.
fn has_sound(wav: &[u8]) -> bool {
    let Some(pcm) = wav.get(44..) else {
        return false;
    };
    if pcm.len() < 3200 || pcm.len() % 2 != 0 {
        return false;
    }
    let energy: f64 = pcm
        .chunks_exact(2)
        .map(|b| f64::from(i16::from_le_bytes([b[0], b[1]])).powi(2))
        .sum();
    energy / (pcm.len() / 2) as f64 > 1024.
}

#[cfg(test)]
mod tests {
    #[test]
    fn silence_and_accidental_taps_do_not_become_transcripts() {
        assert!(!super::has_sound(&vec![0; 32044]));
        assert!(!super::has_sound(&vec![255; 100]));
        let mut wav = vec![0; 44];
        for _ in 0..16000 {
            wav.extend_from_slice(&1000_i16.to_le_bytes());
        }
        assert!(super::has_sound(&wav));
    }
}
#[tauri::command]
pub async fn voice_cancel(app: tauri::AppHandle) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        app.state::<VoiceState>()
            .capture
            .cancel("horary-voice".into())
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}
#[tauri::command]
pub fn voice_stop_speaking(state: tauri::State<'_, VoiceState>) -> Result<(), String> {
    state.stop_speaking()
}
#[tauri::command]
pub fn voice_speak(state: tauri::State<'_, VoiceState>, text: String) -> Result<(), String> {
    if text.len() > 10000 || text.trim().is_empty() {
        return Err("The spoken reply is empty or too long.".into());
    }
    state.stop_speaking()?;
    #[cfg(target_os = "macos")]
    {
        // No shell, downloadable voice, remote service, or model-generated flags.
        let child = std::process::Command::new("/usr/bin/say")
            .arg("--")
            .arg(text)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .map_err(|e| e.to_string())?;
        *state.speech.lock().map_err(|_| "Voice unavailable")? = Some(child);
        Ok(())
    }
    #[cfg(not(target_os="macos"))]
    Err("Spoken replies are not available on this platform yet. The conversation remains available as text.".into())
}
