//! Butter TTS: a portable desktop app that speaks for you in a Discord voice channel.
//!
//! Your microphone is transcribed and re-spoken in a synthetic voice, so the channel
//! hears the synthesised reading rather than your actual voice.

pub mod audio;
pub mod bridge;
pub mod discord;
pub mod logging;
pub mod openai;
pub mod store;

use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::EnvFilter;

/// Hides this crate's own logs behind the noise of every dependency. Without a filter
/// the console fills with serenity and reqwest internals.
const DEFAULT_LOG_FILTER: &str = "butter_tts_lib=info,warn";

#[derive(Debug, thiserror::Error)]
pub enum StartupError {
    #[error("could not start the app window: {0}")]
    Tauri(#[from] tauri::Error),
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() -> Result<(), StartupError> {
    install_crypto_provider();

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(bridge::AppState::default())
        .setup(|app| {
            install_logging(app.handle().clone());
            tracing::info!("Butter TTS ready");
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            bridge::needs_setup,
            bridge::complete_setup,
            bridge::list_microphones,
            bridge::default_microphone,
            bridge::list_voices,
            bridge::load_settings,
            bridge::save_settings,
            bridge::database_path,
            bridge::tuning_ranges,
            bridge::load_transcripts,
            bridge::clear_transcripts,
            bridge::bot_status,
            bridge::start_bot,
            bridge::stop_bot,
        ])
        .run(tauri::generate_context!())?;

    Ok(())
}

/// Picks the TLS backend before anything makes an HTTPS request.
///
/// serenity and songbird bring rustls with the `ring` backend; reqwest brings it with
/// `aws-lc-rs`. With both compiled in, rustls will not guess which to use and panics on
/// the first handshake — which is deep inside the Discord connection, so the app starts
/// fine and only dies when the bot tries to connect. Choosing `ring` here, once, settles
/// it for the whole process.
fn install_crypto_provider() {
    let installed = rustls::crypto::ring::default_provider().install_default();

    // An Err means a provider was already installed, which is the outcome we wanted.
    if installed.is_err() {
        tracing::debug!("a TLS crypto provider was already installed");
    }
}

/// Sends this crate's log lines to the console page.
///
/// Failing here costs the console page its contents, not the app, so it reports and
/// carries on rather than refusing to open the window.
fn install_logging(app: tauri::AppHandle) {
    let filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(DEFAULT_LOG_FILTER));

    let installed = tracing_subscriber::registry()
        .with(filter)
        .with(logging::WebviewLayer::new(app))
        .try_init();

    if let Err(error) = installed {
        eprintln!("the console page will be empty: could not install logging: {error}");
    }
}
