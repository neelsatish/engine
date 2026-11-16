// Shared app state: the briefing, voice, settings and progress that the stage, chat, rail and views all read.
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { Block } from "$lib/time";
import { todayRange } from "$lib/time";

export type Item = {
  id: number;
  kind: "instant" | "answer" | "job" | "outward" | "task";
  title: string;
  detail: string;
  status: "queued" | "running" | "held" | "done" | "failed";
  process: string;
  result: string;
  created: string;
  verdict: "" | "checking" | "good" | "concern";
  verdict_reason: string;
  /// The dump it came from and what the person said there, so the chat can pair them.
  dump: number | null;
  said: string;
};

export type Target = { name: string; target: string };
export type Reply =
  | { kind: "opened"; name: string }
  | { kind: "choices"; items: Target[] }
  | { kind: "routed"; count: number }
  | { kind: "error"; message: string };

export type Prefs = {
  folder: string;
  recent: string[];
  models: { agent: string; answer: string };
  local_ai: boolean;
  voice: { live: boolean; auto_submit: boolean; speak_on_summon: boolean } & Record<string, unknown>;
} & Record<string, unknown>;

export type View = "home" | "briefing" | "schedule" | "settings";

export const engine = $state({
  view: "home" as View,
  items: [] as Item[],
  today: [] as Block[],
  prefs: null as Prefs | null,
  /// idle, listening or transcribing
  voice: "idle" as "idle" | "listening" | "transcribing",
  level: 0,
  /// dumps being sorted right now
  sorting: 0,
  /// typed messages sent but not answered yet, shown in the chat straight away
  pending: [] as string[],
  /// what was just heard, newest last (acting while talking)
  heard: [] as { text: string; at: number }[],
  choices: [] as Target[],
  note: "",
});

export async function loadItems() {
  try {
    engine.items = await invoke<Item[]>("briefing");
  } catch (e) {
    engine.note = `Couldn't load the briefing: ${e}`;
  }
}

export async function loadToday() {
  engine.today = await invoke<Block[]>("blocks", todayRange()).catch(() => []);
}

export async function loadPrefs() {
  engine.prefs = await invoke<Prefs>("get_settings").catch(() => null);
}

export async function savePrefs(patch: Partial<Prefs>) {
  if (!engine.prefs) return;
  try {
    engine.prefs = await invoke<Prefs>("save_settings", { settings: { ...engine.prefs, ...patch } });
  } catch (e) {
    engine.note = String(e);
  }
}

/// Sends a dump. Returns the reply so the composer can give text back on an error.
export async function submit(text: string): Promise<Reply> {
  engine.note = "";
  engine.choices = [];
  engine.sorting++;
  engine.pending = [...engine.pending, text];
  try {
    const reply = await invoke<Reply>("submit", { text });
    if (reply.kind === "choices") engine.choices = reply.items;
    if (reply.kind === "error") engine.note = reply.message;
    return reply;
  } catch (e) {
    engine.note = String(e);
    return { kind: "error", message: String(e) };
  } finally {
    engine.sorting--;
    engine.pending = engine.pending.filter((p, i) => i !== engine.pending.indexOf(text));
    await loadItems(); // the reply's items, even if the "briefing" event raced ahead
  }
}

/// Listens to the core once for the whole app.
export function startListeners(onHeard: (text: string, submitNow: boolean) => void): () => void {
  const offs = [
    listen("briefing", loadItems),
    listen("schedule", loadToday),
    listen<Prefs>("settings", (e) => (engine.prefs = e.payload)),
    listen<{ state: string; message: string }>("voice", (e) => {
      if (e.payload.state === "error") {
        engine.note = e.payload.message;
        engine.voice = "idle";
      } else {
        engine.voice = e.payload.state as typeof engine.voice;
        if (e.payload.state === "listening") engine.heard = [];
      }
      if (engine.voice !== "listening") engine.level = 0;
    }),
    listen<number>("voice-level", (e) => (engine.level = engine.level * 0.6 + e.payload * 0.4)),
    listen<string>("heard-part", (e) => {
      engine.heard = [...engine.heard, { text: e.payload, at: Date.now() }].slice(-4);
    }),
    listen<{ text: string; submit: boolean }>("heard", (e) => onHeard(e.payload.text, e.payload.submit)),
  ];
  loadItems();
  loadToday();
  loadPrefs();
  return () => offs.forEach((off) => off.then((f) => f()));
}

export const running = () => engine.items.filter((i) => i.kind !== "instant" && i.status === "running");
export const queued = () => engine.items.filter((i) => i.kind !== "instant" && i.status === "queued");
export const needsYou = () => engine.items.filter((i) => i.kind !== "instant" && (i.status === "held" || i.status === "failed"));
