//! The voice-changer relay: listen to the microphone, transcribe what was said, and
//! speak it back into the voice channel in the chosen voice.
//!
//! The microphone audio itself never reaches the channel — only the synthesised
//! re-reading of it does.

use std::sync::Arc;

use songbird::input::Input;
use songbird::tracks::TrackHandle;
use songbird::{Call, Event, EventContext, EventHandler as VoiceEventHandler, TrackEvent};
use tokio::sync::{oneshot, Mutex};

use crate::audio::capture::{self, CaptureError, CaptureHandle};
use crate::audio::denoise::StreamDenoiser;
use crate::audio::utterance::{
    find_peak_amplitude, DetectorTuning, UtteranceDetector, CHANNEL_COUNT,
};
use crate::openai::OpenAiClient;
use crate::transcripts;

/// Reports the current microphone loudness, 0.0 to 1.0, so the UI's level meter can
/// show that the bot is hearing something.
pub type LevelReporter = Arc<dyn Fn(f32) + Send + Sync>;

/// Reports each thing said, so the history page fills in as it is spoken rather than
/// only when reopened.
pub type TranscriptReporter = Arc<dyn Fn(transcripts::Transcript) + Send + Sync>;

/// Reports whether the bot is in at least one voice channel, so the UI can tell "connected
/// to Discord" from "actually listening to a microphone". Nothing is captured or spoken
/// until a channel is joined, so the two are genuinely different states.
pub type SessionActiveReporter = Arc<dyn Fn(bool) + Send + Sync>;

/// The ways a session talks back to the UI.
#[derive(Clone)]
pub struct SessionReporters {
    pub report_level: LevelReporter,
    pub report_transcript: TranscriptReporter,
    pub report_session_active: SessionActiveReporter,
}

/// What one running voice session needs to know.
pub struct SessionConfig {
    pub microphone_name: String,
    pub tts_voice: String,
    pub tuning: DetectorTuning,
    pub noise_suppression: bool,
}

/// A live session. Dropping it closes the microphone and ends the relay.
pub struct VoiceSession {
    // Dropping the capture handle is what releases the microphone, which is what turns
    // the recording light off. Held only for that reason.
    _capture: CaptureHandle,
    relay_task: tokio::task::JoinHandle<()>,
}

impl Drop for VoiceSession {
    fn drop(&mut self) {
        self.relay_task.abort();
    }
}

/// Opens the microphone and starts relaying it into the call.
pub async fn start_session(
    call: Arc<Mutex<Call>>,
    openai_client: OpenAiClient,
    config: SessionConfig,
    reporters: SessionReporters,
) -> Result<VoiceSession, CaptureError> {
    let (pcm_sender, pcm_receiver) = tokio::sync::mpsc::unbounded_channel();
    let microphone_name = config.microphone_name.clone();

    // start_capture waits for the audio device to actually open, which is a blocking wait
    // that can run to hundreds of milliseconds or longer if the device is busy. Doing
    // that on an async worker parks a thread that other tasks need, so it goes to the
    // blocking pool instead.
    let capture =
        tokio::task::spawn_blocking(move || capture::start_capture(&microphone_name, pcm_sender))
            .await
            .map_err(|_| CaptureError::ThreadGone)??;

    let relay_task = tokio::spawn(async move {
        run_relay(pcm_receiver, call, openai_client, config, reporters).await;
    });

    Ok(VoiceSession {
        _capture: capture,
        relay_task,
    })
}

/// Feeds captured audio through the detector and speaks each finished utterance.
///
/// Utterances are handled one at a time on purpose: awaiting each one to finish playing
/// before starting the next is what stops the bot talking over itself.
async fn run_relay(
    mut pcm_receiver: tokio::sync::mpsc::UnboundedReceiver<Vec<u8>>,
    call: Arc<Mutex<Call>>,
    openai_client: OpenAiClient,
    config: SessionConfig,
    reporters: SessionReporters,
) {
    let mut detector = UtteranceDetector::with_tuning(config.tuning);
    // Denoise before anything else looks at the audio, so the level meter, the threshold,
    // and the transcription all work on the cleaned signal. None means it is turned off.
    let mut denoiser = config
        .noise_suppression
        .then(|| StreamDenoiser::new(CHANNEL_COUNT));

    while let Some(raw_pcm) = pcm_receiver.recv().await {
        let pcm = match denoiser.as_mut() {
            Some(denoiser) => denoiser.process(&raw_pcm),
            None => raw_pcm,
        };

        // The denoiser holds audio back until it has a whole frame, so early chunks come
        // back empty. Nothing to measure or detect on yet.
        if pcm.is_empty() {
            continue;
        }

        (reporters.report_level)(peak_as_level(&pcm));

        let Some(utterance_pcm) = detector.push_chunk(&pcm) else {
            continue;
        };

        speak_utterance(
            &utterance_pcm,
            &call,
            &openai_client,
            &config.tts_voice,
            &reporters,
        )
        .await;
    }

    tracing::info!("microphone stream ended, relay stopping");
}

