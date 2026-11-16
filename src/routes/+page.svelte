<script lang="ts">
  import "@fontsource-variable/inter";
  import "../app.css";
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { relaunch } from "@tauri-apps/plugin-process";
  import { check, type Update } from "@tauri-apps/plugin-updater";
  import { onMount, tick } from "svelte";
  import Briefing from "$lib/Briefing.svelte";
  import MatrixRain from "$lib/MatrixRain.svelte";
  import Orb from "$lib/Orb.svelte";
  import Schedule from "$lib/Schedule.svelte";
  import Settings from "$lib/Settings.svelte";

  type View = "briefing" | "schedule" | "settings";
  const VIEWS: { id: View; label: string }[] = [
    { id: "briefing", label: "Briefing" },
    { id: "schedule", label: "Schedule" },
    { id: "settings", label: "Settings" },
  ];

  let view = $state<View>("briefing");
  let briefing = $state<Briefing>();
  let update = $state.raw<Update | null>(null);
  let updating = $state(false);
  let note = $state("");

  const dateLabel = () => new Date().toLocaleDateString(undefined, { weekday: "long", day: "numeric", month: "long" });
  let today = $state(dateLabel());

  async function summon() {
    today = dateLabel();
    view = "briefing";
    await tick();
    briefing?.focus();
  }

  // Checks GitHub Releases. A new version downloads in the background, then waits for one click.
  async function lookForUpdate() {
    if (update) return;
    try {
      const found = await check();
      if (found) {
        await found.download();
        update = found;
      }
    } catch (e) {
      console.warn("update check failed", e); // offline, or no release yet: the next check retries
    }
  }

  async function installUpdate() {
    if (!update) return;
    updating = true;
    try {
      await update.install(); // on Windows this runs the installer, which closes Engine
      await relaunch();
    } catch (e) {
      note = `Update failed: ${e}`;
      updating = false;
    }
  }

  async function editAliases() {
    try {
      await invoke("edit_aliases");
    } catch (e) {
      note = `Couldn't open aliases: ${e}`;
    }
  }

  function onKey(e: KeyboardEvent) {
    if (e.key === "Escape") getCurrentWindow().hide();
  }

  onMount(() => {
    const off = listen("summon", summon);
    lookForUpdate();
    const timer = setInterval(lookForUpdate, 6 * 60 * 60 * 1000);
    return () => {
      off.then((f) => f());
      clearInterval(timer);
    };
  });
</script>

<svelte:window onkeydown={onKey} />

<MatrixRain />

<main>
  <header data-tauri-drag-region>
    <div class="brand" data-tauri-drag-region>
      <img class="mark" src="/logo.svg" alt="" width="22" height="22" />
      <span class="name" data-tauri-drag-region>Engine</span>
      <span class="date" data-tauri-drag-region>{today}</span>
    </div>
    <nav aria-label="Views">
      {#each VIEWS as v (v.id)}
        <button class="tab" aria-current={view === v.id ? "page" : undefined} onclick={() => (view = v.id)}>
          {v.label}
        </button>
      {/each}
    </nav>
    <button class="icon" aria-label="Hide Engine" title="Hide (Esc)" onclick={() => getCurrentWindow().hide()}>
      <svg class="ico" viewBox="0 0 24 24" aria-hidden="true"><path d="M5 12h14" /></svg>
    </button>
  </header>

  {#if view === "briefing"}
    <Briefing bind:this={briefing} />
  {:else if view === "schedule"}
    <Schedule />
  {:else}
    <Settings onEditAliases={editAliases} />
  {/if}

  <footer>
    {#if note}
      <span class="note" role="alert">{note}</span>
    {:else}
      <span><kbd>Ctrl</kbd> <kbd>Shift</kbd> <kbd>Space</kbd> summon · <kbd>Esc</kbd> hide</span>
    {/if}
    <span class="actions">
      {#if update}
        <button class="link update" onclick={installUpdate} disabled={updating}>
          {#if updating}<Orb process="updating" size={14} />{/if}
          {updating ? "Updating…" : `Restart to update to ${update.version}`}
        </button>
      {/if}
    </span>
  </footer>
</main>

<style>
  main {
    position: relative;
    z-index: 1;
    display: flex;
    flex-direction: column;
    height: 100vh;
    padding: 0 var(--space-lg) var(--space-md);
    border: 1px solid var(--border);
  }

  header {
    display: flex;
    align-items: center;
    gap: var(--space-md);
    height: 52px;
    flex-shrink: 0;
  }

  .brand {
    display: flex;
    flex: 1;
    align-items: center;
    gap: 10px;
    min-width: 0;
  }

  .mark {
    display: block;
    pointer-events: none;
  }

  .name {
    font-weight: 650;
    letter-spacing: 0.01em;
  }

  .date {
    overflow: hidden;
    color: var(--text-muted);
    font-size: 13px;
    white-space: nowrap;
    text-overflow: ellipsis;
  }

  .date::before {
    content: "·";
    margin-right: 10px;
  }

  nav {
    display: flex;
    gap: 2px;
    padding: 3px;
    border: 1px solid var(--border);
    border-radius: 10px;
    background: var(--glass);
    backdrop-filter: blur(14px);
  }

  .tab {
    padding: 4px 12px;
    border: 0;
    border-radius: 7px;
    background: transparent;
    color: var(--text-muted);
    font-size: 13px;
    transition: background var(--ease), color var(--ease);
  }

  .tab:hover {
    color: var(--text);
  }

  .tab[aria-current="page"] {
    background: var(--surface-2);
    color: var(--text);
  }

  .icon {
    display: grid;
    place-items: center;
    width: 32px;
    height: 32px;
    border: 0;
    border-radius: 8px;
    background: transparent;
    color: var(--text-muted);
    transition: background var(--ease), color var(--ease);
  }

  .icon:hover {
    background: var(--surface-2);
    color: var(--text);
  }

  footer {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-md);
    padding-top: var(--space-md);
    border-top: 1px solid var(--border);
    color: var(--text-muted);
    font-size: 12px;
  }

  .note {
    overflow: hidden;
    color: var(--danger);
    text-overflow: ellipsis;
    white-space: nowrap;
    user-select: text;
  }

  .actions {
    display: flex;
    gap: var(--space-xs);
  }

  .update {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    color: var(--accent);
    font-weight: 600;
  }
</style>
