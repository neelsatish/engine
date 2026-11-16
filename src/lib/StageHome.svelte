<script lang="ts">
  // The home stage: the ASCII logo ripples while you talk, then hands over to an orb with what Engine is doing.
  import AsciiLogo from "$lib/AsciiLogo.svelte";
  import Orb, { type Process } from "$lib/Orb.svelte";
  import { engine, running, queued, needsYou } from "$lib/state.svelte";

  const working = $derived(running());
  const waiting = $derived(queued());
  const asks = $derived(needsYou());
  const busy = $derived(engine.voice === "transcribing" || engine.sorting > 0 || working.length > 0);
  const lastHeard = $derived(engine.heard.at(-1)?.text ?? "");

  // The orb shows the kind of work in progress: transcribing or sorting is thinking; an agent shows its own step.
  const process = $derived<Process>(
    engine.voice === "transcribing" || engine.sorting > 0
      ? "thinking"
      : ((working[0]?.process || "working") as Process),
  );

  const status = $derived.by(() => {
    if (engine.voice === "listening") return "Listening";
    if (engine.voice === "transcribing") return "Catching that";
    if (engine.sorting > 0) return "Sorting it out";
    if (working.length === 1) return working[0].result || working[0].title;
    if (working.length > 1) return `${working.length} agents working`;
    return "Ready";
  });

  const detail = $derived.by(() => {
    if (engine.voice === "listening") return lastHeard ? `“${lastHeard}”` : "Hold ` and speak. Engine acts as you go.";
    if (engine.voice === "transcribing" && lastHeard) return `“${lastHeard}”`;
    if (working.length === 1) return working[0].title;
    if (working.length > 1) return working.map((w) => w.title).slice(0, 2).join(" · ");
    if (waiting.length) return `${waiting.length} waiting to start`;
    if (asks.length) return `${asks.length} need${asks.length === 1 ? "s" : ""} you`;
    return "Hold ` to talk, or type below";
  });
</script>

<div class="home">
  <div class="logo">
    <AsciiLogo level={engine.level} active={engine.voice === "listening"} dim={busy} />
    {#if busy}
      <div class="orb-wrap" aria-hidden="true"><Orb {process} size={132} /></div>
    {/if}
  </div>
  <div class="status" aria-live="polite">
    <span class="state" class:live={engine.voice === "listening"}>{status}</span>
    <span class="detail">{detail}</span>
  </div>
</div>

<style>
  .home {
    display: grid;
    grid-template-rows: 1fr auto;
    height: 100%;
    min-height: 0;
    padding: 18px 24px 22px;
  }

  .logo {
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
    gap: 2px;
    text-align: center;
  }

  .state {
    color: var(--text);
    font-size: 17px;
    font-weight: 600;
    letter-spacing: -0.01em;
  }

  .state.live {
    color: var(--accent);
  }

  .detail {
    max-width: 60ch;
    overflow: hidden;
    color: var(--text-muted);
    font-size: 13px;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
</style>
