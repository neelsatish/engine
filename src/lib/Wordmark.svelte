<script lang="ts">
  // The ENGINE wordmark in chunky pixel letters, with a terminal-style menu under it (arrow keys + Enter, or click).
  // While Engine listens, the letters ripple column by column with your voice.
  import { invoke } from "@tauri-apps/api/core";
  import { getVersion } from "@tauri-apps/api/app";
  import { onMount } from "svelte";
  import { engine, needsYou, type View } from "$lib/state.svelte";

  // 6 x 7 pixel glyphs: # is a filled cell.
  const GLYPHS: Record<string, string[]> = {
    E: ["######", "##....", "##....", "#####.", "##....", "##....", "######"],
    N: ["##..##", "###.##", "######", "##.###", "##..##", "##..##", "##..##"],
    G: [".####.", "##..##", "##....", "##.###", "##..##", "##..##", ".#####"],
    I: ["######", "..##..", "..##..", "..##..", "..##..", "..##..", "######"],
  };
  const WORD = "ENGINE";
  const COLS = WORD.length * 7 - 1;

  // Cells grouped by column, so each column can ripple on its own.
  const columns = Array.from({ length: COLS }, (_, x) => {
    const letter = GLYPHS[WORD[Math.floor(x / 7)]];
    const cx = x % 7;
    return cx === 6 ? [] : letter.flatMap((row, y) => (row[cx] === "#" ? [y] : []));
  });

  const listening = $derived(engine.voice === "listening");
  const amp = $derived(listening ? 0.25 + Math.min(1, engine.level) * 0.9 : 0); // in pixel cells

  let version = $state("");
  onMount(() => {
    getVersion().then((v) => (version = v)).catch(() => {});
  });

  type Entry = { label: string; view?: View; run?: () => void; count?: number };
  const asks = $derived(needsYou().length);
  const entries = $derived<Entry[]>([
    { label: "home", view: "home" },
    { label: "briefing", view: "briefing", count: asks },
    { label: "schedule", view: "schedule" },
    { label: "settings", view: "settings" },
    { label: "review my day", run: () => call("review_day", "Couldn't start the review") },
    { label: "read aloud", run: () => call("speak_briefing", "Couldn't read the briefing") },
  ]);

  async function call(command: string, failure: string) {
    try {
      await invoke(command);
    } catch (e) {
      engine.note = `${failure}: ${e}`;
    }
  }

  function choose(e: Entry) {
    if (e.view) engine.view = e.view;
    e.run?.();
  }

  let menu: HTMLUListElement;
  function onMenuKey(e: KeyboardEvent) {
    if (e.key !== "ArrowDown" && e.key !== "ArrowUp") return;
    e.preventDefault();
    const items = [...menu.querySelectorAll("button")];
    const at = items.indexOf(document.activeElement as HTMLButtonElement);
    const next = (at + (e.key === "ArrowDown" ? 1 : -1) + items.length) % items.length;
    items[next]?.focus();
  }
</script>

<fieldset class="pane wordmark">
  <legend><sup>2</sup>engine</legend>
  <svg
    class="mark"
    class:listening
    viewBox="-0.3 -0.3 {COLS + 0.6} 7.6"
    role="img"
    aria-label="Engine"
    shape-rendering="crispEdges"
    style="--amp: {amp}"
  >
    <defs>
      <!-- One gradient across the whole word (not one per cell), so the letters read as solid blocks. -->
      <linearGradient id="velvet" gradientUnits="userSpaceOnUse" x1="0" y1="0" x2={COLS} y2="7">
        <stop offset="0" stop-color="#c9cbff" />
        <stop offset="0.55" stop-color="#8f92ee" />
        <stop offset="1" stop-color="#6f7fc4" />
      </linearGradient>
    </defs>
    <!-- All shadows first, then all letters, so no shadow falls across a neighbouring column. -->
    {#each ["shade", "cell"] as layer (layer)}
      {#each columns as cells, x (x)}
        <g class="col" style="animation-delay: {-x * 55}ms">
          {#each cells as y (y)}
            {@const off = layer === "shade" ? 0.22 : 0}
            <rect class={layer} x={x + off} y={y + off} width="1.02" height="1.02" />
          {/each}
        </g>
      {/each}
    {/each}
  </svg>
  <div class="ver">v{version}</div>

  <div class="below">
    <ul class="menu" role="menu" bind:this={menu} onkeydown={onMenuKey} aria-label="Menu">
      {#each entries as e (e.label)}
        {@const current = e.view === engine.view}
        <li role="none">
          <button role="menuitem" class:current aria-current={current ? "page" : undefined} onclick={() => choose(e)}>
            <span class="caret" aria-hidden="true">›</span>{e.label}{#if e.count}<span class="n">{e.count}</span>{/if}
          </button>
        </li>
      {/each}
    </ul>
    <p class="keys"><kbd>↑</kbd><kbd>↓</kbd> navigate · <kbd>enter</kbd> select · hold <kbd>`</kbd> or say “engine”</p>
  </div>
</fieldset>

<style>
  .wordmark {
    display: grid;
    grid-template-rows: auto auto minmax(0, 1fr);
    padding: 10px 16px 10px;
    overflow: hidden;
  }

  .mark {
    display: block;
    width: min(100%, 420px);
    height: auto;
    overflow: visible;
  }

  .cell {
    fill: url(#velvet);
  }

  .shade {
    fill: #2a2650;
  }

  .listening .col {
    animation: ripple 900ms ease-in-out infinite;
  }

  @keyframes ripple {
    0%,
    100% {
      transform: translateY(0);
    }
    50% {
      transform: translateY(calc(var(--amp) * -1px));
    }
  }

  .ver {
    width: min(100%, 420px);
    margin-top: 2px;
    color: var(--text-muted);
    font-size: 11px;
    text-align: right;
  }

  .below {
    display: flex;
    flex-direction: column;
    justify-content: flex-end;
    min-height: 0;
  }

  .menu {
    margin: 0;
    padding: 0;
    list-style: none;
  }

  .menu button {
    display: flex;
    align-items: center;
    gap: 6px;
    height: 20px;
    padding: 0 4px 0 0;
    border: 0;
    border-radius: var(--r-sm);
    background: transparent;
    color: var(--text-muted);
    font-size: 12px;
    text-align: left;
  }

  .caret {
    width: 1ch;
    color: transparent;
  }

  .menu button:hover,
  .menu button:focus-visible,
  .menu button.current {
    color: var(--text);
  }

  .menu button:hover .caret,
  .menu button:focus-visible .caret,
  .menu button.current .caret {
    color: var(--accent);
  }

  .menu button:focus-visible {
    outline: 0;
    background: rgb(142 144 245 / 0.12);
  }

  .n {
    padding: 0 5px;
    border-radius: var(--r-sm);
    background: rgb(240 113 138 / 0.18);
    color: var(--meter-hi);
    font-size: 11px;
  }

  .keys {
    margin: 8px 0 0;
    color: var(--text-muted);
    font-size: 11px;
  }

  .keys kbd {
    padding: 0 3px;
    border-bottom-width: 1px;
    font-size: 10px;
  }
</style>
