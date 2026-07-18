//! Joining and leaving voice channels. Driven entirely by the app — there is no Discord
//! command asking for it, so this is plain functions the bridge can call once it has a
//! guild and channel the user picked.
//!
//! These work off the [`Cache`] and [`Songbird`] handles grabbed at startup rather than a
//! serenity `Context`: join/leave are not answering an interaction, so there is no context
//! to hand them, and storing one would form a reference cycle through the client's data.

use std::sync::Arc;

use serde::Serialize;
use serenity::all::{Cache, ChannelId, ChannelType, GuildId};
use songbird::{CoreEvent, Event, EventContext, EventHandler as VoiceEventHandler, Songbird};

use super::session::{self, SessionConfig};
use super::{BotState, SessionRegistry};

/// One server the bot can see, and its voice channels, for the app's channel picker.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GuildVoiceChannels {
    pub guild_id: String,
    pub guild_name: String,
    pub channels: Vec<VoiceChannelInfo>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VoiceChannelInfo {
    pub id: String,
    pub name: String,
}

/// Every voice channel the bot can see, grouped by server and sorted by name so the
/// picker reads the same way every time.
pub fn list_voice_channels(cache: &Cache) -> Vec<GuildVoiceChannels> {
    let mut guilds: Vec<GuildVoiceChannels> = cache
        .guilds()
        .into_iter()
        .filter_map(|guild_id| describe_guild_channels(cache, guild_id))
        .collect();

    guilds.sort_by(|a, b| a.guild_name.cmp(&b.guild_name));
    guilds
}

/// The one guild's voice channels, or `None` if it has dropped out of the cache since its
/// id was listed.
fn describe_guild_channels(cache: &Cache, guild_id: GuildId) -> Option<GuildVoiceChannels> {
    let guild = cache.guild(guild_id)?;

    let mut channels: Vec<VoiceChannelInfo> = guild
        .channels
        .values()
        .filter(|channel| channel.kind == ChannelType::Voice)
        .map(|channel| VoiceChannelInfo {
            id: channel.id.to_string(),
            name: channel.name.clone(),
        })
        .collect();
    channels.sort_by(|a, b| a.name.cmp(&b.name));

    Some(GuildVoiceChannels {
        guild_id: guild_id.to_string(),
        guild_name: guild.name.clone(),
        channels,
    })
}

/// Joins a voice channel and starts listening to the microphone, speaking what it hears
/// back in the chosen TTS voice.
pub async fn join_channel(
    songbird: &Arc<Songbird>,
    state: &Arc<BotState>,
    guild_id: GuildId,
    channel_id: ChannelId,
) -> Result<(), String> {
    let call = songbird
        .join(guild_id, channel_id)
        .await
        .map_err(|error| format!("Could not join that channel: {error}"))?;

    // Clean up if the voice link drops for good. songbird reconnects transient blips on
    // its own and only fires this once it has given up, so reaching here means the
    // session is genuinely dead and the microphone should be released.
    {
        let mut locked_call = call.lock().await;
        locked_call.add_global_event(
            Event::Core(CoreEvent::DriverDisconnect),
            VoiceDisconnectHandler {
                guild_id,
                sessions: Arc::clone(&state.sessions),
                manager: Arc::clone(songbird),
            },
        );
    }

    let session_config = SessionConfig {
        microphone_name: state.config.microphone_name.clone(),
        tts_voice: state.config.tts_voice.clone(),
        tuning: state.config.tuning,
        noise_suppression: state.config.noise_suppression,
    };

    let session = session::start_session(
        call,
        state.openai_client.clone(),
        session_config,
        state.reporters.clone(),
    )
    .await
    .map_err(|error| {
        // Joined but deaf is not worth staying for.
        tracing::error!("could not open the microphone: {error}");
        format!("Could not open the microphone: {error}")
    })?;

    state.sessions.insert(guild_id, session).await;
    tracing::info!("joined voice channel {channel_id}");

    Ok(())
}

/// Leaves a voice channel and stops listening.
pub async fn leave_channel(
    songbird: &Arc<Songbird>,
    state: &Arc<BotState>,
    guild_id: GuildId,
) -> Result<(), String> {
    // Ending the session first closes the microphone; leaving the channel after keeps
    // the mic from staying live if the disconnect is slow.
    state.sessions.remove(guild_id).await;

    if let Err(error) = songbird.remove(guild_id).await {
        tracing::warn!("could not leave the channel cleanly: {error}");
    }

    tracing::info!("left the voice channel");

    Ok(())
}

/// Tears a session down when its voice connection drops for good, so a dropped call does
/// not leave the microphone open with nothing listening to it.
struct VoiceDisconnectHandler {
    guild_id: GuildId,
    sessions: Arc<SessionRegistry>,
    manager: Arc<Songbird>,
}

#[serenity::async_trait]
impl VoiceEventHandler for VoiceDisconnectHandler {
    async fn act(&self, context: &EventContext<'_>) -> Option<Event> {
        let EventContext::DriverDisconnect(data) = context else {
            return None;
        };

        // A `None` reason is a leave or channel-move we asked for — leave_channel has
        // already tidied up. A reason means songbird exhausted its reconnect attempts,
        // so the session is dead and needs clearing away.
        let Some(reason) = &data.reason else {
            return None;
        };

        tracing::warn!("voice connection dropped for good: {reason:?}");

        // Done off the event task rather than awaited inline: removing the call talks
        // back to the same driver that is firing this event, so blocking here to wait on
        // it would be asking the driver to unwind itself mid-callback.
        let guild_id = self.guild_id;
        let sessions = Arc::clone(&self.sessions);
        let manager = Arc::clone(&self.manager);
        tokio::spawn(async move {
            sessions.remove(guild_id).await;
            if let Err(error) = manager.remove(guild_id).await {
                tracing::warn!("could not clean up the dropped call: {error}");
            }
        });

        None
    }
}
