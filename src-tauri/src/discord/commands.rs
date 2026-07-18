//! The slash commands, ported from Soul-GPT's `src/commands`.

use std::sync::Arc;

use serenity::all::{
    ChannelId, ChannelType, CommandDataOptionValue, CommandInteraction, CommandOptionType,
    CreateCommand, CreateCommandOption, EditInteractionResponse, GuildId,
};
use serenity::client::Context;
use songbird::{CoreEvent, Event, EventContext, EventHandler as VoiceEventHandler, Songbird};

use super::session::{self, SessionConfig};
use super::{BotState, SessionRegistry};
use crate::openai::tts::AVAILABLE_TTS_VOICES;

/// Tells Discord which commands exist. Registered globally, so they work in every server
/// the bot is in.
pub async fn register(context: &Context) {
    let commands = vec![
        CreateCommand::new("join")
            .description("Join a voice channel and start speaking for you")
            .add_option(
                CreateCommandOption::new(
                    CommandOptionType::Channel,
                    "channel",
                    "The voice channel to join (defaults to the one you are in)",
                )
                .channel_types(vec![ChannelType::Voice])
                .required(false),
            ),
        CreateCommand::new("leave").description("Leave the voice channel and stop listening"),
        CreateCommand::new("voice")
            .description("Choose the voice the bot speaks in")
            .add_option(build_voice_option()),
        CreateCommand::new("ping").description("Check the bot is alive"),
    ];

    if let Err(error) = serenity::all::Command::set_global_commands(&context.http, commands).await {
        tracing::error!("could not register the slash commands: {error}");
        return;
    }

    tracing::info!("slash commands registered");
}

/// Builds the voice picker from the one list of voices, so a voice added there shows up
/// here without another edit.
fn build_voice_option() -> CreateCommandOption {
    let mut option = CreateCommandOption::new(
        CommandOptionType::String,
        "voice",
        "Which voice to speak in",
    )
    .required(true);

    for voice in AVAILABLE_TTS_VOICES {
        option = option.add_string_choice(voice, voice);
    }

    option
}

pub async fn dispatch(context: &Context, command: &CommandInteraction) {
    // Discord gives an interaction THREE SECONDS to be acknowledged, or it tells the user
    // the app did not respond and throws the token away. /join blows through that on its
    // own: connecting to the voice gateway and opening the microphone both take longer.
    //
    // So acknowledge first and do the work second. Deferring buys 15 minutes to reply,
    // and shows the user a "thinking" state in the meantime. Every command defers, not
    // just the slow ones — a command that is fast today is one edit away from not being.
    if !defer(context, command).await {
        // Without the deferral there is no token to reply with, so the work would have
        // nowhere to report to.
        return;
    }

    let outcome = match command.data.name.as_str() {
        "join" => handle_join(context, command).await,
        "leave" => handle_leave(context, command).await,
        "voice" => handle_voice(context, command).await,
        "ping" => Ok("Pong!".to_string()),
        unknown => {
            tracing::warn!("ignoring unknown command /{unknown}");
            Err("I do not know that command.".to_string())
        }
    };

    let reply = match outcome {
        Ok(message) => message,
        Err(message) => {
            tracing::warn!("/{} failed: {message}", command.data.name);
            message
        }
    };

    respond(context, command, &reply).await;
}

/// Acknowledges the interaction before doing any work. Returns whether it landed.
///
/// Ephemeral, so the reply is only shown to whoever ran the command rather than being
/// chat for the whole channel.
async fn defer(context: &Context, command: &CommandInteraction) -> bool {
    let Err(error) = command.defer_ephemeral(&context.http).await else {
        return true;
    };

    // Nearly always means the three seconds already elapsed — the bot was busy or the
    // gateway lagged.
    tracing::error!(
        "could not acknowledge /{} in time: {error}",
        command.data.name
    );

    false
}

