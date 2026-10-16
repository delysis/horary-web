//! Foreground-only, on-device wake listening. Ambient hypotheses never enter
//! the reading, a model prompt, a file, or a log. Only a completed addressed
//! utterance is published as a one-use voice receipt.
#![forbid(unsafe_code)]
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};
use std::thread::JoinHandle;
use tauri::Emitter;
#[cfg(target_os = "macos")]
use tauri::Manager;

#[cfg(any(target_os = "macos", test))]
pub(crate) const SILENCE_MS: u64 = 1400;
#[cfg(any(target_os = "macos", test))]
pub(crate) const FOLLOW_UP_MS: u64 = 20_000;
#[cfg(any(target_os = "macos", test))]
pub(crate) const RENEW_MS: u64 = 45_000;

#[derive(Clone, serde::Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
enum Update {
    #[cfg(target_os = "macos")]
    Waiting {
        generation: u64,
    },
    #[cfg(target_os = "macos")]
    Listening {
        generation: u64,
    },
    #[cfg(target_os = "macos")]
    Heard {
        generation: u64,
        id: u64,
    },
    Unavailable {
        generation: u64,
    },
}
struct Worker {
    generation: u64,
    cancelled: Arc<AtomicBool>,
    finish: Arc<AtomicBool>,
    thread: JoinHandle<()>,
}
#[derive(Default)]
struct Lease {
    generation: u64,
    worker: Option<Worker>,
}
#[derive(Default)]
pub(crate) struct WakeListener(Mutex<Lease>);
impl WakeListener {
    pub(crate) fn configure(
        &self,
        app: tauri::AppHandle,
        generation: u64,
        enabled: bool,
        follow_up: bool,
    ) -> Result<(), String> {
        let mut lease = self.0.lock().map_err(|_| "Listening is unavailable.")?;
        if generation < lease.generation {
            return Ok(());
        }
        if enabled
            && lease
                .worker
                .as_ref()
                .is_some_and(|worker| worker.generation == generation)
        {
            return Ok(());
        }
        lease.generation = generation;
        stop_worker(&mut lease.worker);
        if !enabled {
            return Ok(());
        }
        let cancelled = Arc::new(AtomicBool::new(false));
        let finish = Arc::new(AtomicBool::new(false));
        let cancel_task = Arc::clone(&cancelled);
        let finish_task = Arc::clone(&finish);
        let thread = std::thread::Builder::new()
            .name("horary-wake-listener".into())
            .spawn(move || {
                if let Err(error) = listen(&app, generation, follow_up, &cancel_task, &finish_task)
                {
                    if !cancel_task.load(Ordering::Acquire) {
                        log::warn!("voice: wake listener stopped: {error}");
                        let _ = app.emit("horary-voice", Update::Unavailable { generation });
                    }
                }
            })
            .map_err(|e| e.to_string())?;
        log::info!(
            "voice: armed local wake listener generation={generation} follow_up={follow_up}"
        );
        lease.worker = Some(Worker {
            generation,
            cancelled,
            finish,
            thread,
        });
        Ok(())
    }
    pub(crate) fn stop(&self) {
        if let Ok(mut lease) = self.0.lock() {
            stop_worker(&mut lease.worker);
        }
    }
    pub(crate) fn finish(&self, generation: u64) {
        if let Ok(lease) = self.0.lock() {
            if let Some(worker) = &lease.worker {
                if worker.generation == generation {
                    worker.finish.store(true, Ordering::Release);
                }
            }
        }
    }
}
fn stop_worker(slot: &mut Option<Worker>) {
    if let Some(worker) = slot.take() {
        worker.cancelled.store(true, Ordering::Release);
        let _ = worker.thread.join();
    }
}

/// Match a whole wake word and preserve the utterance after it byte-for-byte.
/// A mention before the address is discarded rather than sent to the agent.
#[cfg(any(target_os = "macos", test))]
fn addressed_words(text: &str) -> Option<&str> {
    let mut start = None;
    for (offset, ch) in text
        .char_indices()
        .chain(std::iter::once((text.len(), ' ')))
    {
        if ch.is_alphanumeric() {
            start.get_or_insert(offset);
        } else if let Some(begin) = start.take() {
            if text[begin..offset].eq_ignore_ascii_case("oracle") {
                return Some(
                    text[offset..]
                        .trim_start_matches(|c: char| c.is_whitespace() || ",.:;!?—-".contains(c)),
                );
            }
        }
    }
    None
}

