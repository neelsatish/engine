<script lang="ts">
  // The V8 logo drawn in ASCII. Idle, a slow gradient drifts through it; while the person talks, ripples spread
  // from the centre with their voice level. It draws only while Engine is visible and focused.
  import { onMount } from "svelte";

  let { level = 0, active = false, dim = false }: { level?: number; active?: boolean; dim?: boolean } = $props();

  const CHARS = " .:-=+*#%@";
  const COLS = 72;
  const CELL_ASPECT = 1.9; // a character cell is this much taller than wide
  // The Velvet Fold gradient, plus a brighter lift for the ripple crests.
  const STOPS = [
    [111, 127, 196],
    [143, 146, 238],
    [185, 187, 255],
    [214, 210, 255],
  ];

  let canvas: HTMLCanvasElement;
  let wrap: HTMLDivElement;

  // Read props through a getter so the draw loop always sees the newest values.
  const current = () => ({ level, active });

  onMount(() => {
    const ctx = canvas.getContext("2d")!;
    const reduced = matchMedia("(prefers-reduced-motion: reduce)").matches;
    let mask: Float32Array | null = null;
    let rows = 0;
    let raf = 0;
    let last = 0;
    let smooth = 0;
    const palette = Array.from({ length: 96 }, (_, i) => {
      const t = (i / 95) * (STOPS.length - 1);
      const a = STOPS[Math.floor(t)];
      const b = STOPS[Math.min(Math.floor(t) + 1, STOPS.length - 1)];
      const f = t - Math.floor(t);
      return `rgb(${a.map((v, k) => Math.round(v + (b[k] - v) * f)).join(",")})`;
    });

    const img = new Image();
    img.onload = () => {
      // Sample the logo onto a grid of character cells, keeping its proportions.
      rows = Math.round((COLS * (img.height / img.width)) / CELL_ASPECT);
      const off = document.createElement("canvas");
      off.width = COLS;
      off.height = rows;
      const o = off.getContext("2d")!;
      o.drawImage(img, 0, 0, COLS, rows);
      const data = o.getImageData(0, 0, COLS, rows).data;
      mask = new Float32Array(COLS * rows);
      for (let i = 0; i < COLS * rows; i++) {
        const [r, g, b, a] = [data[i * 4], data[i * 4 + 1], data[i * 4 + 2], data[i * 4 + 3]];
        mask[i] = (a / 255) * (0.35 + (0.65 * (r + g + b)) / 765);
      }
      resize();
      draw(performance.now());
    };
    img.src = "/logo.svg";

    let cellW = 10;
    let cellH = 19;
    function resize() {
      if (!rows) return;
      const dpr = devicePixelRatio || 1;
      const { width, height } = wrap.getBoundingClientRect();
      cellW = Math.max(4, Math.min(width / COLS, height / (rows * CELL_ASPECT)));
      cellH = cellW * CELL_ASPECT;
      canvas.width = Math.round(COLS * cellW * dpr);
      canvas.height = Math.round(rows * cellH * dpr);
      canvas.style.width = `${COLS * cellW}px`;
      canvas.style.height = `${rows * cellH}px`;
      ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
      ctx.font = `600 ${cellH * 0.82}px "Cascadia Mono", Consolas, ui-monospace, monospace`;
      ctx.textBaseline = "top";
    }

    function draw(t: number) {
      if (!mask) return;
      const { level: lv, active: on } = current();
      smooth += ((on ? lv : 0) - smooth) * 0.25;
      const time = t / 1000;
      ctx.clearRect(0, 0, COLS * cellW, rows * cellH);
      const cx = COLS / 2;
      const cy = rows / 2;
      for (let y = 0; y < rows; y++) {
        for (let x = 0; x < COLS; x++) {
          const m = mask[y * COLS + x];
          if (m < 0.06) continue;
          const dx = x - cx;
          const dy = (y - cy) * CELL_ASPECT;
          const dist = Math.sqrt(dx * dx + dy * dy);
          // Ripples travel outward from the centre, as tall as the voice is loud.
          const ripple = smooth * Math.sin(dist * 0.42 - time * 9) * 0.5 + 0.5 * smooth;
          const drift = 0.5 + 0.5 * Math.sin(x * 0.07 + y * 0.11 - time * 0.6);
          const lit = Math.min(1, m + ripple * 0.45);
          const ch = CHARS[Math.min(CHARS.length - 1, Math.max(1, Math.round(lit * (CHARS.length - 1))))];
          const shade = Math.min(1, drift * 0.75 + ripple * 0.6);
          ctx.fillStyle = palette[Math.round(shade * (palette.length - 1))];
          ctx.fillText(ch, x * cellW, y * cellH + (on ? Math.sin(dist * 0.42 - time * 9) * smooth * 2 : 0));
        }
      }
    }

    function frame(t: number) {
      raf = requestAnimationFrame(frame);
      // Fast while listening, slow when idle: the idle drift doesn't need 60 frames a second.
      const gap = current().active ? 1000 / 40 : 1000 / 12;
      if (t - last < gap) return;
      last = t;
      draw(t);
    }
    const play = () => {
      if (!raf && !reduced) raf = requestAnimationFrame(frame);
    };
    const pause = () => {
      cancelAnimationFrame(raf);
      raf = 0;
    };
    const sync = () => (document.hidden ? pause() : play());
    const observer = new ResizeObserver(() => {
      resize();
      draw(performance.now());
    });
    observer.observe(wrap);
    sync();
    document.addEventListener("visibilitychange", sync);
    return () => {
      pause();
      observer.disconnect();
      document.removeEventListener("visibilitychange", sync);
    };
  });
</script>

<div class="ascii" class:dim bind:this={wrap} aria-hidden="true">
  <canvas bind:this={canvas}></canvas>
</div>

<style>
  .ascii {
    display: grid;
    place-items: center;
    width: 100%;
    height: 100%;
    min-height: 0;
    transition: opacity 400ms var(--curve), filter 400ms var(--curve);
  }

  .ascii.dim {
    opacity: 0.18;
    filter: blur(1px);
  }
</style>