/// Fills in the reply promised by the deferral.
async fn respond(context: &Context, command: &CommandInteraction, message: &str) {
    let response = EditInteractionResponse::new().content(message);

    if let Err(error) = command.edit_response(&context.http, response).await {
        tracing::error!("could not reply to /{}: {error}", command.data.name);
    }
}

/// The error case is a message to show the user, not a failure to swallow.
type CommandResult = Result<String, String>;

async fn handle_join(context: &Context, command: &CommandInteraction) -> CommandResult {
    let Some(guild_id) = command.guild_id else {
        return Err("This only works in a server.".to_string());
    };

    let state = bot_state(context).await?;
    let voice_channel_id = resolve_voice_channel(context, command, guild_id)?;

    let songbird_manager = songbird::get(context)
        .await
        .ok_or_else(|| "Voice support is not running.".to_string())?;

    let call = songbird_manager
        .join(guild_id, voice_channel_id)
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
                manager: Arc::clone(&songbird_manager),
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
    tracing::info!("joined voice channel {voice_channel_id}");

    Ok("Joined. Talk into your microphone and I will speak for you.".to_string())
}

/// Works out which channel to join: the one asked for, or the one the caller is sitting
/// in.
fn resolve_voice_channel(
    context: &Context,
    command: &CommandInteraction,
    guild_id: GuildId,
) -> Result<ChannelId, String> {
    let requested_channel = command
        .data
        .options
        .first()
        .and_then(|option| match option.value {
            CommandDataOptionValue::Channel(channel_id) => Some(channel_id),
            _ => None,
        });

    if let Some(channel_id) = requested_channel {
        return Ok(channel_id);
    }

    let guild = context
        .cache
        .guild(guild_id)
        .ok_or_else(|| "I cannot see that server.".to_string())?;

    let caller_channel = guild
        .voice_states
        .get(&command.user.id)
        .and_then(|voice_state| voice_state.channel_id);

    caller_channel.ok_or_else(|| {
        "Join a voice channel first, or name one: /join channel:#general".to_string()
    })
}

async fn handle_leave(context: &Context, command: &CommandInteraction) -> CommandResult {
    let Some(guild_id) = command.guild_id else {
        return Err("This only works in a server.".to_string());
    };

    let state = bot_state(context).await?;

    // Ending the session first closes the microphone; leaving the channel after keeps
    // the mic from staying live if the disconnect is slow.
    let was_running = state.sessions.remove(guild_id).await;

    let songbird_manager = songbird::get(context)
        .await
        .ok_or_else(|| "Voice support is not running.".to_string())?;

    if let Err(error) = songbird_manager.remove(guild_id).await {
        tracing::warn!("could not leave the channel cleanly: {error}");
    }

    if !was_running {
        return Ok("I was not in a voice channel.".to_string());
    }

    tracing::info!("left the voice channel");

    Ok("Left the channel.".to_string())
}

async fn handle_voice(context: &Context, command: &CommandInteraction) -> CommandResult {
    let chosen_voice = command
        .data
        .options
        .first()
        .and_then(|option| option.value.as_str())
        .ok_or_else(|| "No voice given.".to_string())?;

    if !crate::openai::tts::is_known_voice(chosen_voice) {
        return Err(format!("{chosen_voice} is not a voice I know."));
    }

    // The voice lives in the settings file, which the app owns. Changing it here would
    // only last until the bot restarted, so say where the real switch is rather than
    // pretending it worked.
    Ok(format!(
        "Voices are set in the Butter TTS app, on the Settings page. It is currently \
         set to {}. Pick {chosen_voice} there and restart the bot.",
        current_voice(context).await
    ))
}

async fn current_voice(context: &Context) -> String {
    let Ok(state) = bot_state(context).await else {
        return "unknown".to_string();
    };

    state.config.tts_voice.clone()
}

async fn bot_state(context: &Context) -> Result<Arc<BotState>, String> {
    let data = context.data.read().await;

    data.get::<BotState>()
        .cloned()
        .ok_or_else(|| "The bot is not set up properly.".to_string())
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

        // A `None` reason is a leave or channel-move we asked for — handle_leave has
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
