//! Slices a continuous microphone stream into individual utterances.
//!
//! Transcription needs a complete phrase rather than an endless stream, so audio is
//! collected while someone is speaking and handed over once they stop.

/// The capture format the whole pipeline works in: signed 16-bit little-endian PCM.
/// Discord expects 48kHz stereo, and resampling anywhere in between would only add a
/// step that can be got wrong.
pub const SAMPLE_RATE_HZ: u32 = 48_000;
pub const CHANNEL_COUNT: u16 = 2;

const BYTES_PER_SAMPLE: usize = 2;
const BYTES_PER_MS: usize =
    (SAMPLE_RATE_HZ as usize * CHANNEL_COUNT as usize * BYTES_PER_SAMPLE) / 1000;

/// Anything quieter than this counts as silence.
///
/// This depends entirely on the gain the microphone runs at, which is why it is tunable.
/// A Yeti at default gain measured a median of 268 and a worst frame of 2272 out of
/// 32767 across four seconds of an idle room; a quieter setup measured 39 and 79. This
/// default clears the louder of those with headroom and still sits far below speech,
/// which peaks in the thousands.
///
/// Getting it wrong is not subtle. Too low and room noise reads as continuous speech, so
/// the trailing silence that ends an utterance never arrives and every utterance runs to
/// the maximum length. Too high and quiet speech is never heard at all.
pub const DEFAULT_SPEECH_THRESHOLD: i32 = 2_500;

/// How much quiet has to follow speech before the utterance is treated as finished.
/// This is dead time added to every reply, on top of the transcribe and speak round
/// trips, so it dominates how responsive the bot feels. Long enough to ride out the
/// natural gap between words, short enough not to feel broken while you wait.
pub const DEFAULT_TRAILING_SILENCE_MS: usize = 600;

/// Ignore blips like a cough, a keyboard clack, or a door closing.
pub const DEFAULT_MIN_UTTERANCE_MS: usize = 400;

/// Force a cut on someone who never pauses, so transcription still happens.
pub const DEFAULT_MAX_UTTERANCE_MS: usize = 20_000;

/// The ranges the sliders offer. Outside these the detector stops behaving sensibly
/// rather than merely feeling different, so the UI does not let you get there.
pub const SPEECH_THRESHOLD_RANGE: (i32, i32) = (50, 8_000);
pub const TRAILING_SILENCE_RANGE_MS: (usize, usize) = (200, 2_000);
pub const MIN_UTTERANCE_RANGE_MS: (usize, usize) = (100, 2_000);
pub const MAX_UTTERANCE_RANGE_MS: (usize, usize) = (5_000, 60_000);

/// How the detector decides where one utterance ends and the next begins. Every value is
/// exposed as a slider, because the right numbers depend on the microphone and the room.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DetectorTuning {
    pub speech_threshold: i32,
    pub trailing_silence_ms: usize,
    pub min_utterance_ms: usize,
    pub max_utterance_ms: usize,
}

impl Default for DetectorTuning {
    fn default() -> Self {
        Self {
            speech_threshold: DEFAULT_SPEECH_THRESHOLD,
            trailing_silence_ms: DEFAULT_TRAILING_SILENCE_MS,
            min_utterance_ms: DEFAULT_MIN_UTTERANCE_MS,
            max_utterance_ms: DEFAULT_MAX_UTTERANCE_MS,
        }
    }
}

impl DetectorTuning {
    /// Pulls every value back into the range the detector can actually work in.
    ///
    /// The settings file is hand-editable and the values arrive from a webview, so this
    /// is the boundary where a nonsense number becomes a merely unhelpful one. A
    /// max_utterance_ms below min_utterance_ms would emit nothing at all.
    pub fn clamped(self) -> Self {
        let min_utterance_ms = clamp_range(self.min_utterance_ms, MIN_UTTERANCE_RANGE_MS);
        let max_utterance_ms = clamp_range(self.max_utterance_ms, MAX_UTTERANCE_RANGE_MS);

        Self {
            speech_threshold: self
                .speech_threshold
                .clamp(SPEECH_THRESHOLD_RANGE.0, SPEECH_THRESHOLD_RANGE.1),
            trailing_silence_ms: clamp_range(self.trailing_silence_ms, TRAILING_SILENCE_RANGE_MS),
            min_utterance_ms,
            max_utterance_ms: max_utterance_ms.max(min_utterance_ms),
        }
    }
}

