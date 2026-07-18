//! The Discord half: logging in, and holding the voice sessions that the app's join/leave
//! commands start and stop. There are no Discord-side commands — the app is the only
//! thing that decides which channel to join or leave.

pub mod session;
pub mod voice;

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use serenity::all::{
    Cache, ChannelId, ConnectionStage, GatewayIntents, GuildId, Ready, ResumedEvent,
    ShardStageUpdateEvent,
};
use serenity::client::{Context, EventHandler};
use serenity::prelude::TypeMapKey;
use serenity::{async_trait, Client};
use songbird::{SerenityInit, Songbird};
use tokio::sync::Mutex;

use crate::openai::OpenAiClient;
use session::{SessionActiveReporter, SessionReporters, VoiceSession};

/// How the bot's link to Discord is doing, reported to the UI as it changes.
///
/// Logging in is not instant and can fail or drop long after [`start`] returns, so the
/// status the user sees is driven by these events rather than by whether the client task
/// was spawned. See [`ConnectionReporter`].
#[derive(Debug, Clone)]
pub enum ConnectionEvent {
    /// The gateway is up: either freshly logged in, or resumed after a blip.
    Ready,
    /// The link dropped and serenity is trying to restore it. Serenity reconnects on its
    /// own; this only surfaces that it is happening.
    Reconnecting,
    /// The client loop exited and will not retry — most often a rejected token. Carries
    /// the reason for the console and the dashboard.
    Lost(String),
}

/// Reports a [`ConnectionEvent`] to the UI. Threaded in from the bridge, so the Discord
/// code can say how the connection is doing without knowing what Tauri is.
pub type ConnectionReporter = Arc<dyn Fn(ConnectionEvent) + Send + Sync>;

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

    #[error("voice support failed to start")]
    VoiceUnavailable,
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

    /// Every guild with a session currently running, so a channel-agnostic "leave" can
    /// find what to leave without the app having to remember which guild it joined.
    pub async fn active_guild_ids(&self) -> Vec<GuildId> {
        self.sessions.lock().await.keys().copied().collect()
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
    /// Reports gateway up/down to the UI. Held here so the event handler can reach it
    /// through the client's data map, the same way the rest of the state is reached.
    pub connection: ConnectionReporter,
}

impl TypeMapKey for BotState {
    type Value = Arc<BotState>;
}

/// A running bot. Dropping this does not stop it — call [`BotHandle::stop`].
pub struct BotHandle {
    shard_manager: Arc<serenity::gateway::ShardManager>,
    state: Arc<BotState>,
    /// The voice manager and cache, grabbed at startup. join/leave and the channel list
    /// need them without a serenity `Context` to reach them through — see [`voice`]. Kept
    /// on the handle rather than in [`BotState`] (which lives in the client's data map) so
    /// they do not form a reference cycle back through the client.
    songbird: Arc<Songbird>,
    cache: Arc<Cache>,
    /// Set before shutting the shard manager down, so the client task can tell an
    /// asked-for exit from a real failure and not report a spurious "connection lost"
    /// when the user simply pressed stop.
    stopping: Arc<AtomicBool>,
}

impl BotHandle {
    /// Disconnects from Discord and ends every voice session, releasing the microphone.
    pub async fn stop(&self) {
        self.stopping.store(true, Ordering::SeqCst);
        // Sessions first: this closes the microphone. Doing it after the shutdown would
        // leave the mic open for as long as the shard manager took to wind down.
        self.state.sessions.clear().await;
        self.shard_manager.shutdown_all().await;
    }

    /// Every server and voice channel the bot can see, for the app's channel picker.
    pub fn list_voice_channels(&self) -> Vec<voice::GuildVoiceChannels> {
        voice::list_voice_channels(&self.cache)
    }

    /// Joins a voice channel and starts listening, whichever channel the app picked.
    pub async fn join_channel(
        &self,
        guild_id: GuildId,
        channel_id: ChannelId,
    ) -> Result<(), String> {
        voice::join_channel(&self.songbird, &self.state, guild_id, channel_id).await
    }

    /// Leaves every voice channel currently joined. The app only ever has the bot in one
    /// channel at a time, so there is nothing to pick — just leave whatever is active.
    pub async fn leave_all_channels(&self) -> Result<(), String> {
        let guild_ids = self.state.sessions.active_guild_ids().await;

        for guild_id in guild_ids {
            voice::leave_channel(&self.songbird, &self.state, guild_id).await?;
        }

        Ok(())
    }
}

