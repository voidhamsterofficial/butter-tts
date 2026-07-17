<script lang="ts">
  import { onMount } from "svelte";
  import { page } from "$app/state";
  import { goto } from "$app/navigation";
  import { bot, describeStatus, statusTone } from "$lib/bot.svelte";
  import Butterfly from "phosphor-svelte/lib/Butterfly";
  import Microphone from "phosphor-svelte/lib/Microphone";
  import ChatCircleDots from "phosphor-svelte/lib/ChatCircleDots";
  import TerminalWindow from "phosphor-svelte/lib/TerminalWindow";
  import BookOpenText from "phosphor-svelte/lib/BookOpenText";
  import GearSix from "phosphor-svelte/lib/GearSix";
  import "../app.css";

  let { children } = $props();

  onMount(() => {
    const connecting = bot.connect();

    return () => {
      void connecting.then((disconnect) => disconnect());
    };
  });

  const navigationItems = [
    { path: "/", label: "Home", icon: Microphone },
    { path: "/history", label: "History", icon: ChatCircleDots },
    { path: "/console", label: "Console", icon: TerminalWindow },
    { path: "/docs", label: "Docs", icon: BookOpenText },
    { path: "/settings", label: "Settings", icon: GearSix },
  ];

  const tone = $derived(statusTone(bot.status, bot.inChannel, bot.isSpeaking));
  const statusText = $derived(describeStatus(bot.status, bot.inChannel, bot.isSpeaking));
</script>

<div class="shell">
  <div class="shell__sky" aria-hidden="true"></div>

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

    <nav class="nav">
      {#each navigationItems as item (item.path)}
        {@const Icon = item.icon}
        <button
          class="nav__item"
          style="position: relative;"
          aria-current={page.url.pathname === item.path ? "page" : undefined}
          onclick={() => goto(item.path)}
        >
          <Icon size={19} weight={page.url.pathname === item.path ? "fill" : "duotone"} />
          {item.label}
          {#if item.path === "/history" && bot.transcripts.length > 0}
            <span class="nav__badge">{bot.transcripts.length}</span>
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