#[cfg(any(target_os = "macos", test))]
fn response_words(text: &str, follow_up: bool) -> Option<&str> {
    if !follow_up {
        return addressed_words(text);
    }
    let mut tokens = text
        .split(|c: char| !c.is_alphanumeric())
        .filter(|token| !token.is_empty());
    let first = tokens.next().unwrap_or("");
    if first.eq_ignore_ascii_case("oracle")
        || (first.eq_ignore_ascii_case("hey")
            && tokens
                .next()
                .is_some_and(|token| token.eq_ignore_ascii_case("oracle")))
    {
        addressed_words(text)
    } else {
        Some(text)
    }
}

#[cfg(any(target_os = "macos", test))]
fn end_of_turn(
    awake: bool,
    has_words: bool,
    quiet_ms: u64,
    unchanged_ms: u64,
    forced: bool,
) -> bool {
    awake && has_words && (forced || (quiet_ms >= SILENCE_MS && unchanged_ms >= SILENCE_MS))
}

#[cfg(target_os = "macos")]
fn listen(
    app: &tauri::AppHandle,
    generation: u64,
    follow_up: bool,
    cancelled: &AtomicBool,
    finish: &AtomicBool,
) -> Result<(), String> {
    use crate::microphone_capture::NativeMicrophoneCapture;
    use speech::{
        request::{AudioBufferRecognitionRequest, RecognitionRequestOptions, TaskHint},
        task::{AudioBufferRecognitionTask, RecognitionTaskEvent},
    };
    use std::time::{Duration, Instant};
    let recognizer = crate::local_dictation::authorized_recognizer(cancelled)?;
    let microphone = NativeMicrophoneCapture::new();
    struct OwnedMicrophone(NativeMicrophoneCapture);
    impl Drop for OwnedMicrophone {
        fn drop(&mut self) {
            let _ = self.0.shutdown();
        }
    }
    let microphone = OwnedMicrophone(microphone);
    struct Recognition(AudioBufferRecognitionTask);
    impl Drop for Recognition {
        fn drop(&mut self) {
            self.0.cancel();
        }
    }
    struct Recording<'a>(&'a NativeMicrophoneCapture);
    impl Drop for Recording<'_> {
        fn drop(&mut self) {
            let _ = self.0.cancel("horary-wake".into());
        }
    }
    let mut follow_up = follow_up;
    loop {
        if cancelled.load(Ordering::Acquire) {
            return Ok(());
        }
        let options = RecognitionRequestOptions::new()
            .with_requires_on_device_recognition(true)
            .with_should_report_partial_results(true)
            .with_task_hint(TaskHint::Dictation)
            .with_contextual_strings(["Oracle"]);
        let request = AudioBufferRecognitionRequest::new().with_options(options);
        let (tx, updates) = crossbeam_channel::bounded(32);
        let overflow = Arc::new(AtomicBool::new(false));
        let full = Arc::clone(&overflow);
        let recognition = Recognition(
            recognizer
                .start_audio_buffer_task(&request, move |event| {
                    let update = match event {
                        RecognitionTaskEvent::DidHypothesizeTranscription(text) => {
                            Some(Ok((text.formatted_string, false)))
                        }
                        RecognitionTaskEvent::DidFinishRecognition(result) => {
                            Some(Ok((result.transcript().to_owned(), true)))
                        }
                        RecognitionTaskEvent::DidFinishSuccessfully(false) => {
                            Some(Err("Local listening could not finish.".to_owned()))
                        }
                        _ => None,
                    };
                    if let Some(update) = update {
                        if tx.try_send(update).is_err() {
                            full.store(true, Ordering::Release);
                        }
                    }
                })
                .map_err(|e| e.to_string())?,
        );
        let audio = microphone
            .0
            .stream("horary-wake".into())
            .map_err(|e| e.to_string())?;
        let recording = Recording(&microphone.0);
        let began = Instant::now();
        let mut awake = follow_up;
        let mut words = String::new();
        let mut last_change = began;
        let mut last_sound = began;
        let mut ending: Option<Instant> = None;
        let _ = app.emit("horary-voice", Update::Waiting { generation });
        'utterance: loop {
            if cancelled.load(Ordering::Acquire) {
                return Ok(());
            }
            if overflow.load(Ordering::Acquire) {
                return Err("Local listening fell behind.".into());
            }
            while let Ok(update) = updates.try_recv() {
                let (text, final_result) = update?;
                if text.len() > 8000 {
                    return Err("The spoken question is too long.".into());
                }
                let addressed = response_words(&text, follow_up);
                let candidate = addressed.unwrap_or("");
                if !awake && addressed.is_some() {
                    awake = true;
                    let _ = app.emit("horary-voice", Update::Listening { generation });
                }
                if awake && words != candidate.trim() {
                    words = candidate.trim().into();
                    last_change = Instant::now();
                    if !words.is_empty() {
                        let _ = app.emit("horary-voice", Update::Listening { generation });
                    }
                }
                if final_result {
                    if awake && !words.is_empty() {
                        // Release microphone and recognition before publishing
                        // the receipt: the reading and reply cannot hear echoes.
                        drop(recording);
                        drop(recognition);
                        if cancelled.load(Ordering::Acquire) {
                            return Ok(());
                        }
                        let id = app
                            .state::<crate::voice::VoiceState>()
                            .publish_native(words)?;
                        log::info!("voice: completed on-device addressed turn");
                        let _ = app.emit("horary-voice", Update::Heard { generation, id });
                        return Ok(());
                    }
                    break 'utterance;
                }
            }
            match audio.recv_timeout(Duration::from_millis(30)) {
                Ok(samples) if ending.is_none() => {
                    if !samples.is_empty()
                        && samples.iter().map(|s| f64::from(*s).powi(2)).sum::<f64>()
                            / samples.len() as f64
                            > 0.000_144
                    {
                        last_sound = Instant::now();
                    }
                    recognition
                        .0
                        .append_interleaved_f32(16000., 1, &samples)
                        .map_err(|e| e.to_string())?;
                }
                Ok(_) | Err(crossbeam_channel::RecvTimeoutError::Timeout) => {}
                Err(crossbeam_channel::RecvTimeoutError::Disconnected) => {
                    return Err("The microphone feed stopped.".into())
                }
            }
            let now = Instant::now();
            if recognition.0.error().is_some() {
                return Err("Local listening could not finish.".into());
            }
            if let Some(deadline) = ending {
                if now >= deadline {
                    return Err("Local listening did not deliver final words.".into());
                }
            } else if end_of_turn(
                awake,
                !words.is_empty(),
                u64::try_from(now.duration_since(last_sound).as_millis()).unwrap_or(u64::MAX),
                u64::try_from(now.duration_since(last_change).as_millis()).unwrap_or(u64::MAX),
                finish.swap(false, Ordering::AcqRel),
            ) {
                recognition.0.end_audio();
                ending = Some(now + Duration::from_secs(3));
            } else if (follow_up
                && words.is_empty()
                && now.duration_since(began) >= Duration::from_millis(FOLLOW_UP_MS))
                || now.duration_since(began) >= Duration::from_millis(RENEW_MS)
            {
                if awake && !words.is_empty() {
                    recognition.0.end_audio();
                    ending = Some(now + Duration::from_secs(3));
                } else {
                    break;
                }
            }
        }
        // Speech tasks are bounded and renewed. Their ambient hypotheses are
        // dropped, never accumulated into a conversation or retained recording.
        follow_up = false;
    }
}
#[cfg(not(target_os = "macos"))]
fn listen(
    _app: &tauri::AppHandle,
    _generation: u64,
    _follow_up: bool,
    _cancelled: &AtomicBool,
    _finish: &AtomicBool,
) -> Result<(), String> {
    Err("On-device wake listening is not available on this platform.".into())
}

