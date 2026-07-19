<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { page } from "$app/state";
  import { goto } from "$app/navigation";
  import { bot, describeStatus, statusTone } from "$lib/bot.svelte";
  import ContextMenu, { type ContextEntry } from "$lib/ContextMenu.svelte";
  import Butterfly from "phosphor-svelte/lib/Butterfly";
  import Microphone from "phosphor-svelte/lib/Microphone";
  import ChatCircleDots from "phosphor-svelte/lib/ChatCircleDots";
  import TerminalWindow from "phosphor-svelte/lib/TerminalWindow";
  import BookOpenText from "phosphor-svelte/lib/BookOpenText";
  import GearSix from "phosphor-svelte/lib/GearSix";
  import FolderSimple from "phosphor-svelte/lib/FolderSimple";
  import HardDrives from "phosphor-svelte/lib/HardDrives";
  import WarningCircle from "phosphor-svelte/lib/WarningCircle";
  import Play from "phosphor-svelte/lib/Play";
  import Stop from "phosphor-svelte/lib/Stop";
  import SignOut from "phosphor-svelte/lib/SignOut";
  import Broom from "phosphor-svelte/lib/Broom";
  import Copy from "phosphor-svelte/lib/Copy";
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

  // Which tab is showing. The packaged app can load under an entry URL like /index.html
  // (the SPA fallback), which would otherwise leave Home unhighlighted even though its
  // page is on screen. Strip that and any trailing slash, then resolve anything that is
  // not one of the real tabs to Home — that is the page a non-tab entry URL is showing.
  const activePath = $derived.by(() => {
    let path: string = page.url.pathname;

    if (path.endsWith("index.html")) {
      path = path.slice(0, -"index.html".length);
    }
    if (path.length > 1 && path.endsWith("/")) {
      path = path.slice(0, -1);
    }

    const isKnownTab = navigationItems.some((item) => item.path === path);
    return isKnownTab ? path : "/";
  });

  const tone = $derived(statusTone(bot.status, bot.inChannel, bot.isSpeaking));
  const statusText = $derived(describeStatus(bot.status, bot.inChannel, bot.isSpeaking));

  // The right-click menu is about wherever you are: the section's own name as a heading,
  // then the handful of things worth doing there, then a way back Home and to Settings.
  const contextTitle = $derived(
    navigationItems.find((item) => item.path === activePath)?.label ?? "Butter TTS",
  );

  const contextEntries = $derived.by<ContextEntry[]>(() => {
    const here: ContextEntry[] = [];

    if (activePath === "/") {
      here.push({
        label: bot.isRunning ? "Go to sleep" : "Wake up",
        icon: bot.isRunning ? Stop : Play,
        onSelect: () => void bot.toggle(),
      });
      if (bot.inChannel) {
        here.push({
          label: "Leave the channel",
          icon: SignOut,
          onSelect: () => void bot.leaveChannel(),
        });
      }
    } else if (activePath === "/console") {
      here.push({
        label: "Copy the console",
        icon: Copy,
        onSelect: () => void bot.copyLog(),
        disabled: bot.logLines.length === 0,
      });
      here.push({
        label: "Clear the console",
        icon: Broom,
        onSelect: () => bot.clearLog(),
        disabled: bot.logLines.length === 0,
      });
    } else if (activePath === "/history") {
      here.push({
        label: "Forget all history",
        icon: Broom,
        danger: true,
        onSelect: () => void bot.clearTranscripts(),
        disabled: bot.transcripts.length === 0,
      });
    }

    const jumps: ContextEntry[] = [];
    if (activePath !== "/") {
      jumps.push({ label: "Home", icon: Butterfly, onSelect: () => goto("/") });
    }
    if (activePath !== "/settings") {
      jumps.push({ label: "Settings", icon: GearSix, onSelect: () => goto("/settings") });
    }

    if (here.length > 0 && jumps.length > 0) {
      here.push("separator");
    }

    return [...here, ...jumps];
  });

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
          : "Bot is ready. Pick a voice channel on the Home page to join.";
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
      <button class="brand" onclick={() => goto("/")} aria-label="Go to the Home tab">
        <span class="brand__mark">
          <Butterfly size={24} weight="fill" />
        </span>
        <span class="brand__text">
          <span class="brand__name">Butter TTS</span>
          <span class="brand__tagline">speak here, heard there</span>
        </span>
      </button>

      <nav class="nav" aria-label="Primary">
        {#each navigationItems as item (item.path)}
          {@const Icon = item.icon}
          <button
            class="nav__item"
            aria-current={activePath === item.path ? "page" : undefined}
            onclick={() => goto(item.path)}
          >
            <Icon size={19} weight={activePath === item.path ? "fill" : "duotone"} />
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

    <ContextMenu title={contextTitle} entries={contextEntries} />
  </div>
  {/if}
</IconContext>
