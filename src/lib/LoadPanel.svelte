<script lang="ts" module>
  // Kept outside the component, so the graphs survive switching views.
  const kept = { cpu: [] as number[], gpu: [] as number[] };
</script>

<script lang="ts">
  // System load in the style of btop: a CPU history graph, then cell meters for CPU, RAM, GPU, VRAM and GPU
  // temperature. Refreshed every two seconds while Engine is visible.
  import { invoke } from "@tauri-apps/api/core";
  import { onMount } from "svelte";
  import { queued, running } from "$lib/state.svelte";

  // Engine's own agents, like btop's process list.
  const agents = $derived([...running(), ...queued()]);

  type Load = {
    cpu: number;
    ram_used_gb: number;
    ram_total_gb: number;
    gpu: { name: string; load: number; vram_used_gb: number; vram_total_gb: number; temp_c: number } | null;
  };

  const CELLS = 22;
  const HISTORY = 60;
  const BARS = "▁▂▃▄▅▆▇█";

  let load = $state<Load | null>(null);
  let cpuHistory = $state<number[]>(kept.cpu);
  let gpuHistory = $state<number[]>(kept.gpu);

  const graph = (h: number[]) =>
    h.map((v) => BARS[Math.min(BARS.length - 1, Math.floor((v / 100) * BARS.length))]).join("").padStart(HISTORY, " ");

  const rows = $derived.by(() => {
    if (!load) return [];
    const r = [
      { label: "cpu", value: `${Math.round(load.cpu)}%`, pct: load.cpu },
      {
        label: "mem",
        value: `${load.ram_used_gb.toFixed(1)}/${load.ram_total_gb.toFixed(0)}G`,
        pct: (load.ram_used_gb / load.ram_total_gb) * 100,
      },
    ];
    if (load.gpu) {
      r.push(
        { label: "gpu", value: `${load.gpu.load}%`, pct: load.gpu.load },
        {
          label: "vram",
          value: `${load.gpu.vram_used_gb.toFixed(1)}/${load.gpu.vram_total_gb.toFixed(0)}G`,
          pct: (load.gpu.vram_used_gb / load.gpu.vram_total_gb) * 100,
        },
        { label: "temp", value: `${load.gpu.temp_c}°C`, pct: Math.min(100, (load.gpu.temp_c / 95) * 100) },
      );
    }
    return r;
  });

  // Each cell's colour follows its position, like btop's gradient meters.
  const cellColor = (k: number) => (k < CELLS * 0.55 ? "var(--meter-lo)" : k < CELLS * 0.82 ? "var(--meter-mid)" : "var(--meter-hi)");

  onMount(() => {
    const tick = async () => {
      if (document.hidden) return;
      const next = await invoke<Load>("system_load").catch(() => null);
      if (!next) return;
      load = next;
      cpuHistory = kept.cpu = [...cpuHistory, next.cpu].slice(-HISTORY);
      if (next.gpu) gpuHistory = kept.gpu = [...gpuHistory, next.gpu.load].slice(-HISTORY);
    };
    tick();
    const timer = window.setInterval(tick, 2000);
    return () => clearInterval(timer);
  });
</script>

