<script lang="ts">
  // The home view: the dump box and today's briefing (Now, Running, Needs you, Done).
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import { onMount } from "svelte";
  import Orb, { type Process } from "$lib/Orb.svelte";

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
    const off = listen("briefing", load);
    return () => off.then((f) => f());
  });
</script>

<form
  class="dump"
  onsubmit={(e) => {
    e.preventDefault();
    submit();
  }}
>
  <label class="label" for="dump">Dump</label>
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
      <svg class="ico" viewBox="0 0 24 24" aria-hidden="true"><path d="M9 10 4 15l5 5" /><path d="M20 4v7a4 4 0 0 1-4 4H4" /></svg>
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

<div class="scroll" aria-live="polite">
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
  {/each}
</div>

<style>
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
