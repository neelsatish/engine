<script lang="ts">
  import "@fontsource-variable/inter";
  import "../app.css";
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { onMount } from "svelte";

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
    result: string;
    created: string;
  };

  let text = $state("");
  let sorting = $state(0);
  let note = $state("");
  let choices = $state<Target[]>([]);
  let items = $state<Item[]>([]);
  let input: HTMLTextAreaElement;

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

  async function dismiss(id: number) {
    try {
      await invoke("dismiss", { id });
    } catch (e) {
      note = `Couldn't dismiss: ${e}`;
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

  const tone = (i: Item) =>
    ({ done: "ok", failed: "warn", running: "live", held: "hold", queued: "muted" })[i.status];
  const time = (i: Item) => i.created.slice(11, 16);

  onMount(() => {
    input.focus();
    load();
    const offs = [listen("summon", () => input.focus()), listen("briefing", load)];
    return () => offs.forEach((off) => off.then((f) => f()));
  });
</script>

<svelte:window onkeydown={onKey} />

<main>
  <header data-tauri-drag-region>
    <div class="brand" data-tauri-drag-region>
      <span class="mark" aria-hidden="true"></span>
      <span data-tauri-drag-region>Engine</span>
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
    <div class="field">
      <textarea
        id="dump"
        bind:this={input}
        bind:value={text}
        onkeydown={onInputKey}
        rows="2"
        placeholder="Dump anything: open an app, ask for a plan, hand off a task."
        spellcheck="false"
      ></textarea>
      <button class="send" type="submit" aria-label="Send" disabled={!text.trim()}>
        <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M9 10 4 15l5 5" /><path d="M20 4v7a4 4 0 0 1-4 4H4" /></svg>
      </button>
    </div>
    <p class="status" aria-live="polite">
      {#if sorting}
        <span class="dot live" aria-hidden="true"></span>Sorting your dump…
      {:else if note}
        <span class="dot warn" aria-hidden="true"></span>{note}
      {/if}
    </p>
  </form>

  {#if choices.length}
    <section aria-label="Which one?">
      <h2>Which one?</h2>
      <div class="choices">
        {#each choices as c (c.target)}
          <button class="choice" onclick={() => pick(c)}>{c.name}</button>
        {/each}
      </div>
    </section>
  {/if}

  <div class="briefing" aria-live="polite">
    {#each sections as s (s.name)}
      <section>
        <h2>{s.name}</h2>
        <ul>
          {#each s.items as i (i.id)}
            <li>
              <span class="dot {tone(i)}" aria-hidden="true"></span>
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
              {#if i.kind !== "instant" && i.status !== "running" && i.status !== "done"}
                <button class="link" onclick={() => dismiss(i.id)}>Dismiss</button>
              {/if}
              <time>{time(i)}</time>
            </li>
          {/each}
        </ul>
      </section>
    {:else}
      <p class="empty">Nothing yet today. Dump anything above.</p>
    {/each}
  </div>

  <footer>
    <span><kbd>Ctrl</kbd> <kbd>Shift</kbd> <kbd>Space</kbd> summon · <kbd>Esc</kbd> hide</span>
    <button class="link" onclick={editAliases}>Edit aliases</button>
  </footer>
</main>

<style>
  main {
    display: flex;
    flex-direction: column;
    height: 100vh;
    padding: 0 var(--space-lg) var(--space-md);
    border: 1px solid var(--border);
  }

  header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    height: 48px;
    flex-shrink: 0;
  }

  .brand {
    display: flex;
    align-items: center;
    gap: var(--space-sm);
    font-weight: 600;
    letter-spacing: 0.01em;
  }

  .mark {
    width: 10px;
    height: 10px;
    border-radius: 3px;
    background: var(--accent);
    box-shadow: 0 0 12px rgb(249 115 22 / 0.5);
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
    display: block;
    margin: 0 0 var(--space-sm);
    font-size: 12px;
    font-weight: 600;
    letter-spacing: 0.08em;
    text-transform: uppercase;
    color: var(--text-muted);
  }

  .dump {
    margin: var(--space-sm) 0 var(--space-md);
  }

  .field {
    display: flex;
    align-items: flex-end;
    gap: var(--space-sm);
    padding: var(--space-md);
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    transition: border-color var(--ease);
  }

  .field:focus-within {
    border-color: var(--accent);
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
    width: 36px;
    height: 36px;
    flex-shrink: 0;
    border: 0;
    border-radius: 10px;
    background: var(--accent);
    color: var(--on-accent);
    transition: opacity var(--ease), transform var(--ease);
  }

  .send:active {
    transform: scale(0.96);
  }

  .send:disabled {
    opacity: 0.35;
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

  section {
    margin-bottom: var(--space-lg);
  }

  .briefing {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    scrollbar-width: thin;
    scrollbar-color: var(--border) transparent;
  }

  .choices {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-sm);
  }

  .choice {
    padding: var(--space-sm) var(--space-md);
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: 999px;
    transition: background var(--ease), border-color var(--ease);
  }

  .choice:hover {
    background: var(--surface-2);
    border-color: var(--accent);
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
    border-bottom: 1px solid var(--border);
  }

  .dot {
    width: 8px;
    height: 8px;
    flex-shrink: 0;
    border-radius: 50%;
    transform: translateY(-1px);
  }

  .dot.ok {
    background: var(--success);
  }
  .dot.warn {
    background: var(--danger);
  }
  .dot.muted {
    background: var(--text-muted);
  }
  .dot.hold {
    background: var(--accent);
  }
  .dot.live {
    background: var(--accent);
    animation: pulse 1.2s ease-in-out infinite;
  }

  @keyframes pulse {
    50% {
      opacity: 0.3;
    }
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
    color: var(--text-muted);
  }

  pre {
    margin: var(--space-sm) 0 0;
    padding: var(--space-md);
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    font: inherit;
    font-size: 14px;
    white-space: pre-wrap;
    overflow-wrap: anywhere;
    user-select: text;
  }

  time,
  .empty {
    color: var(--text-muted);
    font-size: 13px;
  }

  .empty {
    margin: 0;
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
</style>
