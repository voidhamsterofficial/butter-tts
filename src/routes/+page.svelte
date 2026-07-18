<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { goto } from "$app/navigation";
  import { bot, describeStatus, statusTone, type GuildVoiceChannels } from "$lib/bot.svelte";
  import type { Settings } from "$lib/settings";
  import Play from "phosphor-svelte/lib/Play";
  import Stop from "phosphor-svelte/lib/Stop";
  import MicrophoneStage from "phosphor-svelte/lib/MicrophoneStage";
  import MusicNotes from "phosphor-svelte/lib/MusicNotes";
  import WarningCircle from "phosphor-svelte/lib/WarningCircle";
  import ArrowRight from "phosphor-svelte/lib/ArrowRight";
  import DoorOpen from "phosphor-svelte/lib/DoorOpen";
  import SignOut from "phosphor-svelte/lib/SignOut";
  import UsersThree from "phosphor-svelte/lib/UsersThree";
  import SpeakerHigh from "phosphor-svelte/lib/SpeakerHigh";
  import ArrowsClockwise from "phosphor-svelte/lib/ArrowsClockwise";

  let settings = $state<Settings | null>(null);
  // Errors from the wake/sleep button live with that button; errors from picking a
  // channel live with the picker. Keeping them apart means each card shows only what
  // went wrong there.
  let actionError = $state<string | null>(null);
  let channelError = $state<string | null>(null);

  // The channel picker is a two-step choice: a server first, then one of its voice
  // channels. Both are held as Discord IDs; the names are only ever for display.
  let voiceGuilds = $state<GuildVoiceChannels[]>([]);
  let selectedGuildId = $state("");
  let selectedChannelId = $state("");
  let channelsLoading = $state(false);

  onMount(async () => {
    try {
      settings = await invoke<Settings>("load_settings");
    } catch (error) {
      actionError = String(error);
    }
  });

  const tone = $derived(statusTone(bot.status, bot.inChannel, bot.isSpeaking));
  const statusText = $derived(describeStatus(bot.status, bot.inChannel, bot.isSpeaking));
  const isOnline = $derived(bot.status.state === "online");

  // Pressing start with no tokens would only fail at the Discord login. Say so up front.
  const missingSettings = $derived.by(() => {
    if (settings === null) {
      return [];
    }

    const missing: string[] = [];

    if (settings.openai_api_key.trim() === "") {
      missing.push("OpenAI key");
    }

    if (settings.discord_bot_token.trim() === "") {
      missing.push("Discord token");
    }

    return missing;
  });

  const canStart = $derived(missingSettings.length === 0);

  // The ripple tracks your voice, so you can see it hearing you.
  const rippleScale = $derived(1 + Math.min(bot.inputLevel * 2.2, 1) * 0.55);
  const rippleOpacity = $derived(Math.min(bot.inputLevel * 3, 1) * 0.85);

  const hint = $derived.by(() => {
    if (!canStart) {
      return "Pop your keys into Settings and I will be ready to go!";
    }

    if (bot.status.state === "failed") {
      return "Something went wrong — the Console page has the whole story.";
    }

    if (bot.status.state === "starting") {
      return "Waking up and saying hello to Discord…";
    }

    if (bot.status.state === "reconnecting") {
      return "I lost my connection to Discord and I am getting it back…";
    }

    if (isOnline && bot.inChannel) {
      return "I am in the channel and listening! Everyone hears my voice, never your microphone.";
    }

    if (isOnline) {
      return "I am awake and connected — pick a voice channel to hop into.";
    }

    return "Tap me to wake up, then pick a voice channel to hop into.";
  });

  const microphoneLabel = $derived(
    settings === null || settings.microphone_name === "" ? "Default mic" : settings.microphone_name,
  );

  const selectedGuild = $derived(
    voiceGuilds.find((guild) => guild.guildId === selectedGuildId) ?? null,
  );
  const guildChannels = $derived(selectedGuild?.channels ?? []);
  const canJoin = $derived(!bot.isBusy && selectedChannelId !== "");

  // The picker only makes sense once the bot is actually connected to Discord — that is
  // when its guild cache has anything to list. Refresh on connect, clear on disconnect.
  $effect(() => {
    if (isOnline) {
      refreshVoiceChannels();
    } else {
      resetChannelPicker();
    }
  });

  async function refreshVoiceChannels() {
    channelsLoading = true;
    channelError = null;

    // The list is read from an in-memory cache, so it comes back almost instantly — too
    // fast to see the spinner. A short floor keeps the click feeling like it did
    // something rather than flickering.
    const spinFloor = new Promise((resolve) => setTimeout(resolve, 400));

    try {
      const [guilds] = await Promise.all([bot.listVoiceChannels(), spinFloor]);
      voiceGuilds = guilds;
      // One server is the common case — pick it so only the channel is left to choose.
      if (voiceGuilds.length === 1) {
        selectGuild(voiceGuilds[0].guildId);
      }
    } catch (error) {
      channelError = String(error);
    } finally {
      channelsLoading = false;
    }
  }

  function resetChannelPicker() {
    voiceGuilds = [];
    selectedGuildId = "";
    selectedChannelId = "";
  }

  // Switching server clears the channel — a channel picked under the old server is not a
  // valid choice under the new one.
  function selectGuild(guildId: string) {
    selectedGuildId = guildId;
    selectedChannelId = "";
  }

  async function handleToggle() {
    actionError = await bot.toggle();
  }

  async function handleJoin() {
    if (!canJoin) {
      return;
    }

    channelError = await bot.joinChannel(selectedGuildId, selectedChannelId);
  }

  async function handleLeave() {
    channelError = await bot.leaveChannel();
  }
