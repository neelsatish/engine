<script lang="ts">
  // Settings as a compact two-pane menu: sections on the left, the chosen section on the right.
  import { getVersion } from "@tauri-apps/api/app";
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import { disable, enable, isEnabled } from "@tauri-apps/plugin-autostart";
  import { open } from "@tauri-apps/plugin-dialog";
  import { onMount } from "svelte";

  type Project = { name: string; folder: string; runner: string };
  type Voice = { model: string; auto_submit: boolean; speak_on_summon: boolean; engine: string; clip: string; hold_key: boolean; live: boolean; wake: boolean; narrate: boolean };
  type Settings = {
    projects: Project[];
    knowledge: string[];
    voice: Voice;
    models: { agent: string; answer: string };
    folder: string;
    recent: string[];
    local_ai: boolean;
    theme?: string;
  };
  type VoiceStatus = { python: boolean; whisper: boolean; chatterbox: boolean; kokoro: boolean; own_env: boolean };
  type Connection = { name: string; kind: string; status: string };
  type Section = "general" | "models" | "voice" | "projects" | "notes" | "connections";

  let { onEditAliases }: { onEditAliases: () => void } = $props();

  // Each icon is a path list for a 24-unit stroke icon.
  const SECTIONS: { id: Section; label: string; icon: string[] }[] = [
    { id: "general", label: "General", icon: ["M12 15a3 3 0 1 0 0-6 3 3 0 0 0 0 6z", "M19 12a7 7 0 0 0-.1-1.2l2-1.6-2-3.4-2.4 1a7 7 0 0 0-2-1.2L14 3h-4l-.5 2.6a7 7 0 0 0-2 1.2l-2.4-1-2 3.4 2 1.6a7 7 0 0 0 0 2.4l-2 1.6 2 3.4 2.4-1a7 7 0 0 0 2 1.2L10 21h4l.5-2.6a7 7 0 0 0 2-1.2l2.4 1 2-3.4-2-1.6c.1-.4.1-.8.1-1.2z"] },
    { id: "models", label: "Models", icon: ["M12 3l1.8 4.6L18 9l-4.2 1.4L12 15l-1.8-4.6L6 9l4.2-1.4z", "M19 15l.8 2 2 .8-2 .8-.8 2-.8-2-2-.8 2-.8z"] },
    { id: "voice", label: "Voice", icon: ["M9 3h6v11H9z", "M5 11a7 7 0 0 0 14 0", "M12 18v3"] },
    { id: "projects", label: "Projects", icon: ["M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z"] },
    { id: "notes", label: "Notes", icon: ["M6 3h9l4 4v14H6z", "M14 3v5h5", "M9 13h7", "M9 17h5"] },
    { id: "connections", label: "Connections", icon: ["M9 15l6-6", "M11 6l1-1a4 4 0 0 1 6 6l-1 1", "M13 18l-1 1a4 4 0 0 1-6-6l1-1"] },
  ];
  const MODELS = [
    ["haiku", "Haiku · fastest"],
    ["sonnet", "Sonnet · balanced"],
    ["opus", "Opus · most capable"],
    ["fable", "Fable · newest"],
  ];

  let section = $state<Section>("general");
  type LocalStatus = { installed: boolean; running: boolean; model: boolean };
  let local = $state<LocalStatus | null>(null);
  let powering = $state(false);
  const CHATGPT = [
    ["codex:gpt-6.1-sol", "GPT-6.1 Sol"],
    ["codex:gpt-6-astra", "GPT-6 Astra"],
    ["codex:gpt-6.1-terra", "GPT-6.1 Terra"],
    ["codex:gpt-6-luna", "GPT-6 Luna"],
  ];

  async function setLocal(on: boolean) {
    powering = true;
    try {
      await invoke("set_local_ai", { on });
      note = "";
    } catch (e) {
      note = `Local AI: ${e}`;
    } finally {
      powering = false;
      local = await invoke<LocalStatus>("local_status");
    }
  }
  let settings = $state<Settings>({
    projects: [],
    knowledge: [],
    voice: { model: "small.en", auto_submit: false, speak_on_summon: false, engine: "kokoro", clip: "", hold_key: true, live: true, wake: true, narrate: true },
    models: { agent: "haiku", answer: "sonnet" },
    folder: "",
    recent: [],
    local_ai: false,
  });
  let status = $state<VoiceStatus | null>(null);
  const packReady = $derived(!!status?.own_env && !!status?.kokoro);
  let conns = $state<Connection[] | null>(null);
  let connError = $state("");
  let installing = $state(false);
  let installLine = $state("");
  let newName = $state("");
  let newRunner = $state("claude");
  let autostart = $state(false);
  let version = $state("");
  let note = $state("");

  async function save(next: Settings) {
    try {
      settings = await invoke<Settings>("save_settings", { settings: next });
      note = "";
    } catch (e) {
      note = String(e);
    }
  }

  const setVoice = (patch: Partial<Voice>) => save({ ...settings, voice: { ...settings.voice, ...patch } });
  const setModels = (patch: Partial<Settings["models"]>) => save({ ...settings, models: { ...settings.models, ...patch } });

  async function chooseFolder(title: string): Promise<string | null> {
    const picked = await open({ directory: true, title });
    return typeof picked === "string" ? picked : null;
  }

  async function addProject() {
    const name = newName.trim();
    if (!name) return;
    const folder = await chooseFolder(`Choose the folder for “${name}”`);
    if (!folder) return;
    await save({ ...settings, projects: [...settings.projects, { name, folder, runner: newRunner }] });
    newName = "";
  }

  async function addNotes() {
    const folder = await chooseFolder("Choose a notes folder");
    if (folder) await save({ ...settings, knowledge: [...settings.knowledge, folder] });
  }

  async function chooseClip() {
    const picked = await open({
      title: "Choose a voice clip (about 10 seconds)",
      filters: [{ name: "Audio", extensions: ["wav", "mp3", "flac"] }],
    });
    if (typeof picked === "string") await setVoice({ clip: picked });
  }

  async function installVoice() {
    installing = true;
    installLine = "Starting…";
    try {
      await invoke("install_voice");
      installLine = "Installed.";
    } catch (e) {
      installLine = String(e);
    } finally {
      installing = false;
      status = await invoke<VoiceStatus>("voice_status");
    }
  }

  async function setAutostart(on: boolean) {
    try {
      await (on ? enable() : disable());
      autostart = await isEnabled();
    } catch (e) {
      note = `Couldn't change start with Windows: ${e}`;
    }
  }

  async function loadConnections() {
    conns = null;
    connError = "";
    try {
      conns = await invoke<Connection[]>("connections");
    } catch (e) {
      connError = String(e);
      conns = [];
    }
  }

  function show(id: Section) {
    section = id;
    // Checking connections runs `claude mcp list`, so only do it when that section is opened.
    if (id === "connections" && conns === null && !connError) loadConnections();
    if (id === "models" && local === null) invoke<LocalStatus>("local_status").then((s) => (local = s));
    if (id === "voice" && !status) invoke<VoiceStatus>("voice_status").then((s) => (status = s)).catch(() => {});
  }

  const base = (p: string) => p.split(/[\\/]/).filter(Boolean).pop() ?? p;

  onMount(() => {
    invoke<Settings>("get_settings").then((s) => (settings = s), (e) => (note = String(e)));
    isEnabled().then((on) => (autostart = on), () => {});
    getVersion().then((v) => (version = v));
    const offs = [
      listen<string>("voice-install", (e) => (installLine = e.payload)),
      listen<Settings>("settings", (e) => (settings = e.payload)),
    ];
    return () => offs.forEach((off) => off.then((f) => f()));
  });
