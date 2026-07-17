//! Speaking a line of text with the OpenAI text-to-speech API.

use serde::Serialize;

use super::{error_for_status, OpenAiClient, OpenAiError};

const TTS_MODEL: &str = "gpt-4o-mini-tts";

/// WAV rather than Opus or raw PCM. OpenAI's `pcm` comes back as 24kHz mono, which
/// would need resampling to the 48kHz stereo Discord wants; a WAV carries its format in
/// its header, so songbird's decoder can read it and resample without us hand-rolling
/// either step.
const TTS_RESPONSE_FORMAT: &str = "wav";

/// Every preset voice `gpt-4o-mini-tts` supports. OpenAI recommends marin and cedar for
/// best quality. This is the one place the list is defined — the settings page and the
/// `/voice` command both read it, and a saved voice is validated against it.
pub const AVAILABLE_TTS_VOICES: [&str; 13] = [
    "alloy", "ash", "ballad", "cedar", "coral", "echo", "fable", "marin", "nova", "onyx", "sage",
    "shimmer", "verse",
];

pub const DEFAULT_TTS_VOICE: &str = "marin";

/// Long messages cost time and money to synthesise, and nobody wants to listen to
/// someone read out an essay.
pub const MAX_SPOKEN_TEXT_LENGTH: usize = 300;

#[derive(Debug, Serialize)]
struct SpeechRequest<'a> {
    model: &'a str,
    voice: &'a str,
    input: &'a str,
    response_format: &'a str,
}

/// True when the name is a voice the API will actually accept.
pub fn is_known_voice(voice: &str) -> bool {
    AVAILABLE_TTS_VOICES.contains(&voice)
}

/// Falls back to the default for anything the API would reject, so a hand-edited
/// settings file with a typo still speaks instead of failing every line.
pub fn resolve_voice(voice: &str) -> &str {
    if is_known_voice(voice) {
        return voice;
    }

    DEFAULT_TTS_VOICE
}

/// Trims a line down to something worth listening to, or `None` when there is nothing
/// to say.
pub fn prepare_spoken_text(text: &str) -> Option<String> {
    let trimmed = text.trim();

    if trimmed.is_empty() {
        return None;
    }

    // Cut on a character boundary: slicing raw bytes would panic partway through any
    // multi-byte character, and people do say things with accents and emoji in them.
    let spoken_text: String = trimmed.chars().take(MAX_SPOKEN_TEXT_LENGTH).collect();

    Some(spoken_text)
}

impl OpenAiClient {
    /// Converts text into spoken audio, returned as a complete WAV file.
    pub async fn synthesize_speech(&self, text: &str, voice: &str) -> Result<Vec<u8>, OpenAiError> {
        let request = SpeechRequest {
            model: TTS_MODEL,
            voice: resolve_voice(voice),
            input: text,
            response_format: TTS_RESPONSE_FORMAT,
        };

        let response = self
            .http_client
            .post(self.endpoint("/audio/speech"))
            .bearer_auth(&self.api_key)
            .json(&request)
            .send()
            .await
            .map_err(OpenAiError::Transport)?;

        let audio_bytes = error_for_status(response)
            .await?
            .bytes()
            .await
            .map_err(OpenAiError::Transport)?;

        Ok(audio_bytes.to_vec())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_voice_is_one_the_api_accepts() {
        assert!(is_known_voice(DEFAULT_TTS_VOICE));
    }

    #[test]
    fn a_typo_in_the_settings_file_falls_back_rather_than_failing() {
        assert_eq!(resolve_voice("marrin"), DEFAULT_TTS_VOICE);
        assert_eq!(resolve_voice(""), DEFAULT_TTS_VOICE);
    }

    #[test]
    fn a_known_voice_is_used_as_given() {
        assert_eq!(resolve_voice("cedar"), "cedar");
    }

    #[test]
    fn there_is_nothing_to_say_for_blank_text() {
        assert_eq!(prepare_spoken_text(""), None);
        assert_eq!(prepare_spoken_text("   \n\t "), None);
    }

    #[test]
    fn an_essay_is_cut_down_to_the_maximum() {
        let essay = "a".repeat(MAX_SPOKEN_TEXT_LENGTH + 200);

        let spoken = prepare_spoken_text(&essay).expect("an essay has something to say");

        assert_eq!(spoken.chars().count(), MAX_SPOKEN_TEXT_LENGTH);
    }

    #[test]
    fn cutting_long_text_does_not_split_a_character_in_half() {
        // Every char here is multi-byte, so a byte-wise cut would panic or corrupt.
        let long_text = "é".repeat(MAX_SPOKEN_TEXT_LENGTH + 50);

        let spoken = prepare_spoken_text(&long_text).expect("there is something to say");

        assert_eq!(spoken.chars().count(), MAX_SPOKEN_TEXT_LENGTH);
        assert!(spoken.chars().all(|character| character == 'é'));
    }

    #[test]
    fn surrounding_whitespace_is_dropped() {
        let spoken = prepare_spoken_text("  hello there  ").expect("there is something to say");

        assert_eq!(spoken, "hello there");
    }
}
