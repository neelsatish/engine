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
  import Chat from "$lib/Chat.svelte";
  import LoadPanel from "$lib/LoadPanel.svelte";
  import MatrixRain from "$lib/MatrixRain.svelte";
  import Orb from "$lib/Orb.svelte";
  import Schedule from "$lib/Schedule.svelte";
  import Settings from "$lib/Settings.svelte";
  import StageHome from "$lib/StageHome.svelte";
  import { engine, needsYou, startListeners, type View } from "$lib/state.svelte";

  // Each icon is a list of paths for a 24-unit stroke icon.
  const TABS: { id: View; label: string; icon: string[] }[] = [
    { id: "home", label: "Home", icon: ["M4 11l8-7 8 7", "M6 9.5V20h12V9.5"] },
    { id: "briefing", label: "Briefing", icon: ["M5 5h14v14H5z", "M8 9h8", "M8 13h8", "M8 17h5"] },
    { id: "schedule", label: "Schedule", icon: ["M4 6h16v14H4z", "M4 10h16", "M8 3v5", "M16 3v5"] },
    { id: "settings", label: "Settings", icon: ["M12 15a3 3 0 1 0 0-6 3 3 0 0 0 0 6z", "M19 12a7 7 0 0 0-.1-1.2l2-1.6-2-3.4-2.4 1a7 7 0 0 0-2-1.2L14 3h-4l-.5 2.6a7 7 0 0 0-2 1.2l-2.4-1-2 3.4 2 1.6a7 7 0 0 0 0 2.4l-2 1.6 2 3.4 2.4-1a7 7 0 0 0 2 1.2L10 21h4l.5-2.6a7 7 0 0 0 2-1.2l2.4 1 2-3.4-2-1.6c.1-.4.1-.8.1-1.2z"] },
  ];

  let chat = $state<Chat>();
  let update = $state.raw<Update | null>(null);
  let updating = $state(false);
  // Set when Quit is chosen while agents are working: how many would be stopped.
  let quitting = $state(0);
  const asks = $derived(needsYou().length);

  const dateLabel = () => new Date().toLocaleDateString(undefined, { weekday: "long", day: "numeric", month: "long" });
  let today = $state(dateLabel());

  async function summon() {
    today = dateLabel();
    await tick();
    chat?.focus();
    if (engine.prefs?.voice.speak_on_summon) invoke("speak_briefing").catch(() => {});
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
      engine.note = `Update failed: ${e}`;
      updating = false;
    }
  }

  async function quick(command: string, failure: string) {
    try {
      await invoke(command);
    } catch (e) {
      engine.note = `${failure}: ${e}`;
    }
  }

  function onKey(e: KeyboardEvent) {
    if (e.key === "Escape") getCurrentWindow().hide();
  }

  onMount(() => {
    const stop = startListeners((text, submitNow) => chat?.useHeard(text, submitNow));
    const offs = [listen("summon", summon), listen<number>("confirm-quit", (e) => (quitting = e.payload))];
    lookForUpdate();
    const timer = setInterval(lookForUpdate, 6 * 60 * 60 * 1000);
    chat?.focus();
    return () => {
      stop();
      offs.forEach((off) => off.then((f) => f()));
      clearInterval(timer);
    };
  });
</script>

<svelte:window onkeydown={onKey} />

<MatrixRain />

