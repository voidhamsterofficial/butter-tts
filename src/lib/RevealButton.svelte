<script lang="ts">
  // A small "show me this file" button, used next to the database path on the Settings
  // and History pages so the file can be backed up without hunting for it. Opens the
  // containing folder in Finder/Explorer with the file selected.
  import { revealItemInDir } from "@tauri-apps/plugin-opener";
  import FolderOpen from "phosphor-svelte/lib/FolderOpen";
  import WarningCircle from "phosphor-svelte/lib/WarningCircle";

  let { path }: { path: string } = $props();
  let failed = $state(false);

  async function reveal() {
    failed = false;
    try {
      await revealItemInDir(path);
    } catch {
      // Rare — the file was moved or deleted out from under us. Say so on the button
      // rather than failing silently.
      failed = true;
    }
  }
</script>

<button
  type="button"
  class="button button--ghost button--small"
  onclick={reveal}
  disabled={path === ""}
>
  {#if failed}
    <WarningCircle size={13} weight="fill" />
    Couldn't open it
  {:else}
    <FolderOpen size={13} weight="duotone" />
    Show file
  {/if}
</button>
