<script lang="ts">
  import "@fontsource-variable/jetbrains-mono";
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
  import Wordmark from "$lib/Wordmark.svelte";
  import { engine, needsYou, running, savePrefs, startListeners, type View } from "$lib/state.svelte";

  // The views, like workspaces in a tiling desktop: Alt+1..4 switches between them.
  const VIEWS: { id: View; label: string }[] = [
    { id: "home", label: "home" },
    { id: "briefing", label: "briefing" },
    { id: "schedule", label: "schedule" },
    { id: "settings", label: "settings" },
  ];

  let chat = $state<Chat>();
  let update = $state.raw<Update | null>(null);
  let updating = $state(false);
  // Set when Quit is chosen while agents are working: how many would be stopped.
  let quitting = $state(0);
  const asks = $derived(needsYou().length);
  const agents = $derived(running().length);
  const v = $derived((engine.prefs?.voice ?? {}) as Record<string, unknown>);

  // The theme is an attribute on <html>, so app.css can swap the whole palette.
  const mono = $derived(engine.prefs?.theme === "mono");
  $effect(() => {
    document.documentElement.dataset.theme = mono ? "mono" : "velvet";
  });

  const clock = () =>
    new Date().toLocaleString(undefined, { weekday: "long", day: "numeric", month: "short", hour: "2-digit", minute: "2-digit" });
  let now = $state(clock());

  async function summon() {
    now = clock();
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
    // Esc closes an open menu first; only with nothing open does it hide Engine.
    if (e.key === "Escape" && !e.defaultPrevented && !document.querySelector('[aria-expanded="true"]')) getCurrentWindow().hide();
    if (e.altKey && e.key >= "1" && e.key <= "4") {
      e.preventDefault();
      engine.view = VIEWS[Number(e.key) - 1].id;
    }
  }

  onMount(() => {
    const stop = startListeners((text, submitNow) => chat?.useHeard(text, submitNow));
    const offs = [listen("summon", summon), listen<number>("confirm-quit", (e) => (quitting = e.payload))];
    lookForUpdate();
    const timer = setInterval(lookForUpdate, 6 * 60 * 60 * 1000);
    const ticker = setInterval(() => (now = clock()), 15_000);
    chat?.focus();
    return () => {
      stop();
      offs.forEach((off) => off.then((f) => f()));
      clearInterval(timer);
      clearInterval(ticker);
    };
  });
</script>

<svelte:window onkeydown={onKey} />

<MatrixRain />

<main class:home={engine.view === "home"}>
  <header data-tauri-drag-region>
    <nav class="spaces" aria-label="Views">
      {#each VIEWS as view, n (view.id)}
        <button
          class="space"
          aria-current={engine.view === view.id ? "page" : undefined}
          title="{view.label} (Alt+{n + 1})"
          onclick={() => (engine.view = view.id)}
        >
          {n + 1}<span class="space-name">{view.label}</span>{#if view.id === "briefing" && asks}<span class="flag">{asks}</span>{/if}
        </button>
      {/each}
    </nav>
    <span class="clock" data-tauri-drag-region>{now}</span>
    <div class="tray" data-tauri-drag-region>
      {#if agents}<span class="ind" title="Agents working"><Orb process="working" size={12} />{agents}</span>{/if}
      <span class="ind" class:on={engine.voice === "listening"} title="Microphone">
        {engine.voice === "listening" ? "● rec" : v.wake === false ? "mic" : "wake"}
      </span>
      <button
        class="ind"
        title="Theme: {mono ? 'black and white' : 'velvet'} (click to switch)"
        aria-label="Switch theme"
        onclick={() => savePrefs({ theme: mono ? "velvet" : "mono" })}>{mono ? "b&w" : "velvet"}</button
      >
      <span class="ind" class:on={engine.prefs?.local_ai} title="Where Engine thinks">{engine.prefs?.local_ai ? "local" : "cloud"}</span>
      {#if update}
        <button class="ind update" onclick={installUpdate} disabled={updating} title="Restart Engine on version {update.version}">
          {updating ? "updating…" : `↑ ${update.version}`}
        </button>
      {/if}
      <button class="ind hide" aria-label="Hide Engine" title="Hide (Esc)" onclick={() => getCurrentWindow().hide()}>—</button>
    </div>
  </header>

  {#if engine.view === "home"}
    <div class="sys"><LoadPanel /></div>
    <div class="fetch"><StageHome /></div>
  {:else}
    <fieldset class="pane view">
      <legend><sup>1</sup>{engine.view}</legend>
      <div class="view-body">
        {#if engine.view === "briefing"}
          <Briefing />
        {:else if engine.view === "schedule"}
          <Schedule />
        {:else}
          <Settings onEditAliases={() => quick("edit_aliases", "Couldn't open shortcuts")} />
        {/if}
      </div>
    </fieldset>
  {/if}

  <div class="word"><Wordmark /></div>
  <div class="talk"><Chat bind:this={chat} /></div>

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
    grid-template-columns: minmax(0, 1fr) minmax(0, 1fr);
    grid-template-rows: 26px minmax(0, 0.82fr) minmax(0, 1.18fr);
    grid-template-areas:
      "bar  bar"
      "view word"
      "view talk";
    gap: 8px;
    height: 100vh;
    padding: 0 8px 8px;
    border: 1px solid var(--pane-border);
  }

  main.home {
    grid-template-areas:
      "bar   bar"
      "sys   word"
      "fetch talk";
  }

  main > div {
    display: grid;
    min-width: 0;
    min-height: 0;
  }

  .sys {
    grid-area: sys;
  }

  .fetch {
    grid-area: fetch;
  }

  .word {
    grid-area: word;
  }

  .talk {
    grid-area: talk;
  }

  .view {
    grid-area: view;
    display: flex;
    flex-direction: column;
  }

  .view-body {
    display: flex;
    flex: 1;
    flex-direction: column;
    min-height: 0;
    padding-top: 4px;
  }

  /* The top bar, like a tiling desktop's status bar. */
  header {
    grid-area: bar;
    display: grid;
    grid-template-columns: 1fr auto 1fr;
    align-items: center;
    font-size: 12px;
  }

  .spaces {
    display: flex;
    gap: 2px;
  }

  .space {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    height: 20px;
    padding: 0 7px;
    border: 0;
    border-radius: var(--r-sm);
    background: transparent;
    color: var(--text-muted);
    font-size: 12px;
  }

  .space-name {
    color: inherit;
  }

  .space:hover {
    color: var(--text);
  }

  .space[aria-current="page"] {
    background: rgb(142 144 245 / 0.16);
    color: var(--pane-label);
  }

  .flag {
    padding: 0 4px;
    border-radius: var(--r-sm);
    background: rgb(240 113 138 / 0.2);
    color: var(--meter-hi);
    font-size: 10px;
  }

  .clock {
    color: var(--text-secondary);
    white-space: nowrap;
  }

  .tray {
    display: flex;
    align-items: center;
    justify-content: flex-end;
    gap: 2px;
  }

  .ind {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    height: 20px;
    padding: 0 7px;
    border: 0;
    border-radius: var(--r-sm);
    background: transparent;
    color: var(--text-muted);
    font-size: 11px;
  }

  .ind.on {
    color: var(--pane-label);
  }

  button.ind:hover:not(:disabled) {
    background: rgb(142 144 245 / 0.12);
    color: var(--text);
  }

  .ind.update {
    color: var(--success);
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
