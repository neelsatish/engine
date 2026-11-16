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
    | { kind: "not_found"; query: string }
    | { kind: "unsupported" }
    | { kind: "error"; message: string };
  type Tone = "ok" | "warn" | "muted";
  type Entry = { id: number; text: string; tone: Tone; time: string };

  let text = $state("");
  let busy = $state(false);
  let entries = $state<Entry[]>([]);
  let choices = $state<Target[]>([]);
  let input: HTMLTextAreaElement;
  let nextId = 0;

  const clock = () => new Date().toLocaleTimeString([], { hour: "numeric", minute: "2-digit" });
  const log = (text: string, tone: Tone) =>
    (entries = [{ id: nextId++, text, tone, time: clock() }, ...entries].slice(0, 50));

  function show(reply: Reply) {
    choices = [];
    switch (reply.kind) {
      case "opened":
        return log(`Opened ${reply.name}`, "ok");
      case "choices":
        choices = reply.items;
        return;
      case "not_found":
        return log(`Couldn't find “${reply.query}”. Add it under Edit aliases.`, "warn");
      case "unsupported":
        return log("Right now Engine can open apps and sites. Planning and agents come next.", "muted");
      case "error":
        return log(reply.message, "warn");
    }
  }

  async function submit() {
    const said = text.trim();
    if (!said || busy) return;
    busy = true;
    try {
      show(await invoke<Reply>("submit", { text: said }));
      text = "";
    } catch (e) {
      log(String(e), "warn");
    } finally {
      busy = false;
      input.focus();
    }
  }

  async function pick(choice: Target) {
    show(await invoke<Reply>("open_choice", { choice }));
    input.focus();
  }

  async function editAliases() {
    try {
      await invoke("edit_aliases");
    } catch (e) {
      log(`Couldn't open aliases: ${e}`, "warn");
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

  onMount(() => {
    input.focus();
    const off = listen("summon", () => input.focus());
    return () => off.then((f) => f());
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
        placeholder="Tell Engine what to do. Try “open YouTube”."
        spellcheck="false"
      ></textarea>
      <button class="send" type="submit" aria-label="Send" disabled={busy || !text.trim()}>
        <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M9 10 4 15l5 5" /><path d="M20 4v7a4 4 0 0 1-4 4H4" /></svg>
      </button>
    </div>
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

  <section class="now" aria-live="polite">
    <h2>Now</h2>
    {#if entries.length}
      <ul>
        {#each entries as e (e.id)}
          <li>
            <span class="dot {e.tone}" aria-hidden="true"></span>
            <span class="text">{e.text}</span>
            <time>{e.time}</time>
          </li>
        {/each}
      </ul>
    {:else}
      <p class="empty">Nothing yet. Open an app or a site to start.</p>
    {/if}
  </section>

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
    margin: var(--space-sm) 0 var(--space-lg);
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

  section {
    margin-bottom: var(--space-lg);
  }

  .now {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
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

  .text {
    flex: 1;
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
    padding: var(--space-xs) var(--space-sm);
    border: 0;
    border-radius: 6px;
    background: transparent;
    color: var(--text-muted);
    transition: color var(--ease), background var(--ease);
  }

  .link:hover {
    color: var(--text);
    background: var(--surface-2);
  }
</style>
