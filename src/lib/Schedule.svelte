<script lang="ts">
  // The schedule: a week grid you can drag blocks around in, and an agenda list. Plans from agents land here.
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import { onMount } from "svelte";
  import { KINDS, type Block, dayKey, fmt, mondayOf, pad } from "$lib/time";

  const START_HOUR = 7;
  const END_HOUR = 24;
  const HOUR_PX = 44;
  const SNAP_MIN = 15;

  let mode = $state<"week" | "agenda">("week");
  let weekStart = $state(mondayOf(new Date()));
  let blocks = $state<Block[]>([]);
  let editing = $state<Block | null>(null);
  let note = $state("");
  let gridWidth = $state(0);
  let drag = $state<{ id: number; x0: number; y0: number; dx: number; dy: number; moved: boolean } | null>(null);
  let scroller = $state<HTMLDivElement>();

  const days = $derived(
    Array.from({ length: 7 }, (_, i) => {
      const d = new Date(weekStart);
      d.setDate(d.getDate() + i);
      return d;
    }),
  );
  const colWidth = $derived(gridWidth / 7);
  const todayKey = dayKey(new Date());
  const range = $derived.by(() => {
    if (mode === "week") {
      const end = new Date(weekStart);
      end.setDate(end.getDate() + 7);
      return { from: fmt(weekStart), to: fmt(end) };
    }
    const from = new Date();
    from.setHours(0, 0, 0, 0);
    const to = new Date(from);
    to.setDate(to.getDate() + 30);
    return { from: fmt(from), to: fmt(to) };
  });

  async function load() {
    try {
      blocks = await invoke<Block[]>("blocks", range);
    } catch (e) {
      note = String(e);
    }
  }

  $effect(() => {
    range; // reload whenever the visible range changes
    load();
  });

  // Open the week grid scrolled to an hour before now, once its scroller exists.
  $effect(() => {
    if (mode !== "week" || !scroller) return;
    const hour = Math.max(new Date().getHours() - 1, START_HOUR);
    const el = scroller;
    setTimeout(() => (el.scrollTop = (hour - START_HOUR) * HOUR_PX));
  });

  // Each day's blocks with their position, side by side when they overlap.
  const laid = $derived(
    days.map((day) => {
      const key = dayKey(day);
      const mine = blocks.filter((b) => b.start.slice(0, 10) === key).sort((a, b) => a.start.localeCompare(b.start));
      const laneEnds: string[] = [];
      const placed = mine.map((b) => {
        let lane = laneEnds.findIndex((end) => end <= b.start);
        if (lane === -1) lane = laneEnds.push(b.end) - 1;
        else laneEnds[lane] = b.end;
        const s = new Date(b.start);
        const e = new Date(b.end);
        const startMin = Math.max(s.getHours() * 60 + s.getMinutes(), START_HOUR * 60);
        const endMin = e.toDateString() === s.toDateString() ? e.getHours() * 60 + e.getMinutes() : END_HOUR * 60;
        const top = ((startMin - START_HOUR * 60) / 60) * HOUR_PX;
        const height = Math.max(((endMin - startMin) / 60) * HOUR_PX, 20);
        return { b, lane, top, height };
      });
      return { day, key, placed, lanes: Math.max(laneEnds.length, 1) };
    }),
  );

  const agenda = $derived.by(() => {
    const groups = new Map<string, Block[]>();
    for (const b of blocks) groups.set(b.start.slice(0, 10), [...(groups.get(b.start.slice(0, 10)) ?? []), b]);
    return [...groups.entries()];
  });

  function shiftWeek(weeks: number) {
    const d = new Date(weekStart);
    d.setDate(d.getDate() + weeks * 7);
    weekStart = d;
  }

  function newBlock() {
    const s = new Date();
    s.setMinutes(0, 0, 0);
    s.setHours(s.getHours() + 1);
    const e = new Date(s);
    e.setHours(e.getHours() + 1);
    editing = { id: 0, title: "", start: fmt(s), end: fmt(e), kind: "study", note: "" };
  }

  async function save(b: Block) {
    try {
      await invoke("save_block", { block: b });
      editing = null;
      note = "";
    } catch (e) {
      note = String(e);
    }
  }

  async function remove(b: Block) {
    try {
      await invoke("delete_block", { id: b.id });
      editing = null;
    } catch (e) {
      note = String(e);
    }
  }

  function down(e: PointerEvent, b: Block) {
    try {
      (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
    } catch {
      // no live pointer (synthetic events): dragging still works while the pointer stays on the block
    }
    drag = { id: b.id, x0: e.clientX, y0: e.clientY, dx: 0, dy: 0, moved: false };
  }

  function move(e: PointerEvent) {
    if (!drag) return;
    drag.dx = e.clientX - drag.x0;
    drag.dy = e.clientY - drag.y0;
    if (Math.abs(drag.dx) + Math.abs(drag.dy) > 4) drag.moved = true;
  }

  const snapped = (d: { dx: number; dy: number }) => ({
    days: colWidth ? Math.round(d.dx / colWidth) : 0,
    minutes: Math.round(((d.dy / HOUR_PX) * 60) / SNAP_MIN) * SNAP_MIN,
  });

  async function up(b: Block) {
    const d = drag;
    drag = null;
    if (!d) return;
    if (!d.moved) {
      editing = { ...b };
      return;
    }
    const { days: dd, minutes } = snapped(d);
    if (!dd && !minutes) return;
    const shift = (s: string) => {
      const t = new Date(s);
      t.setDate(t.getDate() + dd);
      t.setMinutes(t.getMinutes() + minutes);
      return fmt(t);
    };
    await save({ ...b, start: shift(b.start), end: shift(b.end) });
  }

  const time = (s: string) => s.slice(11, 16);
  const dayLabel = (d: Date) => d.toLocaleDateString(undefined, { weekday: "short", day: "numeric" });
  const longDay = (key: string) =>
    new Date(`${key}T00:00`).toLocaleDateString(undefined, { weekday: "long", day: "numeric", month: "long" });
  const weekLabel = $derived(
    `${days[0].toLocaleDateString(undefined, { day: "numeric", month: "short" })} – ${days[6].toLocaleDateString(undefined, { day: "numeric", month: "short" })}`,
  );

  // Editor fields: the block's date and times split for the date and time inputs.
  const edit = {
    get date() {
      return editing?.start.slice(0, 10) ?? "";
    },
    set date(v: string) {
      if (!editing || !v) return;
      editing.start = `${v}T${editing.start.slice(11)}`;
      editing.end = `${v}T${editing.end.slice(11)}`;
    },
  };

  onMount(() => {
    const off = listen("schedule", load);
    return () => off.then((f) => f());
  });
</script>

<div class="bar">
  <div class="seg" role="tablist" aria-label="Schedule view">
    <button role="tab" aria-selected={mode === "week"} onclick={() => (mode = "week")}>Week</button>
    <button role="tab" aria-selected={mode === "agenda"} onclick={() => (mode = "agenda")}>Agenda</button>
  </div>
  {#if mode === "week"}
    <div class="nav">
      <button class="icon-btn" aria-label="Previous week" onclick={() => shiftWeek(-1)}>
        <svg class="ico" viewBox="0 0 24 24" aria-hidden="true"><path d="m15 18-6-6 6-6" /></svg>
      </button>
      <button class="link" onclick={() => (weekStart = mondayOf(new Date()))}>{weekLabel}</button>
      <button class="icon-btn" aria-label="Next week" onclick={() => shiftWeek(1)}>
        <svg class="ico" viewBox="0 0 24 24" aria-hidden="true"><path d="m9 18 6-6-6-6" /></svg>
      </button>
    </div>
  {/if}
  <button class="button primary" onclick={newBlock}>Add block</button>
</div>

{#if note}<p class="error" role="alert">{note}</p>{/if}

{#if mode === "week"}
  <section class="glass week">
    <div class="heads">
      <span></span>
      {#each days as d (d.getTime())}
        <span class="head" class:today={dayKey(d) === todayKey}>{dayLabel(d)}</span>
      {/each}
    </div>
    <div class="scroll grid-scroll" bind:this={scroller}>
      <div class="grid" style="height: {(END_HOUR - START_HOUR) * HOUR_PX}px">
        <div class="hours">
          {#each Array.from({ length: END_HOUR - START_HOUR }, (_, i) => START_HOUR + i) as h (h)}
            <span style="top: {(h - START_HOUR) * HOUR_PX}px">{pad(h)}:00</span>
          {/each}
        </div>
        <div class="days" bind:clientWidth={gridWidth}>
          {#each laid as col (col.key)}
            <div class="day" class:today={col.key === todayKey}>
              {#each col.placed as p (p.b.id)}
                {@const active = drag?.id === p.b.id && drag.moved}
                {@const s = active && drag ? snapped(drag) : { days: 0, minutes: 0 }}
                <button
                  class="block {p.b.kind}"
                  class:dragging={active}
                  style="top: {p.top}px; height: {p.height}px; left: calc({(p.lane / col.lanes) * 100}% + 2px);
                    width: calc({100 / col.lanes}% - 4px);
                    transform: translate({s.days * colWidth}px, {(s.minutes / 60) * HOUR_PX}px)"
                  title="{p.b.title}, {time(p.b.start)}–{time(p.b.end)}"
                  onpointerdown={(e) => down(e, p.b)}
                  onpointermove={move}
                  onpointerup={() => up(p.b)}
                  onkeydown={(e) => e.key === "Enter" && (editing = { ...p.b })}
                >
                  <span class="b-title">{p.b.title}</span>
                  <span class="b-time">{time(p.b.start)}–{time(p.b.end)}</span>
                </button>
              {/each}
            </div>
          {/each}
        </div>
      </div>
    </div>
  </section>
{:else}
  <div class="scroll">
    {#each agenda as [key, items] (key)}
      <section class="card glass">
        <h2>{longDay(key)} <span class="count">{items.length}</span></h2>
        <ul class="rows">
          {#each items as b (b.id)}
            <li>
              <span class="indicator"><span class="kind-dot {b.kind}" aria-hidden="true"></span></span>
              <button class="agenda-item" onclick={() => (editing = { ...b })}>
                <span class="text">{b.title}</span>
                <span class="sub">{time(b.start)}–{time(b.end)} · {b.kind}</span>
              </button>
            </li>
          {/each}
        </ul>
      </section>
    {:else}
      <section class="card glass empty">
        <p>Nothing scheduled in the next 30 days. Ask for a plan ("make me a 5-day study plan for my test") and its
          sessions land here.</p>
      </section>
    {/each}
  </div>
{/if}

{#if editing}
  <form
    class="editor glass"
    onsubmit={(e) => {
      e.preventDefault();
      if (editing) save(editing);
    }}
  >
    <h2 class="label">{editing.id ? "Edit block" : "New block"}</h2>
    <input class="input title" bind:value={editing.title} placeholder="What is it?" aria-label="Title" />
    <div class="fields">
      <input class="input" type="date" bind:value={edit.date} aria-label="Date" />
      <input
        class="input"
        type="time"
        value={editing.start.slice(11)}
        oninput={(e) => editing && (editing.start = `${editing.start.slice(0, 10)}T${e.currentTarget.value}`)}
        aria-label="Starts"
      />
      <input
        class="input"
        type="time"
        value={editing.end.slice(11)}
        oninput={(e) => editing && (editing.end = `${editing.end.slice(0, 10)}T${e.currentTarget.value}`)}
        aria-label="Ends"
      />
      <select class="input" bind:value={editing.kind} aria-label="Kind">
        {#each KINDS as k (k)}<option value={k}>{k}</option>{/each}
      </select>
    </div>
    {#if editing.note}<p class="hint">Plan note: {editing.note}</p>{/if}
    <div class="actions">
      <button class="button primary" type="submit" disabled={!editing.title.trim()}>Save</button>
      {#if editing.id}
        <button class="button" type="button" onclick={() => editing && remove(editing)}>Delete</button>
      {/if}
      <button class="button" type="button" onclick={() => (editing = null)}>Cancel</button>
    </div>
  </form>
{/if}

<style>
  .bar {
    display: flex;
    align-items: center;
    gap: var(--space-sm);
    padding: var(--space-xs) 0 var(--space-sm);
  }

  .bar .button {
    margin-left: auto;
  }

  .seg {
    display: flex;
    gap: 2px;
    padding: 3px;
    border: 1px solid var(--border);
    border-radius: 10px;
    background: var(--glass);
  }

  .seg button {
    padding: 3px 12px;
    border: 0;
    border-radius: 7px;
    background: transparent;
    color: var(--text-muted);
    font-size: 13px;
  }

  .seg button[aria-selected="true"] {
    background: var(--surface-2);
    color: var(--text);
  }

  .nav {
    display: flex;
    align-items: center;
    gap: 2px;
  }

  .icon-btn {
    display: grid;
    place-items: center;
    width: 28px;
    height: 28px;
    border: 0;
    border-radius: 7px;
    background: transparent;
    color: var(--text-muted);
  }

  .icon-btn:hover {
    background: var(--surface-2);
    color: var(--text);
  }

  .week {
    display: flex;
    flex: 1;
    flex-direction: column;
    min-height: 0;
    margin-bottom: var(--space-sm);
    overflow: hidden;
  }

  .heads {
    display: grid;
    grid-template-columns: 44px repeat(7, 1fr);
    border-bottom: 1px solid var(--border);
  }

  .head {
    padding: 8px 4px;
    color: var(--text-muted);
    font-size: 12px;
    text-align: center;
  }

  .head.today {
    color: var(--accent);
    font-weight: 650;
  }

  .grid {
    position: relative;
    display: grid;
    grid-template-columns: 44px 1fr;
  }

  .hours {
    position: relative;
  }

  .hours span {
    position: absolute;
    right: 6px;
    color: var(--text-muted);
    font-size: 10px;
    transform: translateY(-50%);
    font-variant-numeric: tabular-nums;
  }

  .hours span:first-child {
    transform: none;
  }

  .days {
    display: grid;
    grid-template-columns: repeat(7, 1fr);
    background-image: repeating-linear-gradient(
      to bottom,
      rgb(42 37 64 / 0.55) 0 1px,
      transparent 1px 44px
    );
  }

  .day {
    position: relative;
    border-left: 1px solid rgb(42 37 64 / 0.55);
  }

  .day.today {
    background: rgb(142 144 245 / 0.05);
  }

  .block {
    position: absolute;
    display: flex;
    flex-direction: column;
    overflow: hidden;
    padding: 3px 6px;
    border: 1px solid transparent;
    border-left-width: 3px;
    border-radius: 6px;
    background: rgb(142 144 245 / 0.16);
    color: var(--text);
    font-size: 11px;
    line-height: 1.25;
    text-align: left;
    cursor: grab;
    touch-action: none;
    transition: box-shadow var(--ease);
  }

  .block:hover {
    box-shadow: 0 0 0 1px var(--accent);
  }

  .block.dragging {
    z-index: 2;
    cursor: grabbing;
    box-shadow: 0 6px 20px rgb(0 0 0 / 0.4);
    transition: none;
  }

  .b-title {
    overflow: hidden;
    font-weight: 600;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .b-time {
    color: var(--text-secondary);
  }

  .block.study { border-left-color: #8e90f5; background: rgb(142 144 245 / 0.16); }
  .block.test { border-left-color: #f0718a; background: rgb(240 113 138 / 0.16); }
  .block.deadline { border-left-color: #f2b85c; background: rgb(242 184 92 / 0.16); }
  .block.recording { border-left-color: #5fd4a0; background: rgb(95 212 160 / 0.14); }
  .block.other { border-left-color: #9f9ab8; background: rgb(159 154 184 / 0.14); }
  .block.reminder { border-left-color: #c9a8ff; background: rgb(201 168 255 / 0.16); }

  .kind-dot {
    width: 8px;
    height: 8px;
    border-radius: 2px;
    background: #9f9ab8;
  }

  .kind-dot.study { background: #8e90f5; }
  .kind-dot.test { background: #f0718a; }
  .kind-dot.deadline { background: #f2b85c; }
  .kind-dot.recording { background: #5fd4a0; }
  .kind-dot.reminder { background: #c9a8ff; }

  .agenda-item {
    display: flex;
    flex: 1;
    flex-direction: column;
    padding: 0;
    border: 0;
    background: transparent;
    text-align: left;
  }

  .agenda-item:hover .text {
    color: var(--accent);
  }

  .sub {
    color: var(--text-muted);
    font-size: 13px;
  }

  .empty p {
    margin: var(--space-sm) 0 var(--space-md);
    color: var(--text-secondary);
  }

  .editor {
    position: absolute;
    right: var(--space-lg);
    bottom: 64px;
    left: var(--space-lg);
    z-index: 5;
    padding: var(--space-md);
    background: rgb(19 16 28 / 0.95);
    box-shadow: 0 16px 48px rgb(0 0 0 / 0.55);
  }

  .editor .title {
    width: 100%;
    margin-bottom: var(--space-sm);
  }

  .fields {
    display: grid;
    grid-template-columns: 1.3fr 1fr 1fr 1fr;
    gap: var(--space-sm);
  }

  .editor .hint {
    margin: var(--space-sm) 0 0;
    overflow-wrap: anywhere;
  }

  .actions {
    display: flex;
    gap: var(--space-sm);
    margin-top: var(--space-md);
  }

  .error {
    margin: 0 0 var(--space-sm);
    color: var(--danger);
    font-size: 13px;
  }
</style>
