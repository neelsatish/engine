<script lang="ts">
  // Ctrl+K: a terminal-style command palette. Type to filter views, actions, models and themes; Enter on free text
  // sends it to Engine like the chat does.
  import { invoke } from "@tauri-apps/api/core";
  import { tick } from "svelte";
  import { engine, savePrefs, submit, type View } from "$lib/state.svelte";

  let open = $state(false);
  let query = $state("");
  let at = $state(0);
  let input = $state<HTMLInputElement>();

  type Command = { label: string; hint: string; run: () => void };
  const view = (v: View) => () => (engine.view = v);
  const say = (text: string) => () => void submit(text);
  const model = (id: string) => () => void savePrefs({ models: { ...(engine.prefs?.models ?? { agent: id, answer: "sonnet" }), agent: id } });
  const COMMANDS: Command[] = [
    { label: "home", hint: "view · alt+1", run: view("home") },
    { label: "briefing", hint: "view · alt+2", run: view("briefing") },
    { label: "schedule", hint: "view · alt+3", run: view("schedule") },
    { label: "settings", hint: "view · alt+4", run: view("settings") },
    { label: "review my day", hint: "action", run: () => void invoke("review_day") },
    { label: "read the briefing aloud", hint: "action", run: () => void invoke("speak_briefing") },
    { label: "clear the chat", hint: "action", run: say("clear the chat") },
    { label: "clear the briefing", hint: "action", run: say("clear the briefing") },
    { label: "stop everything", hint: "action", run: say("stop everything") },
    { label: "minimize everything", hint: "windows", run: say("minimize everything") },
    { label: "what's on my screen?", hint: "agent", run: say("what's on my screen?") },
    { label: "theme: velvet", hint: "theme", run: () => void savePrefs({ theme: "velvet" }) },
    { label: "theme: black and white", hint: "theme", run: () => void savePrefs({ theme: "mono" }) },
    { label: "model: haiku", hint: "agents", run: model("haiku") },
    { label: "model: sonnet", hint: "agents", run: model("sonnet") },
    { label: "model: opus", hint: "agents", run: model("opus") },
    { label: "model: fable", hint: "agents", run: model("fable") },
    { label: "local ai: on", hint: "brain", run: () => void invoke("set_local_ai", { on: true }) },
    { label: "local ai: off", hint: "brain", run: () => void invoke("set_local_ai", { on: false }) },
  ];

  // Every word typed must appear in the label, in any order.
  const matches = $derived.by(() => {
    const words = query.toLowerCase().split(/\s+/).filter(Boolean);
    const found = COMMANDS.filter((c) => words.every((w) => c.label.includes(w)));
    return query.trim() ? [...found, { label: `ask engine: ${query.trim()}`, hint: "enter", run: say(query.trim()) }] : found;
  });

  export async function show() {
    open = true;
    query = "";
    at = 0;
    await tick();
    input?.focus();
  }

  function run(c: Command) {
    open = false;
    c.run();
  }

  function onKey(e: KeyboardEvent) {
    if (e.key === "Escape") {
      e.preventDefault();
      open = false;
    } else if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      e.preventDefault();
      at = (at + (e.key === "ArrowDown" ? 1 : -1) + matches.length) % matches.length;
    } else if (e.key === "Enter" && matches[at]) {
      e.preventDefault();
      run(matches[at]);
    }
  }

  $effect(() => {
    void query;
    at = 0;
  });
</script>

<svelte:window
  onkeydown={(e) => {
    if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "k") {
      e.preventDefault();
      if (open) open = false;
      else show();
    }
  }}
/>

{#if open}
  <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
  <div class="scrim" onclick={() => (open = false)}>
    <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
    <div class="palette" role="dialog" aria-label="Command palette" aria-modal="true" tabindex="-1" onclick={(e) => e.stopPropagation()}>
      <div class="line">
        <span class="ps1">❯</span>
        <input
          bind:this={input}
          bind:value={query}
          onkeydown={onKey}
          placeholder="type a command, or anything to ask engine…"
          aria-label="Command"
          aria-expanded="true"
          aria-controls="palette-list"
          role="combobox"
          spellcheck="false"
        />
        <kbd>esc</kbd>
      </div>
      <ul id="palette-list" role="listbox">
        {#each matches as c, n (c.label)}
          <li role="option" aria-selected={n === at}>
            <button class:on={n === at} onmouseenter={() => (at = n)} onclick={() => run(c)}>
              <span class="caret">›</span><span class="label">{c.label}</span><span class="hint">{c.hint}</span>
            </button>
          </li>
        {/each}
      </ul>
    </div>
  </div>
{/if}

<style>
  .scrim {
    position: fixed;
    inset: 0;
    z-index: 50;
    display: grid;
    place-items: start center;
    padding-top: 90px;
    background: rgb(0 0 0 / 0.55);
  }

  .palette {
    width: min(560px, calc(100vw - 32px));
    border: 1px solid var(--pane-border);
    border-radius: var(--r-lg);
    background: var(--pane-bg);
    box-shadow: 0 20px 60px rgb(0 0 0 / 0.6);
    backdrop-filter: blur(8px);
  }

  .line {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 12px 14px;
    border-bottom: 1px dashed var(--pane-border);
  }

  .ps1 {
    color: var(--accent);
  }

  input {
    flex: 1;
    border: 0;
    outline: 0;
    background: transparent;
    color: var(--text);
    font: inherit;
    font-size: 14px;
  }

  input::placeholder {
    color: var(--text-muted);
  }

  ul {
    max-height: 340px;
    margin: 0;
    padding: 6px;
    overflow-y: auto;
    list-style: none;
  }

  button {
    display: grid;
    grid-template-columns: 1.5ch 1fr auto;
    align-items: baseline;
    line-height: 28px;
    width: 100%;
    height: 28px;
    padding: 0 8px;
    border: 0;
    border-radius: var(--r-sm);
    background: transparent;
    color: var(--text-secondary);
    font-size: 13px;
    text-align: left;
  }

  .caret {
    color: transparent;
  }

  button.on {
    background: color-mix(in oklab, var(--accent), transparent 86%);
    color: var(--text);
  }

  button.on .caret {
    color: var(--accent);
  }

  .hint {
    color: var(--text-muted);
    font-size: 11px;
  }
</style>
