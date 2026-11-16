<script lang="ts">
  // The chat panel: what Engine just did, and the composer (folder, model, Local AI, mic, send).
  import { invoke } from "@tauri-apps/api/core";
  import { open as openDialog } from "@tauri-apps/plugin-dialog";
  import { tick } from "svelte";
  import Orb, { type Process } from "$lib/Orb.svelte";
  import { engine, savePrefs, submit as send, type Item, type Reply, type Target } from "$lib/state.svelte";

  let text = $state("");
  let input: HTMLTextAreaElement;
  let folderMenu = $state(false);
  let powering = $state(false);
  const CLAUDE_MODELS = [["haiku", "Haiku"], ["sonnet", "Sonnet"], ["opus", "Opus"], ["fable", "Fable"]];
  const CHATGPT_MODELS = [
    ["codex:gpt-6.1-sol", "GPT-6.1 Sol"],
    ["codex:gpt-6-astra", "GPT-6 Astra"],
    ["codex:gpt-6.1-terra", "GPT-6.1 Terra"],
    ["codex:gpt-6-luna", "GPT-6 Luna"],
  ];
  const fallback = { folder: "", recent: [] as string[], models: { agent: "haiku", answer: "sonnet" }, local_ai: false };
  const prefs = $derived(engine.prefs ?? fallback);
  const voice = $derived(engine.voice);
  const note = $derived(engine.note);
  const choices = $derived(engine.choices);
  const baseName = (p: string) => p.split(/[\\/]/).filter(Boolean).pop() ?? p;

  // The latest few things Engine did or heard, newest first.
  const feed = $derived([...engine.items].sort((a, b) => b.id - a.id).slice(0, 5));
  const heard = $derived(engine.heard.filter((h) => Date.now() - h.at < 20_000));

  export function focus() {
    input?.focus();
  }

  /** Words from push-to-talk when Engine isn't acting live: into the box, or sent at once. */
  export async function useHeard(words: string, submitNow: boolean) {
    if (!words) return;
    text = text.trim() ? `${text.trim()} ${words}` : words;
    await tick();
    if (submitNow) submit();
    else input.focus();
  }

  async function submit() {
    const said = text.trim();
    if (!said) return;
    text = "";
    const reply = await send(said);
    if (reply.kind === "error" && !text.trim()) text = said; // give the words back so nothing is lost
    input?.focus();
  }

  async function pick(choice: Target) {
    const reply = await invoke<Reply>("open_choice", { choice });
    engine.choices = [];
    if (reply.kind === "error") engine.note = reply.message;
    input.focus();
  }

  async function toggleLocal() {
    powering = true;
    engine.note = "";
    try {
      await invoke("set_local_ai", { on: !prefs.local_ai });
    } catch (e) {
      engine.note = `Local AI: ${e}`;
    } finally {
      powering = false;
    }
  }

  async function chooseFolder() {
    folderMenu = false;
    const picked = await openDialog({ directory: true, title: "Choose a folder for Engine to work in" });
    if (typeof picked === "string") await savePrefs({ folder: picked });
    input.focus();
  }

  async function useFolder(folder: string) {
    folderMenu = false;
    await savePrefs({ folder });
    input.focus();
  }

  async function toggleMic() {
    engine.note = "";
    try {
      if (voice === "listening") {
        const words = await invoke<string>("voice_stop");
        if (words && !engine.prefs?.voice.live) await useHeard(words, engine.prefs?.voice.auto_submit ?? false);
      } else {
        await invoke("voice_start");
      }
    } catch (e) {
      engine.note = String(e);
    }
  }

  function onInputKey(e: KeyboardEvent) {
    if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      submit();
    }
  }

  const orbFor = (i: Item): Process | undefined =>
    i.status === "running"
      ? ((i.process || "writing") as Process)
      : ({ queued: "waiting", held: "held" } as Partial<Record<Item["status"], Process>>)[i.status];
  const label = (i: Item) => (i.kind === "instant" ? i.result || i.title : i.status === "running" ? i.result || i.title : i.title);
