<script lang="ts">
  // Purple ASCII rain on the back layer. Kept cheap on purpose: about 20 fps, only while Engine is visible
  // and focused (it freezes on its last frame otherwise), and one still frame under reduced motion.
  import { onMount } from "svelte";

  const CHARS = "01<>/\\{}[]()=+*#%&;:~|^ABCDEFHKMNRSTXZ";
  const CELL = 16; // px per character cell
  const FPS = 20;
  const BG = "#0e0c15";

  let canvas: HTMLCanvasElement;

  onMount(() => {
    const ctx = canvas.getContext("2d")!;
    const still = matchMedia("(prefers-reduced-motion: reduce)").matches;
    let w = 0;
    let h = 0;
    let drops: number[] = [];
    let speeds: number[] = [];
    let raf = 0;
    let last = 0;

    const pick = () => CHARS[(Math.random() * CHARS.length) | 0];

    function step() {
      // Fading the last frame toward the background is what leaves each column its trail.
      ctx.globalAlpha = 1;
      ctx.fillStyle = "rgba(14, 12, 21, 0.12)";
      ctx.fillRect(0, 0, w, h);
      ctx.fillStyle = gradient;
      for (let i = 0; i < drops.length; i++) {
        if (Math.random() > speeds[i]) continue;
        const y = ++drops[i] * CELL;
        if (y > 0) {
          ctx.globalAlpha = 0.65 + Math.random() * 0.35;
          ctx.fillText(pick(), i * CELL + 2, y);
        }
        if (y > h && Math.random() > 0.96) drops[i] = -Math.floor(Math.random() * 24);
      }
    }

    let gradient: CanvasGradient;
    function resize() {
      const dpr = devicePixelRatio || 1;
      w = innerWidth;
      h = innerHeight;
      canvas.width = Math.round(w * dpr);
      canvas.height = Math.round(h * dpr);
      ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
      ctx.font = `${CELL - 3}px "Cascadia Mono", Consolas, ui-monospace, monospace`;
      ctx.textBaseline = "top";
      gradient = ctx.createLinearGradient(0, 0, 0, h);
      gradient.addColorStop(0, "#b9bbff");
      gradient.addColorStop(0.5, "#8f92ee");
      gradient.addColorStop(1, "#6f7fc4");
      const cols = Math.ceil(w / CELL);
      drops = Array.from({ length: cols }, () => -Math.floor(Math.random() * (h / CELL)));
      speeds = Array.from({ length: cols }, () => 0.25 + Math.random() * 0.6);
      ctx.fillStyle = BG;
      ctx.fillRect(0, 0, w, h);
      // Run ahead so the rain is already falling (and so reduced motion still gets a full picture).
      for (let i = 0; i < 90; i++) step();
    }

    function frame(t: number) {
      raf = requestAnimationFrame(frame);
      if (t - last < 1000 / FPS) return;
      last = t;
      step();
    }
    const play = () => {
      if (!raf && !still) raf = requestAnimationFrame(frame);
    };
    const pause = () => {
      cancelAnimationFrame(raf);
      raf = 0;
    };
    const sync = () => (document.hidden || !document.hasFocus() ? pause() : play());

    resize();
    sync();
    addEventListener("resize", resize);
    addEventListener("focus", sync);
    addEventListener("blur", sync);
    document.addEventListener("visibilitychange", sync);
    return () => {
      pause();
      removeEventListener("resize", resize);
      removeEventListener("focus", sync);
      removeEventListener("blur", sync);
      document.removeEventListener("visibilitychange", sync);
    };
  });
</script>

<canvas bind:this={canvas} aria-hidden="true"></canvas>

<style>
  canvas {
    position: fixed;
    inset: 0;
    width: 100%;
    height: 100%;
    z-index: 0;
    pointer-events: none;
    opacity: 0.75;
  }
</style>
