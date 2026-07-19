<script module lang="ts">
  import type { Component } from "svelte";

  /** One thing the menu can do. `icon` is a Phosphor component, same as the nav uses. */
  export type ContextAction = {
    label: string;
    icon: Component;
    onSelect: () => void;
    /** Destructive actions (clear, forget) read in the alert colour. */
    danger?: boolean;
    disabled?: boolean;
  };

  /** A menu is a list of actions with optional dividers between groups. */
  export type ContextEntry = ContextAction | "separator";
</script>

<script lang="ts">
  // A small custom right-click menu. It lives once in the layout and is handed a title and
  // a list of entries that change with whatever page you are on, so the menu is always
  // about the section under the cursor.

  let { title, entries }: { title: string; entries: ContextEntry[] } = $props();

  let open = $state(false);
  // Where the click happened, and where the menu actually sits after being nudged back on
  // screen. Kept apart so the clamp below never feeds its own output back in.
  let rawX = $state(0);
  let rawY = $state(0);
  let posX = $state(0);
  let posY = $state(0);
  let menuElement = $state<HTMLElement | null>(null);

  function handleContextMenu(event: MouseEvent) {
    const target = event.target as HTMLElement;

    // Leave editable fields to the native menu, so copy and paste still work where you
    // actually type — the API key and token boxes especially.
    if (target.closest("input, textarea, [contenteditable='true']")) {
      return;
    }

    event.preventDefault();
    rawX = event.clientX;
    rawY = event.clientY;
    posX = event.clientX;
    posY = event.clientY;
    open = true;
  }

  function close() {
    open = false;
  }

  function choose(action: ContextAction) {
    close();
    action.onSelect();
  }

  function isAction(entry: ContextEntry): entry is ContextAction {
    return entry !== "separator";
  }

  // Once the menu has a measured size, pull it back inside the window if the click was near
  // an edge. Depends only on the raw click and the element, never on posX/posY, so writing
  // the adjusted position cannot re-trigger it.
  $effect(() => {
    if (!open || menuElement === null) {
      return;
    }

    const margin = 8;
    const rect = menuElement.getBoundingClientRect();
    const maxX = window.innerWidth - rect.width - margin;
    const maxY = window.innerHeight - rect.height - margin;

    posX = Math.max(margin, Math.min(rawX, maxX));
    posY = Math.max(margin, Math.min(rawY, maxY));
  });
</script>

<svelte:window
  oncontextmenu={handleContextMenu}
  onclick={close}
  onblur={close}
  onkeydown={(event) => event.key === "Escape" && close()}
/>

{#if open}
  <div
    class="ctx"
    style="left: {posX}px; top: {posY}px;"
    bind:this={menuElement}
    role="menu"
    aria-label="{title} actions"
  >
    <div class="ctx__title">{title}</div>

    {#each entries as entry, index (isAction(entry) ? entry.label : `separator-${index}`)}
      {#if isAction(entry)}
        {@const Icon = entry.icon}
        <button
          class="ctx__item"
          data-danger={entry.danger ?? false}
          disabled={entry.disabled ?? false}
          onclick={() => choose(entry)}
          role="menuitem"
        >
          <Icon size={15} weight="duotone" />
          {entry.label}
        </button>
      {:else}
        <div class="ctx__separator" role="separator"></div>
      {/if}
    {/each}
  </div>
{/if}