<fieldset class="pane load" aria-label="System load">
  <legend><sup>1</sup>system{#if load?.gpu}<span class="gpu"> · {load.gpu.name}</span>{/if}</legend>
  {#if load}
    <div class="graphs" aria-hidden="true">
      <div class="graph"><span class="g-label">cpu</span><span class="spark"><span>{graph(cpuHistory)}</span></span></div>
      {#if load.gpu}<div class="graph"><span class="g-label">gpu</span><span class="spark gpu-spark"><span>{graph(gpuHistory)}</span></span></div>{/if}
    </div>
    <div class="meters">
      {#each rows as r (r.label)}
        {@const filled = Math.round((Math.max(0, Math.min(100, r.pct)) / 100) * CELLS)}
        <div class="meter" title="{r.label} {r.value}">
          <span class="m-label">{r.label}</span>
          <span class="cells" aria-hidden="true">
            {#each { length: CELLS } as _, k (k)}<i style={k < filled ? `background: ${cellColor(k)}` : ""}></i>{/each}
          </span>
          <span class="value">{r.value}</span>
        </div>
      {/each}
    </div>
  {:else}
    <p class="empty">reading…</p>
  {/if}
  <div class="procs" aria-label="Agents">
    <div class="p-row p-head"><span>st</span><span>agent</span><span>step</span></div>
    {#each agents as a (a.id)}
      <div class="p-row" title={a.title}>
        <span class={a.status}>{a.status === "running" ? "run" : "wait"}</span>
        <span class="p-title">{a.title}</span>
        <span class="p-step">{a.status === "running" ? a.result || "starting" : "queued"}</span>
      </div>
    {:else}
      <div class="p-empty">no agents running · ask in chat to start one</div>
    {/each}
  </div>
</fieldset>

<style>
  .load {
    display: flex;
    flex-direction: column;
    gap: 8px;
    overflow: hidden;
  }

  .gpu {
    color: var(--text-muted);
  }

  .graphs {
    display: flex;
    flex-direction: column;
    gap: 2px;
    padding: 6px 8px;
    border: 1px solid rgb(58 53 102 / 0.55);
    border-radius: var(--r-md);
    background: rgb(6 5 11 / 0.6);
  }

  .graph {
    display: flex;
    gap: 8px;
    min-width: 0;
    line-height: 1.15;
  }

  .g-label {
    flex-shrink: 0;
    width: 3ch;
    color: var(--text-muted);
    font-size: 11px;
  }

  .spark {
    display: flex;
    flex: 1;
    justify-content: flex-end; /* newest samples on the right; the oldest are cut off first when narrow */
    min-width: 0;
    overflow: hidden;
    color: var(--meter-mid);
    font-size: 15px;
    line-height: 1;
    letter-spacing: -0.04em;
    white-space: pre;
    text-shadow: 0 0 6px rgb(143 146 238 / 0.45);
  }

  .gpu-spark {
    color: #c9a3f5;
    text-shadow: 0 0 6px rgb(201 163 245 / 0.4);
  }

  .meters {
    display: flex;
    flex-direction: column;
    gap: 5px;
  }

  .meter {
    display: grid;
    grid-template-columns: 4.5ch minmax(0, 1fr) 9.5ch;
    align-items: center;
    gap: 8px;
    font-size: 12px;
  }

  .m-label {
    color: var(--text-secondary);
  }

  .cells {
    display: grid;
    grid-template-columns: repeat(22, 1fr);
    gap: 2px;
    height: 9px;
  }

  .cells i {
    border-radius: 1px;
    background: rgb(58 53 102 / 0.45);
    transition: background-color 400ms var(--curve);
  }

  .value {
    color: var(--text);
    font-variant-numeric: tabular-nums;
    text-align: right;
  }

  .empty {
    margin: 0;
    color: var(--text-muted);
  }

  .procs {
    flex: 1;
    min-height: 0;
    margin-top: 2px;
    padding-top: 6px;
    overflow: hidden;
    border-top: 1px dashed rgb(58 53 102 / 0.8);
    font-size: 11px;
  }

  .p-row {
    display: grid;
    grid-template-columns: 5ch minmax(0, 1.2fr) minmax(0, 1fr);
    gap: 8px;
    line-height: 1.6;
  }

  .p-row span {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .p-head {
    color: var(--text-muted);
  }

  .p-title {
    color: var(--text);
  }

  .p-step {
    color: var(--text-secondary);
  }

  .running {
    color: var(--success);
  }

  .queued {
    color: #c9a3f5;
  }

  .p-empty {
    color: var(--text-muted);
    line-height: 1.6;
  }
</style>
