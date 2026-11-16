# Engine

A personal assistant for Windows that ties your AI tools together. Built with Tauri 2 (Rust core, Svelte UI).

Design: [docs/superpowers/specs/2026-11-16-engine-design.md](docs/superpowers/specs/2026-11-16-engine-design.md).
UI tokens: [design-system/engine/MASTER.md](design-system/engine/MASTER.md).

## Layout

- **Stage, top left:** the Engine logo drawn in ASCII. It ripples in the Velvet Fold gradient with your voice
  while you talk, then hands over to a thinking orb with what Engine is doing ("Sorting it out", "2 agents working").
  Briefing, Schedule and Settings open here too.
- **Chat, down the right side:** a conversation with Engine. Your messages (typed or spoken) sit on the right; under
  each, what Engine did: full answers, agents with their live step and a Stop button, their reports when done, and
  approvals or tasks that need you, with Approve, Done and Dismiss right there. The composer at the bottom has the
  folder, model, Local AI, mic and send controls.
- **Tab rail, bottom left:** Home, Briefing (with a count of what needs you), Schedule, Settings, plus Review my day
  and Read aloud.
- **System, bottom middle:** CPU, RAM, GPU load, VRAM and GPU temperature, refreshed every two seconds while Engine is
  visible. CPU temperature needs admin rights on Windows, so it isn't shown.

## What works now

- Open apps, sites and folders: "open Opera", "hey Engine, open my Gmail", "launch Spotify".
  Engine knows every app in your Start menu. Sites and folders come from your aliases. A plain "open X" never
  waits on a model.
- Dump anything else and Engine sorts it into items:
  - *instant*: opened right away;
  - *answer*: answered by Claude, shown under Done (click to expand);
  - *job*: handed to a background agent (see below);
  - *outward* (send, post, upload, delete): an agent drafts it, and it comes back as an approval card.
- The briefing has four sections: Now, Running, Needs you, Done. There is one briefing per day, and unfinished items
  carry over to the next day. Dismiss removes an item.
- **Quick commands** run instantly, with no model involved:
  - media keys: "pause", "next song", "previous";
  - volume: "volume up", "volume 30", "mute";
  - "play lofi on Spotify" and "play cooking videos on YouTube";
  - "search the web for …" and "lock my pc";
  - "stop everything" (or "stop all processes", "never mind") ends running agents and stops Engine talking.
- **Reminders:** "remind me in 20 minutes to stretch" or "remind me at 6pm to call mum" shows a Windows notification
  when it's due, and the reminder also appears in the Schedule.
- **Clipboard:** dumps that mention what you copied ("summarize what I copied") get your clipboard text attached.
- **Folder, model and Local AI chips** in the dump box. Pick a folder, like choosing a working folder in Claude, and jobs from
  your next dumps work there; recent folders are one click away. The model chip picks the agents' model: Haiku,
  Sonnet, Opus or Fable.
- `Ctrl+Shift+Space` shows or hides Engine from anywhere; `Esc` hides it. Closing the window hides it to the tray;
  quit from the tray icon's menu.

Sorting and answers use the Claude Code CLI you're already signed in to (`claude` on your PATH), so there's no API
key. Those calls run with `--safe-mode`, so your own Claude Code plugins, hooks and CLAUDE.md don't change them. The
briefing is stored in `%APPDATA%\com.engine.desktop\engine.db`.

### Agents

Each job runs as a headless Claude Code agent on Haiku, up to 3 at once (the rest queue). An agent works on its own:
it searches and reads the web, reads and writes files, and runs commands. Running shows what it's doing right now,
with an orb for the kind of step (thinking, viewing, searching, writing, working). When it finishes, its summary
goes under Done and Engine opens anything it asked for: a website, an app or alias, or a file it made. A second, quick
check then reads the report against the task and marks it **Looks good** or **Concern**, with a reason.

Agents get your own Claude Code setup: your CLAUDE.md, skills, plugins, MCP servers and claude.ai connectors (Gmail,
Google Drive, Notion and so on). The one exception is superpowers, which is turned off for agents because its
brainstorming step waits for a person to approve a design, and a background agent has nobody to ask.

A project can be worked on by **Codex** instead (set it per project in Settings). Codex runs `codex exec` on your
ChatGPT sign-in, in its workspace-write sandbox, with its MCP servers, apps, plugins, browser and computer use turned
off so it can't act outward. Approved actions always run on Claude, where the one-call guard and your connectors are.