</script>

{#snippet toggle(on: boolean, label: string, flip: () => void)}
  <button class="switch" role="switch" aria-checked={on} aria-label={label} onclick={flip}><span></span></button>
{/snippet}

<div class="settings">
  <nav class="side" aria-label="Settings sections">
    {#each SECTIONS as s (s.id)}
      <button class="side-item" aria-current={section === s.id ? "page" : undefined} onclick={() => show(s.id)}>
        <svg class="ico" viewBox="0 0 24 24" aria-hidden="true">{#each s.icon as d (d)}<path {d} />{/each}</svg>
        {s.label}
      </button>
    {/each}
    <span class="version">Engine {version}</span>
  </nav>

  <section class="pane glass" aria-label={SECTIONS.find((s) => s.id === section)?.label}>
    {#if note}<p class="error" role="alert">{note}</p>{/if}

    {#if section === "general"}
      <div class="row">
        <div class="label-col"><span class="name">Start with Windows</span><span class="desc">Hidden in the tray at sign-in</span></div>
        {@render toggle(autostart, "Start with Windows", () => setAutostart(!autostart))}
      </div>
      <div class="row">
        <div class="label-col"><span class="name">Theme</span><span class="desc">Also on the top bar</span></div>
        <select class="input" aria-label="Theme" value={settings.theme === "mono" ? "mono" : "velvet"} onchange={(e) => save({ ...settings, theme: e.currentTarget.value })}>
          <option value="velvet">Velvet · purple</option>
          <option value="mono">Black and white</option>
        </select>
      </div>
      <div class="row">
        <div class="label-col"><span class="name">Shortcuts</span><span class="desc">Names for sites and folders</span></div>
        <button class="button" onclick={onEditAliases}>Edit shortcuts</button>
      </div>
      <div class="row">
        <div class="label-col"><span class="name">Hotkeys</span><span class="desc">Show or hide · hold to talk</span></div>
        <span class="keys"><kbd>Ctrl</kbd><kbd>Shift</kbd><kbd>Space</kbd> · hold <kbd>`</kbd></span>
      </div>
    {:else if section === "models"}
      <div class="row">
        <div class="label-col"><span class="name">Agents</span><span class="desc">Background jobs</span></div>
        <select class="input" aria-label="Agent model" value={settings.models.agent} onchange={(e) => setModels({ agent: e.currentTarget.value })}>
          <optgroup label="Claude">{#each MODELS as [id, name] (id)}<option value={id}>{name}</option>{/each}</optgroup>
          <optgroup label="ChatGPT (Codex)">{#each CHATGPT as [id, name] (id)}<option value={id}>{name}</option>{/each}</optgroup>
        </select>
      </div>
      <div class="row">
        <div class="label-col"><span class="name">Answers</span><span class="desc">Quick answers, plans, reviews</span></div>
        <select class="input" aria-label="Answer model" value={settings.models.answer} onchange={(e) => setModels({ answer: e.currentTarget.value })}>
          {#each MODELS as [id, name] (id)}<option value={id}>{name}</option>{/each}
        </select>
      </div>
      <div class="row">
        <div class="label-col">
          <span class="name">Local AI</span>
          <span class="desc">
            {local === null
              ? "Checking…"
              : !local.installed
                ? "Ollama isn't installed yet"
                : settings.local_ai
                  ? "On · sorting, answers and checks run on Qwen3 8B, on your PC"
                  : "Off · Engine uses Claude Haiku"}
          </span>
        </div>
        {@render toggle(settings.local_ai, "Local AI", () => !powering && setLocal(!settings.local_ai))}
      </div>
    {:else if section === "voice"}
      <div class="row">
        <div class="label-col">
          <span class="name">Speech recognition</span>
          <span class="desc">{!status ? "Checking…" : status.whisper ? "Ready, on this PC" : "Needs the voice pack below"}</span>
        </div>
        <select class="input" aria-label="Whisper model" value={settings.voice.model} onchange={(e) => setVoice({ model: e.currentTarget.value })}>
          <option value="base.en">Base · English, fastest</option>
          <option value="small.en">Small · English, fast</option>
          <option value="small">Small · any language</option>
          <option value="large-v3-turbo">Large turbo · most accurate</option>
        </select>
      </div>
      <div class="row">
        <div class="label-col"><span class="name">Hold <kbd>`</kbd> to talk</span><span class="desc">A quick tap still types it</span></div>
        {@render toggle(settings.voice.hold_key, "Hold the backtick key to talk", () => setVoice({ hold_key: !settings.voice.hold_key }))}
      </div>
      <div class="row">
        <div class="label-col"><span class="name">Wake word</span><span class="desc">Say “Engine” and it answers. The mic stays open; audio never leaves this PC</span></div>
        {@render toggle(settings.voice.wake, "Wake word", () => setVoice({ wake: !settings.voice.wake }))}
      </div>
      <div class="row">
        <div class="label-col"><span class="name">Act while I talk</span><span class="desc">Each phrase runs as soon as you pause</span></div>
        {@render toggle(settings.voice.live, "Act while I talk", () => setVoice({ live: !settings.voice.live }))}
      </div>
      <div class="row">
        <div class="label-col"><span class="name">Send what I say</span><span class="desc">Skip checking it first</span></div>
        {@render toggle(settings.voice.auto_submit, "Send what I say", () => setVoice({ auto_submit: !settings.voice.auto_submit }))}
      </div>
      <div class="row">
        <div class="label-col"><span class="name">Talk me through tasks</span><span class="desc">“Writing”, “Sending”, “Almost done”, then an overview</span></div>
        {@render toggle(settings.voice.narrate, "Talk me through tasks", () => setVoice({ narrate: !settings.voice.narrate }))}
      </div>
      <div class="row">
        <div class="label-col"><span class="name">Read briefing on open</span><span class="desc">When you summon Engine</span></div>
        {@render toggle(settings.voice.speak_on_summon, "Read briefing on open", () => setVoice({ speak_on_summon: !settings.voice.speak_on_summon }))}
      </div>
      <div class="row">
        <div class="label-col">
          <span class="name">Engine's voice</span>
          <span class="desc">
            {settings.voice.engine === "chatterbox"
              ? "Only clone a voice you may use"
              : "British, made on this PC"}
          </span>
        </div>
        <select class="input" aria-label="Engine's voice" value={settings.voice.engine} onchange={(e) => setVoice({ engine: e.currentTarget.value })}>
          <option value="kokoro" disabled={!status?.kokoro}>British · Kokoro</option>
          <option value="chatterbox" disabled={!status?.chatterbox}>Chatterbox · cloned</option>
        </select>
      </div>
      {#if settings.voice.engine === "chatterbox"}
        <div class="row">
          <div class="label-col"><span class="name">Voice clip</span><span class="desc" title={settings.voice.clip}>{settings.voice.clip ? base(settings.voice.clip) : "Default voice"}</span></div>
          <button class="button" onclick={chooseClip}>Choose clip</button>
        </div>
      {/if}
      <div class="row">
        <div class="label-col">
          <span class="name">Voice pack</span>
          <span class="desc" title={installLine}>
            {installing || installLine ? installLine : packReady ? "Installed" : "Whisper, Chatterbox and the British voice · about 3 GB"}
          </span>
        </div>
        <button class="button" onclick={installVoice} disabled={installing || packReady}>
          {installing ? "Installing…" : packReady ? "Installed" : status?.own_env ? "Update voice pack" : "Install voice pack"}
        </button>
      </div>
    {:else if section === "projects"}
      {#each settings.projects as p, i (p.name)}
        <div class="row">
          <div class="label-col"><span class="name">{p.name}</span><span class="desc" title={p.folder}>{p.folder}</span></div>
          <select
            class="input small"
            aria-label="Who works on {p.name}"
            value={p.runner}
            onchange={(e) => save({ ...settings, projects: settings.projects.map((q, j) => (j === i ? { ...q, runner: e.currentTarget.value } : q)) })}
          >
            <option value="claude">Claude</option>
            <option value="codex">Codex</option>
          </select>
          <button class="icon-btn" aria-label="Remove {p.name}" title="Remove" onclick={() => save({ ...settings, projects: settings.projects.filter((_, j) => j !== i) })}>
            <svg class="ico" viewBox="0 0 24 24" aria-hidden="true"><path d="M6 6l12 12M18 6 6 18" /></svg>
          </button>
        </div>
      {:else}
        <p class="empty">Name a folder, and dumps that mention it run there.</p>
      {/each}
      <form class="add" onsubmit={(e) => { e.preventDefault(); addProject(); }}>
        <input class="input grow" bind:value={newName} placeholder="New project, e.g. my app" aria-label="Project name" />
        <select class="input small" bind:value={newRunner} aria-label="Who works on it">
          <option value="claude">Claude</option>
          <option value="codex">Codex</option>
        </select>
        <button class="button primary" type="submit" disabled={!newName.trim()}>Add project</button>
      </form>
    {:else if section === "notes"}
      {#each settings.knowledge as k, i (k)}
        <div class="row">
          <div class="label-col"><span class="name">{i === 0 ? "Obsidian vault" : base(k)}</span><span class="desc" title={k}>{k}</span></div>
          <button class="icon-btn" aria-label="Remove {k}" title="Remove" onclick={() => save({ ...settings, knowledge: settings.knowledge.filter((_, j) => j !== i) })}>
            <svg class="ico" viewBox="0 0 24 24" aria-hidden="true"><path d="M6 6l12 12M18 6 6 18" /></svg>
          </button>
        </div>
      {:else}
        <p class="empty">Folders agents may read. Add your Obsidian vault first.</p>
      {/each}
      <div class="add"><button class="button primary" onclick={addNotes}>Add folder</button></div>
    {:else}
      {#if conns === null}
        <p class="empty">Checking…</p>
      {:else}
        {#each conns as c (c.kind + c.name)}
          <div class="row">
            <span class="conn-dot {c.status.replace(' ', '-')}" aria-hidden="true"></span>
            <div class="label-col">
              <span class="name">{c.name}</span>
              <span class="desc">{c.kind === "connector" ? "Connector" : c.kind === "plugin" ? "Plugin" : "Local"} · {c.status}</span>
            </div>
          </div>
        {:else}
          <p class="empty">{connError || "No connections found."}</p>
        {/each}
      {/if}
      <div class="add">
        <button class="button primary" onclick={() => invoke("manage_connectors")}>Add connectors</button>
        <button class="button" onclick={loadConnections} disabled={conns === null}>Check again</button>
      </div>
    {/if}
  </section>
</div>

<style>
  .settings {
    display: grid;
    flex: 1;
    grid-template-columns: 150px 1fr;
    gap: var(--space-md);
    min-height: 0;
    padding: var(--space-xs) 0 var(--space-md);
  }

  .side {
    display: flex;
    flex-direction: column;
    gap: 2px;
    padding: 6px;
    border: 1px solid var(--border);
    border-radius: var(--radius);
    background: var(--glass);
    backdrop-filter: blur(14px);
  }

  .side-item {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 8px 10px;
    border: 0;
    border-radius: var(--r-sm);
    background: transparent;
    color: var(--text-muted);
    font-size: 13px;
    text-align: left;
    transition: background var(--ease), color var(--ease);
  }

  .side-item svg {
    width: 16px;
    height: 16px;
  }

  .side-item:hover {
    color: var(--text);
  }

  .side-item[aria-current="page"] {
    background: var(--surface-2);
    color: var(--text);
  }

  .side-item[aria-current="page"] svg {
    color: var(--accent);
  }

  .version {
    margin-top: auto;
    padding: 0 10px;
    color: var(--text-muted);
    font-size: 12px;
  }

  .pane {
    min-height: 0;
    overflow-y: auto;
    padding: 6px var(--space-md);
    scrollbar-width: thin;
    scrollbar-color: var(--border) transparent;
  }

  .row {
    display: flex;
    align-items: center;
    gap: var(--space-sm);
    min-height: 52px;
    padding: 8px 0;
    border-bottom: 1px solid rgb(42 37 64 / 0.6);
  }

  .row:last-of-type {
    border-bottom: 0;
  }

  .label-col {
    display: flex;
    flex: 1;
    flex-direction: column;
    min-width: 0;
  }

  .name {
    color: var(--text);
    font-size: 14px;
  }

  .desc {
    overflow: hidden;
    color: var(--text-muted);
    font-size: 13px;
    text-overflow: ellipsis;
    white-space: nowrap;
    user-select: text;
  }

  select.input {
    flex-shrink: 0;
    width: 190px;
    font-size: 13px;
  }

  select.input.small {
    width: 96px;
  }

  .keys {
    color: var(--text-muted);
    font-size: 12px;
  }

  .keys kbd {
    margin: 0 1px;
  }

  .add {
    display: flex;
    gap: var(--space-sm);
    padding: 12px 0 8px;
  }

  .grow {
    flex: 1;
    min-width: 0;
  }

  .empty {
    margin: 0;
    padding: 14px 0 4px;
    color: var(--text-muted);
    font-size: 13px;
  }

  .icon-btn:hover {
    color: var(--danger);
  }

  .conn-dot {
    width: 8px;
    height: 8px;
    flex-shrink: 0;
    border-radius: 50%;
    background: var(--danger);
  }

  .conn-dot.connected {
    background: var(--success);
  }

  .conn-dot.needs-auth {
    background: #f2b85c;
  }

  .error {
    margin: 8px 0;
    padding: var(--space-sm) var(--space-md);
    border: 1px solid rgb(240 113 138 / 0.5);
    border-radius: var(--r-sm);
    background: rgb(240 113 138 / 0.1);
    color: var(--text);
    font-size: 13px;
    user-select: text;
  }
</style>
