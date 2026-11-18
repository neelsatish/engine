<script lang="ts">
  // The briefing view: today's blocks, then Now, Running, Needs you and Done, with approval cards and tasks.
  import { invoke } from "@tauri-apps/api/core";
  import Orb, { type Process } from "$lib/Orb.svelte";
  import { engine, submit as send, type Item } from "$lib/state.svelte";

  let revising = $state<number | null>(null);
  let reviseNote = $state("");
  const items = $derived(engine.items);
  const today = $derived(engine.today);

  // Shown while the briefing is empty: one click sends an example.
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

  async function act(command: "dismiss" | "cancel" | "retry" | "approve" | "task_done", id: number) {
    try {
      await invoke(command, { id });
    } catch (e) {
      engine.note = `Couldn't ${command.replace("_", " ")}: ${e}`;
    }
  }

  async function sendRevision(id: number) {
    const words = reviseNote.trim();
    if (!words) return;
    try {
      await invoke("revise", { id, note: words });
      revising = null;
      reviseNote = "";
    } catch (e) {
      engine.note = `Couldn't revise: ${e}`;
    }
  }

  // A running job shows the orb for its current step (viewing, searching, writing...); a running answer is writing.
  const orbFor = (i: Item): Process | undefined =>
    i.status === "running"
      ? ((i.process || "writing") as Process)
      : ({ queued: "waiting", held: "held" } as Partial<Record<Item["status"], Process>>)[i.status];
  const time = (i: Item) => i.created.slice(11, 16);
</script>

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
          <button class="chip" onclick={() => send(example)}>{example}</button>
        {/each}
      </div>
    </section>
    {/if}
  {/each}
</div>

<style>
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
      border-radius: var(--r-sm);
      font-size: 12px;
      font-weight: 500;
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
      border-radius: var(--r-md);
      color: var(--text-secondary);
      font: inherit;
      font-size: 13px;
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
