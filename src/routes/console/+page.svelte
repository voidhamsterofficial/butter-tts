<script lang="ts">
  import { tick } from "svelte";
  import { bot, formatTime } from "$lib/bot.svelte";
  import TerminalWindow from "phosphor-svelte/lib/TerminalWindow";
  import Copy from "phosphor-svelte/lib/Copy";
  import Broom from "phosphor-svelte/lib/Broom";

  let feedElement = $state<HTMLDivElement | null>(null);
  let isPinnedToBottom = $state(true);
  let copyLabel = $state("Copy");

  // Anything within this of the bottom counts as "still following along".
  const PINNED_THRESHOLD_PX = 40;

  function handleScroll() {
    if (feedElement === null) {
      return;
    }

    const distanceFromBottom =
      feedElement.scrollHeight - feedElement.scrollTop - feedElement.clientHeight;

    isPinnedToBottom = distanceFromBottom < PINNED_THRESHOLD_PX;
  }

  // Follow the log as it grows, but never while the user has scrolled up to read
  // something — yanking them back mid-read is the worst thing a console can do.
  $effect(() => {
    const lineCount = bot.logLines.length;

    if (lineCount === 0 || !isPinnedToBottom) {
      return;
    }

    void tick().then(() => {
      if (feedElement !== null) {
        feedElement.scrollTop = feedElement.scrollHeight;
      }
    });
  });

  async function handleCopy() {
    const text = bot.logLines
      .map((line) => `${formatTime(line.timestampMs)} ${line.level.toUpperCase()} ${line.message}`)
      .join("\n");

    try {
      await navigator.clipboard.writeText(text);
      copyLabel = "Copied!";
    } catch {
      copyLabel = "Copy failed";
    }

    setTimeout(() => {
      copyLabel = "Copy";
    }, 1500);
  }
</script>

<div class="page__head">
  <h1 class="page__title">Console</h1>
  <p class="page__subtitle">Everything I am up to, as it happens.</p>
</div>

<section class="card list">
  <div class="list__head">
    <span class="list__title">
      <TerminalWindow size={17} weight="duotone" />
      Output
    </span>
    <span class="list__count">{bot.logLines.length}</span>

    <div class="list__actions">
      <button
        class="button button--ghost button--small"
        onclick={handleCopy}
        disabled={bot.logLines.length === 0}
      >
        <Copy size={13} weight="duotone" />
        {copyLabel}
      </button>
      <button
        class="button button--ghost button--small"
        onclick={() => bot.clearLog()}
        disabled={bot.logLines.length === 0}
      >
        <Broom size={13} weight="duotone" />
        Clear
      </button>
    </div>
  </div>

  <!-- role="log" announces new lines to a screen reader as they arrive; tabindex makes
       the region keyboard-scrollable, since it holds no focusable children of its own.
       ARIA guidance allows tabindex=0 on a scrollable region for exactly this — the lint
       rule does not special-case it. -->
  <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
  <div
    class="list__body"
    bind:this={feedElement}
    onscroll={handleScroll}
    role="log"
    aria-label="Bot activity log"
    tabindex="0"
  >
    {#if bot.logLines.length === 0}
      <div class="list__empty">
        <TerminalWindow size={54} weight="duotone" />
        <div class="list__empty-title">All quiet</div>
        <p class="list__empty-body">
          Wake me up from the Home page. What I hear, what I say, and anything that goes
          wrong will show up right here.
        </p>
      </div>
    {:else}
      {#each bot.logLines as line (line.timestampMs + line.message)}
        <div class="line" data-level={line.level}>
          <span class="line__time">{formatTime(line.timestampMs)}</span>
          <span class="line__level">{line.level}</span>
          <span class="line__message">{line.message}</span>
        </div>
      {/each}
    {/if}
  </div>
</section>
