<script lang="ts">
  // The agents' model picker: the model, an optional stronger advisor it can consult, and how hard it thinks.
  import { engine, savePrefs } from "$lib/state.svelte";

  const CLAUDE = [
    ["haiku", "Haiku", "fast · light jobs"],
    ["sonnet", "Sonnet", "balanced · most jobs"],
    ["opus", "Opus", "deep work"],
    ["fable", "Fable", "strongest"],
  ];
  const CHATGPT = [
    ["codex:gpt-6.1-sol", "GPT-6.1 Sol", "via Codex"],
    ["codex:gpt-6-astra", "GPT-6 Astra", "via Codex"],
    ["codex:gpt-6.1-terra", "GPT-6.1 Terra", "via Codex"],
    ["codex:gpt-6-luna", "GPT-6 Luna", "via Codex"],
  ];
  const ADVISORS = [
    ["", "none"],
    ["fable", "fable"],
    ["opus", "opus"],
  ];
  const EFFORTS = ["", "low", "medium", "high", "xhigh", "max"]; // "" is Claude Code's default

  let open = $state(false);
  let wrap: HTMLDivElement;
  const models = $derived(engine.prefs?.models ?? { agent: "haiku", answer: "sonnet", effort: "", advisor: "" });
  const codex = $derived(models.agent.startsWith("codex:"));
  const name = $derived([...CLAUDE, ...CHATGPT].find((m) => m[0] === models.agent)?.[1] ?? models.agent);
  const level = $derived(Math.max(0, EFFORTS.indexOf(models.effort ?? "")));
  const label = $derived(
    [name + (models.advisor && !codex ? ` + ${models.advisor}` : ""), level && !codex ? EFFORTS[level] : ""].filter(Boolean).join(" · "),
  );

  const set = (patch: Partial<typeof models>) => savePrefs({ models: { ...models, ...patch } });

  function outside(e: MouseEvent) {
    if (open && !wrap.contains(e.target as Node)) open = false;
  }
</script>

<svelte:window onclick={outside} onkeydown={(e) => e.key === "Escape" && open && ((open = false), e.stopPropagation())} />

<div class="wrap" bind:this={wrap}>
  <button
    type="button"
    class="tool-chip"
    aria-haspopup="dialog"
    aria-expanded={open}
    title="The model background agents use"
    onclick={() => (open = !open)}
  >
    <svg class="ico" viewBox="0 0 24 24" aria-hidden="true"
      ><path d="M12 3l1.8 4.6L18 9l-4.2 1.4L12 15l-1.8-4.6L6 9l4.2-1.4z" /><path d="M19 15l.8 2 2 .8-2 .8-.8 2-.8-2-2-.8 2-.8z" /></svg
    >
    <span class="chip-text">{label}</span>
  </button>

  {#if open}
    <div class="menu" role="dialog" aria-label="Agent model">
      {#each [["claude", CLAUDE], ["chatgpt · codex", CHATGPT]] as [group, list] (group)}
        <p class="group">{group}</p>
        {#each list as [id, title, note] (id)}
          <button class="model" class:on={models.agent === id} aria-pressed={models.agent === id} onclick={() => set({ agent: id })}>
            <span class="caret" aria-hidden="true">›</span><span class="m-title">{title}</span><span class="m-note">{note}</span>
          </button>
        {/each}
      {/each}

      <div class="row" class:off={codex} title={codex ? "Advisors and effort apply to Claude agents" : ""}>
        <span class="k">advisor</span>
        <div class="seg" role="radiogroup" aria-label="Advisor">
          {#each ADVISORS as [id, text] (id)}
            <button role="radio" aria-checked={(models.advisor ?? "") === id} disabled={codex} onclick={() => set({ advisor: id })}>{text}</button>
          {/each}
        </div>
      </div>
      <div class="row" class:off={codex}>
        <span class="k">effort</span>
        <input
          type="range"
          min="0"
          max="5"
          step="1"
          value={level}
          disabled={codex}
          aria-label="Effort"
          aria-valuetext={EFFORTS[level] || "default"}
          oninput={(e) => set({ effort: EFFORTS[Number(e.currentTarget.value)] })}
        />
        <span class="v">{EFFORTS[level] || "default"}</span>
      </div>
      <p class="hint">An advisor is a stronger model the agent consults before big steps. More effort is slower and smarter.</p>
    </div>
  {/if}
</div>

<style>
  .wrap {
    position: relative;
  }

  .tool-chip {
    max-width: 190px !important;
  }

  .menu {
    position: absolute;
    bottom: calc(100% + 8px);
    left: 0;
    z-index: 10;
    width: 320px;
    padding: 8px;
    border: 1px solid var(--pane-border);
    border-radius: var(--r-md);
    background: rgb(12 10 20 / 0.98);
    box-shadow: 0 12px 32px rgb(0 0 0 / 0.5);
    font-size: 12px;
    text-shadow: none;
  }

  .group {
    margin: 6px 4px 2px;
    color: var(--text-muted);
    font-size: 11px;
  }

  .group:first-child {
    margin-top: 0;
  }

  .model {
    display: grid;
    grid-template-columns: 1.5ch 14ch 1fr;
    white-space: nowrap;
    align-items: center;
    width: 100%;
    height: 24px;
    padding: 0 6px;
    border: 0;
    border-radius: var(--r-sm);
    background: transparent;
    color: var(--text-secondary);
    text-align: left;
  }

  .model:hover,
  .model:focus-visible {
    background: rgb(142 144 245 / 0.1);
    outline: 0;
  }

  .caret {
    color: transparent;
  }

  .model.on {
    color: var(--text);
  }

  .model.on .caret {
    color: var(--accent);
  }

  .m-note {
    color: var(--text-muted);
    font-size: 11px;
  }

  .row {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-top: 8px;
    padding: 8px 4px 0;
    border-top: 1px dashed rgb(58 53 102 / 0.8);
  }

  .row + .row {
    border-top: 0;
    padding-top: 2px;
  }

  .row.off {
    opacity: 0.45;
  }

  .k {
    width: 7ch;
    color: var(--pane-label);
  }

  .seg {
    display: flex;
    gap: 2px;
  }

  .seg button {
    height: 22px;
    padding: 0 8px;
    border: 1px solid var(--pane-border);
    border-radius: var(--r-sm);
    background: transparent;
    color: var(--text-secondary);
    font-size: 11px;
  }

  .seg button[aria-checked="true"] {
    border-color: var(--accent);
    background: rgb(142 144 245 / 0.16);
    color: var(--text);
  }

  input[type="range"] {
    flex: 1;
    accent-color: var(--accent);
  }

  .v {
    width: 7ch;
    color: var(--text);
    text-align: right;
  }

  .hint {
    margin: 8px 4px 0;
    color: var(--text-muted);
    font-size: 11px;
    line-height: 1.4;
  }
</style>
