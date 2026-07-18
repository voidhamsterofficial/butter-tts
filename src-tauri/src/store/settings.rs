//! Everything the app needs from the user to run, kept as the single row in the
//! database's `settings` table. The two credential columns are encrypted — see
//! [`super::crypto`].

use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

use super::{crypto, StoreError};
use crate::audio::utterance::DetectorTuning;

/// Everything the app needs from the user to run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub openai_api_key: String,
    pub discord_bot_token: String,
    /// The capture device to listen to, as the audio host names it. Empty means the
    /// system default device.
    pub microphone_name: String,
    /// The OpenAI preset voice the bot speaks with.
    pub tts_voice: String,

    /// Whether to run the microphone through RNNoise before anything else, to strip out
    /// background noise. On by default.
    pub noise_suppression: bool,

    /// How loud a moment has to be to count as speech rather than room noise. Depends on
    /// the microphone's gain — see [`crate::audio::utterance::DEFAULT_SPEECH_THRESHOLD`].
    pub speech_threshold: i32,
    /// How long a pause has to be before the bot decides you have finished a sentence.
    pub trailing_silence_ms: usize,
    /// Anything shorter than this is a cough or a keyboard clack, not speech.
    pub min_utterance_ms: usize,
    /// The longest the bot will listen before cutting in and transcribing anyway.
    pub max_utterance_ms: usize,
}

impl Default for Settings {
    fn default() -> Self {
        let tuning = DetectorTuning::default();

        Self {
            openai_api_key: String::new(),
            discord_bot_token: String::new(),
            microphone_name: String::new(),
            tts_voice: crate::openai::tts::DEFAULT_TTS_VOICE.to_string(),
            noise_suppression: true,
            speech_threshold: tuning.speech_threshold,
            trailing_silence_ms: tuning.trailing_silence_ms,
            min_utterance_ms: tuning.min_utterance_ms,
            max_utterance_ms: tuning.max_utterance_ms,
        }
    }
}

impl Settings {
    /// The detector's half of the settings, in the shape the audio code wants.
    pub fn detector_tuning(&self) -> DetectorTuning {
        DetectorTuning {
            speech_threshold: self.speech_threshold,
            trailing_silence_ms: self.trailing_silence_ms,
            min_utterance_ms: self.min_utterance_ms,
            max_utterance_ms: self.max_utterance_ms,
        }
    }

    /// Names the settings the bot cannot start without, so the UI can say what is
    /// missing instead of letting the user press start and watch it fail.
    pub fn missing_requirements(&self) -> Vec<&'static str> {
        let mut missing = Vec::new();

        if self.openai_api_key.trim().is_empty() {
            missing.push("OpenAI API key");
        }

        if self.discord_bot_token.trim().is_empty() {
            missing.push("Discord bot token");
        }

        missing
    }

    pub fn is_ready_to_start(&self) -> bool {
        self.missing_requirements().is_empty()
    }
}

/// Reads the settings from the database, or hands back empty defaults when there is no
/// row yet.
///
/// A missing row is the first run, not a failure — the UI takes empty settings as a cue
/// to send the user to the settings page.
pub fn load() -> Result<Settings, StoreError> {
    let connection = super::open()?;
    load_from(&connection)
}

/// Writes the settings to the database, replacing whatever was there.
pub fn save(settings: &Settings) -> Result<(), StoreError> {
    let connection = super::open()?;
    save_to(&connection, settings)
}

/// The database half of [`load`], split out so it can be tested against an in-memory
/// database rather than wherever the real one happens to live.
fn load_from(connection: &Connection) -> Result<Settings, StoreError> {
    let settings = connection
        .query_row(
            "SELECT openai_api_key, discord_bot_token, microphone_name, tts_voice,
                    noise_suppression, speech_threshold, trailing_silence_ms,
                    min_utterance_ms, max_utterance_ms
             FROM settings WHERE id = 0",
            [],
            |row| {
                let openai_key_blob: Vec<u8> = row.get(0)?;
                let discord_token_blob: Vec<u8> = row.get(1)?;
                let noise_suppression: i64 = row.get(4)?;
                let trailing_silence_ms: i64 = row.get(6)?;
                let min_utterance_ms: i64 = row.get(7)?;
                let max_utterance_ms: i64 = row.get(8)?;

                Ok(Settings {
                    openai_api_key: crypto::open(&openai_key_blob).unwrap_or_default(),
                    discord_bot_token: crypto::open(&discord_token_blob).unwrap_or_default(),
                    microphone_name: row.get(2)?,
                    tts_voice: row.get(3)?,
                    noise_suppression: noise_suppression != 0,
                    speech_threshold: row.get(5)?,
                    trailing_silence_ms: trailing_silence_ms as usize,
                    min_utterance_ms: min_utterance_ms as usize,
                    max_utterance_ms: max_utterance_ms as usize,
                })
            },
        )
        .optional()
        .map_err(StoreError::Query)?;

    Ok(settings.unwrap_or_default())
}