fn clamp_range(value: usize, range: (usize, usize)) -> usize {
    value.clamp(range.0, range.1)
}

/// Finds the loudest sample in a chunk of 16-bit PCM, from 0 to 32767.
pub fn find_peak_amplitude(pcm_chunk: &[u8]) -> i32 {
    let peak_amplitude = pcm_chunk
        .chunks_exact(BYTES_PER_SAMPLE)
        .map(|sample_bytes| {
            let sample = i16::from_le_bytes([sample_bytes[0], sample_bytes[1]]);
            // Widened before the abs: i16::MIN has no positive counterpart and would
            // overflow on its own.
            i32::from(sample).abs()
        })
        .max()
        .unwrap_or(0);

    peak_amplitude
}

/// Collects captured audio and emits it one utterance at a time.
#[derive(Debug, Default)]
pub struct UtteranceDetector {
    tuning: DetectorTuning,
    utterance_pcm: Vec<u8>,
    speech_bytes: usize,
    trailing_silent_bytes: usize,
    is_speaking: bool,
}

impl UtteranceDetector {
    pub fn new() -> Self {
        Self::default()
    }

    /// Uses tuning measured for a particular microphone and room. The values are clamped
    /// here, so a hand-edited settings file cannot produce a detector that never speaks.
    pub fn with_tuning(tuning: DetectorTuning) -> Self {
        Self {
            tuning: tuning.clamped(),
            ..Self::default()
        }
    }

    /// Feeds one chunk of captured audio in, returning the completed utterance once the
    /// speaker has stopped or has run on too long. Returns `None` while an utterance is
    /// still being collected, and for anything too short to be worth transcribing.
    pub fn push_chunk(&mut self, pcm_chunk: &[u8]) -> Option<Vec<u8>> {
        let peak_amplitude = find_peak_amplitude(pcm_chunk);
        let is_loud_enough = peak_amplitude >= self.tuning.speech_threshold;

        // Silence before anyone has spoken is just dead air — dropping it keeps a quiet
        // session from buffering the whole time it sits there.
        if !is_loud_enough && !self.is_speaking {
            return None;
        }

        self.is_speaking = true;
        self.utterance_pcm.extend_from_slice(pcm_chunk);

        if is_loud_enough {
            self.speech_bytes += pcm_chunk.len();
            self.trailing_silent_bytes = 0;
        } else {
            self.trailing_silent_bytes += pcm_chunk.len();
        }

        let silence_needed_bytes = self.tuning.trailing_silence_ms * BYTES_PER_MS;
        let length_limit_bytes = self.tuning.max_utterance_ms * BYTES_PER_MS;

        let has_stopped_speaking = self.trailing_silent_bytes >= silence_needed_bytes;
        let has_run_too_long = self.utterance_pcm.len() >= length_limit_bytes;

        if !has_stopped_speaking && !has_run_too_long {
            return None;
        }

        self.finish_utterance()
    }

