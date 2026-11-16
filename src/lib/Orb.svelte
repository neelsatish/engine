<script lang="ts" module>
  import type { OrbOptions } from "@yogesharc/thinking-orbs/vanilla";

  /** One orb per kind of work Engine does. Agents (phase 3) map their tool use onto these. */
  export const PROCESSES = {
    thinking: { state: "reasoning", label: "Thinking" },
    writing: { state: "using", variant: "type", label: "Writing" },
    viewing: { state: "using", variant: "scroll", label: "Viewing" },
    searching: { state: "searching", label: "Searching" },
    working: { state: "working", label: "Working" },
    waiting: { state: "waiting", label: "Waiting" },
    held: { state: "idle", variant: "blink", label: "Needs you" },
    updating: { state: "background", variant: "spiral", label: "Updating" },
    listening: { state: "using", variant: "record", label: "Listening" },
  } satisfies Record<string, OrbOptions>;

  export type Process = keyof typeof PROCESSES;
</script>

<script lang="ts">
  // Thinking Orbs by Yogesh, MIT: https://github.com/yogesharc/thinking-orbs
  import { mountOrb } from "@yogesharc/thinking-orbs/vanilla";

  let { process, size = 18 }: { process: Process; size?: number } = $props();
  let svg: SVGSVGElement;

  $effect(() => {
    const orb = mountOrb(svg, { ...PROCESSES[process], size });
    return () => orb.destroy();
  });
</script>

<svg bind:this={svg} class="orb"></svg>

<style>
  .orb {
    display: block;
    flex-shrink: 0;
  }
</style>