/// Logs in and starts handling commands.
///
/// Returns as soon as the client has been built and its loop spawned — which is *not*
/// the same as being connected. The gateway handshake and login happen on the background
/// task afterwards and can take a moment or fail outright, so the caller is told the bot
/// is only "starting"; [`ConnectionEvent::Ready`] is what confirms it is actually up.
pub async fn start(
    config: BotConfig,
    reporters: SessionReporters,
    report_connection: ConnectionReporter,
) -> Result<BotHandle, BotError> {
    let openai_client = OpenAiClient::new(&config.openai_api_key)?;
    let sessions = Arc::new(SessionRegistry::new(
        reporters.report_session_active.clone(),
    ));

    let state = Arc::new(BotState {
        config: config.clone(),
        openai_client,
        sessions: Arc::clone(&sessions),
        reporters,
        connection: Arc::clone(&report_connection),
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

    client
        .data
        .write()
        .await
        .insert::<BotState>(Arc::clone(&state));

    // Grabbed now, while the client is still here to read them from: the app drives
    // join/leave without a serenity Context, so the handle carries the voice manager and
    // the cache the way that code would otherwise reach for.
    let cache = Arc::clone(&client.cache);
    let songbird = songbird_manager(&client).await?;

    let shard_manager = Arc::clone(&client.shard_manager);
    let stopping = Arc::new(AtomicBool::new(false));
    let stopping_for_task = Arc::clone(&stopping);

    tokio::spawn(async move {
        // start() returns Ok on a clean shutdown_all() and Err on a fatal gateway
        // problem — a rejected token is the usual one, and it only shows up here because
        // the builder above accepts any well-formed token without checking it.
        let outcome = client.start().await;

        // A stop the user asked for is not a failure worth flagging; the bridge has
        // already moved the status to offline.
        if stopping_for_task.load(Ordering::SeqCst) {
            return;
        }

        match outcome {
            Ok(()) => {
                tracing::warn!("the Discord client stopped on its own");
                report_connection(ConnectionEvent::Lost(
                    "The connection to Discord closed.".to_string(),
                ));
            }
            Err(error) => {
                tracing::error!("the Discord client stopped: {error}");
                report_connection(ConnectionEvent::Lost(describe_client_exit(&error)));
            }
        }
    });

    Ok(BotHandle {
        shard_manager,
        state,
        songbird,
        cache,
        stopping,
    })
}

/// Pulls the voice manager out of the client's data map, where `register_songbird` put it.
/// It is always there after that call, so a miss means voice support genuinely failed to
/// install rather than an expected absence.
async fn songbird_manager(client: &Client) -> Result<Arc<Songbird>, BotError> {
    let data = client.data.read().await;

    data.get::<songbird::SongbirdKey>()
        .cloned()
        .ok_or(BotError::VoiceUnavailable)
}

/// A plain sentence for the most common reasons the client loop gives up, since the raw
/// error is aimed at a log, not a dashboard.
fn describe_client_exit(error: &serenity::Error) -> String {
    if is_unauthorized(error) {
        return "Discord rejected the bot token. Check it on the Settings page.".to_string();
    }

    format!("Lost the connection to Discord: {error}")
}

/// Turns serenity's login failure into something the console can say plainly, since a
/// bad token is the most likely reason to land here.
fn map_login_error(error: serenity::Error) -> BotError {
    if is_unauthorized(&error) {
        return BotError::BadToken;
    }

    BotError::Client(error)
}

/// Whether an error is Discord rejecting our credentials, as opposed to anything else.
fn is_unauthorized(error: &serenity::Error) -> bool {
    matches!(
        error,
        serenity::Error::Http(serenity::http::HttpError::UnsuccessfulRequest(response))
            if response.status_code == serenity::http::StatusCode::UNAUTHORIZED
    )
}

/// Reports a connection event through whatever reporter the bridge stored in the client
/// data. A no-op if the bot was somehow set up without one.
async fn report_connection(context: &Context, event: ConnectionEvent) {
    let data = context.data.read().await;
    if let Some(state) = data.get::<BotState>() {
        (state.connection)(event);
    }
}

struct Handler;

#[async_trait]
impl EventHandler for Handler {
    async fn ready(&self, context: Context, ready: Ready) {
        tracing::info!("logged in as {}", ready.user.name);
        tracing::info!("ready — waiting for the app to join a voice channel");
        report_connection(&context, ConnectionEvent::Ready).await;
    }

    /// Fired when a dropped gateway connection is picked back up without a full re-login.
    async fn resume(&self, context: Context, _event: ResumedEvent) {
        tracing::info!("reconnected to Discord");
        report_connection(&context, ConnectionEvent::Ready).await;
    }

    /// The shard's connection stage changed. Leaving the connected stage means the link
    /// dropped and serenity is working to restore it — worth telling the user, since a
    /// stall here is why the bot might briefly stop responding.
    async fn shard_stage_update(&self, context: Context, event: ShardStageUpdateEvent) {
        if event.old == ConnectionStage::Connected && event.new != ConnectionStage::Connected {
            tracing::warn!("lost the connection to Discord, reconnecting…");
            report_connection(&context, ConnectionEvent::Reconnecting).await;
        }
    }
}