    /// Hands back the collected audio if it holds enough actual speech to be worth
    /// transcribing, then resets ready for the next utterance.
    fn finish_utterance(&mut self) -> Option<Vec<u8>> {
        // Measured against the loud audio only. The buffer also holds the trailing
        // silence that ended the utterance, and counting that would let a short blip
        // followed by a long pause look like a long enough phrase.
        let speech_ms = self.speech_bytes / BYTES_PER_MS;
        let utterance_pcm = std::mem::take(&mut self.utterance_pcm);

        self.speech_bytes = 0;
        self.trailing_silent_bytes = 0;
        self.is_speaking = false;

        if speech_ms < self.tuning.min_utterance_ms {
            return None;
        }

        Some(utterance_pcm)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LOUD_AMPLITUDE: i16 = 8_000;
    const SILENT_AMPLITUDE: i16 = 40;

    /// The worst frame measured on a Yeti at default gain across four seconds of an idle
    /// room. Room noise, not speech.
    const MEASURED_YETI_NOISE_FLOOR: i16 = 2_272;

    /// Builds a chunk of constant-amplitude PCM of the given duration.
    fn make_chunk(duration_ms: usize, amplitude: i16) -> Vec<u8> {
        let sample_count = (duration_ms * BYTES_PER_MS) / BYTES_PER_SAMPLE;
        amplitude.to_le_bytes().repeat(sample_count)
    }

    #[test]
    fn peak_amplitude_of_silence_is_zero() {
        let silence = make_chunk(10, 0);
        assert_eq!(find_peak_amplitude(&silence), 0);
    }

    #[test]
    fn peak_amplitude_ignores_sign() {
        let negative_pcm = (-1_234_i16).to_le_bytes().repeat(4);
        assert_eq!(find_peak_amplitude(&negative_pcm), 1_234);
    }

    #[test]
    fn silence_alone_never_starts_an_utterance() {
        let mut detector = UtteranceDetector::new();
        let silence = make_chunk(1_000, SILENT_AMPLITUDE);

        assert!(detector.push_chunk(&silence).is_none());
        assert!(detector.push_chunk(&silence).is_none());
        assert_eq!(detector.utterance_pcm.len(), 0);
    }

    #[test]
    fn speech_followed_by_silence_is_emitted() {
        let mut detector = UtteranceDetector::new();
        let speech = make_chunk(1_000, LOUD_AMPLITUDE);
        let silence = make_chunk(DEFAULT_TRAILING_SILENCE_MS, SILENT_AMPLITUDE);

        assert!(detector.push_chunk(&speech).is_none());

        let utterance = detector.push_chunk(&silence);
        let Some(utterance_pcm) = utterance else {
            panic!("expected an utterance once the speaker stopped");
        };

        // The emitted audio is the speech plus the trailing silence that ended it.
        let expected_bytes = speech.len() + silence.len();
        assert_eq!(utterance_pcm.len(), expected_bytes);
    }

    #[test]
    fn a_blip_too_short_to_be_speech_is_discarded() {
        let mut detector = UtteranceDetector::new();
        let blip = make_chunk(DEFAULT_MIN_UTTERANCE_MS - 100, LOUD_AMPLITUDE);
        let silence = make_chunk(DEFAULT_TRAILING_SILENCE_MS, SILENT_AMPLITUDE);

        assert!(detector.push_chunk(&blip).is_none());
        assert!(detector.push_chunk(&silence).is_none());
    }

    #[test]
    fn trailing_silence_resets_when_speech_resumes() {
        let mut detector = UtteranceDetector::new();
        let speech = make_chunk(500, LOUD_AMPLITUDE);
        let pause = make_chunk(DEFAULT_TRAILING_SILENCE_MS - 100, SILENT_AMPLITUDE);

        // A natural gap between words must not cut the utterance in half.
        assert!(detector.push_chunk(&speech).is_none());
        assert!(detector.push_chunk(&pause).is_none());
        assert!(detector.push_chunk(&speech).is_none());
        assert_eq!(detector.trailing_silent_bytes, 0);
    }

    #[test]
    fn an_endless_talker_is_cut_at_the_maximum() {
        let mut detector = UtteranceDetector::new();
        let long_speech = make_chunk(DEFAULT_MAX_UTTERANCE_MS, LOUD_AMPLITUDE);

        let utterance = detector.push_chunk(&long_speech);
        let Some(utterance_pcm) = utterance else {
            panic!("expected the utterance to be cut at the maximum length");
        };

        assert_eq!(utterance_pcm.len(), long_speech.len());
    }

    #[test]
    fn a_noisy_rooms_worst_frame_is_still_silence_at_the_default_threshold() {
        // The threshold this app shipped with originally was 500, which this noise floor
        // sails past. The symptom is not a missed word: room noise reads as continuous
        // speech, the trailing silence never arrives, and every utterance runs to the
        // 20-second cut.
        let mut detector = UtteranceDetector::new();
        let room_noise = make_chunk(2_000, MEASURED_YETI_NOISE_FLOOR);

        assert!(detector.push_chunk(&room_noise).is_none());
        assert!(
            !detector.is_speaking,
            "room noise must not start an utterance"
        );
    }

    #[test]
    fn speech_over_a_noisy_room_still_ends_when_the_speaker_stops() {
        let mut detector = UtteranceDetector::new();
        let speech = make_chunk(1_000, LOUD_AMPLITUDE);
        let room_noise = make_chunk(DEFAULT_TRAILING_SILENCE_MS, MEASURED_YETI_NOISE_FLOOR);

        assert!(detector.push_chunk(&speech).is_none());

        // The pause after speaking is a noisy room, not true silence — it still has to
        // count as the end of the utterance.
        assert!(detector.push_chunk(&room_noise).is_some());
    }

    #[test]
    fn a_quieter_microphone_can_ask_for_a_lower_threshold() {
        let mut detector = UtteranceDetector::with_tuning(DetectorTuning {
            speech_threshold: 500,
            ..DetectorTuning::default()
        });
        let quiet_speech = make_chunk(1_000, 600);
        let quiet_room = make_chunk(DEFAULT_TRAILING_SILENCE_MS, 79);

        assert!(detector.push_chunk(&quiet_speech).is_none());
        assert!(detector.push_chunk(&quiet_room).is_some());
    }

    #[test]
    fn a_shorter_silence_ends_an_utterance_sooner() {
        let mut detector = UtteranceDetector::with_tuning(DetectorTuning {
            trailing_silence_ms: 250,
            ..DetectorTuning::default()
        });
        let speech = make_chunk(1_000, LOUD_AMPLITUDE);
        let short_pause = make_chunk(250, SILENT_AMPLITUDE);

        assert!(detector.push_chunk(&speech).is_none());
        // The default 600ms would still be waiting here.
        assert!(detector.push_chunk(&short_pause).is_some());
    }

    #[test]
    fn tuning_is_pulled_back_into_a_range_that_works() {
        let nonsense = DetectorTuning {
            speech_threshold: -5_000,
            trailing_silence_ms: 0,
            min_utterance_ms: 0,
            max_utterance_ms: 0,
        };

        let clamped = nonsense.clamped();

        assert_eq!(clamped.speech_threshold, SPEECH_THRESHOLD_RANGE.0);
        assert_eq!(clamped.trailing_silence_ms, TRAILING_SILENCE_RANGE_MS.0);
        assert_eq!(clamped.min_utterance_ms, MIN_UTTERANCE_RANGE_MS.0);
        assert_eq!(clamped.max_utterance_ms, MAX_UTTERANCE_RANGE_MS.0);
    }

    #[test]
    fn a_maximum_below_the_minimum_is_raised_rather_than_left_impossible() {
        // Hand-edited into a state where no utterance could ever be both long enough to
        // send and short enough to collect.
        let contradictory = DetectorTuning {
            min_utterance_ms: 2_000,
            max_utterance_ms: 5_000,
            ..DetectorTuning::default()
        };

        let clamped = contradictory.clamped();

        assert!(clamped.max_utterance_ms >= clamped.min_utterance_ms);
    }

    #[test]
    fn sensible_tuning_is_left_exactly_as_chosen() {
        let chosen = DetectorTuning {
            speech_threshold: 1_200,
            trailing_silence_ms: 800,
            min_utterance_ms: 300,
            max_utterance_ms: 15_000,
        };

        assert_eq!(chosen.clamped(), chosen);
    }

    #[test]
    fn the_detector_is_reusable_after_emitting() {
        let mut detector = UtteranceDetector::new();
        let speech = make_chunk(1_000, LOUD_AMPLITUDE);
        let silence = make_chunk(DEFAULT_TRAILING_SILENCE_MS, SILENT_AMPLITUDE);

        detector.push_chunk(&speech);
        detector.push_chunk(&silence);

        assert!(detector.push_chunk(&speech).is_none());
        assert!(detector.push_chunk(&silence).is_some());
    }
}
