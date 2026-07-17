//! What the UI can ask the app to do, and what the app tells the UI about.
//!
//! Everything crossing into the webview goes through here: the commands the pages call,
//! and the three streams of events they listen to (log lines, bot status, mic level).

use std::sync::Arc;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};
use tokio::sync::Mutex;

use crate::audio::capture;
use crate::audio::utterance;
use crate::discord::{self, BotConfig, BotHandle};
use crate::settings::{self, Settings};
use crate::transcripts::{self, Transcript};

/// Event names the frontend subscribes to.
pub const LOG_EVENT: &str = "bot://log";
pub const STATUS_EVENT: &str = "bot://status";
pub const LEVEL_EVENT: &str = "bot://level";
pub const TRANSCRIPT_EVENT: &str = "bot://transcript";
/// Carries a bool: whether the bot is in a voice channel and actually listening, as
/// opposed to merely connected to Discord.
pub const SESSION_EVENT: &str = "bot://session";

/// The bounds the tuning sliders run between, so the UI reads them from the one place
/// they are defined rather than repeating the numbers.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SliderRange {
    pub min: i64,
    pub max: i64,
    pub default: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TuningRanges {
    pub speech_threshold: SliderRange,
    pub trailing_silence_ms: SliderRange,
    pub min_utterance_ms: SliderRange,
    pub max_utterance_ms: SliderRange,
}

/// Whether the bot is connected, and why not when it isn't.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", tag = "state", content = "detail")]
pub enum BotStatus {
    Offline,
    Starting,
    Online,
    /// Carries the reason, so the dashboard can show it without the user opening the
    /// console.
    Failed(String),
}

/// A line for the console page.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LogLine {
    /// Milliseconds since the Unix epoch; the UI formats it.
    pub timestamp_ms: u64,
    pub level: String,
    pub message: String,
}

/// The bot, and whatever the UI needs to know about it. One per app.
pub struct AppState {
    bot: Mutex<Option<BotHandle>>,
    status: Mutex<BotStatus>,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            bot: Mutex::new(None),
            status: Mutex::new(BotStatus::Offline),
        }
    }
}

/// Sends a log line to the console page.
///
/// Called from the tracing layer, so it must never log anything itself — that would
/// recurse.
pub fn emit_log(app: &AppHandle, level: &str, message: &str) {
    let timestamp_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|since_epoch| since_epoch.as_millis() as u64)
        .unwrap_or(0);

    let line = LogLine {
        timestamp_ms,
        level: level.to_string(),
        message: message.to_string(),
    };

    let _webview_listening = app.emit(LOG_EVENT, line);
}

async fn set_status(app: &AppHandle, status: BotStatus) {
    let state = app.state::<AppState>();
    *state.status.lock().await = status.clone();

    let _webview_listening = app.emit(STATUS_EVENT, status);
}

// ---------------------------------------------------------------------------
// Commands
// ---------------------------------------------------------------------------

/// Every input device, for the settings page's picker.
#[tauri::command]
pub fn list_microphones() -> Result<Vec<String>, String> {
    capture::list_microphone_names().map_err(|error| error.to_string())
}

/// The device used when none is chosen, so the picker can label it.
#[tauri::command]
pub fn default_microphone() -> Option<String> {
    capture::default_microphone_name()
}

/// The voices the settings page offers.
#[tauri::command]
pub fn list_voices() -> Vec<String> {
    crate::openai::tts::AVAILABLE_TTS_VOICES
        .iter()
        .map(|voice| (*voice).to_string())
        .collect()
}

#[tauri::command]
pub fn load_settings() -> Result<Settings, String> {
    settings::load().map_err(|error| error.to_string())
}

#[tauri::command]
pub fn save_settings(settings: Settings) -> Result<(), String> {
    settings::save(&settings).map_err(|error| error.to_string())
}

/// Where the settings file lives, so the settings page can show it.
#[tauri::command]
pub fn settings_file_path() -> Result<String, String> {
    settings::settings_path()
        .map(|path| path.display().to_string())
        .map_err(|error| error.to_string())
}