</script>

<div class="page__head">
  <h1 class="page__title">Hiya!</h1>
  <p class="page__subtitle">Your voice, re-spoken by a friend in Discord.</p>
</div>

<section class="card stage">
  <span class="status" data-tone={tone}>
    <span class="status__dot"></span>
    {statusText}
  </span>

  <div class="blob" data-tone={tone}>
    <span
      class="blob__ripple"
      style="transform: scale({rippleScale}); opacity: {rippleOpacity};"
      aria-hidden="true"
    ></span>

    <button
      class="blob__button"
      onclick={handleToggle}
      disabled={bot.isBusy || (!canStart && !bot.isRunning)}
      aria-label={bot.isRunning ? "Send the bot to sleep" : "Wake the bot up"}
    >
      <span class="face">
        <span class="face__eyes">
          <span class="face__eye"></span>
          <span class="face__eye"></span>
        </span>
        <span class="face__mouth"></span>
      </span>
      <span class="face__blush" aria-hidden="true">
        <span class="face__cheek"></span>
        <span class="face__cheek"></span>
      </span>
    </button>
  </div>

  <button
    class="button"
    onclick={handleToggle}
    disabled={bot.isBusy || (!canStart && !bot.isRunning)}
  >
    {#if bot.isRunning}
      <Stop size={16} weight="fill" />
      Go to sleep
    {:else}
      <Play size={16} weight="fill" />
      {bot.isBusy ? "One moment…" : "Wake up"}
    {/if}
  </button>

  <p class="stage__hint">{hint}</p>

  <div class="stage__meta">
    <span class="chip">
      <MicrophoneStage size={15} weight="duotone" />
      <span class="chip__label">Mic</span>
      {microphoneLabel}
    </span>
    <span class="chip">
      <MusicNotes size={15} weight="duotone" />
      <span class="chip__label">Voice</span>
      {settings?.tts_voice ?? "—"}
    </span>
  </div>

  {#if !canStart && settings !== null}
    <p class="notice">
      <WarningCircle size={17} weight="duotone" />
      I still need your {missingSettings.join(" and ")}.
      <button class="button button--ghost button--small" onclick={() => goto("/settings")}>
        Settings
        <ArrowRight size={13} weight="bold" />
      </button>
    </p>
  {/if}

  {#if bot.status.state === "failed"}
    <p class="notice" data-tone="error">
      <WarningCircle size={17} weight="fill" />
      {bot.status.detail}
    </p>
  {:else if actionError !== null}
    <p class="notice" data-tone="error">
      <WarningCircle size={17} weight="fill" />
      {actionError}
    </p>
  {/if}
</section>

{#if isOnline}
  <section class="card channels">
    <div class="channels__head">
      <span class="channels__title">
        <DoorOpen size={18} weight="duotone" />
        {bot.inChannel ? "In a voice channel" : "Join a voice channel"}
      </span>

      {#if !bot.inChannel}
        <button
          class="channels__refresh"
          data-loading={channelsLoading}
          onclick={refreshVoiceChannels}
          disabled={channelsLoading}
          aria-label="Refresh the list of servers and channels"
        >
          <ArrowsClockwise size={15} weight="bold" />
        </button>
      {/if}
    </div>

    {#if bot.inChannel}
      <p class="channels__note">
        I am in and listening — everyone hears my voice, never your microphone.
      </p>
      <button class="button button--ghost" onclick={handleLeave} disabled={bot.isBusy}>
        <SignOut size={16} weight="duotone" />
        Leave the channel
      </button>
    {:else if channelsLoading && voiceGuilds.length === 0}
      <p class="channels__note">Peeking at your servers…</p>
    {:else if voiceGuilds.length === 0}
      <p class="channels__note">
        I cannot see any servers yet. Make sure I am invited to one, then refresh.
      </p>
    {:else}
      <div class="channels__pickers">
        <div class="field">
          <span class="field__label">
            <UsersThree size={15} weight="duotone" />
            Server
          </span>
          <div class="field__row">
            <select
              class="field__select"
              value={selectedGuildId}
              onchange={(event) => selectGuild(event.currentTarget.value)}
            >
              <option value="" disabled>Pick a server</option>
              {#each voiceGuilds as guild (guild.guildId)}
                <option value={guild.guildId}>{guild.guildName}</option>
              {/each}
            </select>
          </div>
        </div>

        <div class="field">
          <span class="field__label">
            <SpeakerHigh size={15} weight="duotone" />
            Channel
          </span>
          <div class="field__row" data-disabled={selectedGuildId === ""}>
            <select
              class="field__select"
              bind:value={selectedChannelId}
              disabled={selectedGuildId === "" || guildChannels.length === 0}
            >
              <option value="" disabled>
                {#if selectedGuildId === ""}
                  Pick a server first
                {:else if guildChannels.length === 0}
                  No voice channels here
                {:else}
                  Pick a channel
                {/if}
              </option>
              {#each guildChannels as channel (channel.id)}
                <option value={channel.id}>{channel.name}</option>
              {/each}
            </select>
          </div>
        </div>
      </div>

      <button class="button channels__join" onclick={handleJoin} disabled={!canJoin}>
        <DoorOpen size={16} weight="duotone" />
        Join and start listening
      </button>
    {/if}

    {#if channelError !== null}
      <p class="notice" data-tone="error">
        <WarningCircle size={17} weight="fill" />
        {channelError}
      </p>
    {/if}
  </section>
{/if}