#[cfg(test)]
mod tests {
    #[test]
    fn wake_word_is_whole_and_only_addressed_words_are_kept() {
        assert_eq!(
            super::addressed_words("Oracle, where is my sister’s ring?"),
            Some("where is my sister’s ring?")
        );
        assert_eq!(
            super::addressed_words("ambient chatter. Hey ORACLE: will Bob sell 12 books?"),
            Some("will Bob sell 12 books?")
        );
        assert_eq!(super::addressed_words("Oracle"), Some(""));
        assert_eq!(super::addressed_words("Oracles are interesting"), None);
        assert_eq!(
            super::addressed_words("Not addressed to the document"),
            None
        );
    }
    #[test]
    fn expected_reply_preserves_negation_numbers_and_an_incidental_wake_word() {
        assert_eq!(
            super::response_words("No, Bob does not own the 12 books.", true),
            Some("No, Bob does not own the 12 books.")
        );
        assert_eq!(
            super::response_words("The Oracle book belongs to my sister.", true),
            Some("The Oracle book belongs to my sister.")
        );
        assert_eq!(
            super::response_words("Oracle, no, tomorrow at 3.", true),
            Some("no, tomorrow at 3.")
        );
        assert_eq!(
            super::response_words("Hey Oracle, tomorrow.", true),
            Some("tomorrow.")
        );
        assert_eq!(super::response_words("ambient speech", false), None);
    }
    #[test]
    fn endpoint_requires_addressed_words_and_a_real_pause_in_sound_and_transcription() {
        assert!(!super::end_of_turn(false, true, 9000, 9000, false));
        assert!(!super::end_of_turn(true, false, 9000, 9000, true));
        assert!(!super::end_of_turn(true, true, 1399, 4000, false));
        assert!(!super::end_of_turn(true, true, 4000, 1399, false));
        assert!(super::end_of_turn(true, true, 1400, 1400, false));
        assert!(super::end_of_turn(true, true, 0, 0, true));
    }
}
