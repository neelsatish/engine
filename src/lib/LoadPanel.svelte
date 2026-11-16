<script lang="ts">
  // System load: CPU, RAM, GPU, VRAM and GPU temperature, refreshed every two seconds while Engine is visible.
  import { invoke } from "@tauri-apps/api/core";
  import { onMount } from "svelte";

  type Load = {
    cpu: number;
    ram_used_gb: number;
    ram_total_gb: number;
    gpu: { name: string; load: number; vram_used_gb: number; vram_total_gb: number; temp_c: number } | null;
  };

  let load = $state<Load | null>(null);

  const rows = $derived.by(() => {
    if (!load) return [];
    const r = [
      { label: "CPU", value: `${Math.round(load.cpu)}%`, pct: load.cpu },
      {
        label: "RAM",
        value: `${load.ram_used_gb.toFixed(1)} / ${load.ram_total_gb.toFixed(0)} GB`,
        pct: (load.ram_used_gb / load.ram_total_gb) * 100,
      },
    ];
    if (load.gpu) {
      r.push(
        { label: "GPU", value: `${load.gpu.load}%`, pct: load.gpu.load },
        {
          label: "VRAM",
          value: `${load.gpu.vram_used_gb.toFixed(1)} / ${load.gpu.vram_total_gb.toFixed(0)} GB`,
          pct: (load.gpu.vram_used_gb / load.gpu.vram_total_gb) * 100,
        },
        { label: "GPU temp", value: `${load.gpu.temp_c}°C`, pct: Math.min(100, (load.gpu.temp_c / 95) * 100) },
      );
    }
    return r;
  });

  onMount(() => {
    let timer = 0;
    const tick = async () => {
      if (!document.hidden) load = await invoke<Load>("system_load").catch(() => load);
    };
    tick();
    timer = window.setInterval(tick, 2000);
    return () => clearInterval(timer);
  });
</script>

<section class="load glass" aria-label="System load">
  <h2 class="label">System {#if load?.gpu}<span class="gpu">{load.gpu.name}</span>{/if}</h2>
  {#each rows as r (r.label)}
    <div class="meter">
      <div class="top"><span>{r.label}</span><span class="value">{r.value}</span></div>
      <div class="bar" aria-hidden="true"><span style="width: {Math.max(2, Math.min(100, r.pct))}%" class:hot={r.pct > 85}></span></div>
    </div>
  {:else}
    <p class="empty">Reading…</p>
  {/each}
</section>

<style>
  .load {
    display: flex;
    flex-direction: column;
    gap: 9px;
    min-height: 0;
    padding: 12px 14px;
    overflow: hidden;
  }

  .label {
    margin: 0 0 2px;
    justify-content: space-between;
  }

  .gpu {
    overflow: hidden;
    color: var(--text-muted);
    font-size: 12px;
    font-weight: 500;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .top {
    display: flex;
    justify-content: space-between;
    color: var(--text-secondary);
    font-size: 12px;
  }

  .value {
    color: var(--text);
    font-variant-numeric: tabular-nums;
  }

  .bar {
    height: 4px;
    margin-top: 4px;
    border-radius: 999px;
    background: var(--surface-2);
    overflow: hidden;
  }

  .bar span {
    display: block;
    height: 100%;
    border-radius: inherit;
    background: var(--accent);
    transition: width 600ms var(--curve);
  }

  .bar span.hot {
    background: #f2b85c;
  }

  .empty {
    margin: 0;
    color: var(--text-muted);
    font-size: 13px;
  }
</style>
