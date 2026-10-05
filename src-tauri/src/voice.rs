//! Explicit local microphone capture and optional macOS installed-voice speech.
#![forbid(unsafe_code)]
use crate::microphone_capture::NativeMicrophoneCapture;
use serde::Serialize;
use std::sync::{
    atomic::{AtomicBool, AtomicU64, Ordering},
    Mutex,
};
use tauri::Manager;

#[derive(Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum VoiceMode {
    Auto,
    Native,
    Direct,
    GemmaTranscription,
}
impl VoiceMode {
    fn configured() -> Result<Self, String> {
        match std::env::var("HORARY_VOICE_MODE").as_deref() {
            Ok("auto") | Err(std::env::VarError::NotPresent) => Ok(Self::Auto),
            Ok("native") => Ok(Self::Native),
            Ok("direct") => Ok(Self::Direct),
            Ok("gemma-transcription") => Ok(Self::GemmaTranscription),
            _ => Err("The selected voice route is unavailable.".into()),
        }
    }
}

pub enum VoicePayload {
    Text(String),
    Audio(Vec<u8>),
}
pub struct PendingVoice {
    pub payload: VoicePayload,
    pub received_at_ms: f64,
    pub mode: VoiceMode,
    pub preparation_ms: u64,
}
#[derive(Serialize)]
pub struct VoiceReceipt {
    pub id: u64,
    pub text: Option<String>,
}

pub struct VoiceState {
    capture: NativeMicrophoneCapture,
    speech: Mutex<Option<std::process::Child>>,
    pending: Mutex<Option<(u64, PendingVoice)>>,
    cancelled: AtomicBool,
    finishing: AtomicBool,
    next_id: AtomicU64,
}
impl Default for VoiceState {
    fn default() -> Self {
        Self {
            capture: NativeMicrophoneCapture::new(),
            speech: Mutex::new(None),
            pending: Mutex::new(None),
            cancelled: AtomicBool::new(false),
            finishing: AtomicBool::new(false),
            next_id: AtomicU64::new(1),
        }
    }
}
impl VoiceState {
    pub fn discard_pending(&self) {
        self.cancelled.store(true, Ordering::Release);
        if let Ok(mut pending) = self.pending.lock() {
            pending.take();
        }
    }
    pub fn take(&self, id: u64) -> Result<PendingVoice, String> {
        let mut pending = self
            .pending
            .lock()
            .map_err(|_| "Listening is unavailable.")?;
        if self.cancelled.load(Ordering::Acquire)
            || pending.as_ref().is_none_or(|(saved, _)| *saved != id)
        {
            return Err("These spoken words are no longer waiting to be sent.".into());
        }
        pending
            .take()
            .map(|(_, voice)| voice)
            .ok_or_else(|| "No spoken words are waiting.".into())
    }
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
        self.discard_pending();
        let _ = self.capture.shutdown();
        let _ = self.stop_speaking();
    }
}
#[tauri::command]
pub async fn voice_start(app: tauri::AppHandle) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<VoiceState>();
        if state.finishing.load(Ordering::Acquire) {
            return Err("The last words are still being heard.".into());
        }
        state.discard_pending();
        state.cancelled.store(false, Ordering::Release);
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
pub async fn voice_finish(app: tauri::AppHandle) -> Result<VoiceReceipt, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<VoiceState>();
        state
            .finishing
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map_err(|_| "The last words are still being heard.")?;
        struct FinishLease<'a>(&'a AtomicBool);
        impl Drop for FinishLease<'_> {
            fn drop(&mut self) {
                self.0.store(false, Ordering::Release);
            }
        }
        let _lease = FinishLease(&state.finishing);
        let bytes = state
            .capture
            .stop("horary-voice".into())
            .map_err(|e| e.to_string())?;
        if !has_sound(&bytes) {
            return Err("No audible words were recorded.".into());
        }
        let received_at_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|_| "The question's moment could not be noted.")?
            .as_secs_f64()
            * 1000.;
        let started = std::time::Instant::now();
        let mode = VoiceMode::configured()?;
        let (mode, payload) = match mode {
            VoiceMode::Direct => (mode, VoicePayload::Audio(bytes)),
            VoiceMode::GemmaTranscription => (
                mode,
                VoicePayload::Text(crate::conversation::transcribe(&app, bytes)?),
            ),
            VoiceMode::Native | VoiceMode::Auto => {
                match crate::local_dictation::transcribe(&bytes, &state.cancelled) {
                    Ok(text) => (VoiceMode::Native, VoicePayload::Text(text)),
                    Err(error) if mode == VoiceMode::Native => return Err(error),
                    Err(_) => {
                        log::info!("voice: local dictation unavailable; using direct local audio");
                        (VoiceMode::Direct, VoicePayload::Audio(bytes))
                    }
                }
            }
        };
        let preparation_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
        let text = match &payload {
            VoicePayload::Text(text) => Some(text.clone()),
            VoicePayload::Audio(_) => None,
        };
        let id = state.next_id.fetch_add(1, Ordering::Relaxed);
        let mut pending = state
            .pending
            .lock()
            .map_err(|_| "Listening is unavailable.")?;
        if state.cancelled.load(Ordering::Acquire) {
            return Err("Listening cancelled.".into());
        }
        *pending = Some((
            id,
            PendingVoice {
                payload,
                received_at_ms,
                mode,
                preparation_ms,
            },
        ));
        log::info!(
            "voice: route={} preparation_ms={preparation_ms}",
            serde_json::to_string(&mode).unwrap_or_default()
        );
        Ok(VoiceReceipt { id, text })
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
    use super::*;
    #[test]
    fn pending_audio_is_owned_once_and_cancellation_discards_it() {
        let state = VoiceState::default();
        *state.pending.lock().unwrap() = Some((
            1,
            PendingVoice {
                payload: VoicePayload::Audio(vec![1, 2]),
                received_at_ms: 0.,
                mode: VoiceMode::Direct,
                preparation_ms: 0,
            },
        ));
        assert!(state.take(2).is_err());
        assert!(
            matches!(state.take(1).unwrap().payload, VoicePayload::Audio(bytes) if bytes == [1,2])
        );
        assert!(state.take(1).is_err());
        *state.pending.lock().unwrap() = Some((
            3,
            PendingVoice {
                payload: VoicePayload::Text("private words".into()),
                received_at_ms: 0.,
                mode: VoiceMode::Native,
                preparation_ms: 0,
            },
        ));
        state.discard_pending();
        assert!(state.take(3).is_err());
        assert!(state.pending.lock().unwrap().is_none());
    }
    #[test]
    fn silence_and_accidental_taps_do_not_become_transcripts() {
        assert!(!super::has_sound(&vec![0; 32044]));
        assert!(!super::has_sound(&[255; 100]));
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
        let state = app.state::<VoiceState>();
        state.discard_pending();
        state
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
