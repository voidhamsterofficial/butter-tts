//! The app's settings: a hand-editable YAML file living next to the exe.
//!
//! Keeping the file beside the exe rather than in AppData is what makes the app
//! portable — copy the exe and its settings to a USB stick and it runs the same way
//! on the next machine, leaving nothing behind on this one.
//!
//! The tokens are stored in the clear. Anything that can read the folder the exe sits
//! in can read the Discord bot token, which is full control of the bot.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::audio::utterance::DetectorTuning;

const SETTINGS_FILE_NAME: &str = "butter-tts.settings.yaml";

/// Written above the settings whenever the app saves them, since the file is meant to
/// be readable and editable by hand.
const SETTINGS_FILE_HEADER: &str = "\
# Butter TTS settings.
#
# This file sits next to butter-tts.exe and travels with it. Edit it by hand if you
# like — the app reads it at startup and whenever the bot is started.
#
# WARNING: these tokens are stored in plain text. Anyone who can read this file can
# control your Discord bot and spend against your OpenAI account.
#
# The listening values below are the sliders on the app's Settings page. Editing them
# here works too, but the sliders show you a live meter while you talk, which makes the
# threshold far easier to get right.
#
#   noise_suppression    Runs the mic through RNNoise to strip background noise before
#                        anything else. On by default; turn off to compare.
#   speech_threshold     How loud a moment must be (0-32767) to count as speech rather
#                        than room noise. Rambling clips that never end? Raise it.
#                        Ignored unless you shout? Lower it.
#   trailing_silence_ms  How long a pause ends a sentence. This is added to every reply,
#                        so it decides how snappy the bot feels.
#   min_utterance_ms     Shorter than this is a cough, not a word.
#   max_utterance_ms     The longest it listens before transcribing anyway.
";

/// Everything the app needs from the user to run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
// Missing keys fall back to the defaults below, so a hand-trimmed or older file still
// loads instead of failing the app to a dead end.
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

#[derive(Debug, thiserror::Error)]
pub enum SettingsError {
    #[error("could not work out where the app is running from: {0}")]
    ExeLocation(#[source] std::io::Error),

    #[error("the app is running from a filesystem root, so there is nowhere to keep its settings")]
    NoExeDirectory,

    #[error("could not read {path}: {source}")]
    Read {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("could not write {path}: {source}")]
    Write {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("{path} is not valid YAML: {source}")]
    Parse {
        path: PathBuf,
        #[source]
        source: serde_norway::Error,
    },

    #[error("could not turn the settings into YAML: {0}")]
    Encode(#[source] serde_norway::Error),
}

/// Where the settings file lives: alongside the running exe.
pub fn settings_path() -> Result<PathBuf, SettingsError> {
    let exe_path = std::env::current_exe().map_err(SettingsError::ExeLocation)?;

    let Some(exe_directory) = exe_path.parent() else {
        return Err(SettingsError::NoExeDirectory);
    };

    Ok(exe_directory.join(SETTINGS_FILE_NAME))
}

/// Reads the settings from disk, or hands back empty defaults when there is no file yet.
///
/// A missing file is the first run, not a failure — the UI takes empty settings as a cue
/// to send the user to the settings page.
pub fn load() -> Result<Settings, SettingsError> {
    let path = settings_path()?;

    if !path.exists() {
        return Ok(Settings::default());
    }

    read_from(&path)
}

/// Writes the settings to disk, replacing whatever was there.
pub fn save(settings: &Settings) -> Result<(), SettingsError> {
    let path = settings_path()?;
    write_to(&path, settings)
}

/// The filesystem half of [`load`], split out so it can be tested against a temporary
/// file rather than wherever the exe happens to live.
fn read_from(path: &Path) -> Result<Settings, SettingsError> {
    let yaml = std::fs::read_to_string(path).map_err(|source| SettingsError::Read {
        path: path.to_path_buf(),
        source,
    })?;

    serde_norway::from_str(&yaml).map_err(|source| SettingsError::Parse {
        path: path.to_path_buf(),
        source,
    })
}

/// The filesystem half of [`save`].
fn write_to(path: &Path, settings: &Settings) -> Result<(), SettingsError> {
    let body = serde_norway::to_string(settings).map_err(SettingsError::Encode)?;
    let file_contents = format!("{SETTINGS_FILE_HEADER}\n{body}");

    std::fs::write(path, file_contents).map_err(|source| SettingsError::Write {
        path: path.to_path_buf(),
        source,
    })
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

    /// A unique scratch path per test, so tests can run in parallel without fighting
    /// over one file.
    fn temp_settings_path(test_name: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!("butter-tts-{test_name}.settings.yaml"));
        let _ = std::fs::remove_file(&path);
        path
    }

    #[test]
    fn settings_survive_a_round_trip_through_the_file() {
        let path = temp_settings_path("round-trip");
        let original = sample_settings();

        write_to(&path, &original).expect("writing settings should succeed");
        let loaded = read_from(&path).expect("reading settings back should succeed");

        assert_eq!(loaded, original);
    }

    #[test]
    fn the_saved_file_explains_itself_and_is_readable() {
        let path = temp_settings_path("readable");

        write_to(&path, &sample_settings()).expect("writing settings should succeed");
        let file_contents = std::fs::read_to_string(&path).expect("the file should exist");

        assert!(file_contents.contains("# Butter TTS settings."));
        assert!(file_contents.contains("plain text"));
        // Hand-editable means the keys are readable, not a wall of quoted JSON.
        assert!(file_contents.contains("openai_api_key: sk-test-key"));
    }

    #[test]
    fn a_file_missing_keys_falls_back_to_defaults() {
        let path = temp_settings_path("partial");
        std::fs::write(&path, "openai_api_key: sk-only-this-one\n")
            .expect("writing the partial file should succeed");

        let loaded = read_from(&path).expect("a partial file should still load");

        assert_eq!(loaded.openai_api_key, "sk-only-this-one");
        assert_eq!(loaded.discord_bot_token, "");
        // The voice has a real default, so a trimmed file still speaks.
        assert_eq!(loaded.tts_voice, crate::openai::tts::DEFAULT_TTS_VOICE);
        // A file written before the threshold existed must not load as a threshold of 0,
        // which would treat everything as speech.
        assert_eq!(
            loaded.speech_threshold,
            crate::audio::utterance::DEFAULT_SPEECH_THRESHOLD
        );
    }

    #[test]
    fn a_hand_tuned_threshold_is_honoured() {
        let path = temp_settings_path("threshold");
        std::fs::write(&path, "speech_threshold: 900\n").expect("writing should succeed");

        let loaded = read_from(&path).expect("the file should load");

        assert_eq!(loaded.speech_threshold, 900);
    }

    #[test]
    fn a_file_written_before_noise_suppression_existed_defaults_it_on() {
        let path = temp_settings_path("upgrade-noise");
        // No noise_suppression key, as an older version would have saved.
        std::fs::write(&path, "openai_api_key: sk-old\n").expect("writing should succeed");

        let loaded = read_from(&path).expect("an older file should still load");

        assert!(loaded.noise_suppression);
    }

    #[test]
    fn broken_yaml_reports_the_file_it_could_not_read() {
        let path = temp_settings_path("broken");
        std::fs::write(&path, "openai_api_key: [unclosed\n").expect("writing should succeed");

        let error = read_from(&path).expect_err("broken YAML should not load");

        assert!(matches!(error, SettingsError::Parse { .. }));
        assert!(error.to_string().contains("is not valid YAML"));
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
