//! The Discord half: logging in, handling the slash commands, and holding the voice
//! sessions the commands start and stop.

pub mod commands;
pub mod session;

use std::collections::HashMap;
use std::sync::Arc;

use serenity::all::{GatewayIntents, GuildId, Interaction, Ready};
use serenity::client::{Context, EventHandler};
use serenity::prelude::TypeMapKey;
use serenity::{async_trait, Client};
use songbird::SerenityInit;
use tokio::sync::Mutex;

use crate::openai::OpenAiClient;
use session::{SessionActiveReporter, SessionReporters, VoiceSession};

/// Everything the bot needs to run, read from the settings when it is started.
#[derive(Debug, Clone)]
pub struct BotConfig {
    pub discord_bot_token: String,
    pub openai_api_key: String,
    pub microphone_name: String,
    pub tts_voice: String,
    pub tuning: crate::audio::utterance::DetectorTuning,
    pub noise_suppression: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum BotError {
    #[error("the OpenAI key is not usable: {0}")]
    OpenAi(#[from] crate::openai::OpenAiError),

    #[error("could not start the Discord client: {0}")]
    Client(#[source] serenity::Error),

    #[error("Discord rejected the bot token")]
    BadToken,
}

/// The sessions currently running, one per guild. A guild can only have the bot in one
/// voice channel at a time.
///
/// Every change reports whether any session remains, so the UI's "listening" state stays
/// true no matter which path started or ended a session — join, leave, or shutdown.
pub struct SessionRegistry {
    sessions: Mutex<HashMap<GuildId, VoiceSession>>,
    report_active: SessionActiveReporter,
}

impl SessionRegistry {
    pub fn new(report_active: SessionActiveReporter) -> Self {
        Self {
            sessions: Mutex::new(HashMap::new()),
            report_active,
        }
    }

    /// Replaces any session already running in the guild, which also closes its
    /// microphone.
    pub async fn insert(&self, guild_id: GuildId, session: VoiceSession) {
        let mut sessions = self.sessions.lock().await;
        sessions.insert(guild_id, session);
        (self.report_active)(!sessions.is_empty());
    }

    /// Ends the guild's session, if one was running. True when there was something to
    /// stop.
    pub async fn remove(&self, guild_id: GuildId) -> bool {
        let mut sessions = self.sessions.lock().await;
        let was_running = sessions.remove(&guild_id).is_some();
        (self.report_active)(!sessions.is_empty());
        was_running
    }

    pub async fn is_running(&self, guild_id: GuildId) -> bool {
        self.sessions.lock().await.contains_key(&guild_id)
    }

    pub async fn clear(&self) {
        self.sessions.lock().await.clear();
        (self.report_active)(false);
    }
}

/// Shared state reachable from the command handlers.
pub struct BotState {
    pub config: BotConfig,
    pub openai_client: OpenAiClient,
    pub sessions: Arc<SessionRegistry>,
    pub reporters: SessionReporters,
}

impl TypeMapKey for BotState {
    type Value = Arc<BotState>;
}

/// A running bot. Dropping this does not stop it — call [`BotHandle::stop`].
pub struct BotHandle {
    shard_manager: Arc<serenity::gateway::ShardManager>,
    sessions: Arc<SessionRegistry>,
}

impl BotHandle {
    /// Disconnects from Discord and ends every voice session, releasing the microphone.
    pub async fn stop(&self) {
        // Sessions first: this closes the microphone. Doing it after the shutdown would
        // leave the mic open for as long as the shard manager took to wind down.
        self.sessions.clear().await;
        self.shard_manager.shutdown_all().await;
    }
}

/// Logs in and starts handling commands.
///
/// Returns once the client is connected, with a handle for stopping it. The client
/// itself keeps running on a background task.
pub async fn start(config: BotConfig, reporters: SessionReporters) -> Result<BotHandle, BotError> {
    let openai_client = OpenAiClient::new(&config.openai_api_key)?;
    let sessions = Arc::new(SessionRegistry::new(
        reporters.report_session_active.clone(),
    ));

    let state = Arc::new(BotState {
        config: config.clone(),
        openai_client,
        sessions: Arc::clone(&sessions),
        reporters,
    });

    // GUILD_VOICE_STATES is what lets songbird track the bot's own connection. The
    // message intents Soul-GPT asked for are not needed: this bot reads the microphone,
    // not the chat.
    let intents = GatewayIntents::GUILDS | GatewayIntents::GUILD_VOICE_STATES;

    let mut client = Client::builder(&config.discord_bot_token, intents)
        .event_handler(Handler)
        .register_songbird()
        .await
        .map_err(map_login_error)?;

    client.data.write().await.insert::<BotState>(state);

    let shard_manager = Arc::clone(&client.shard_manager);

    tokio::spawn(async move {
        if let Err(error) = client.start().await {
            tracing::error!("the Discord client stopped: {error}");
        }
    });

    Ok(BotHandle {
        shard_manager,
        sessions,
    })
}

/// Turns serenity's login failure into something the console can say plainly, since a
/// bad token is the most likely reason to land here.
fn map_login_error(error: serenity::Error) -> BotError {
    let is_unauthorized = matches!(
        &error,
        serenity::Error::Http(serenity::http::HttpError::UnsuccessfulRequest(response))
            if response.status_code == serenity::http::StatusCode::UNAUTHORIZED
    );

    if is_unauthorized {
        return BotError::BadToken;
    }

    BotError::Client(error)
}

struct Handler;

#[async_trait]
impl EventHandler for Handler {
    async fn ready(&self, context: Context, ready: Ready) {
        tracing::info!("logged in as {}", ready.user.name);

        commands::register(&context).await;

        tracing::info!("ready — use /join in a voice channel to start");
    }

    async fn interaction_create(&self, context: Context, interaction: Interaction) {
        let Interaction::Command(command) = interaction else {
            return;
        };

        commands::dispatch(&context, &command).await;
    }
}
