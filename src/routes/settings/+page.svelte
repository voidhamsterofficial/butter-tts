<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { open } from "@tauri-apps/plugin-dialog";
  import { bot } from "$lib/bot.svelte";
  import type { Settings, TuningRanges, DatabaseLocation } from "$lib/settings";
  import Eye from "phosphor-svelte/lib/Eye";
  import HardDrives from "phosphor-svelte/lib/HardDrives";
  import FolderSimple from "phosphor-svelte/lib/FolderSimple";
  import EyeSlash from "phosphor-svelte/lib/EyeSlash";
  import Key from "phosphor-svelte/lib/Key";
  import DiscordLogo from "phosphor-svelte/lib/DiscordLogo";
  import MicrophoneStage from "phosphor-svelte/lib/MicrophoneStage";
  import MusicNotes from "phosphor-svelte/lib/MusicNotes";
  import FloppyDisk from "phosphor-svelte/lib/FloppyDisk";
  import ArrowCounterClockwise from "phosphor-svelte/lib/ArrowCounterClockwise";
  import CheckCircle from "phosphor-svelte/lib/CheckCircle";
  import WarningCircle from "phosphor-svelte/lib/WarningCircle";
  import Info from "phosphor-svelte/lib/Info";
  import SlidersHorizontal from "phosphor-svelte/lib/SlidersHorizontal";
  import Sparkle from "phosphor-svelte/lib/Sparkle";
  import RevealButton from "$lib/RevealButton.svelte";

  let settings = $state<Settings | null>(null);
  let ranges = $state<TuningRanges | null>(null);
  let microphones = $state<string[]>([]);
  let defaultMicrophone = $state<string | null>(null);
  let voices = $state<string[]>([]);
  let databasePath = $state("");

  let showOpenAiKey = $state(false);
  let showDiscordToken = $state(false);
  let saveMessage = $state<{ text: string; tone: "ok" | "error" } | null>(null);
  let isSaving = $state(false);

  let databaseLocation = $state<DatabaseLocation | null>(null);
  let isMovingDatabase = $state(false);
  let moveMessage = $state<{ text: string; tone: "ok" | "error" } | null>(null);

  // These are read when the bot starts, so editing them while it runs would be a lie.
  // Moving the database is locked for a stronger reason: the bot writes to it during a
  // session, so moving the file then would lose what it was writing.
  const isLocked = $derived(bot.status.state === "online" || bot.status.state === "starting");

  onMount(async () => {
    try {
      const [loaded, loadedRanges, mics, defaultMic, loadedVoices, path, location] =
        await Promise.all([
          invoke<Settings>("load_settings"),
          invoke<TuningRanges>("tuning_ranges"),
          invoke<string[]>("list_microphones"),
          invoke<string | null>("default_microphone"),
          invoke<string[]>("list_voices"),
          invoke<string>("database_path"),
          invoke<DatabaseLocation>("database_location"),
        ]);

      settings = loaded;
      ranges = loadedRanges;
      microphones = mics;
      defaultMicrophone = defaultMic;
      voices = loadedVoices;
      databasePath = path;
      databaseLocation = location;
    } catch (error) {
      saveMessage = { text: String(error), tone: "error" };
    }
  });

  async function moveToDefault() {
    if (isLocked || isMovingDatabase || databaseLocation === "default") {
      return;
    }

    await runMove(() => invoke<string>("move_database_to_default"));
  }

  async function chooseFolder() {
    if (isLocked || isMovingDatabase) {
      return;
    }

    // The native folder picker. Returns the chosen path, or null if the user backed out.
    const directory = await open({ directory: true, title: "Where should I keep your data?" });

    if (typeof directory !== "string") {
      return;
    }

    await runMove(() => invoke<string>("move_database_to", { directory }));
  }

  // Both moves share the same busy/report handling; only the backend call differs. The
  // resulting location is read back from the backend rather than assumed, since picking the
  // default folder through the "choose" dialog lands as the default, not a custom spot.
  async function runMove(move: () => Promise<string>) {
    isMovingDatabase = true;
    moveMessage = null;

    try {
      databasePath = await move();
      databaseLocation = await invoke<DatabaseLocation>("database_location");
      moveMessage = { text: "Moved! Your data is in its new home.", tone: "ok" };
    } catch (error) {
      moveMessage = { text: String(error), tone: "error" };
    } finally {
      isMovingDatabase = false;
    }
  }

  // Where the live mic level sits on the threshold slider's own scale, so the meter and
  // the marker can be read against each other.
  const meterPercent = $derived.by(() => {
    if (ranges === null) {
      return 0;
    }

    const span = ranges.speechThreshold.max - ranges.speechThreshold.min;
    const position = (bot.peakAmplitude - ranges.speechThreshold.min) / span;

    return Math.max(0, Math.min(1, position)) * 100;
  });

  const thresholdPercent = $derived.by(() => {
    if (ranges === null || settings === null) {
      return 0;
    }

    const span = ranges.speechThreshold.max - ranges.speechThreshold.min;
    const position = (settings.speech_threshold - ranges.speechThreshold.min) / span;

    return Math.max(0, Math.min(1, position)) * 100;
  });

  const isHearingSpeech = $derived(
    settings !== null && bot.peakAmplitude >= settings.speech_threshold,
  );

  async function handleSave(event: Event) {
    event.preventDefault();

    if (settings === null) {
      return;
    }

    isSaving = true;

    try {
      await invoke("save_settings", { settings });
      saveMessage = { text: "Saved! Restart me for it to take effect.", tone: "ok" };
    } catch (error) {
      saveMessage = { text: String(error), tone: "error" };
    } finally {
      isSaving = false;
    }
  }

  function resetTuning() {
    if (settings === null || ranges === null) {
      return;
    }

    settings.speech_threshold = ranges.speechThreshold.default;
    settings.trailing_silence_ms = ranges.trailingSilenceMs.default;
    settings.min_utterance_ms = ranges.minUtteranceMs.default;
    settings.max_utterance_ms = ranges.maxUtteranceMs.default;
  }
