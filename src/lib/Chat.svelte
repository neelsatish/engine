<script lang="ts">
  // The chat panel: what you asked and what Engine did about it, newest at the bottom, with the composer
  // (folder, model, Local AI, mic, send) underneath. Answers show in full; agents show their live step, then
  // their report.
  import { invoke } from "@tauri-apps/api/core";
  import { open as openDialog } from "@tauri-apps/plugin-dialog";
  import { tick } from "svelte";
  import Orb, { type Process } from "$lib/Orb.svelte";
  import { engine, savePrefs, submit as send, type Item, type Reply, type Target } from "$lib/state.svelte";

  let text = $state("");
  let input: HTMLTextAreaElement;
  let thread: HTMLElement;
  let folderMenu = $state(false);
  let powering = $state(false);
  let expanded = $state<number[]>([]);
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

  type Turn = { key: string; said: string; items: Item[] };

  // Items grouped by the message they came from, oldest first. Items with no message (a day review, say) stand alone.
  const turns = $derived.by(() => {
    const groups = new Map<string, Turn>();
    for (const i of [...engine.items].sort((a, b) => a.id - b.id)) {
      const key = i.dump != null ? `d${i.dump}` : `i${i.id}`;
      const turn = groups.get(key) ?? { key, said: i.said, items: [] };
      turn.items.push(i);
      groups.set(key, turn);
    }
    return [...groups.values()].slice(-40);
  });

  // What you've said that Engine is still working out: typed messages in flight, and phrases just heard.
  const waiting = $derived.by(() => {
    const answered = new Set(turns.map((t) => t.said.trim().toLowerCase()));
    const heard = engine.heard.map((h) => h.text);
    return [...engine.pending, ...heard].filter((t) => !answered.has(t.trim().toLowerCase()));
  });

  // Keep the newest message in view.
  $effect(() => {
    void turns.length;
    void waiting.length;
    void engine.items.map((i) => i.status + i.result.length).join();
    tick().then(() => thread?.scrollTo({ top: thread.scrollHeight }));
  });

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

  async function act(command: string, id: number) {
    try {
      await invoke(command, { id });
    } catch (e) {
      engine.note = String(e);
    }
  }

  // A reply as lines with light formatting (headings, bullets, **bold**), built as text so nothing is run as HTML.
  type Line = { head: boolean; bullet: boolean; parts: { text: string; bold: boolean }[] };
  function lines(reply: string): Line[] {
    return reply
      .split("\n")
      .map((l) => l.trim())
      .filter(Boolean)
      .map((l) => {
        const head = /^#{1,6}\s/.test(l);
        const bullet = /^[-*•]\s/.test(l);
        const body = head ? l.replace(/^#+\s*/, "") : bullet ? l.slice(2) : l;
        const parts = body.split(/\*\*(.+?)\*\*/g).map((t, n) => ({ text: t, bold: n % 2 === 1 })).filter((p) => p.text);
        return { head, bullet, parts };
      });
  }

  const LONG = 7; // lines shown before "Show more"
  const toggle = (id: number) => (expanded = expanded.includes(id) ? expanded.filter((e) => e !== id) : [...expanded, id]);
  const stepOrb = (i: Item): Process => (i.status === "queued" ? "waiting" : ((i.process || "working") as Process));
</script>

{#snippet reply(i: Item)}
  {@const all = lines(i.result)}
  {@const open = expanded.includes(i.id)}
  <div class="reply">
    {#each open ? all : all.slice(0, LONG) as l, n (n)}
      <p class:head={l.head} class:bullet={l.bullet}>
        {#each l.parts as p, k (k)}{#if p.bold}<strong>{p.text}</strong>{:else}{p.text}{/if}{/each}
      </p>
    {/each}
    {#if all.length > LONG}
      <button class="link" onclick={() => toggle(i.id)}>{open ? "Show less" : `Show ${all.length - LONG} more lines`}</button>
    {/if}
  </div>
{/snippet}

<section class="chat glass" aria-label="Chat">
  <div class="chat-head">
    <h2 class="label">Chat</h2>
    <button class="link" onclick={() => (engine.view = "briefing")}>Briefing</button>
  </div>

  <div class="thread" bind:this={thread} aria-live="polite">
    {#each turns as t (t.key)}
      {#if t.said}<div class="you"><p>{t.said}</p></div>{/if}
      {#each t.items as i (i.id)}
        {#if i.kind === "instant"}
          <div class="act" class:warn={i.status === "failed"}>
            <span class="dot {i.status === 'failed' ? 'warn' : 'ok'}" aria-hidden="true"></span>
            <span>{i.result || i.title}</span>
          </div>
        {:else if i.kind === "answer"}
          {#if i.status === "running"}
            <div class="act"><Orb process="thinking" size={16} /><span>Thinking…</span></div>
          {:else if i.status === "failed"}
            <div class="act warn"><span class="dot warn" aria-hidden="true"></span><span>{i.result || "Couldn't answer that."}</span></div>
          {:else}
            {@render reply(i)}
          {/if}
        {:else if i.kind === "job"}
          <div class="job" class:failed={i.status === "failed"}>
            <div class="job-head">
              {#if i.status === "queued" || i.status === "running"}
                <Orb process={stepOrb(i)} size={16} />
              {:else}
                <span class="dot {i.status === 'failed' ? 'warn' : 'ok'}" aria-hidden="true"></span>
              {/if}
              <span class="job-title">{i.title}</span>
              <span class="state">
                {({ queued: "Waiting", running: "Working", done: "Done", failed: "Didn't finish", held: "Needs you" } as Record<string, string>)[i.status] ?? ""}
              </span>
            </div>
            {#if i.status === "running"}
              <p class="step">{i.result || "Starting…"}</p>
            {:else if i.status === "done" && i.result}
              {@render reply(i)}
              {#if i.verdict === "concern"}<p class="concern">Check this: {i.verdict_reason}</p>{/if}
            {:else if i.status === "failed"}
              <p class="step">{i.result}</p>
              <div class="row-actions"><button class="button" onclick={() => act("retry", i.id)}>Try again</button></div>
            {/if}
            {#if i.status === "queued" || i.status === "running"}
              <div class="row-actions">
                <button class="button" onclick={() => act(i.status === "queued" ? "dismiss" : "cancel", i.id)}>Stop</button>
              </div>
            {/if}
          </div>
        {:else if i.status === "held"}
          <div class="job ask">
            <div class="job-head">
              <Orb process="held" size={16} />
              <span class="job-title">{i.kind === "outward" ? `Needs your OK: ${i.title}` : `Your turn: ${i.title}`}</span>
            </div>
            {#if i.detail}<p class="step">{i.detail}</p>{/if}
            <div class="row-actions">
              {#if i.kind === "outward"}
                <button class="button primary" onclick={() => act("approve", i.id)}>Approve</button>
                <button class="button" onclick={() => (engine.view = "briefing")}>Revise</button>
              {:else}
                <button class="button primary" onclick={() => act("task_done", i.id)}>Done</button>
              {/if}
              <button class="button" onclick={() => act("dismiss", i.id)}>Dismiss</button>
            </div>
          </div>
        {/if}
      {/each}
    {/each}

    {#each waiting as w, n (n)}
      <div class="you"><p>{w}</p></div>
    {/each}
    {#if engine.sorting > 0}
      <div class="act"><Orb process="thinking" size={16} /><span>Working out what to do…</span></div>
    {/if}

    {#if !turns.length && !waiting.length}
      <div class="empty">
        <p>Ask a question or hand Engine a job.</p>
        <p class="hint">“Open Spotify”, “what's on today?”, “draft a reply to the email from…”. Or hold <kbd>`</kbd>, or say “Engine”.</p>
      </div>
    {/if}
  </div>

  {#if choices.length}
    <div class="chips">
      {#each choices as c (c.target)}
        <button class="chip" onclick={() => pick(c)}>{c.name}</button>
      {/each}
    </div>
  {/if}

  <form
    class="dump"
    onsubmit={(e) => {
      e.preventDefault();
      submit();
    }}
  >
    <div class="field">
      <textarea
        id="dump"
        aria-label="Message Engine"
        bind:this={input}
        bind:value={text}
        onkeydown={onInputKey}
        rows="2"
        placeholder="Ask or tell Engine anything…"
        spellcheck="false"
      ></textarea>
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
              <span class="chip-text">{prefs.folder ? baseName(prefs.folder) : "Folder"}</span>
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
            {powering ? "…" : prefs.local_ai ? "Local" : "Cloud"}
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
    {#if note}
      <p class="status" aria-live="polite"><span class="dot warn" aria-hidden="true"></span>{note}</p>
    {/if}
  </form>
</section>

<style>
  .chat {
    display: flex;
    flex-direction: column;
    min-height: 0;
    min-width: 0;
    padding: 10px 12px 12px;
  }

  .chat-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 0 2px 6px;
  }

  .chat-head .label {
    margin: 0;
  }

  .thread {
    display: flex;
    flex: 1;
    flex-direction: column;
    gap: 8px;
    min-height: 0;
    padding: 4px 2px 10px;
    overflow-y: auto;
    scrollbar-width: thin;
    scrollbar-color: var(--border) transparent;
    font-size: 13px;
    line-height: 1.5;
    user-select: text;
  }

  .you {
    display: flex;
    justify-content: flex-end;
    margin-top: 6px;
  }

  .you p {
    max-width: 85%;
    margin: 0;
    padding: 7px 11px;
    border-radius: var(--r-md) var(--r-md) 4px var(--r-md);
    background: color-mix(in oklab, var(--accent), transparent 78%);
    color: var(--text);
    white-space: pre-wrap;
    overflow-wrap: anywhere;
  }

  .act {
    display: flex;
    align-items: center;
    gap: 8px;
    color: var(--text-secondary);
  }

  .act.warn {
    color: var(--warning, #f2b85c);
  }

  .reply {
    padding: 2px 2px 0;
    color: var(--text);
    overflow-wrap: anywhere;
  }

  .reply p {
    margin: 0 0 4px;
  }

  .reply p.head {
    margin-top: 6px;
    font-weight: 600;
  }

  .reply p.bullet {
    position: relative;
    padding-left: 14px;
  }

  .reply p.bullet::before {
    content: "";
    position: absolute;
    top: 0.65em;
    left: 3px;
    width: 4px;
    height: 4px;
    border-radius: 50%;
    background: var(--accent);
  }

  .job {
    display: flex;
    flex-direction: column;
    gap: 6px;
    padding: 9px 11px;
    border: 1px solid var(--border);
    border-radius: var(--r-md);
    background: var(--surface-2);
  }

  .job.failed {
    border-color: rgb(242 184 92 / 0.45);
  }

  .job.ask {
    border-color: color-mix(in oklab, var(--accent), transparent 45%);
  }

  .job-head {
    display: flex;
    align-items: center;
    gap: 8px;
    min-width: 0;
  }

  .job-title {
    flex: 1;
    min-width: 0;
    color: var(--text);
    font-weight: 600;
    overflow-wrap: anywhere;
  }

  .state {
    flex-shrink: 0;
    color: var(--text-muted);
    font-size: 12px;
  }

  .step {
    margin: 0;
    color: var(--text-secondary);
    overflow-wrap: anywhere;
  }

  .concern {
    margin: 0;
    color: var(--warning, #f2b85c);
  }

  .row-actions {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
  }

  .row-actions .button {
    height: 28px;
    padding: 0 10px;
    font-size: 12px;
  }

  .empty {
    margin: auto 0;
    padding: 0 8px;
    color: var(--text-secondary);
    text-align: center;
  }

  .empty p {
    margin: 0 0 6px;
  }

  .empty .hint {
    color: var(--text-muted);
    font-size: 12px;
  }

  .chips {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
    padding: 0 2px 8px;
  }

  .dump {
    position: relative;
    z-index: 5; /* above the thread, so the folder menu isn't hidden */
  }

  .field {
    display: flex;
    flex-direction: column;
    gap: 8px;
    padding: 10px 10px 8px;
    border: 1px solid var(--border);
    border-radius: var(--r-lg);
    background: rgb(14 12 21 / 0.55);
    transition: border-color var(--ease), box-shadow var(--ease);
  }

  .field:focus-within {
    border-color: var(--accent);
    box-shadow: 0 0 0 3px rgb(142 144 245 / 0.14);
  }

  textarea {
    resize: none;
    border: 0;
    outline: 0;
    background: transparent;
    color: var(--text);
    font: inherit;
    font-size: 14px;
    line-height: 1.45;
    user-select: text;
  }

  textarea::placeholder {
    color: var(--text-muted);
  }

  .tools {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 6px;
  }

  .chips-left,
  .chips-right {
    display: flex;
    align-items: center;
    gap: 6px;
    min-width: 0;
  }

  .tool-chip {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    max-width: 120px;
    height: 28px;
    padding: 0 8px;
    border: 1px solid var(--border);
    border-radius: var(--r-sm);
    background: var(--surface-2);
    color: var(--text-secondary);
    font-size: 12px;
    white-space: nowrap;
    cursor: pointer;
    transition: border-color var(--ease), color var(--ease);
  }

  .chip-text {
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .tool-chip:hover:not(:disabled) {
    border-color: color-mix(in oklab, var(--border), white 22%);
    color: var(--text);
  }

  .tool-chip svg {
    width: 13px;
    height: 13px;
    flex-shrink: 0;
  }

  .tool-chip select {
    max-width: 78px;
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
    bottom: calc(100% + 6px);
    left: 0;
    z-index: 10;
    display: flex;
    flex-direction: column;
    min-width: 240px;
    max-width: 340px;
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

  .mic,
  .send {
    display: grid;
    place-items: center;
    width: 32px;
    height: 32px;
    flex-shrink: 0;
    border-radius: var(--r-sm);
    transition:
      transform var(--m-out) var(--curve),
      background-color var(--m-out) var(--curve),
      border-color var(--ease),
      color var(--ease),
      opacity var(--m-out) var(--curve);
  }

  .mic {
    border: 1px solid var(--border);
    background: var(--surface-2);
    color: var(--text-secondary);
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
    border: 0;
    background: var(--accent);
    color: var(--on-accent);
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
    gap: 8px;
    margin: 8px 2px 0;
    color: var(--text-muted);
    font-size: 12px;
    user-select: text;
  }
</style>