</script>

<section class="chat" aria-label="Chat">
  <div class="feed glass">
    <div class="feed-head">
      <h2 class="label">Activity</h2>
      <button class="link" onclick={() => (engine.view = "briefing")}>See all</button>
    </div>
    <ul class="feed-list" aria-live="polite">
      {#each heard as h (h.at)}
        <li class="you"><span class="who">You</span><span class="line">{h.text}</span></li>
      {/each}
      {#each feed as i (i.id)}
        {@const process = orbFor(i)}
        <li>
          <span class="indicator">
            {#if process}<Orb {process} size={16} />{:else}<span class="dot {i.status === 'failed' ? 'warn' : 'ok'}"></span>{/if}
          </span>
          <span class="line" title={label(i)}>{label(i)}</span>
          <time>{i.created.slice(11, 16)}</time>
        </li>
      {:else}
        {#if !heard.length}<li class="quiet"><span class="line">Nothing yet. Hold ` and say what you need.</span></li>{/if}
      {/each}
    </ul>
    {#if choices.length}
      <div class="chips">
        {#each choices as c (c.target)}
          <button class="chip" onclick={() => pick(c)}>{c.name}</button>
        {/each}
      </div>
    {/if}
  </div>

<form
  class="dump"
  onsubmit={(e) => {
    e.preventDefault();
    submit();
  }}
>
  <div class="field glass">
    <div class="top">
    <textarea
      id="dump"
      aria-label="Dump"
      bind:this={input}
      bind:value={text}
      onkeydown={onInputKey}
      rows="1"
      placeholder="Ask or tell Engine anything…"
      spellcheck="false"
    ></textarea>
    </div>
    <div class="tools">
      <div class="chips-left">
        <div class="menu-wrap">
          <button
            type="button"
            class="tool-chip"
            aria-haspopup="menu"
            aria-expanded={folderMenu}
            title={prefs.folder ? `Jobs work in ${prefs.folder}` : "Choose a folder for jobs to work in"}
            onclick={() => (folderMenu = !folderMenu)}
          >
            <svg class="ico" viewBox="0 0 24 24" aria-hidden="true"
              ><path d="M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z" /></svg
            >
            {prefs.folder ? baseName(prefs.folder) : "No folder"}
          </button>
          {#if folderMenu}
            <div class="menu glass" role="menu">
              {#each prefs.recent as f (f)}
                <button role="menuitemradio" aria-checked={f === prefs.folder} onclick={() => useFolder(f)} title={f}>
                  <span class="m-name">{baseName(f)}</span><span class="m-path">{f}</span>
                </button>
              {/each}
              <button role="menuitem" onclick={chooseFolder}><span class="m-name">Choose folder…</span></button>
              {#if prefs.folder}
                <button role="menuitem" onclick={() => useFolder("")}
                  ><span class="m-name">No folder</span><span class="m-path">Each job gets its own folder</span></button
                >
              {/if}
            </div>
          {/if}
        </div>
        <label class="tool-chip model" title="The model background agents use">
          <svg class="ico" viewBox="0 0 24 24" aria-hidden="true"
            ><path d="M12 3l1.8 4.6L18 9l-4.2 1.4L12 15l-1.8-4.6L6 9l4.2-1.4z" /><path d="M19 15l.8 2 2 .8-2 .8-.8 2-.8-2-2-.8 2-.8z" /></svg
          >
          <select
            aria-label="Agent model"
            value={prefs.models.agent}
            onchange={(e) => savePrefs({ models: { ...prefs.models, agent: e.currentTarget.value } })}
          >
            <optgroup label="Claude">
              {#each CLAUDE_MODELS as [id, name] (id)}<option value={id}>{name}</option>{/each}
            </optgroup>
            <optgroup label="ChatGPT (Codex)">
              {#each CHATGPT_MODELS as [id, name] (id)}<option value={id}>{name}</option>{/each}
            </optgroup>
          </select>
        </label>
        <button
          type="button"
          class="tool-chip power"
          class:on={prefs.local_ai}
          aria-pressed={prefs.local_ai}
          disabled={powering}
          title={prefs.local_ai
            ? "Local AI is on: Engine thinks on your PC (Qwen3 8B). Click to turn it off."
            : "Local AI is off: Engine thinks with Claude Haiku. Click to run on your PC instead."}
          onclick={toggleLocal}
        >
          <svg class="ico" viewBox="0 0 24 24" aria-hidden="true"><path d="M12 3v8" /><path d="M6.4 6.4a8 8 0 1 0 11.2 0" /></svg>
          {powering ? (prefs.local_ai ? "Stopping…" : "Starting…") : prefs.local_ai ? "Local" : "Cloud"}
        </button>
      </div>
      <div class="chips-right">
    <button
      class="mic"
      class:on={voice === "listening"}
      type="button"
      aria-label={voice === "listening" ? "Stop and transcribe" : "Speak"}
      aria-pressed={voice === "listening"}
      title="Speak (or hold the ` key anywhere)"
      disabled={voice === "transcribing"}
      onclick={toggleMic}
    >
      <svg class="ico" viewBox="0 0 24 24" aria-hidden="true"
        ><rect x="9" y="3" width="6" height="11" rx="3" /><path d="M5 11a7 7 0 0 0 14 0" /><path d="M12 18v3" /></svg
      >
    </button>
    <button class="send" type="submit" aria-label="Send" title="Send (Enter)" disabled={!text.trim()}>
      <svg class="ico" viewBox="0 0 24 24" aria-hidden="true"><path d="M9 10 4 15l5 5" /><path d="M20 4v7a4 4 0 0 1-4 4H4" /></svg>
    </button>
      </div>
    </div>
  </div>
  <p class="status" aria-live="polite">
    {#if note}
      <span class="indicator"><span class="dot warn" aria-hidden="true"></span></span>{note}
    {/if}
  </p>
</form>
</section>

<style>
  .chat {
    display: flex;
    flex-direction: column;
    gap: 10px;
    min-height: 0;
    min-width: 0;
  }

  .feed {
    display: flex;
    flex: 1;
    flex-direction: column;
    min-height: 0;
    padding: 10px 14px 6px;
  }

  .feed-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
  }

  .feed-head .label {
    margin: 0;
  }

  .feed-list {
    flex: 1;
    min-height: 0;
    margin: 4px 0 0;
    padding: 0;
    overflow-y: auto;
    list-style: none;
    scrollbar-width: thin;
    scrollbar-color: var(--border) transparent;
  }

  .feed-list li {
    display: flex;
    align-items: baseline;
    gap: 10px;
    padding: 4px 0;
    font-size: 13px;
  }

  .feed-list .line {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: var(--text-secondary);
  }

  .feed-list .you .line {
    color: var(--text);
  }

  .who {
    width: 18px;
    flex-shrink: 0;
    color: var(--accent);
    font-size: 12px;
    font-weight: 600;
  }

  .quiet .line {
    color: var(--text-muted);
  }

  time {
    color: var(--text-muted);
    font-size: 12px;
    font-variant-numeric: tabular-nums;
  }

  .dump {
      position: relative;
      z-index: 5; /* above the cards below, so the folder menu isn't hidden behind them */
      margin: var(--space-xs) 0 var(--space-sm);
    }

  .field {
      display: flex;
      flex-direction: column;
      gap: var(--space-sm);
      padding: 14px var(--space-md) 10px;
      border-radius: var(--r-lg);
      transition: border-color var(--ease), box-shadow var(--ease);
    }

  .field:focus-within {
      border-color: var(--accent);
      box-shadow: 0 0 0 3px rgb(142 144 245 / 0.14);
    }

  .top {
      display: flex;
    }

  .tools {
      display: flex;
      align-items: center;
      justify-content: space-between;
      gap: var(--space-sm);
    }

  .chips-left,
    .chips-right {
      display: flex;
      align-items: center;
      gap: var(--space-sm);
      min-width: 0;
    }

  .tool-chip {
      display: inline-flex;
      align-items: center;
      gap: 6px;
      max-width: 220px;
      height: 30px;
      padding: 0 10px;
      border: 1px solid var(--border);
      border-radius: var(--r-sm);
      background: var(--surface-2);
      color: var(--text-secondary);
      font-size: 13px;
      white-space: nowrap;
      overflow: hidden;
      text-overflow: ellipsis;
      cursor: pointer;
      transition: border-color var(--ease), color var(--ease);
    }

  .tool-chip:hover:not(:disabled) {
      border-color: color-mix(in oklab, var(--border), white 22%);
      color: var(--text);
    }

  .tool-chip svg {
      width: 14px;
      height: 14px;
      flex-shrink: 0;
    }

  .tool-chip select {
      border: 0;
      outline: 0;
      background: transparent;
      color: inherit;
      font: inherit;
      cursor: pointer;
    }

  .tool-chip.power.on {
      border-color: rgb(95 212 160 / 0.55);
      color: var(--success);
    }

  .tool-chip:disabled {
      opacity: 0.6;
      cursor: default;
    }

  .tool-chip option,
    .tool-chip optgroup {
      background: #13101c;
      color: var(--text);
    }

  .menu-wrap {
      position: relative;
    }

  .menu {
      position: absolute;
      top: calc(100% + 6px);
      left: 0;
      z-index: 10;
      display: flex;
      flex-direction: column;
      min-width: 260px;
      max-width: 420px;
      padding: 4px;
      border-radius: var(--r-md);
      background: rgb(19 16 28 / 0.97);
      box-shadow: 0 12px 32px rgb(0 0 0 / 0.45);
    }

  .menu button {
      display: flex;
      flex-direction: column;
      padding: 7px 10px;
      border: 0;
      border-radius: var(--r-sm);
      background: transparent;
      text-align: left;
    }

  .menu button:hover,
    .menu button[aria-checked="true"] {
      background: var(--surface-2);
    }

  .m-name {
      color: var(--text);
      font-size: 13px;
    }

  .m-path {
      overflow: hidden;
      color: var(--text-muted);
      font-size: 12px;
      text-overflow: ellipsis;
      white-space: nowrap;
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

  .mic {
      display: grid;
      place-items: center;
      width: 36px;
      height: 36px;
      flex-shrink: 0;
      border: 1px solid var(--border);
      border-radius: var(--r-sm);
      background: var(--surface-2);
      color: var(--text-secondary);
      transition: border-color var(--ease), color var(--ease), box-shadow var(--ease);
    }

  .mic:hover:not(:disabled) {
      border-color: var(--accent);
      color: var(--text);
    }

  .mic.on {
      border-color: var(--danger);
      background: rgb(240 113 138 / 0.12);
      color: var(--danger);
    }

  .mic:disabled {
      opacity: 0.5;
      cursor: default;
    }

  .send {
      display: grid;
      place-items: center;
      width: 36px;
      height: 36px;
      flex-shrink: 0;
      border: 0;
      border-radius: var(--r-sm);
      background: var(--accent);
      color: var(--on-accent);
      transition:
        transform var(--m-out) var(--curve),
        background-color var(--m-out) var(--curve),
        opacity var(--m-out) var(--curve);
    }

  @media (hover: hover) and (pointer: fine) {
      .send:hover:not(:disabled),
      .mic:hover:not(:disabled) {
        transform: translateY(-1px);
        transition-duration: var(--m-in);
      }
    }

  .send:hover:not(:disabled) {
      background: var(--accent-hover);
    }

  .send:active:not(:disabled),
    .mic:active:not(:disabled) {
      transform: translateY(1px) scale(0.985);
      transition-duration: var(--m-press);
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
</style>
