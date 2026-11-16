<script lang="ts">
  // The home view: the dump box and today's briefing (Now, Running, Needs you, Done).
  import { invoke } from "@tauri-apps/api/core";
  import { open as openDialog } from "@tauri-apps/plugin-dialog";
  import { listen } from "@tauri-apps/api/event";
  import { onMount, tick } from "svelte";
  import Orb, { type Process } from "$lib/Orb.svelte";
  import { type Block, todayRange } from "$lib/time";

  type Target = { name: string; target: string };
  type Reply =
    | { kind: "opened"; name: string }
    | { kind: "choices"; items: Target[] }
    | { kind: "routed"; count: number }
    | { kind: "error"; message: string };
  type Item = {
    id: number;
    kind: "instant" | "answer" | "job" | "outward" | "task";
    title: string;
    detail: string;
    status: "queued" | "running" | "held" | "done" | "failed";
    process: string;
    result: string;
    created: string;
    verdict: "" | "checking" | "good" | "concern";
    verdict_reason: string;
  };

  let text = $state("");
  let sorting = $state(0);
  let note = $state("");
  let choices = $state<Target[]>([]);
  let items = $state<Item[]>([]);
  let input: HTMLTextAreaElement;
  let revising = $state<number | null>(null);
  let today = $state<Block[]>([]);
  // The working folder and agent model, shown as chips in the dump box (saved in Settings).
  type Prefs = { folder: string; recent: string[]; models: { agent: string; answer: string } };
  let prefs = $state<Prefs & Record<string, unknown>>({ folder: "", recent: [], models: { agent: "haiku", answer: "sonnet" } });
  let folderMenu = $state(false);
  const MODEL_NAMES: Record<string, string> = { haiku: "Haiku", sonnet: "Sonnet", opus: "Opus", fable: "Fable" };
  const baseName = (p: string) => p.split(/[\\/]/).filter(Boolean).pop() ?? p;

  async function loadPrefs() {
    try {
      prefs = await invoke("get_settings");
    } catch {
      // a broken settings file shows its error in Settings
    }
  }

  async function savePrefs(patch: Partial<Prefs>) {
    try {
      prefs = await invoke("save_settings", { settings: { ...prefs, ...patch } });
    } catch (e) {
      note = String(e);
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

  // Voice: "listening" while the mic is open, "transcribing" while Whisper works.
  let voice = $state<"idle" | "listening" | "transcribing">("idle");
  let reviseNote = $state("");

  // Shown while the briefing is empty: one click puts an example in the dump box.
  const EXAMPLES = [
    "open spotify",
    "make me a 5-day study plan for chemistry",
    "research the best budget headphones and open the top pick",
  ];

  const sections = $derived(
    [
      { name: "Now", items: items.filter((i) => i.kind === "instant") },
      { name: "Running", items: items.filter((i) => i.kind !== "instant" && ["running", "queued"].includes(i.status)) },
      { name: "Needs you", items: items.filter((i) => i.kind !== "instant" && ["held", "failed"].includes(i.status)) },
      { name: "Done", items: items.filter((i) => i.kind !== "instant" && i.status === "done") },
    ].filter((s) => s.items.length),
  );

  export function focus() {
    input?.focus();
  }

  async function load() {
    try {
      items = await invoke<Item[]>("briefing");
    } catch (e) {
      note = `Couldn't load the briefing: ${e}`;
    }
  }

  async function loadToday() {
    today = await invoke<Block[]>("blocks", todayRange()).catch(() => []);
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

  async function act(command: "dismiss" | "cancel" | "retry" | "approve" | "task_done", id: number) {
    try {
      await invoke(command, { id });
    } catch (e) {
      note = `Couldn't ${command.replace("_", " ")}: ${e}`;
    }
  }

  async function sendRevision(id: number) {
    const text = reviseNote.trim();
    if (!text) return;
    try {
      await invoke("revise", { id, note: text });
      revising = null;
      reviseNote = "";
    } catch (e) {
      note = `Couldn't revise: ${e}`;
    }
  }

  // What was heard goes into the dump box; with auto-submit on, it's sent straight away.
  async function useHeard(heard: string, submitNow: boolean) {
    if (!heard) return;
    text = text.trim() ? `${text.trim()} ${heard}` : heard;
    await tick();
    if (submitNow) submit();
    else input.focus();
  }

  async function toggleMic() {
    note = "";
    try {
      if (voice === "listening") {
        voice = "transcribing";
        const heard = await invoke<string>("voice_stop");
        const settings = await invoke<{ voice: { auto_submit: boolean } }>("get_settings").catch(() => null);
        if (heard) await useHeard(heard, settings?.voice.auto_submit ?? false);
        else note = "Didn't catch that. Try again, a little closer to the mic.";
      } else {
        await invoke("voice_start");
      }
    } catch (e) {
      note = String(e);
      voice = "idle";
    }
  }

  async function reviewDay() {
    try {
      await invoke("review_day");
    } catch (e) {
      note = `Couldn't start the review: ${e}`;
    }
  }

  async function readAloud() {
    try {
      await invoke("speak_briefing");
    } catch (e) {
      note = `Couldn't read the briefing: ${e}`;
    }
  }

  function onInputKey(e: KeyboardEvent) {
    if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      submit();
    }
  }

  // A running job shows the orb for its current step (viewing, searching, writing...); a running answer is writing.
  const orbFor = (i: Item): Process | undefined =>
    i.status === "running"
      ? ((i.process || "writing") as Process)
      : ({ queued: "waiting", held: "held" } as Partial<Record<Item["status"], Process>>)[i.status];
  const time = (i: Item) => i.created.slice(11, 16);

  onMount(() => {
    input.focus();
    load();
    loadToday();
    loadPrefs();
    const offs = [
      listen("briefing", load),
      listen("schedule", loadToday),
      listen<{ state: string; message: string }>("voice", (e) => {
        if (e.payload.state === "error") {
          note = e.payload.message;
          voice = "idle";
        } else {
          voice = e.payload.state as typeof voice;
        }
      }),
      listen<{ text: string; submit: boolean }>("heard", (e) => useHeard(e.payload.text, e.payload.submit)),
      listen<Prefs & Record<string, unknown>>("settings", (e) => (prefs = e.payload)),
    ];
    return () => offs.forEach((off) => off.then((f) => f()));
  });
</script>

<form
  class="dump"
  onsubmit={(e) => {
    e.preventDefault();
    submit();
  }}
>
  <div class="dump-head">
    <label class="label" for="dump">Dump</label>
    <span class="head-actions">
    <button class="link speak" onclick={reviewDay} title="A short look back at today and ahead to tomorrow">
      <svg class="ico" viewBox="0 0 24 24" aria-hidden="true"
        ><path d="M12 3a6 6 0 0 0 9 9 9 9 0 1 1-9-9" /></svg
      >
      Review my day
    </button>
    <button class="link speak" onclick={readAloud} title="Read the briefing aloud">
      <svg class="ico" viewBox="0 0 24 24" aria-hidden="true"
        ><path d="M11 5 6 9H3v6h3l5 4z" /><path d="M15.5 8.5a5 5 0 0 1 0 7" /><path d="M18.5 5.5a9 9 0 0 1 0 13" /></svg
      >
      Read aloud
    </button>
    </span>
  </div>
  <div class="field glass">
    <div class="top">
    <textarea
      id="dump"
      bind:this={input}
      bind:value={text}
      onkeydown={onInputKey}
      rows="2"
      placeholder="Dump anything: open an app, ask for a plan, hand off a task."
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
            {#each Object.entries(MODEL_NAMES) as [id, name] (id)}<option value={id}>{name}</option>{/each}
          </select>
        </label>
      </div>
      <div class="chips-right">
    <button
      class="mic"
      class:on={voice === "listening"}
      type="button"
      aria-label={voice === "listening" ? "Stop and transcribe" : "Speak"}
      aria-pressed={voice === "listening"}
      title="Speak (or hold Ctrl+Alt+Space anywhere)"
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
    {#if voice === "listening"}
      <span class="indicator"><Orb process="listening" size={16} /></span>Listening… click the mic again (or let go of the keys) when you're done.
    {:else if voice === "transcribing"}
      <span class="indicator"><Orb process="thinking" size={16} /></span>Turning your words into text…
    {:else if sorting}
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

<div class="scroll" aria-live="polite">
  {#if today.length}
    <section class="card glass">
      <h2>Today <span class="count">{today.length}</span></h2>
      <ul class="rows">
        {#each today as b (b.id)}
          <li>
            <span class="indicator"><span class="kind {b.kind}" aria-hidden="true"></span></span>
            <span class="text body">{b.title}</span>
            <time>{b.start.slice(11, 16)}–{b.end.slice(11, 16)}</time>
          </li>
        {/each}
      </ul>
    </section>
  {/if}
  {#each sections as s (s.name)}
    <section class="card glass">
      <h2>{s.name} <span class="count">{s.items.length}</span></h2>
      <ul class="rows">
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
                  <summary class="text">
                    {i.title}
                    {#if i.verdict === "checking"}
                      <span class="verdict checking">Checking…</span>
                    {:else if i.verdict}
                      <span class="verdict {i.verdict}" title={i.verdict_reason}>
                        {i.verdict === "good" ? "Looks good" : "Concern"}
                      </span>
                    {/if}
                  </summary>
                  {#if i.verdict === "concern" && i.verdict_reason}
                    <p class="concern-note">{i.verdict_reason}</p>
                  {/if}
                  <pre>{i.result}</pre>
                </details>
              {:else if i.kind === "outward" && i.status === "held"}
                <span class="text">{i.title}</span>
                <span class="sub">Needs your approval. Nothing happens until you approve.</span>
                {#if i.detail}<pre class="proposal">{i.detail}</pre>{/if}
                <div class="card-actions">
                  <button class="button primary" onclick={() => act("approve", i.id)}>Approve</button>
                  <button
                    class="button"
                    onclick={() => {
                      revising = revising === i.id ? null : i.id;
                      reviseNote = "";
                    }}>Revise</button
                  >
                  <button class="button" onclick={() => act("dismiss", i.id)}>Reject</button>
                </div>
                {#if revising === i.id}
                  <form
                    class="revise"
                    onsubmit={(e) => {
                      e.preventDefault();
                      sendRevision(i.id);
                    }}
                  >
                    <input class="input" bind:value={reviseNote} placeholder="What should change?" aria-label="Revision note" />
                    <button class="button" type="submit" disabled={!reviseNote.trim()}>Send</button>
                  </form>
                {/if}
              {:else if i.kind === "task" && i.status === "held"}
                <span class="text">{i.title}</span>
                {#if i.detail}<span class="sub">When you're done, an agent will: {i.detail}</span>{/if}
                <div class="card-actions">
                  <button class="button primary" onclick={() => act("task_done", i.id)}>Done</button>
                  <button class="button" onclick={() => act("dismiss", i.id)}>Dismiss</button>
                </div>
              {:else}
                <span class="text">{i.title}</span>
                {#if i.result}<span class="sub">{i.result}</span>{/if}
              {/if}
            </div>
            {#if (i.kind === "outward" || i.kind === "task") && i.status === "held"}
              <!-- actions sit inside the card body -->
            {:else if i.kind === "job" && i.status === "running"}
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
    {#if !today.length}
    <section class="card glass empty">
      <img src="/logo.svg" alt="" width="40" height="40" />
      <p>Nothing yet today. Dump anything above, or try one:</p>
      <div class="chips">
        {#each EXAMPLES as example (example)}
          <button
            class="chip"
            onclick={() => {
              text = example;
              input.focus();
            }}>{example}</button
          >
        {/each}
      </div>
    </section>
    {/if}
  {/each}
</div>

<style>
  .dump {
    position: relative;
    z-index: 5; /* above the cards below, so the folder menu isn't hidden behind them */
    margin: var(--space-xs) 0 var(--space-sm);
  }

  .field {
    display: flex;
    flex-direction: column;
    gap: var(--space-sm);
    padding: var(--space-md) var(--space-md) 10px;
    border-radius: 14px;
    transition: border-color var(--ease), box-shadow var(--ease);
  }

  .field:focus-within {
    border-color: var(--accent);
    box-shadow: 0 0 0 4px var(--accent-glow);
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
    border-radius: 999px;
    background: rgb(29 25 42 / 0.8);
    color: var(--text-secondary);
    font-size: 12px;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    cursor: pointer;
    transition: border-color var(--ease), color var(--ease);
  }

  .tool-chip:hover {
    border-color: var(--accent);
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
    background: rgb(19 16 28 / 0.97);
    box-shadow: 0 12px 32px rgb(0 0 0 / 0.5);
  }

  .menu button {
    display: flex;
    flex-direction: column;
    padding: 7px 10px;
    border: 0;
    border-radius: 7px;
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
    font-size: 11px;
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

  .dump-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
  }

  .dump-head .label {
    margin: 0;
  }

  .head-actions {
    display: flex;
    gap: 2px;
  }

  .speak {
    display: inline-flex;
    align-items: center;
    gap: 6px;
  }

  .speak svg {
    width: 15px;
    height: 15px;
  }

  .dump-head {
    margin-bottom: var(--space-xs);
  }

  .mic {
    display: grid;
    place-items: center;
    width: 38px;
    height: 38px;
    flex-shrink: 0;
    border: 1px solid var(--border);
    border-radius: 11px;
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
    color: var(--danger);
    box-shadow: 0 0 0 4px rgb(240 113 138 / 0.18);
  }

  .mic:disabled {
    opacity: 0.5;
    cursor: default;
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

  .verdict {
    margin-left: 6px;
    padding: 1px 8px;
    border: 1px solid var(--border);
    border-radius: 999px;
    font-size: 11px;
    font-weight: 600;
    vertical-align: 1px;
  }

  .verdict.good {
    border-color: rgb(95 212 160 / 0.4);
    color: var(--success);
  }

  .verdict.concern {
    border-color: rgb(242 184 92 / 0.5);
    color: #f2b85c;
  }

  .verdict.checking {
    color: var(--text-muted);
  }

  .concern-note {
    margin: var(--space-sm) 0 0;
    color: #f2b85c;
    font-size: 13px;
    user-select: text;
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

  .kind {
    width: 8px;
    height: 8px;
    border-radius: 2px;
    background: #9f9ab8;
  }

  .kind.study { background: #8e90f5; }
  .kind.test { background: #f0718a; }
  .kind.deadline { background: #f2b85c; }
  .kind.recording { background: #5fd4a0; }
  .kind.reminder { background: #c9a8ff; border-radius: 50%; }

  .proposal {
    max-height: 200px;
    overflow-y: auto;
  }

  .card-actions,
  .revise {
    display: flex;
    gap: var(--space-sm);
    margin: var(--space-sm) 0 var(--space-xs);
  }

  .revise .input {
    flex: 1;
  }

  time {
    color: var(--text-muted);
    font-size: 13px;
    font-variant-numeric: tabular-nums;
  }
</style>