</script>

<div class="page__head">
  <h1 class="page__title">Settings</h1>
  <p class="page__subtitle">Kept wherever you chose on first launch.</p>
</div>

{#if settings === null || ranges === null}
  <section class="card form">
    <p class="notice">
      <Info size={17} weight="duotone" />
      Getting things ready…
    </p>
  </section>
{:else}
  <form class="card form" onsubmit={handleSave}>
    {#if isLocked}
      <p class="notice">
        <Info size={17} weight="duotone" />
        I am awake right now! Send me to sleep from the Home page to change these — I only
        read them when I wake up.
      </p>
    {/if}

    <div class="field">
      <span class="field__label">
        <Key size={15} weight="duotone" />
        OpenAI key
      </span>
      <div class="field__row">
        <input
          class="field__input"
          type={showOpenAiKey ? "text" : "password"}
          placeholder="sk-…"
          autocomplete="off"
          spellcheck="false"
          disabled={isLocked}
          bind:value={settings.openai_api_key}
        />
        <button
          type="button"
          class="field__reveal"
          onclick={() => (showOpenAiKey = !showOpenAiKey)}
          aria-label={showOpenAiKey ? "Hide the key" : "Show the key"}
        >
          {#if showOpenAiKey}
            <EyeSlash size={15} weight="duotone" />
          {:else}
            <Eye size={15} weight="duotone" />
          {/if}
        </button>
      </div>
      <p class="field__hint">This pays for hearing you and for my voice.</p>
    </div>

    <div class="field">
      <span class="field__label">
        <DiscordLogo size={15} weight="duotone" />
        Discord bot token
      </span>
      <div class="field__row">
        <input
          class="field__input"
          type={showDiscordToken ? "text" : "password"}
          placeholder="Your bot's token"
          autocomplete="off"
          spellcheck="false"
          disabled={isLocked}
          bind:value={settings.discord_bot_token}
        />
        <button
          type="button"
          class="field__reveal"
          onclick={() => (showDiscordToken = !showDiscordToken)}
          aria-label={showDiscordToken ? "Hide the token" : "Show the token"}
        >
          {#if showDiscordToken}
            <EyeSlash size={15} weight="duotone" />
          {:else}
            <Eye size={15} weight="duotone" />
          {/if}
        </button>
      </div>
      <p class="field__hint">
        From the Discord developer portal. The application ID comes from the token itself,
        so there is nothing else to fill in.
      </p>
    </div>

    <div class="field">
      <span class="field__label">
        <MicrophoneStage size={15} weight="duotone" />
        Microphone
      </span>
      <div class="field__row">
        <select class="field__select" disabled={isLocked} bind:value={settings.microphone_name}>
          <option value="">
            Default{defaultMicrophone === null ? "" : ` (${defaultMicrophone})`}
          </option>
          {#each microphones as microphone (microphone)}
            <option value={microphone}>{microphone}</option>
          {/each}
        </select>
      </div>
      {#if microphones.length === 0}
        <p class="field__hint">I cannot find any microphones! Plug one in and reopen this page.</p>
      {/if}
    </div>

    <div class="field">
      <span class="field__label">
        <MusicNotes size={15} weight="duotone" />
        My voice
      </span>
      <div class="field__row">
        <select class="field__select" disabled={isLocked} bind:value={settings.tts_voice}>
          {#each voices as voice (voice)}
            <option value={voice}>{voice}</option>
          {/each}
        </select>
      </div>
      <p class="field__hint">OpenAI reckons marin and cedar sound the nicest.</p>
    </div>

    <div class="field">
      <span class="field__label">
        <Sparkle size={15} weight="duotone" />
        Clean up my audio
      </span>
      <label class="toggle">
        <span class="toggle__text">
          <span class="toggle__name">Noise suppression</span>
          <span class="toggle__hint">
            Strips fans, hiss, and keyboard clatter before I listen, so only your voice
            gets through. Turn it off to compare.
          </span>
        </span>
        <input
          type="checkbox"
          disabled={isLocked}
          bind:checked={settings.noise_suppression}
        />
        <span class="toggle__track" data-on={settings.noise_suppression}>
          <span class="toggle__knob"></span>
        </span>
      </label>
    </div>

    <div class="field">
      <span class="field__label">
        <SlidersHorizontal size={15} weight="duotone" />
        Listening
      </span>

      <div class="slider" style="margin-bottom: 10px;">
        <div class="slider__head">
          <span class="slider__name">How loud counts as talking</span>
          <span class="slider__value">{settings.speech_threshold}</span>
        </div>
        <p class="slider__hint">
          The pink line is where you have set it. Talk normally and watch the bar: it should
          jump past the line when you speak, and stay well under it when you are quiet.
        </p>

        <!-- A live picture of the mic level that redraws many times a second — useful to
             watch, but hidden from screen readers so it does not fire off a torrent of
             announcements. The slider below carries the value that actually matters. -->
        <div class="meter" aria-hidden="true">
          <div class="meter__fill" style="width: {meterPercent}%"></div>
          <div class="meter__mark" style="left: {thresholdPercent}%"></div>
        </div>
        <div class="meter__legend" aria-hidden="true">
          <span>your mic: {bot.peakAmplitude}</span>
          <span style="color: {isHearingSpeech ? 'var(--mint-deep)' : 'var(--cocoa-faint)'}">
            {isHearingSpeech ? "hearing you!" : "quiet"}
          </span>
        </div>

        <input
          class="slider__input"
          type="range"
          aria-label="How loud counts as talking"
          min={ranges.speechThreshold.min}
          max={ranges.speechThreshold.max}
          step="10"
          disabled={isLocked}
          bind:value={settings.speech_threshold}
        />
        <div class="slider__scale">
          <span>sensitive</span>
          <span>strict</span>
        </div>
      </div>

      <div class="slider" style="margin-bottom: 10px;">
        <div class="slider__head">
          <span class="slider__name">Pause before I reply</span>
          <span class="slider__value">{settings.trailing_silence_ms} ms</span>
        </div>
        <p class="slider__hint">
          How long you have to stop talking before I decide you are done. This waiting is
          added to every single reply, so it decides how snappy I feel.
        </p>
        <input
          class="slider__input"
          type="range"
          aria-label="Pause before I reply, in milliseconds"
          min={ranges.trailingSilenceMs.min}
          max={ranges.trailingSilenceMs.max}
          step="50"
          disabled={isLocked}
          bind:value={settings.trailing_silence_ms}
        />
        <div class="slider__scale">
          <span>snappy</span>
          <span>patient</span>
        </div>
      </div>

      <div class="slider" style="margin-bottom: 10px;">
        <div class="slider__name" style="margin-bottom: 4px;">
          Shortest thing worth saying
          <span class="slider__value" style="float: right;">{settings.min_utterance_ms} ms</span>
        </div>
        <p class="slider__hint">Anything shorter is a cough or a keyboard clack, not a word.</p>
        <input
          class="slider__input"
          type="range"
          aria-label="Shortest thing worth saying, in milliseconds"
          min={ranges.minUtteranceMs.min}
          max={ranges.minUtteranceMs.max}
          step="50"
          disabled={isLocked}
          bind:value={settings.min_utterance_ms}
        />
      </div>

      <div class="slider">
        <div class="slider__name" style="margin-bottom: 4px;">
          Longest I will listen
          <span class="slider__value" style="float: right;">
            {(settings.max_utterance_ms / 1000).toFixed(0)} s
          </span>
        </div>
        <p class="slider__hint">
          If you never pause, I will cut in and speak anyway once you hit this.
        </p>
        <input
          class="slider__input"
          type="range"
          aria-label="Longest I will listen, in milliseconds"
          min={ranges.maxUtteranceMs.min}
          max={ranges.maxUtteranceMs.max}
          step="1000"
          disabled={isLocked}
          bind:value={settings.max_utterance_ms}
        />
      </div>
    </div>

    <div class="form__actions">
      <button class="button" type="submit" disabled={isLocked || isSaving}>
        <FloppyDisk size={16} weight="duotone" />
        {isSaving ? "Saving…" : "Save"}
      </button>

      <button class="button button--ghost" type="button" onclick={resetTuning} disabled={isLocked}>
        <ArrowCounterClockwise size={15} weight="duotone" />
        Reset sliders
      </button>

      {#if saveMessage !== null}
        <span class="notice" data-tone={saveMessage.tone}>
          {#if saveMessage.tone === "ok"}
            <CheckCircle size={16} weight="fill" />
          {:else}
            <WarningCircle size={16} weight="fill" />
          {/if}
          {saveMessage.text}
        </span>
      {/if}
    </div>

    <div class="field">
      <span class="field__label">
        <HardDrives size={15} weight="duotone" />
        Where your data lives
      </span>

      <div class="location">
        <button
          type="button"
          class="location__option"
          data-active={databaseLocation === "default"}
          disabled={isLocked || isMovingDatabase || databaseLocation === "default"}
          onclick={moveToDefault}
        >
          <span class="location__icon"><HardDrives size={18} weight="duotone" /></span>
          <span class="location__text">
            <span class="location__name">Default folder</span>
            <span class="location__hint">
              Your system's app data folder. Survives the app being updated or reinstalled.
            </span>
          </span>
        </button>

        <button
          type="button"
          class="location__option"
          data-active={databaseLocation === "custom"}
          disabled={isLocked || isMovingDatabase}
          onclick={chooseFolder}
        >
          <span class="location__icon"><FolderSimple size={18} weight="duotone" /></span>
          <span class="location__text">
            <span class="location__name">Somewhere you choose</span>
            <span class="location__hint">
              {databaseLocation === "custom"
                ? "Pick a different folder — a USB stick, a synced folder, wherever."
                : "Pick any folder — a USB stick, a synced folder, wherever."}
            </span>
          </span>
        </button>
      </div>

      {#if isLocked}
        <p class="field__hint">Send me to sleep on the Home page to move the database.</p>
      {/if}

      {#if moveMessage !== null}
        <p class="notice" data-tone={moveMessage.tone}>
          {#if moveMessage.tone === "ok"}
            <CheckCircle size={16} weight="fill" />
          {:else}
            <WarningCircle size={16} weight="fill" />
          {/if}
          {moveMessage.text}
        </p>
      {/if}

      <div class="field__footer">
        <p class="field__hint">
          Saved, keys encrypted, in <code>{databasePath}</code> — anyone who can read that
          file and run the app can still use your bot and your OpenAI account, since there
          is no password to keep them out.
        </p>
        {#if databasePath !== ""}
          <RevealButton path={databasePath} />
        {/if}
      </div>
    </div>
  </form>
{/if}