Each job gets its own folder under `%APPDATA%\com.engine.desktop\jobs\`, with the agent's full log in
`agent-log.jsonl`. A job about one of your **projects** works in that project's folder instead. Cancel stops a running
agent and everything it started; Retry re-runs a failed job.

### Approvals and Needs you

Agents are draft-only. Before every connector call, Claude Code runs `engine --guard`, which refuses outward tools:
sending, replying, forwarding, posting, uploading, sharing, deleting, calendar invites and similar. Drafts and new
documents are allowed, and the outward shell commands Engine knows about (`git push`, `gh`) are blocked too. That's
best effort, not a sandbox: an arbitrary script could still reach the network.

When a job needs an outward step, the agent prepares it and it appears under Needs you as an approval card showing
exactly what would happen (recipient and full text for an email, for example):

- **Approve** starts an agent that does exactly that action. The guard lets that one tool through, once.
- **Revise** takes your note and has an agent redraft the card.
- **Reject** drops it.

When something needs you personally (record some clips, choose a platform), it appears as a task. If an agent should
carry on afterwards, the card says what it will do, and **Done** starts it.

### Schedule

The Schedule view has a week grid and an agenda of the next 30 days. When an answer or an agent produces a plan with
times (a study plan, say), its sessions land here as blocks, and today's blocks show at the top of the Briefing.
If you've set a notes folder, the plan is also written to `<vault>/Engine/` as a Markdown note, so it shows in
Obsidian. Engine never overwrites a note there: a second plan with the same name gets a number. Drag a block to move
it (it snaps to 15 minutes), click it to edit or delete it, or use **Add block**.

### Voice

**Engine acts while you talk.** Each phrase runs as soon as you pause, so "open Spotify" opens Spotify while you
keep going, and bigger requests start in parallel. Letting go of the key handles the last phrase, so there's no
send. Turn this off in Settings → Voice to get the words in the dump box instead.

**Say "Engine".** With the wake word on (Settings → Voice), Engine keeps the microphone open and answers "Yes sir?"
in a British voice, then acts on what you say next. "Engine, open Spotify" acts at once. Each burst of speech is
transcribed on this PC and forgotten unless it starts with "Engine"; nothing is recorded or sent anywhere.

**Engine talks you through it.** While an agent works you hear a word at each step ("On it", "Writing", "Sending",
"Almost done"), and when a task or answer is done Engine reads a short overview. Talking to Engine cuts it off.
Turn it off with "Talk me through tasks".

Hold the key under Esc (`` ` ``) anywhere and speak, with or without Shift; a quick tap still types the character,
and with Ctrl, Alt or Win held it passes straight through. Engine comes up so you can see it listening.
`Ctrl+Alt+Space` and the mic button in the dump box work too. "Open Claude" or "open ChatGPT" opens the installed
app when there is one, and the website otherwise. Engine records your default microphone,
turns it into text on this PC with Whisper (faster-whisper), and puts the words in the dump box for you to check, or
sends them straight away if you turn that on. Recordings are deleted as soon as they're transcribed. **Read aloud**
speaks a short summary of the briefing, and Settings can make Engine do that whenever you open it.

Speech recognition uses Engine's own voice Python when it's installed (otherwise `python` on your PATH with
`faster-whisper`), and the Whisper model you pick: Small English by default (about 0.2 s a pass on a laptop GPU;
Base is quicker, Large v3 turbo most accurate), on the GPU when PyTorch's CUDA build is installed (Engine borrows its
cuBLAS and cuDNN). The model stays loaded while you use voice (always, with the wake word on) and is otherwise
unloaded after 15 idle minutes. The **voice pack** in Settings (about 3 GB, started by you) installs Whisper,
Kokoro (the British voice, whose model downloads on first use) and Chatterbox into Engine's own Python 3.11 with
`uv`. Engine can then speak in the British voice, or in a voice cloned from a short clip with Chatterbox. Only clone a
voice you own or have permission to use. Without it, Engine speaks with Windows' built-in voice.

### Settings

- **Projects:** name a folder ("my app"), and dumps that mention it run there.
- **Notes:** folders agents may read, with your Obsidian vault first. Jobs that mention your notes, subjects or
  documents search them.
- **Start with Windows:** Engine starts hidden in the tray when you sign in.
- **Connections:** everything Claude agents can use (claude.ai connectors such as Gmail, Drive and Notion, plus MCP
  servers), with each one's status and a button to add more on claude.ai.
- **Models:** which Claude model the agents use, and which one writes answers. Sorting your dumps always uses Haiku.
  Agents can also run on ChatGPT models through Codex: GPT-6.1 Sol, GPT-6 Astra, GPT-6.1 Terra and GPT-6 Luna.
- **Local AI (power button):** the power chip in the dump box, or the switch in Settings → Models.
  - **On:** Engine starts Ollama, and its own thinking (sorting, answers, audits, reviews) runs on Qwen3 8B on your
    PC, falling back to Claude if it fails.
  - **Off:** Engine unloads the model, stops Ollama so it uses nothing, and goes back to Claude Haiku.
  - Agents always stay on Claude or ChatGPT, because a local model can't drive tools well enough.

**Review my day** (next to Read aloud) writes a short end-of-day review: what got done, what's open, what looked
doubtful, what tomorrow holds, and the first three things to do. It lands under Done.

Settings are saved in `%APPDATA%\com.engine.desktop\settings.json`.

## Aliases

Click **Edit aliases** in Engine, or open `%APPDATA%\com.engine.desktop\aliases.json`. Each entry is a name and
what to open: a URL, a folder path, or an app.

```json
[
  { "name": "gmail", "target": "https://mail.google.com" },
  { "name": "school", "target": "C:\\Users\\you\\Documents\\School" }
]
```

Changes apply straight away, without a restart.

## Install and updates

Download `Engine_<version>_x64-setup.exe` from the [latest release](https://github.com/neelsatish/engine/releases/latest)
and run it. No admin rights are needed.

After that, Engine updates itself. It checks GitHub Releases when it starts and every 6 hours, downloads any new
version in the background, and then shows **Restart to update** in the footer.

To publish a new version, commit your changes, then run:

```bash
npm version patch
git push --follow-tags
```

`npm version` bumps the version and tags it. The tag starts the `Release` GitHub Action, which builds the installer,
signs it, and publishes the release. The signing key's private half is the `TAURI_SIGNING_PRIVATE_KEY` repository
secret; keep a backup of it (it lives at `~/.tauri/engine.key` on the machine that made it). Without it, installed
copies can't verify new versions and would have to be reinstalled by hand.

## Develop

```bash
npm install
npm run tauri dev
```

Tests: `cargo test` in `src-tauri`. Add `-- --include-ignored` to also check your real Start menu and run the
router on sample dumps through Claude Code (this uses your plan).
Release build: `npm run tauri build`.

## Credits

Status orbs: [Thinking Orbs](https://github.com/yogesharc/thinking-orbs) by Yogesh, MIT licence.
