<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { bot, formatTime, type Transcript } from "$lib/bot.svelte";
  import ChatCircleDots from "phosphor-svelte/lib/ChatCircleDots";
  import Quotes from "phosphor-svelte/lib/Quotes";
  import Copy from "phosphor-svelte/lib/Copy";
  import Broom from "phosphor-svelte/lib/Broom";
  import WarningCircle from "phosphor-svelte/lib/WarningCircle";

  let historyPath = $state("");
  let actionError = $state<string | null>(null);
  let copyLabel = $state("Copy all");

  onMount(async () => {
    try {
      historyPath = await invoke<string>("transcripts_file_path");
    } catch (error) {
      actionError = String(error);
    }
  });

  // Newest first: the last thing said is what you came here to see.
  const newestFirst = $derived([...bot.transcripts].reverse());

  function formatDay(timestampMs: number): string {
    const said = new Date(timestampMs);
    const today = new Date();
    const yesterday = new Date(today);
    yesterday.setDate(today.getDate() - 1);

    if (said.toDateString() === today.toDateString()) {
      return "Today";
    }

    if (said.toDateString() === yesterday.toDateString()) {
      return "Yesterday";
    }

    return said.toLocaleDateString(undefined, {
      weekday: "long",
      day: "numeric",
      month: "long",
    });
  }

  /** True when this entry starts a new day, so the list can label the run. */
  function startsNewDay(entries: Transcript[], index: number): boolean {
    if (index === 0) {
      return true;
    }

    const previous = new Date(entries[index - 1].timestamp_ms).toDateString();
    const current = new Date(entries[index].timestamp_ms).toDateString();

    return previous !== current;
  }

  async function handleCopy() {
    const text = newestFirst
      .map((entry) => `${formatTime(entry.timestamp_ms)}  ${entry.text}`)
      .join("\n");

    try {
      await navigator.clipboard.writeText(text);
      copyLabel = "Copied!";
    } catch {
      copyLabel = "Copy failed";
    }

    setTimeout(() => {
      copyLabel = "Copy all";
    }, 1500);
  }

  async function handleClear() {
    actionError = await bot.clearTranscripts();
  }
</script>

<div class="page__head">
  <h1 class="page__title">History</h1>
  <p class="page__subtitle">Everything you have said, saved as text. No audio is ever kept.</p>
</div>

<section class="card list">
  <div class="list__head">
    <span class="list__title">
      <Quotes size={17} weight="duotone" />
      Things you said
    </span>
    <span class="list__count">{bot.transcripts.length}</span>

    <div class="list__actions">
      <button
        class="button button--ghost button--small"
        onclick={handleCopy}
        disabled={bot.transcripts.length === 0}
      >
        <Copy size={13} weight="duotone" />
        {copyLabel}
      </button>
      <button
        class="button button--ghost button--small"
        onclick={handleClear}
        disabled={bot.transcripts.length === 0}
      >
        <Broom size={13} weight="duotone" />
        Forget all
      </button>
    </div>
  </div>

  <div class="list__body">
    {#if actionError !== null}
      <p class="notice" data-tone="error">
        <WarningCircle size={17} weight="fill" />
        {actionError}
      </p>
    {/if}

    {#if bot.transcripts.length === 0}
      <div class="list__empty">
        <ChatCircleDots size={54} weight="duotone" />
        <div class="list__empty-title">Nothing said yet</div>
        <p class="list__empty-body">
          Wake me up and join a voice channel. Everything you say gets written down here —
          just the words, never the sound.
        </p>
      </div>
    {:else}
      {#each newestFirst as entry, index (entry.timestamp_ms + entry.text)}
        {#if startsNewDay(newestFirst, index)}
          <div class="said__day">{formatDay(entry.timestamp_ms)}</div>
        {/if}

        <article class="said">
          <span class="said__bubble">
            <Quotes size={16} weight="fill" />
          </span>
          <div class="said__body">
            <p class="said__text">{entry.text}</p>
            <div class="said__meta">
              <span>{formatTime(entry.timestamp_ms)}</span>
              <span>·</span>
              <span>{entry.voice}</span>
            </div>
          </div>
        </article>
      {/each}
    {/if}
  </div>
</section>

{#if historyPath !== ""}
  <p class="field__hint" style="margin-top: 12px;">
    Kept in plain text at <code>{historyPath}</code>
  </p>
{/if}
