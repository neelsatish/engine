<script lang="ts">
  // The home tile, laid out like fastfetch: the ASCII logo (it ripples while you talk, then hands over to an orb
  // with what Engine is doing) beside Engine's own stats.
  import AsciiLogo from "$lib/AsciiLogo.svelte";
  import Orb, { type Process } from "$lib/Orb.svelte";
  import { engine, running, queued, needsYou } from "$lib/state.svelte";

  const working = $derived(running());
  const waiting = $derived(queued());
  const asks = $derived(needsYou());
  const busy = $derived(engine.voice === "transcribing" || engine.sorting > 0 || working.length > 0);
  const lastHeard = $derived(engine.heard.at(-1)?.text ?? "");
  const done = $derived(engine.items.filter((i) => i.kind !== "instant" && i.status === "done").length);

  // The orb shows the kind of work in progress: transcribing or sorting is thinking; an agent shows its own step.
  const process = $derived<Process>(
    engine.voice === "transcribing" || engine.sorting > 0 ? "thinking" : ((working[0]?.process || "working") as Process),
  );

  const status = $derived.by(() => {
    if (engine.voice === "listening") return "listening";
    if (engine.voice === "transcribing") return "catching that";
    if (engine.sorting > 0) return "sorting it out";
    if (working.length === 1) return working[0].result || working[0].title;
    if (working.length > 1) return `${working.length} agents working`;
    return "ready";
  });

  const detail = $derived.by(() => {
    if (engine.voice === "listening") return lastHeard ? `“${lastHeard}”` : "speak, Engine acts as you go";
    if (engine.voice === "transcribing" && lastHeard) return `“${lastHeard}”`;
    if (working.length === 1) return working[0].title;
    if (working.length > 1) return working.map((w) => w.title).slice(0, 2).join(" · ");
    if (asks.length) return `${asks.length} need${asks.length === 1 ? "s" : ""} you`;
    return "hold ` or say “engine”";
  });

  const p = $derived(engine.prefs);
  const v = $derived((p?.voice ?? {}) as Record<string, unknown>);
  const name = (m: string) => (m.startsWith("codex:") ? m.slice(6).replace("gpt-", "GPT-") : m[0]?.toUpperCase() + m.slice(1));
  const info = $derived<[string, string][]>([
    ["agents", p ? name(p.models.agent) : "…"],
    ["answers", p ? name(p.models.answer) : "…"],
    ["brain", p?.local_ai ? "local · qwen3 8b" : "cloud · claude"],
    ["voice", `${String(v.model ?? "…")} · ${v.engine === "chatterbox" ? "chatterbox" : "british"}`],
    ["wake", v.wake === false ? "off" : "say “engine”"],
    ["folder", p?.folder ? (p.folder.split(/[\\/]/).filter(Boolean).pop() ?? p.folder) : "none"],
    ["tasks", `${working.length} running · ${waiting.length} queued`],
    ["today", `${done} done · ${asks.length} need you`],
  ]);
  const SWATCHES = ["#0e0c15", "#3a3566", "#6f7fc4", "#8f92ee", "#b9bbff", "#c9a3f5", "#f0718a", "#5fd4a0"];
</script>

<fieldset class="pane home">
  <legend><sup>3</sup>home</legend>
  <div class="logo">
    <div class="art">
      <AsciiLogo level={engine.level} active={engine.voice === "listening"} dim={busy} />
      {#if busy}
        <div class="orb-wrap" aria-hidden="true"><Orb {process} size={96} /></div>
      {/if}
    </div>
    <div class="status" aria-live="polite">
      <span class="state" class:live={engine.voice === "listening"}>{status}</span>
      <span class="detail">{detail}</span>
    </div>
  </div>

  <div class="info">
    <p class="who"><span class="accent">engine</span>@<span class="accent">this-pc</span></p>
    <p class="rule" aria-hidden="true">──────────────</p>
    <dl>
      {#each info as [k, val] (k)}
        <dt>{k}</dt>
        <dd title={val}>{val}</dd>
      {/each}
    </dl>
    <div class="swatches" aria-hidden="true">
      {#each SWATCHES as c (c)}<i style="background: {c}"></i>{/each}
    </div>
  </div>
</fieldset>

<style>
  .home {
    display: grid;
    grid-template-columns: minmax(0, 1.05fr) minmax(0, 1fr);
    gap: 14px;
    overflow: hidden;
  }

  .logo {
    display: grid;
    grid-template-rows: minmax(0, 1fr) auto;
    min-height: 0;
  }

  .art {
    position: relative;
    min-height: 0;
  }

  .orb-wrap {
    position: absolute;
    inset: 0;
    display: grid;
    place-items: center;
    color: var(--accent);
    animation: rise 420ms var(--curve);
  }

  @keyframes rise {
    from {
      opacity: 0;
      transform: scale(0.94);
    }
  }

  .status {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 1px;
    padding-top: 4px;
    text-align: center;
  }

  .state {
    max-width: 100%;
    overflow: hidden;
    color: var(--text);
    font-size: 14px;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .state::before {
    content: "● ";
    color: var(--success);
    font-size: 10px;
    vertical-align: 2px;
  }

  .state.live {
    color: var(--accent);
  }

  .state.live::before {
    color: var(--meter-hi);
  }

  .detail {
    max-width: 100%;
    overflow: hidden;
    color: var(--text-muted);
    font-size: 11px;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .info {
    display: flex;
    flex-direction: column;
    justify-content: center;
    min-width: 0;
    font-size: 12px;
  }

  .info p {
    margin: 0;
  }

  .who {
    color: var(--text);
  }

  .accent {
    color: var(--pane-label);
  }

  .rule {
    overflow: hidden;
    color: var(--pane-border);
    white-space: nowrap;
  }

  dl {
    display: grid;
    grid-template-columns: 8ch minmax(0, 1fr);
    row-gap: 2px;
    margin: 4px 0 10px;
  }

  dt {
    color: var(--pane-label);
  }

  dd {
    margin: 0;
    overflow: hidden;
    color: var(--text-secondary);
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .swatches {
    display: flex;
    gap: 3px;
  }

  .swatches i {
    width: 18px;
    height: 10px;
    border-radius: 1px;
  }
</style>