/// The database half of [`save`].
fn save_to(connection: &Connection, settings: &Settings) -> Result<(), StoreError> {
    connection
        .execute(
            "INSERT INTO settings (
                id, openai_api_key, discord_bot_token, microphone_name, tts_voice,
                noise_suppression, speech_threshold, trailing_silence_ms,
                min_utterance_ms, max_utterance_ms
             ) VALUES (0, ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
             ON CONFLICT(id) DO UPDATE SET
                openai_api_key = excluded.openai_api_key,
                discord_bot_token = excluded.discord_bot_token,
                microphone_name = excluded.microphone_name,
                tts_voice = excluded.tts_voice,
                noise_suppression = excluded.noise_suppression,
                speech_threshold = excluded.speech_threshold,
                trailing_silence_ms = excluded.trailing_silence_ms,
                min_utterance_ms = excluded.min_utterance_ms,
                max_utterance_ms = excluded.max_utterance_ms",
            params![
                crypto::seal(&settings.openai_api_key),
                crypto::seal(&settings.discord_bot_token),
                settings.microphone_name,
                settings.tts_voice,
                settings.noise_suppression as i64,
                settings.speech_threshold,
                settings.trailing_silence_ms as i64,
                settings.min_utterance_ms as i64,
                settings.max_utterance_ms as i64,
            ],
        )
        .map_err(StoreError::Query)?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_settings() -> Settings {
        Settings {
            openai_api_key: "sk-test-key".to_string(),
            discord_bot_token: "discord-test-token".to_string(),
            microphone_name: "Yeti Nano".to_string(),
            tts_voice: "cedar".to_string(),
            noise_suppression: true,
            speech_threshold: 2_500,
            trailing_silence_ms: 600,
            min_utterance_ms: 400,
            max_utterance_ms: 20_000,
        }
    }

    fn test_db() -> Connection {
        let connection = Connection::open_in_memory().expect("in-memory db should open");
        connection
            .execute_batch(super::super::SCHEMA_SQL)
            .expect("schema should apply");
        connection
    }

    #[test]
    fn settings_survive_a_round_trip_through_the_database() {
        let connection = test_db();
        let original = sample_settings();

        save_to(&connection, &original).expect("saving settings should succeed");
        let loaded = load_from(&connection).expect("reading settings back should succeed");

        assert_eq!(loaded, original);
    }

    #[test]
    fn credentials_are_not_stored_as_plain_text() {
        let connection = test_db();
        save_to(&connection, &sample_settings()).expect("saving settings should succeed");

        let raw_key: Vec<u8> = connection
            .query_row(
                "SELECT openai_api_key FROM settings WHERE id = 0",
                [],
                |row| row.get(0),
            )
            .expect("the row should exist");

        assert!(!raw_key.windows(11).any(|window| window == b"sk-test-key"));
    }

    #[test]
    fn a_missing_row_falls_back_to_defaults() {
        let connection = test_db();

        let loaded = load_from(&connection).expect("a missing row should still load");

        assert_eq!(loaded, Settings::default());
        // A file written before the threshold existed must not load as a threshold of 0,
        // which would treat everything as speech.
        assert_eq!(
            loaded.speech_threshold,
            crate::audio::utterance::DEFAULT_SPEECH_THRESHOLD
        );
    }

    #[test]
    fn saving_twice_replaces_rather_than_duplicates() {
        let connection = test_db();

        save_to(&connection, &sample_settings()).expect("first save should succeed");
        save_to(
            &connection,
            &Settings {
                microphone_name: "Different mic".to_string(),
                ..sample_settings()
            },
        )
        .expect("second save should succeed");

        let row_count: i64 = connection
            .query_row("SELECT count(*) FROM settings", [], |row| row.get(0))
            .expect("should be able to count rows");
        assert_eq!(row_count, 1);

        let loaded = load_from(&connection).expect("reading settings back should succeed");
        assert_eq!(loaded.microphone_name, "Different mic");
    }

    #[test]
    fn empty_settings_report_what_is_missing() {
        let empty = Settings::default();

        assert!(!empty.is_ready_to_start());
        assert_eq!(
            empty.missing_requirements(),
            vec!["OpenAI API key", "Discord bot token"]
        );
    }

    #[test]
    fn whitespace_is_not_a_token() {
        let settings = Settings {
            openai_api_key: "   ".to_string(),
            discord_bot_token: "\t\n".to_string(),
            ..Settings::default()
        };

        assert!(!settings.is_ready_to_start());
        assert_eq!(settings.missing_requirements().len(), 2);
    }

    #[test]
    fn filled_in_settings_are_ready_to_start() {
        let settings = sample_settings();

        assert!(settings.is_ready_to_start());
        assert!(settings.missing_requirements().is_empty());
    }

    #[test]
    fn a_microphone_is_not_required_because_the_default_device_will_do() {
        let settings = Settings {
            microphone_name: String::new(),
            ..sample_settings()
        };

        assert!(settings.is_ready_to_start());
    }
}
