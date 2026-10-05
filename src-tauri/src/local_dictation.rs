//! In-memory platform dictation. Never permit Speech.framework's network route.
#![forbid(unsafe_code)]
use std::sync::atomic::{AtomicBool, Ordering};

pub fn transcribe(wav: &[u8], cancelled: &AtomicBool) -> Result<String, String> {
    if cancelled.load(Ordering::Acquire) {
        return Err("Listening cancelled.".into());
    }
    #[cfg(target_os = "macos")]
    {
        apple(wav, cancelled)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = wav;
        Err("Local dictation is not available here.".into())
    }
}

#[cfg(target_os = "macos")]
fn apple(wav: &[u8], cancelled: &AtomicBool) -> Result<String, String> {
    use speech::{
        recognizer::SpeechRecognizer,
        request::{
            AudioBufferRecognitionRequest, CallbackQueue, RecognitionRequestOptions, TaskHint,
        },
        task::RecognitionTaskEvent,
    };
    use std::{
        sync::mpsc,
        time::{Duration, Instant},
    };
    let recognizer = SpeechRecognizer::new().with_callback_queue(CallbackQueue::background());
    if !recognizer.is_available()
        || !recognizer
            .supports_on_device_recognition()
            .map_err(|e| e.to_string())?
    {
        return Err("Local dictation is not available for this language.".into());
    }
    let mut authorization = SpeechRecognizer::authorization_status();
    if authorization == speech::error::AuthorizationStatus::NotDetermined {
        // A permission dialog must not trap the owned voice worker for the
        // framework's synchronous 30-second wait. Dropping this future is safe.
        authorization = tauri::async_runtime::block_on(async {
            let authorization = speech::async_api::AsyncSpeechRecognizer::request_authorization();
            tokio::pin!(authorization);
            let deadline = Instant::now() + Duration::from_secs(6);
            loop {
                tokio::select! {
                    result = &mut authorization => return result.map_err(|e| e.to_string()),
                    _ = tokio::time::sleep(Duration::from_millis(30)) => {
                        if cancelled.load(Ordering::Acquire) || Instant::now() >= deadline {
                            return Err("Local dictation permission is not ready.".into());
                        }
                    }
                }
            }
        })?;
    }
    if !authorization.is_authorized() {
        return Err("Local dictation was not permitted.".into());
    }
    if cancelled.load(Ordering::Acquire) {
        return Err("Listening cancelled.".into());
    }
    let samples = pcm_samples(wav)?;
    let request = AudioBufferRecognitionRequest::new().with_options(
        RecognitionRequestOptions::new()
            .with_requires_on_device_recognition(true)
            .with_should_report_partial_results(false)
            .with_task_hint(TaskHint::Dictation),
    );
    let (tx, rx) = mpsc::channel();
    let task = recognizer
        .start_audio_buffer_task(&request, move |event| match event {
            RecognitionTaskEvent::DidFinishRecognition(result) => {
                let _ = tx.send(Ok(result.transcript().to_owned()));
            }
            RecognitionTaskEvent::DidFinishSuccessfully(false) => {
                let _ = tx.send(Err("Local dictation could not finish.".to_owned()));
            }
            _ => {}
        })
        .map_err(|e| e.to_string())?;
    // The task owns its native copy. No audio URL or temporary recording exists.
    let result = (|| {
        for chunk in samples.chunks(16000) {
            if cancelled.load(Ordering::Acquire) {
                return Err("Listening cancelled.".into());
            }
            task.append_interleaved_i16(16000., 1, chunk)
                .map_err(|e| e.to_string())?;
        }
        task.end_audio();
        let deadline = Instant::now() + Duration::from_secs(6);
        loop {
            if cancelled.load(Ordering::Acquire) {
                return Err("Listening cancelled.".into());
            }
            match rx.recv_timeout(Duration::from_millis(30)) {
                Ok(result) => return result.and_then(checked_text),
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    return Err("Local dictation stopped.".into())
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {}
            }
            if task.error().is_some() {
                return Err("Local dictation could not finish.".into());
            }
            if Instant::now() >= deadline {
                return Err("Local dictation took too long.".into());
            }
        }
    })();
    task.cancel();
    result
}

#[cfg(any(target_os = "macos", test))]
fn checked_text(text: String) -> Result<String, String> {
    let text = text.trim();
    if text.is_empty() || text.len() > 8000 {
        Err("No usable words were heard.".into())
    } else {
        Ok(text.into())
    }
}

#[cfg(any(target_os = "macos", test))]
fn pcm_samples(wav: &[u8]) -> Result<Vec<i16>, String> {
    // Only the owned recorder's PCM16/16 kHz/mono WAV is admitted here.
    if wav.len() < 44
        || &wav[0..4] != b"RIFF"
        || &wav[8..12] != b"WAVE"
        || &wav[12..16] != b"fmt "
        || wav[16..20] != 16_u32.to_le_bytes()
        || wav[20..24] != [1, 0, 1, 0]
        || wav[24..28] != 16000_u32.to_le_bytes()
        || wav[34..36] != [16, 0]
        || &wav[36..40] != b"data"
        || wav.len() > 44 + 16000 * 2 * 120
        || !(wav.len() - 44).is_multiple_of(2)
        || u32::from_le_bytes(wav[40..44].try_into().map_err(|_| "Invalid recording.")?) as usize
            != wav.len() - 44
    {
        return Err("The recording could not be read.".into());
    }
    Ok(wav[44..]
        .chunks_exact(2)
        .map(|b| i16::from_le_bytes([b[0], b[1]]))
        .collect())
}

#[cfg(test)]
mod tests {
    #[cfg(target_os = "macos")]
    #[test]
    #[ignore = "Requires installed on-device dictation, prior Speech permission and a synthetic WAV."]
    fn platform_dictation_recognizes_a_local_fixture() -> Result<(), String> {
        let authorization = speech::recognizer::SpeechRecognizer::authorization_status();
        // A headless probe must never request a system permission without the
        // app bundle's purpose string. Unavailable permission is a failed gate.
        if !authorization.is_authorized() {
            return Err(format!(
                "Platform dictation is not qualified: {authorization:?}"
            ));
        }
        let path =
            std::env::var_os("HORARY_VOICE_TEST_WAV").ok_or("A synthetic WAV is required.")?;
        let wav = std::fs::read(path).map_err(|e| e.to_string())?;
        let started = std::time::Instant::now();
        let text = super::transcribe(&wav, &std::sync::atomic::AtomicBool::new(false))?;
        eprintln!(
            "PLATFORM DICTATION: elapsed_ms={} text={text}",
            started.elapsed().as_millis()
        );
        if !text.to_lowercase().contains("married") {
            return Err("The synthetic marriage question was not recognized.".into());
        }
        Ok(())
    }

    #[test]
    fn reject_foreign_audio_and_empty_or_unbounded_transcripts() {
        assert!(super::pcm_samples(&[0; 44]).is_err());
        assert!(super::pcm_samples(&[]).is_err());
        assert!(super::checked_text(" ".into()).is_err());
        assert!(super::checked_text("a".repeat(8001)).is_err());
        assert_eq!(
            super::checked_text(" Where is my ring? ".into()).unwrap(),
            "Where is my ring?"
        );
    }
}
