<script lang="ts">
  // Settings: projects (named folders agents work in), notes folders agents can read, and starting with Windows.
  import { getVersion } from "@tauri-apps/api/app";
  import { invoke } from "@tauri-apps/api/core";
  import { disable, enable, isEnabled } from "@tauri-apps/plugin-autostart";
  import { open } from "@tauri-apps/plugin-dialog";
  import { listen } from "@tauri-apps/api/event";
  import { onMount } from "svelte";

  type Project = { name: string; folder: string; runner: string };
  type Voice = { model: string; auto_submit: boolean; speak_on_summon: boolean; engine: string; clip: string };
  type Settings = { projects: Project[]; knowledge: string[]; voice: Voice };
  type VoiceStatus = { python: boolean; whisper: boolean; chatterbox: boolean; own_env: boolean };

  let { onEditAliases }: { onEditAliases: () => void } = $props();

  let settings = $state<Settings>({
    projects: [],
    knowledge: [],
    voice: { model: "small", auto_submit: false, speak_on_summon: false, engine: "windows", clip: "" },
  });
  let status = $state<VoiceStatus | null>(null);
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

  const setVoice = (patch: Partial<Voice>) => save({ ...settings, voice: { ...settings.voice, ...patch } });

  async function chooseClip() {
    const picked = await open({ title: "Choose a voice clip (about 10 seconds)", filters: [{ name: "Audio", extensions: ["wav", "mp3", "flac"] }] });
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

  onMount(async () => {
    try {
      settings = await invoke<Settings>("get_settings");
    } catch (e) {
      note = String(e);
    }
    autostart = await isEnabled().catch(() => false);
    version = await getVersion();
    status = await invoke<VoiceStatus>("voice_status").catch(() => null);
  });

  onMount(() => {
    const off = listen<string>("voice-install", (e) => (installLine = e.payload));
    return () => off.then((f) => f());
  });
</script>

<div class="scroll">
  {#if note}<p class="error" role="alert">{note}</p>{/if}

  <section class="card glass">
    <h2>Projects <span class="count">{settings.projects.length}</span></h2>
    <p class="hint">
      Name a folder, and dumps that mention it run there: "keep going on my app" works in your app's folder. Claude
      agents get your skills and connectors; Codex uses your ChatGPT sign-in and works offline in its sandbox.
    </p>
    {#if settings.projects.length}
      <ul class="rows">
        {#each settings.projects as p, i (p.name)}
          <li>
            <div class="body">
              <span class="name">{p.name}</span>
              <span class="path" title={p.folder}>{p.folder}</span>
            </div>
            <select
              class="input"
              aria-label="Who works on {p.name}"
              value={p.runner}
              onchange={(e) =>
                save({ ...settings, projects: settings.projects.map((q, j) => (j === i ? { ...q, runner: e.currentTarget.value } : q)) })}
            >
              <option value="claude">Claude</option>
              <option value="codex">Codex</option>
            </select>
            <button
              class="link"
              aria-label="Remove {p.name}"
              onclick={() => save({ ...settings, projects: settings.projects.filter((_, j) => j !== i) })}>Remove</button
            >
          </li>
        {/each}
      </ul>
    {/if}
    <form
      class="add"
      onsubmit={(e) => {
        e.preventDefault();
        addProject();
      }}
    >
      <label class="sr" for="project-name">Project name</label>
      <input id="project-name" class="input" bind:value={newName} placeholder="Project name, e.g. my app" />
      <select class="input" bind:value={newRunner} aria-label="Who works on it">
        <option value="claude">Claude</option>
        <option value="codex">Codex</option>
      </select>
      <button class="button" type="submit" disabled={!newName.trim()}>Choose folder…</button>
    </form>
  </section>

  <section class="card glass">
    <h2>Notes <span class="count">{settings.knowledge.length}</span></h2>
    <p class="hint">Folders agents can read when a task mentions your notes, subjects or documents. Put your Obsidian vault first.</p>
    {#if settings.knowledge.length}
      <ul class="rows">
        {#each settings.knowledge as k, i (k)}
          <li>
            <div class="body">
              {#if i === 0}<span class="name">Obsidian vault</span>{/if}
              <span class="path" title={k}>{k}</span>
            </div>
            <button
              class="link"
              aria-label="Remove {k}"
              onclick={() => save({ ...settings, knowledge: settings.knowledge.filter((_, j) => j !== i) })}>Remove</button
            >
          </li>
        {/each}
      </ul>
    {/if}
    <div class="add"><button class="button" onclick={addNotes}>Add folder…</button></div>
  </section>

  <section class="card glass">
    <h2>Voice</h2>
    <p class="hint">
      Hold <kbd>Ctrl</kbd> <kbd>Alt</kbd> <kbd>Space</kbd> anywhere and speak, or click the mic in the dump box. Speech is
      turned into text on this PC with Whisper.
    </p>
    <ul class="rows">
      <li>
        <div class="body">
          <span class="name">Speech recognition</span>
          <span class="path">
            {#if !status}Checking…
            {:else if status.whisper}Ready.
            {:else if status.python}Needs faster-whisper: install the voice pack below, or run pip install faster-whisper.
            {:else}Needs Python, or the voice pack below.{/if}
          </span>
        </div>
        <select class="input" aria-label="Whisper model" value={settings.voice.model} onchange={(e) => setVoice({ model: e.currentTarget.value })}>
          <option value="small">Small (fast)</option>
          <option value="base.en">Base, English (fastest)</option>
          <option value="large-v3-turbo">Large v3 turbo (best, 1.6 GB download)</option>
        </select>
      </li>
      <li>
        <div class="body">
          <span class="name">Send what I say straight away</span>
          <span class="path">Otherwise it waits in the dump box so you can check it.</span>
        </div>
        <button class="switch" role="switch" aria-checked={settings.voice.auto_submit} aria-label="Send what I say straight away"
          onclick={() => setVoice({ auto_submit: !settings.voice.auto_submit })}><span></span></button>
      </li>
      <li>
        <div class="body">
          <span class="name">Read the briefing when I open Engine</span>
          <span class="path">Or use Read aloud on the Briefing whenever you like.</span>
        </div>
        <button class="switch" role="switch" aria-checked={settings.voice.speak_on_summon} aria-label="Read the briefing when I open Engine"
          onclick={() => setVoice({ speak_on_summon: !settings.voice.speak_on_summon })}><span></span></button>
      </li>
      <li>
        <div class="body">
          <span class="name">Engine's voice</span>
          <span class="path">
            {settings.voice.engine === "chatterbox"
              ? "Chatterbox clones the clip below. Use only a voice you own or have permission to use."
              : "Windows' built-in voice."}
          </span>
        </div>
        <select class="input" aria-label="Engine's voice" value={settings.voice.engine} onchange={(e) => setVoice({ engine: e.currentTarget.value })}>
          <option value="windows">Windows voice</option>
          <option value="chatterbox" disabled={!status?.chatterbox}>Chatterbox (cloned)</option>
        </select>
      </li>
      {#if settings.voice.engine === "chatterbox"}
        <li>
          <div class="body">
            <span class="name">Voice clip</span>
            <span class="path" title={settings.voice.clip}>{settings.voice.clip || "No clip yet: Chatterbox uses its default voice."}</span>
          </div>
          <button class="button" onclick={chooseClip}>Choose clip…</button>
        </li>
      {/if}
      <li>
        <div class="body">
          <span class="name">Voice pack</span>
          <span class="path" title={installLine}>
            {#if installing || installLine}{installLine}
            {:else if status?.own_env}Installed: Chatterbox and Whisper in Engine's own Python.
            {:else}Chatterbox for a cloned voice, plus Whisper. About 3 GB, in its own Python, so your other Python stays as it is.{/if}
          </span>
        </div>
        <button class="button" onclick={installVoice} disabled={installing || status?.own_env}>
          {installing ? "Installing…" : status?.own_env ? "Installed" : "Install"}
        </button>
      </li>
    </ul>
  </section>

  <section class="card glass">
    <h2>General</h2>
    <ul class="rows">
      <li>
        <div class="body">
          <span class="name">Start with Windows</span>
          <span class="path">Engine starts hidden in the tray when you sign in.</span>
        </div>
        <button
          class="switch"
          role="switch"
          aria-checked={autostart}
          aria-label="Start with Windows"
          onclick={() => setAutostart(!autostart)}><span></span></button
        >
      </li>
      <li>
        <div class="body">
          <span class="name">Shortcuts</span>
          <span class="path">Names for sites and folders, like gmail or school.</span>
        </div>
        <button class="button" onclick={onEditAliases}>Edit aliases</button>
      </li>
      <li>
        <div class="body">
          <span class="name">Version</span>
          <span class="path">Engine {version}. Updates install from GitHub when you click Restart to update.</span>
        </div>
      </li>
    </ul>
  </section>
</div>

<style>
  .scroll {
    padding-top: var(--space-sm);
  }

  .body {
    display: flex;
    flex-direction: column;
    flex: 1;
    min-width: 0;
  }

  .name {
    color: var(--text);
  }

  .path {
    overflow: hidden;
    color: var(--text-muted);
    font-size: 13px;
    text-overflow: ellipsis;
    white-space: nowrap;
    user-select: text;
  }

  .rows > li {
    align-items: center;
  }

  select.input {
    flex-shrink: 0;
    max-width: 220px;
  }

  .add {
    display: flex;
    gap: var(--space-sm);
    padding: 10px 0 14px;
  }

  .add .input {
    flex: 1;
  }

  .error {
    margin: 0 0 var(--space-sm);
    padding: var(--space-sm) var(--space-md);
    border: 1px solid rgb(240 113 138 / 0.5);
    border-radius: 9px;
    background: rgb(240 113 138 / 0.1);
    color: var(--text);
    font-size: 13px;
    user-select: text;
  }

  .sr {
    position: absolute;
    width: 1px;
    height: 1px;
    overflow: hidden;
    clip: rect(0 0 0 0);
  }

  .switch {
    position: relative;
    width: 40px;
    height: 22px;
    flex-shrink: 0;
    border: 1px solid var(--border);
    border-radius: 999px;
    background: var(--surface-2);
    transition: background var(--ease), border-color var(--ease);
  }

  .switch span {
    position: absolute;
    top: 2px;
    left: 2px;
    width: 16px;
    height: 16px;
    border-radius: 50%;
    background: var(--text-muted);
    transition: transform var(--ease), background var(--ease);
  }

  .switch[aria-checked="true"] {
    border-color: transparent;
    background: var(--accent-gradient);
  }

  .switch[aria-checked="true"] span {
    transform: translateX(18px);
    background: var(--on-accent);
  }
</style>
