<script lang="ts">
  import "@fontsource-variable/inter";
  import "../app.css";
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { relaunch } from "@tauri-apps/plugin-process";
  import { check, type Update } from "@tauri-apps/plugin-updater";
  import { onMount } from "svelte";
  import MatrixRain from "$lib/MatrixRain.svelte";
  import Orb, { type Process } from "$lib/Orb.svelte";

  type Target = { name: string; target: string };
  type Reply =
    | { kind: "opened"; name: string }
    | { kind: "choices"; items: Target[] }
    | { kind: "routed"; count: number }
    | { kind: "error"; message: string };
  type Item = {
    id: number;
    kind: "instant" | "answer" | "job" | "outward";
    title: string;
    status: "queued" | "running" | "held" | "done" | "failed";
    process: string;
    result: string;
    created: string;
  };

  let text = $state("");
  let sorting = $state(0);
  let note = $state("");
  let choices = $state<Target[]>([]);
  let items = $state<Item[]>([]);
  let update = $state.raw<Update | null>(null);
  let updating = $state(false);
  let input: HTMLTextAreaElement;

  const dateLabel = () => new Date().toLocaleDateString(undefined, { weekday: "long", day: "numeric", month: "long" });
  let today = $state(dateLabel());

  // Shown while the briefing is empty: one click puts an example in the dump box.
  const EXAMPLES = [
    "open spotify",
    "make me a 5-day study plan for chemistry",
    "research the best budget headphones and open the top pick",
  ];

  function tryExample(example: string) {
    text = example;
    input.focus();
  }

  const now = $derived(items.filter((i) => i.kind === "instant"));
  const running = $derived(items.filter((i) => i.kind !== "instant" && (i.status === "running" || i.status === "queued")));
  const needsYou = $derived(items.filter((i) => i.kind !== "instant" && (i.status === "held" || i.status === "failed")));
  const done = $derived(items.filter((i) => i.kind !== "instant" && i.status === "done"));
  const sections = $derived(
    [
      { name: "Now", items: now },
      { name: "Running", items: running },
      { name: "Needs you", items: needsYou },
      { name: "Done", items: done },
    ].filter((s) => s.items.length),
  );

  async function load() {
    today = dateLabel();
    try {
      items = await invoke<Item[]>("briefing");
    } catch (e) {
      note = `Couldn't load the briefing: ${e}`;
    }
  }

  function show(reply: Reply, said = "") {
    choices = [];
    if (reply.kind === "choices") choices = reply.items;
    if (reply.kind === "error") {
      note = reply.message;
      if (said && !text.trim()) text = said; // give the dump back so nothing is lost
    }
  }

  async function submit() {
    const said = text.trim();
    if (!said) return;
    text = "";
    note = "";
    sorting++;
    try {
      show(await invoke<Reply>("submit", { text: said }), said);
    } catch (e) {
      show({ kind: "error", message: String(e) }, said);
    } finally {
      sorting--;
      input.focus();
    }
  }

  async function pick(choice: Target) {
    show(await invoke<Reply>("open_choice", { choice }));
    input.focus();
  }

  async function act(command: "dismiss" | "cancel" | "retry", id: number) {
    try {
      await invoke(command, { id });
    } catch (e) {
      note = `Couldn't ${command}: ${e}`;
    }
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

  function onInputKey(e: KeyboardEvent) {
    if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      submit();
    }
  }

  // Work in progress shows an orb for what it's doing; finished items get a plain dot.
  // A running job shows the orb for its current step (viewing, searching, writing...); a running answer is writing.
  const orbFor = (i: Item): Process | undefined =>
    i.status === "running"
      ? ((i.process || "writing") as Process)
      : ({ queued: "waiting", held: "held" } as Partial<Record<Item["status"], Process>>)[i.status];
  const time = (i: Item) => i.created.slice(11, 16);

  onMount(() => {
    input.focus();
    load();
    const offs = [listen("summon", () => input.focus()), listen("briefing", load)];
    lookForUpdate();
    const timer = setInterval(lookForUpdate, 6 * 60 * 60 * 1000);
    return () => {
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
      <img class="mark" src="/logo.svg" alt="" width="22" height="22" />
      <span class="name" data-tauri-drag-region>Engine</span>
      <span class="date" data-tauri-drag-region>{today}</span>
    </div>
    <button class="icon" aria-label="Hide Engine" title="Hide (Esc)" onclick={() => getCurrentWindow().hide()}>
      <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M5 12h14" /></svg>
    </button>
  </header>

  <form
    class="dump"
    onsubmit={(e) => {
      e.preventDefault();
      submit();
    }}
  >
    <label for="dump">Dump</label>
    <div class="field glass">
      <textarea
        id="dump"
        bind:this={input}
        bind:value={text}
        onkeydown={onInputKey}
        rows="2"
        placeholder="Dump anything: open an app, ask for a plan, hand off a task."
        spellcheck="false"
      ></textarea>
      <button class="send" type="submit" aria-label="Send" title="Send (Enter)" disabled={!text.trim()}>
        <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M9 10 4 15l5 5" /><path d="M20 4v7a4 4 0 0 1-4 4H4" /></svg>
      </button>
    </div>
    <p class="status" aria-live="polite">
      {#if sorting}
        <span class="indicator"><Orb process="thinking" size={16} /></span>Sorting your dump…
      {:else if note}
        <span class="indicator"><span class="dot warn" aria-hidden="true"></span></span>{note}
      {/if}
    </p>
  </form>

  {#if choices.length}
    <section class="card glass" aria-label="Which one?">
      <h2>Which one?</h2>
      <div class="chips">
        {#each choices as c (c.target)}
          <button class="chip" onclick={() => pick(c)}>{c.name}</button>
        {/each}
      </div>
    </section>
  {/if}

  <div class="briefing" aria-live="polite">
    {#each sections as s (s.name)}
      <section class="card glass">
        <h2>{s.name} <span class="count">{s.items.length}</span></h2>
        <ul>
          {#each s.items as i (i.id)}
            {@const process = orbFor(i)}
            <li>
              <span class="indicator">
                {#if process}
                  <Orb {process} />
                {:else}
                  <span class="dot {i.status === 'failed' ? 'warn' : 'ok'}" aria-hidden="true"></span>
                {/if}
              </span>
              <div class="body">
                {#if i.kind === "instant"}
                  <span class="text">{i.result || i.title}</span>
                {:else if i.status === "done"}
                  <details>
                    <summary class="text">{i.title}</summary>
                    <pre>{i.result}</pre>
                  </details>
                {:else}
                  <span class="text">{i.title}</span>
                  {#if i.result}<span class="sub">{i.result}</span>{/if}
                {/if}
              </div>
              {#if i.kind === "job" && i.status === "running"}
                <button class="link" onclick={() => act("cancel", i.id)}>Cancel</button>
              {:else if i.kind !== "instant" && i.status !== "running" && i.status !== "done"}
                {#if i.kind === "job" && i.status === "failed"}
                  <button class="link" onclick={() => act("retry", i.id)}>Retry</button>
                {/if}
                <button class="link" onclick={() => act("dismiss", i.id)}>Dismiss</button>
              {/if}
              <time>{time(i)}</time>
            </li>
          {/each}
        </ul>
      </section>
    {:else}
      <section class="card glass empty">
        <img src="/logo.svg" alt="" width="40" height="40" />
        <p>Nothing yet today. Dump anything above, or try one:</p>
        <div class="chips">
          {#each EXAMPLES as example (example)}
            <button class="chip" onclick={() => tryExample(example)}>{example}</button>
          {/each}
        </div>
      </section>
    {/each}
  </div>

  <footer>
    <span><kbd>Ctrl</kbd> <kbd>Shift</kbd> <kbd>Space</kbd> summon · <kbd>Esc</kbd> hide</span>
    <span class="actions">
      {#if update}
        <button class="link update" onclick={installUpdate} disabled={updating}>
          {#if updating}<Orb process="updating" size={14} />{/if}
          {updating ? "Updating…" : `Restart to update to ${update.version}`}
        </button>
      {/if}
      <button class="link" onclick={editAliases}>Edit aliases</button>
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

  /* Panels over the rain: translucent and blurred, so text never sits on the moving characters. */
  .glass {
    background: var(--glass);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    backdrop-filter: blur(14px);
  }

  header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    height: 52px;
    flex-shrink: 0;
  }

  .brand {
    display: flex;
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

  svg {
    width: 18px;
    height: 18px;
    fill: none;
    stroke: currentColor;
    stroke-width: 2;
    stroke-linecap: round;
    stroke-linejoin: round;
  }

  label,
  h2 {
    display: flex;
    align-items: center;
    gap: var(--space-sm);
    margin: 0 0 var(--space-sm);
    font-size: 12px;
    font-weight: 600;
    letter-spacing: 0.08em;
    text-transform: uppercase;
    color: var(--text-muted);
  }

  .count {
    min-width: 20px;
    padding: 0 6px;
    border-radius: 999px;
    background: var(--surface-2);
    color: var(--text-secondary);
    font-size: 11px;
    letter-spacing: 0;
    text-align: center;
  }

  .dump {
    margin: var(--space-xs) 0 var(--space-sm);
  }

  .field {
    display: flex;
    align-items: flex-end;
    gap: var(--space-sm);
    padding: var(--space-md);
    border-radius: 14px;
    transition: border-color var(--ease), box-shadow var(--ease);
  }

  .field:focus-within {
    border-color: var(--accent);
    box-shadow: 0 0 0 4px var(--accent-glow);
  }

  textarea {
    flex: 1;
    resize: none;
    border: 0;
    outline: 0;
    background: transparent;
    color: var(--text);
    font: inherit;
    font-size: 18px;
    line-height: 1.45;
    user-select: text;
  }

  textarea::placeholder {
    color: var(--text-muted);
  }

  .send {
    display: grid;
    place-items: center;
    width: 38px;
    height: 38px;
    flex-shrink: 0;
    border: 0;
    border-radius: 11px;
    background: var(--accent-gradient);
    color: var(--on-accent);
    box-shadow: 0 4px 16px var(--accent-glow);
    transition: opacity var(--ease), transform var(--ease), filter var(--ease);
  }

  .send:hover:not(:disabled) {
    filter: brightness(1.08);
  }

  .send:active:not(:disabled) {
    transform: scale(0.96);
  }

  .send:disabled {
    opacity: 0.35;
    box-shadow: none;
    cursor: default;
  }

  .status {
    display: flex;
    align-items: baseline;
    gap: var(--space-sm);
    min-height: 20px;
    margin: var(--space-sm) 0 0;
    color: var(--text-muted);
    font-size: 13px;
    user-select: text;
  }

  .card {
    padding: 14px var(--space-md) 4px;
    margin-bottom: var(--space-md);
  }

  .briefing {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    scrollbar-width: thin;
    scrollbar-color: var(--border) transparent;
  }

  .chips {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-sm);
    padding-bottom: 10px;
  }

  .chip {
    padding: 6px 14px;
    background: rgb(29 25 42 / 0.8);
    border: 1px solid var(--border);
    border-radius: 999px;
    color: var(--text-secondary);
    font-size: 13px;
    transition: background var(--ease), border-color var(--ease), color var(--ease);
  }

  .chip:hover {
    border-color: var(--accent);
    color: var(--text);
  }

  .empty {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: var(--space-sm);
    padding: var(--space-lg) var(--space-md);
    text-align: center;
  }

  .empty img {
    opacity: 0.85;
  }

  .empty p {
    margin: 0 0 var(--space-xs);
    color: var(--text-secondary);
  }

  .empty .chips {
    justify-content: center;
    padding: 0;
  }

  ul {
    margin: 0;
    padding: 0;
    list-style: none;
  }

  li {
    display: flex;
    align-items: baseline;
    gap: var(--space-md);
    padding: 10px 0;
    border-bottom: 1px solid rgb(42 37 64 / 0.6);
  }

  li:last-child {
    border-bottom: 0;
  }

  /* One line tall, so dots and orbs sit centred on an item's first line. */
  .indicator {
    display: grid;
    place-items: center;
    align-self: flex-start;
    width: 18px;
    height: 1.5em;
    flex-shrink: 0;
    color: var(--accent);
  }

  .dot {
    width: 8px;
    height: 8px;
    border-radius: 50%;
  }

  .dot.ok {
    background: var(--success);
    box-shadow: 0 0 8px rgb(95 212 160 / 0.45);
  }
  .dot.warn {
    background: var(--danger);
    box-shadow: 0 0 8px rgb(240 113 138 / 0.45);
  }

  .body {
    display: flex;
    flex-direction: column;
    flex: 1;
    min-width: 0;
  }

  .text {
    user-select: text;
  }

  .sub {
    color: var(--text-muted);
    font-size: 13px;
    user-select: text;
  }

  summary {
    cursor: pointer;
  }

  summary::marker {
    color: var(--accent);
  }

  pre {
    margin: var(--space-sm) 0 var(--space-xs);
    padding: var(--space-md);
    background: rgb(14 12 21 / 0.7);
    border: 1px solid var(--border);
    border-radius: 10px;
    color: var(--text-secondary);
    font: inherit;
    font-size: 14px;
    white-space: pre-wrap;
    overflow-wrap: anywhere;
    user-select: text;
  }

  time {
    color: var(--text-muted);
    font-size: 13px;
    font-variant-numeric: tabular-nums;
  }

  footer {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding-top: var(--space-md);
    border-top: 1px solid var(--border);
    color: var(--text-muted);
    font-size: 12px;
  }

  kbd {
    padding: 1px 6px;
    border: 1px solid var(--border);
    border-radius: 5px;
    background: var(--surface);
    font: inherit;
    font-size: 11px;
  }

  .link {
    flex-shrink: 0;
    padding: var(--space-xs) var(--space-sm);
    border: 0;
    border-radius: 6px;
    background: transparent;
    color: var(--text-muted);
    font-size: 12px;
    transition: color var(--ease), background var(--ease);
  }

  .link:hover {
    color: var(--text);
    background: var(--surface-2);
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