<main>
  <header data-tauri-drag-region>
    <div class="brand" data-tauri-drag-region>
      <img class="mark" src="/logo.svg" alt="" width="20" height="20" />
      <span class="name" data-tauri-drag-region>Engine</span>
      <span class="date" data-tauri-drag-region>{today}</span>
    </div>
    {#if update}
      <button class="button primary update-pill" onclick={installUpdate} disabled={updating} title="Restart Engine on version {update.version}">
        {#if updating}<Orb process="updating" size={14} />{/if}
        {updating ? "Updating…" : `Update to ${update.version}`}
      </button>
    {/if}
    <button class="icon-btn" aria-label="Hide Engine" title="Hide (Esc)" onclick={() => getCurrentWindow().hide()}>
      <svg class="ico" viewBox="0 0 24 24" aria-hidden="true"><path d="M5 12h14" /></svg>
    </button>
  </header>

  <div class="left">
  <section class="stage glass" class:flat={engine.view !== "home"} aria-label={TABS.find((t) => t.id === engine.view)?.label}>
    {#if engine.view === "home"}
      <StageHome />
    {:else if engine.view === "briefing"}
      <div class="view"><Briefing /></div>
    {:else if engine.view === "schedule"}
      <div class="view"><Schedule /></div>
    {:else}
      <div class="view"><Settings onEditAliases={() => quick("edit_aliases", "Couldn't open shortcuts")} /></div>
    {/if}
  </section>

  <div class="dock">
    <nav class="rail glass" aria-label="Views">
      {#each TABS as t (t.id)}
        <button class="rail-item" aria-current={engine.view === t.id ? "page" : undefined} onclick={() => (engine.view = t.id)}>
          <svg class="ico" viewBox="0 0 24 24" aria-hidden="true">{#each t.icon as d (d)}<path {d} />{/each}</svg>
          <span>{t.label}</span>
          {#if t.id === "briefing" && asks}<span class="count">{asks}</span>{/if}
        </button>
      {/each}
      <div class="rail-actions">
        <button class="icon-btn" aria-label="Review my day" title="Review my day" onclick={() => quick("review_day", "Couldn't start the review")}>
          <svg class="ico" viewBox="0 0 24 24" aria-hidden="true"><path d="M12 3a6 6 0 0 0 9 9 9 9 0 1 1-9-9" /></svg>
        </button>
        <button class="icon-btn" aria-label="Read the briefing aloud" title="Read the briefing aloud" onclick={() => quick("speak_briefing", "Couldn't read the briefing")}>
          <svg class="ico" viewBox="0 0 24 24" aria-hidden="true"
            ><path d="M11 5 6 9H3v6h3l5 4z" /><path d="M15.5 8.5a5 5 0 0 1 0 7" /><path d="M18.5 5.5a9 9 0 0 1 0 13" /></svg
          >
        </button>
      </div>
    </nav>

    <LoadPanel />
  </div>
  </div>

  <Chat bind:this={chat} />

  {#if quitting}
    <div class="confirm glass" role="alertdialog" aria-label="Quit Engine?">
      <span>
        {quitting === 1 ? "An agent is" : `${quitting} agents are`} still working. Quitting stops
        {quitting === 1 ? "it" : "them"}; you can retry later.
      </span>
      <button class="button primary" onclick={() => invoke("quit_now")}>Quit and stop</button>
      <button class="button" onclick={() => (quitting = 0)}>Keep working</button>
    </div>
  {/if}
</main>

<style>
  main {
    position: relative;
    z-index: 1;
    display: grid;
    grid-template-rows: 44px minmax(0, 1fr);
    grid-template-columns: minmax(0, 1fr) clamp(320px, 36%, 440px);
    gap: 12px;
    height: 100vh;
    padding: 0 16px 16px;
    border: 1px solid var(--border);
  }

  /* The stage and the dock under it; the chat runs the full height beside them. */
  .left {
    display: grid;
    grid-template-rows: minmax(0, 1fr) 212px;
    gap: 12px;
    min-width: 0;
    min-height: 0;
  }

  header {
    grid-column: 1 / -1;
    display: flex;
    align-items: center;
    gap: var(--space-sm);
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

  .update-pill {
    height: 28px;
    padding: 0 12px;
  }

  .stage {
    position: relative;
    min-height: 0;
    overflow: hidden;
  }

  /* The other views bring their own panels: the stage steps back to a plain frame. */
  .stage.flat {
    background: rgb(14 12 21 / 0.35);
  }

  .view {
    display: flex;
    flex-direction: column;
    height: 100%;
    min-height: 0;
    padding: 14px 16px 0;
  }

  .dock {
    display: grid;
    grid-template-columns: 180px minmax(0, 1fr);
    gap: 12px;
    min-height: 0;
  }

  .rail {
    display: flex;
    flex-direction: column;
    gap: 2px;
    padding: 8px;
  }

  .rail-item {
    display: flex;
    align-items: center;
    gap: 10px;
    height: 36px;
    padding: 0 10px;
    border: 0;
    border-radius: var(--r-sm);
    background: transparent;
    color: var(--text-muted);
    font-size: 13px;
    text-align: left;
    transition: background-color var(--ease), color var(--ease);
  }

  .rail-item span:not(.count) {
    flex: 1;
  }

  .rail-item svg {
    width: 17px;
    height: 17px;
  }

  .rail-item:hover {
    color: var(--text);
  }

  .rail-item[aria-current="page"] {
    background: var(--surface-2);
    color: var(--text);
  }

  .rail-item[aria-current="page"] svg {
    color: var(--accent);
  }

  .rail-actions {
    display: flex;
    gap: 4px;
    margin-top: auto;
    padding-top: 6px;
    border-top: 1px solid var(--line);
  }

  .confirm {
    position: absolute;
    right: 16px;
    bottom: 16px;
    left: 16px;
    z-index: 20;
    display: flex;
    align-items: center;
    gap: var(--space-sm);
    padding: var(--space-sm) var(--space-md);
    border-color: rgb(242 184 92 / 0.5);
    font-size: 13px;
  }

  .confirm span {
    flex: 1;
  }
</style>
