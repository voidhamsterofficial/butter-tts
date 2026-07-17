<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { goto } from "$app/navigation";
  import { bot, describeStatus, statusTone } from "$lib/bot.svelte";
  import type { Settings } from "$lib/settings";
  import Play from "phosphor-svelte/lib/Play";
  import Stop from "phosphor-svelte/lib/Stop";
  import MicrophoneStage from "phosphor-svelte/lib/MicrophoneStage";
  import MusicNotes from "phosphor-svelte/lib/MusicNotes";
  import WarningCircle from "phosphor-svelte/lib/WarningCircle";
  import ArrowRight from "phosphor-svelte/lib/ArrowRight";

  let settings = $state<Settings | null>(null);
  let actionError = $state<string | null>(null);

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

    if (isOnline && bot.inChannel) {
      return "I am in the channel and listening! Everyone hears my voice, never your microphone.";
    }

    if (isOnline) {
      return "I am awake and connected — type /join in a voice channel and I will start listening.";
    }

    return "Tap me to wake up, then type /join in a Discord voice channel.";
  });

  const microphoneLabel = $derived(
    settings === null || settings.microphone_name === "" ? "Default mic" : settings.microphone_name,
  );

  async function handleToggle() {
    actionError = await bot.toggle();
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
      disabled={bot.isBusy || (!canStart && !isOnline)}
      aria-label={isOnline ? "Send the bot to sleep" : "Wake the bot up"}
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

  <button class="button" onclick={handleToggle} disabled={bot.isBusy || (!canStart && !isOnline)}>
    {#if isOnline}
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
