<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { page } from "$app/state";
  import { goto } from "$app/navigation";
  import { bot, describeStatus, statusTone } from "$lib/bot.svelte";
  import Butterfly from "phosphor-svelte/lib/Butterfly";
  import Microphone from "phosphor-svelte/lib/Microphone";
  import ChatCircleDots from "phosphor-svelte/lib/ChatCircleDots";
  import TerminalWindow from "phosphor-svelte/lib/TerminalWindow";
  import BookOpenText from "phosphor-svelte/lib/BookOpenText";
  import GearSix from "phosphor-svelte/lib/GearSix";
  import FolderSimple from "phosphor-svelte/lib/FolderSimple";
  import HardDrives from "phosphor-svelte/lib/HardDrives";
  import WarningCircle from "phosphor-svelte/lib/WarningCircle";
  import { IconContext } from "phosphor-svelte";
  import "../app.css";

  let { children } = $props();

  // "checking" avoids a flash of the setup screen on every ordinary launch, since the
  // answer usually comes back within a frame or two.
  let setupState = $state<"checking" | "needed" | "ready">("checking");
  let settingUp = $state(false);
  let setupError = $state<string | null>(null);

  onMount(async () => {
    try {
      const needed = await invoke<boolean>("needs_setup");
      setupState = needed ? "needed" : "ready";
    } catch (error) {
      // Erring toward asking is safer than erring toward silently picking a location.
      setupError = String(error);
      setupState = "needed";
    }
  });

  // Only listens for the bot's events once there is somewhere for its settings to have
  // come from.
  $effect(() => {
    if (setupState !== "ready") {
      return;
    }

    const connecting = bot.connect();

    return () => {
      void connecting.then((disconnect) => disconnect());
    };
  });

  async function chooseLocation(portable: boolean) {
    settingUp = true;
    setupError = null;

    try {
      await invoke("complete_setup", { portable });
      setupState = "ready";
    } catch (error) {
      setupError = String(error);
    } finally {
      settingUp = false;
    }
  }

  const navigationItems = [
    { path: "/", label: "Home", icon: Microphone },
    { path: "/history", label: "History", icon: ChatCircleDots },
    { path: "/console", label: "Console", icon: TerminalWindow },
    { path: "/docs", label: "Docs", icon: BookOpenText },
    { path: "/settings", label: "Settings", icon: GearSix },
  ];

  const tone = $derived(statusTone(bot.status, bot.inChannel, bot.isSpeaking));
  const statusText = $derived(describeStatus(bot.status, bot.inChannel, bot.isSpeaking));

  // A calm, spoken-status announcement for screen readers. Deliberately keyed off the
  // connection state and whether we are in a channel — not isSpeaking — so it announces
  // real transitions (connecting, ready, listening, dropped) without chattering on every
  // syllable the way the visible "Hearing you / Listening" label does.
  const statusAnnouncement = $derived.by(() => {
    switch (bot.status.state) {
      case "offline":
        return "Bot is offline.";
      case "starting":
        return "Connecting to Discord.";
      case "reconnecting":
        return "Connection to Discord lost, reconnecting.";
      case "failed":
        return `Bot failed to start: ${bot.status.detail}`;
      case "online":
        return bot.inChannel
          ? "Bot is listening in the voice channel."
          : "Bot is ready. Use /join in a Discord voice channel.";
    }
  });
</script>

<!-- Every icon in the app sits beside visible text or inside an aria-labelled control,
     so to a screen reader they are decoration. Marking them hidden here, once, stops each
     from being announced as an unlabelled "image". -->
<IconContext values={{ "aria-hidden": "true", focusable: "false" }}>
  {#if setupState === "needed"}
  <div class="setup">
    <div class="setup__card card">
      <span class="brand__mark setup__mark">
        <Butterfly size={28} weight="fill" />
      </span>
      <h1 class="setup__title">Where should I keep your stuff?</h1>
      <p class="setup__subtitle">
        Your settings and history live in one file, encrypted where it matters. Pick where
        that file goes — I only ask once.
      </p>

      <div class="setup__options">
        <button
          class="setup__option"
          onclick={() => chooseLocation(false)}
          disabled={settingUp}
        >
          <span class="setup__option-icon"><HardDrives size={22} weight="duotone" /></span>
          <span class="setup__option-text">
            <span class="setup__option-title">Use the default location</span>
            <span class="setup__option-hint">
              Kept in your system's app data folder. Recommended — it survives this app
              being updated or reinstalled.
            </span>
          </span>
        </button>

        <button
          class="setup__option setup__option--ghost"
          onclick={() => chooseLocation(true)}
          disabled={settingUp}
        >
          <span class="setup__option-icon"><FolderSimple size={22} weight="duotone" /></span>
          <span class="setup__option-text">
            <span class="setup__option-title">Keep it portable</span>
            <span class="setup__option-hint">
              Kept right next to this app, so you can carry the whole folder on a USB
              stick. On an installed app, reinstalling clears it.
            </span>
          </span>
        </button>
      </div>

      {#if setupError !== null}
        <p class="notice" data-tone="error">
          <WarningCircle size={17} weight="fill" />
          {setupError}
        </p>
      {/if}
    </div>
  </div>
{:else if setupState === "ready"}
  <div class="shell">
    <div class="shell__sky" aria-hidden="true"></div>

    <!-- Announces real connection changes (connecting, ready, listening, dropped) to a
         screen reader without repeating the visible label's speaking/listening churn. -->
    <div class="visually-hidden" aria-live="polite" role="status">{statusAnnouncement}</div>

    <aside class="sidebar">
      <div class="brand">
        <span class="brand__mark">
          <Butterfly size={24} weight="fill" />
        </span>
        <div>
          <div class="brand__name">Butter TTS</div>
          <div class="brand__tagline">speak here, heard there</div>
        </div>
      </div>

      <nav class="nav" aria-label="Primary">
        {#each navigationItems as item (item.path)}
          {@const Icon = item.icon}
          <button
            class="nav__item"
            aria-current={page.url.pathname === item.path ? "page" : undefined}
            onclick={() => goto(item.path)}
          >
            <Icon size={19} weight={page.url.pathname === item.path ? "fill" : "duotone"} />
            {item.label}
            {#if item.path === "/history" && bot.transcripts.length > 0}
              <span class="nav__badge">
                {bot.transcripts.length}<span class="visually-hidden"> saved</span>
              </span>
            {/if}
          </button>
        {/each}
      </nav>

      <div class="sidebar__footer">
        <span class="status" data-tone={tone}>
          <span class="status__dot"></span>
          {statusText}
        </span>
      </div>
    </aside>

    <main class="page">
      {@render children()}
    </main>
  </div>
  {/if}
</IconContext>