/// The bounds for the tuning sliders.
#[tauri::command]
pub fn tuning_ranges() -> TuningRanges {
    let defaults = utterance::DetectorTuning::default();

    TuningRanges {
        speech_threshold: SliderRange {
            min: utterance::SPEECH_THRESHOLD_RANGE.0.into(),
            max: utterance::SPEECH_THRESHOLD_RANGE.1.into(),
            default: defaults.speech_threshold.into(),
        },
        trailing_silence_ms: SliderRange {
            min: utterance::TRAILING_SILENCE_RANGE_MS.0 as i64,
            max: utterance::TRAILING_SILENCE_RANGE_MS.1 as i64,
            default: defaults.trailing_silence_ms as i64,
        },
        min_utterance_ms: SliderRange {
            min: utterance::MIN_UTTERANCE_RANGE_MS.0 as i64,
            max: utterance::MIN_UTTERANCE_RANGE_MS.1 as i64,
            default: defaults.min_utterance_ms as i64,
        },
        max_utterance_ms: SliderRange {
            min: utterance::MAX_UTTERANCE_RANGE_MS.0 as i64,
            max: utterance::MAX_UTTERANCE_RANGE_MS.1 as i64,
            default: defaults.max_utterance_ms as i64,
        },
    }
}

/// Everything said so far, oldest first.
#[tauri::command]
pub fn load_transcripts() -> Result<Vec<Transcript>, String> {
    transcripts::load_all().map_err(|error| error.to_string())
}

#[tauri::command]
pub fn clear_transcripts() -> Result<(), String> {
    transcripts::clear().map_err(|error| error.to_string())
}

/// Where the history file lives, so the history page can show it.
#[tauri::command]
pub fn transcripts_file_path() -> Result<String, String> {
    transcripts::transcripts_path()
        .map(|path| path.display().to_string())
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn bot_status(state: State<'_, AppState>) -> Result<BotStatus, String> {
    Ok(state.status.lock().await.clone())
}

/// Starts the bot with whatever is in the settings file right now.
#[tauri::command]
pub async fn start_bot(app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    let mut running_bot = state.bot.lock().await;

    if running_bot.is_some() {
        return Err("The bot is already running.".to_string());
    }

    let settings = settings::load().map_err(|error| error.to_string())?;
    let missing = settings.missing_requirements();

    if !missing.is_empty() {
        // Say what is missing rather than letting the login fail with something vague.
        return Err(format!(
            "Add your {} on the Settings page.",
            missing.join(" and ")
        ));
    }

    set_status(&app, BotStatus::Starting).await;
    tracing::info!("starting the bot");

    // Read before the fields below are moved out of settings.
    let tuning = settings.detector_tuning();

    let config = BotConfig {
        discord_bot_token: settings.discord_bot_token,
        openai_api_key: settings.openai_api_key,
        microphone_name: settings.microphone_name,
        tts_voice: settings.tts_voice,
        tuning,
        noise_suppression: settings.noise_suppression,
    };

    let reporters = build_reporters(app.clone());

    match discord::start(config, reporters).await {
        Ok(handle) => {
            *running_bot = Some(handle);
            set_status(&app, BotStatus::Online).await;
            Ok(())
        }
        Err(error) => {
            let reason = error.to_string();
            tracing::error!("could not start the bot: {reason}");
            set_status(&app, BotStatus::Failed(reason.clone())).await;
            Err(reason)
        }
    }
}

#[tauri::command]
pub async fn stop_bot(app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    let mut running_bot = state.bot.lock().await;

    let Some(handle) = running_bot.take() else {
        return Ok(());
    };

    handle.stop().await;
    tracing::info!("the bot has stopped");

    set_status(&app, BotStatus::Offline).await;
    // Nothing is listening now, so leave the meter empty rather than frozen mid-reading.
    let _webview_listening = app.emit(LEVEL_EVENT, 0.0_f32);

    Ok(())
}

/// Hands the session ways to push at the UI without the audio or Discord code knowing
/// what Tauri is.
fn build_reporters(app: AppHandle) -> discord::session::SessionReporters {
    let level_app = app.clone();
    let transcript_app = app.clone();

    discord::session::SessionReporters {
        report_level: Arc::new(move |level: f32| {
            let _webview_listening = level_app.emit(LEVEL_EVENT, level);
        }),
        report_transcript: Arc::new(move |transcript: Transcript| {
            let _webview_listening = transcript_app.emit(TRANSCRIPT_EVENT, transcript);
        }),
        report_session_active: Arc::new(move |is_active: bool| {
            let _webview_listening = app.emit(SESSION_EVENT, is_active);
        }),
    }
}