/// Transcribes one utterance and speaks it back.
///
/// A failure here costs one utterance, never the session: the bot should keep listening
/// after a dropped request or a rejected key, with the reason visible in the console.
async fn speak_utterance(
    utterance_pcm: &[u8],
    call: &Arc<Mutex<Call>>,
    openai_client: &OpenAiClient,
    voice: &str,
    reporters: &SessionReporters,
) {
    let transcribed_text = match openai_client.transcribe_speech(utterance_pcm).await {
        Ok(text) => text,
        Err(error) => {
            tracing::error!("could not transcribe that: {error}");
            return;
        }
    };

    // Silence, noise, or an unintelligible blip all come back as nothing to say.
    let Some(spoken_text) = crate::openai::tts::prepare_spoken_text(&transcribed_text) else {
        return;
    };

    tracing::info!("heard: \"{spoken_text}\"");
    remember(&spoken_text, voice, reporters);

    let speech_wav = match openai_client.synthesize_speech(&spoken_text, voice).await {
        Ok(wav) => wav,
        Err(error) => {
            tracing::error!("could not speak that: {error}");
            return;
        }
    };

    play_and_wait(call, speech_wav).await;
}

/// Writes what was said to the lasting history.
///
/// Recorded as soon as it is heard rather than after it is spoken, so a failed synthesis
/// still leaves a record of the words. Failing to write history must not cost the
/// utterance, so this reports and moves on.
fn remember(spoken_text: &str, voice: &str, reporters: &SessionReporters) {
    let transcript = transcripts::record(spoken_text, voice);

    if let Err(error) = transcripts::append(&transcript) {
        tracing::warn!("could not save that to the history: {error}");
    }

    // Shown even if the write failed: the words were still said, and the warning above
    // says why they will not be there next time.
    (reporters.report_transcript)(transcript);
}

/// Plays a clip and waits for it to finish, so the next utterance does not start on top
/// of this one.
async fn play_and_wait(call: &Arc<Mutex<Call>>, speech_wav: Vec<u8>) {
    let input = Input::from(speech_wav);
    let track_handle = {
        let mut locked_call = call.lock().await;
        locked_call.play_input(input)
    };

    let Some(finished) = watch_for_track_end(&track_handle) else {
        // The track ended or failed before a listener could be attached, so there is
        // nothing left to wait for.
        return;
    };

    if finished.await.is_err() {
        tracing::warn!("stopped waiting for audio to finish: the track went away");
    }
}

/// Attaches an end-of-track listener, handing back the receiver that fires when the clip
/// finishes.
fn watch_for_track_end(track_handle: &TrackHandle) -> Option<oneshot::Receiver<()>> {
    let (end_sender, end_receiver) = oneshot::channel();
    let notifier = TrackEndNotifier {
        end_sender: Mutex::new(Some(end_sender)),
    };

    let watching = track_handle.add_event(Event::Track(TrackEvent::End), notifier);

    if let Err(error) = watching {
        tracing::warn!("could not watch for the end of a clip: {error}");
        return None;
    }

    Some(end_receiver)
}

/// Fires a oneshot when its track stops playing.
struct TrackEndNotifier {
    end_sender: Mutex<Option<oneshot::Sender<()>>>,
}

#[serenity::async_trait]
impl VoiceEventHandler for TrackEndNotifier {
    async fn act(&self, _context: &EventContext<'_>) -> Option<Event> {
        // Already fired: a track can report its end more than once.
        let end_sender = self.end_sender.lock().await.take()?;

        // The receiver is gone when the session ended mid-clip, which is not worth
        // reporting.
        let _waiter_still_there = end_sender.send(());

        None
    }
}

/// Turns a chunk's peak amplitude into the 0.0-1.0 the level meter wants.
fn peak_as_level(pcm: &[u8]) -> f32 {
    let peak_amplitude = find_peak_amplitude(pcm);
    let level = peak_amplitude as f32 / f32::from(i16::MAX);

    level.clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn silence_reads_as_no_level() {
        let silence = vec![0_u8; 100];

        assert_eq!(peak_as_level(&silence), 0.0);
    }

    #[test]
    fn a_full_scale_sample_reads_as_a_full_meter() {
        let full_scale = i16::MAX.to_le_bytes().repeat(10);

        assert_eq!(peak_as_level(&full_scale), 1.0);
    }

    #[test]
    fn the_meter_never_reads_over_full() {
        // i16::MIN is one louder than i16::MAX in the negative direction.
        let past_full_scale = i16::MIN.to_le_bytes().repeat(10);

        assert!(peak_as_level(&past_full_scale) <= 1.0);
    }

    #[test]
    fn a_half_scale_sample_reads_as_a_half_meter() {
        let half_scale = (i16::MAX / 2).to_le_bytes().repeat(10);

        let level = peak_as_level(&half_scale);

        assert!(
            (level - 0.5).abs() < 0.01,
            "expected about 0.5, got {level}"
        );
    }
}
