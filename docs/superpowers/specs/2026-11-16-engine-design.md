# Engine: design

Date: 2026-11-16. Status: draft, awaiting approval.

## Intent

Engine is a personal, Jarvis-style assistant for one person on one Windows 11 PC. It is the single front door to
the AI tools they already use (Claude, ChatGPT/Codex, ElevenLabs, later a local model). They talk or type one messy
"brain dump" in the morning; Engine splits it into jobs, does the instant ones itself (open an app or site), answers
the quick ones, hands the big ones to background agents, checks the agents' work, and gives back one clear
briefing: what to do now, what is running, what is done, and what needs them.

Why its own app: the user wants something that feels like their own product, runs light, and lives on the desktop.
Efficiency is a value here, not a nice-to-have (explicitly: no Electron).

### The reference scenario

A morning dump like this one is the acceptance test for the whole product. It is illustrative; the real tasks will
vary.

> Hey Engine, open my email and draft replies to anything urgent. Keep going on my app project and tell me what's
> left this week. Research what's trending in my niche and turn it into a content plan, with a list of what I need
> to record. I have a test next week: use my notes and the syllabus to plan what to study each day. And open
> YouTube.

Expected result: Engine opens what was asked, then gives one list: what to do now, what each agent is working on
("Codex is working on the app"), what is finished, and what needs the user ("approve these replies"; "record these
clips and drop them in this folder, and the agents handle the edit and upload").

A "study planner" here means the user's notes (for example an Obsidian vault with a syllabus) plus Engine's own
built-in schedule.

Engine v1 does not have to complete every integration in that dump (see Non-goals). It must correctly split it into
items, act on the instant one, answer the quick one, dispatch the background ones, hold the outward ones for
approval, and show all of it in the briefing.

## Goals (v1)

1. **Desktop app.** Tauri 2: Rust core, TypeScript web UI. Dark mode only, clean, minimal. Lives in the system tray,
   starts with Windows (toggle), single instance, summoned with a global hotkey.
2. **Input.** A dump box: type, or hold a push-to-talk hotkey and speak.
3. **Router.** Splits a dump into items and classifies each as one of four kinds:
   - *instant*: do it now (open an app, a site, a folder);
   - *answer*: one model call, result goes in the briefing (e.g. a 7-day study plan);
   - *job*: a background agent task (draft short ideas, continue coding a project, research trends);
   - *outward*: anything other people will see or that cannot be undone (send or reply to email, upload, post,
     delete). Always held for approval.
4. **App launcher.** "Open Opera", "open Gmail", "open YouTube": installed desktop apps, Store apps, websites, and
   folders, matched by name or by a user alias. Clear "open X" requests skip the model entirely so they feel instant.
5. **Agents.** Each job runs as a background process of a CLI the user is already signed in to: Claude Code
   (`claude -p`, headless) or Codex (`codex exec`), in the right working folder, on the user's existing plans. Output
   is streamed so the UI shows live status. A concurrency cap (default 3) queues the rest.
6. **Approvals.** Agents run draft-only: tools that send, upload, publish or delete are blocked for them. When an
   agent's result includes an outward action (a drafted email reply, a ready-to-upload video), it becomes an approval
   card. Approve runs the action; Reject discards it; Revise takes a note and starts a new job with the draft and
   the note as context.
7. **Audit.** When a job finishes, a second model call checks the result against the original ask and marks it
   *looks good* or *concern* with a one-line reason. (The end-of-day review is phase 5.)
8. **Briefing.** One view with four sections: Now, Running, Needs you (approvals plus things only the user can do,
   such as "record these 3 clips and put them in `<folder>`"), Done.
9. **Voice out.** A local voice model reads a short spoken summary of the briefing in Engine's own voice. Toggle in
   settings.
10. **Logo.** A macOS (Big Sur and later) style app icon: a rendered V8 engine on a rounded-square tile, soft studio
    lighting, reads well on a dark taskbar. Used for the app icon, tray icon and installer.
11. **Knowledge.** Engine can read the user's notes when answering or planning: their Obsidian vault, plus any other
    folder they add (e.g. a syllabus folder, or PDFs exported from a handwriting app). "I have a test on <topic>"
    means: find the matching notes and the syllabus, then plan from them.
12. **Schedule.** A built-in calendar (week and agenda views) of blocks: study sessions, deadlines, tests, recording
    sessions. Plans an agent produces (e.g. a 7-day revision plan) land here as blocks, and the same plan is written
    as a Markdown note into the vault so it also shows in Obsidian.

## Non-goals (v1)

- **Phone access.** Briefings and approvals live in the desktop app only. (Tauri 2 can target mobile later.)
- **Always-listening wake word ("hey Engine").** Phase 2. Push-to-talk first.
- **Local model.** Phase 2 (Ollama is not installed yet). Later use: offline or cheap routing.
- **Dedicated integrations:** YouTube upload and analytics, Gmail, the study planner, trend research. In v1 these go
  to a general Claude Code or Codex agent with whatever tools that agent already has. Purpose-built integrations
  are added one at a time afterwards, each as its own small project.
- **Proprietary note-app formats.** Only exported PDFs, via a knowledge folder.
- **Calendar sync** (Google, Outlook) and **Mermaid diagrams** in the UI. Later, if wanted.
- **Search indexes or embeddings over the notes.** Agents search the vault folder directly; revisit only if that is
  too slow.
- **Rebuilding what existing tools already do.** Engine orchestrates. It never reimplements a pipeline an agent can
  already run (for example, video production lives in the user's own project and skills).

## Anti-goals (failure even if it technically works)

- **Bloat.** If it idles heavy or starts slowly, it has failed its reason for existing. Target: idle memory well under
  200 MB including the WebView2 processes, cold start under 2 s, near-zero CPU at idle.
- **Acting without permission.** Engine must never send, upload, publish or delete without an explicit approval click.
- **Leaking secrets.** API keys never in plain-text files, logs, prompts or the UI.
- **A chatbot clone.** The core loop is dump in, actions and a briefing out, not a long chat thread.

## Constraints

- Windows 11 (WebView2 is built in). Installed: Rust 1.97.1, Node 24, Claude Code CLI 2.1.319, Codex CLI 0.163.0.
- No API keys needed. Claude and Codex run on the user's existing signed-in plans through their CLIs, and voice
  runs locally on an NVIDIA GPU (8 GB of VRAM is the target). ElevenLabs stays an optional fallback.
- Chatterbox can reuse an existing local install if one is set in Settings.
- UI design follows the `ui-ux-pro-max` skill (the user's design authority).

## Approaches considered

1. **Tauri 2 (chosen by the user).** Rust core for system work (launching apps, hotkeys, mic, spawning agents, tray);
   web UI rendered by the OS WebView. Small and fast, easy to make look polished, and can target mobile later. Costs
   two languages (Rust plus some TypeScript).
2. **Pure Rust UI (egui or Slint).** One language, very light, but a polished chat-and-cards interface is much slower
   to build. Rejected: wouldn't reach the look the user wants as quickly.
3. **Electron.** Fastest to build, but bundles a whole browser. Rejected by the user outright for inefficiency.

## Decided by the user

- Engine is its own desktop app; briefings and approvals live in it, not on the phone.
- Rust, via Tauri. No Electron.
- Really clean UI, dark mode.
- Logo: macOS app-icon style, an actual V8 engine.
- Capabilities: brain dump, small agents for big tasks, a daily overview, auditing, opening apps and sites.
- Tools to connect: Claude, ChatGPT, ElevenLabs, a local model later.
- Voice runs on local models (speech in and out), with a cloned voice for Engine.

## The design

### Architecture

```
            ┌──────────── Tauri app (one process + WebView2) ────────────┐
 hotkey ──▶ │  UI (TypeScript)        Rust core                          │
 mic    ──▶ │  dump box  ◀── events ── router ── launcher (apps/sites)   │
            │  briefing               jobs ───── spawns: claude -p        │
            │  approvals              audit                codex exec     │
            │  settings               voice ──── Whisper (STT, in-process)│
            │                                    Chatterbox (TTS helper) │
            │                         store ──── SQLite + Windows keyring │
            └────────────────────────────────────────────────────────────┘
```

The Rust core owns all state and side effects. The UI only renders state and sends user intents (submit dump,
approve, reject, retry) through Tauri commands; the core pushes changes back as events.

### Router

- Fast path, no model: a dump that is only "open/launch/start X" goes straight to the launcher.
- Otherwise one call to Claude via `claude -p --model haiku` with a fixed system prompt and a JSON schema. Output:
  a list of items, each `{kind, title, detail, runner?, project?, needs_approval, follow_up?}`. `follow_up` is a job
  that waits on the user (see Needs-you tasks).
- The router knows the user's **Projects** (see Settings) so it can attach a job to the right working folder
  ("my app" means that folder; "the channel" means that one).
- *Answer* items get their own model call; the result is stored and shown in the briefing. An item that needs the
  user's own files, sites or accounts (e.g. "open my study planner and plan my week") is a *job*, not an answer,
  because an agent has the tools to read them; a plain model call does not.

### Briefing model

- One briefing per day. Later dumps the same day add to it.
- Unfinished items (running jobs, pending approvals, open Needs-you tasks) carry over to the next day's briefing.
- Done items leave the briefing at the end of the day and stay in history.
- Jobs keep running if the window is closed (the app stays in the tray); quitting the app cancels them, with a
  warning first.

### Launcher

- Index on start and on demand: Start Menu shortcuts (user and all-users `Start Menu\Programs\**\*.lnk`) plus Store
  apps (AppUserModelIDs, as `Get-StartApps` lists them).
- Plus a user **Aliases** list: name to URL, folder, or app (e.g. `gmail` to `https://mail.google.com`).
- Fuzzy match on name; on a near-tie, show the top choices instead of guessing.
- URLs open in the default browser.

### Agents (jobs)

- Each job: runner (`claude` or `codex`), working folder (from its Project), prompt (written by the router with the
  user's original words included), status, log file, result, audit verdict.
- Claude Code: `claude -p "<prompt>" --output-format stream-json` with outward tools denied via
  `--disallowedTools`. Codex: `codex exec --json` with its workspace-write sandbox.
- The prompt tells the agent it is draft-only and to report any outward action as a structured "proposed action"
  block, which Engine turns into an approval card.
- **How strict draft-only is (decided):** Engine reliably blocks the outward tools it knows about: email
  send/reply/forward, uploads (YouTube), posting, and `git push`. Agents keep everything else: editing and
  committing files in their project folder, running commands there, and reading the web. This is best effort
  against an arbitrary script that sends something on its own; that risk is accepted because full lockdown (no
  network, no shell) would stop coding and research jobs from working. Headless agents run in a non-interactive
  permission mode with that deny list applied.
- Agents get read access to the knowledge folders (Claude Code: `--add-dir <vault>`).
- Live progress in Running; Cancel kills the process tree; failures show the last lines of the log and a Retry.

### Approvals

- A card shows exactly what will happen: recipient and full text for an email, file and title and visibility for an
  upload. Approve runs it (v1: by starting a follow-up agent with that one action allowed); Reject discards it;
  Revise starts a fresh job with the original draft plus the user's note (no session resume needed).
- Nothing outward ever runs without a card being approved.

### Needs-you tasks and follow-ups

- Some jobs end by asking the user to do something ("record these 3 clips into `<folder>`"). That becomes a
  Needs-you card.
- If the router attached a `follow_up` job (e.g. "edit the clips and draft the upload"), the card has a **Done**
  button that starts it. No folder watching in v1 (phase 5).

### Knowledge

- Settings hold a list of knowledge folders; the first is the Obsidian vault. Obsidian notes are plain Markdown, so
  no plugin or API is needed.
- Jobs that mention the user's subjects, notes or documents get those folders attached; the agent searches them
  itself.

### Schedule

- Blocks in SQLite: `{title, start, end, kind (study, deadline, test, recording, other), linked note?, item?}`.
- Agents return plans as a structured "schedule" block (same idea as "proposed action"); Engine adds the blocks
  (adding to the user's own calendar is not outward, so no approval) and writes the plan as a note into
  `<vault>/Engine/`.
- Views: this week, and an agenda list. Today's blocks also show at the top of the briefing.
- The user can drag, edit or delete blocks by hand.

### Voice

All voice runs locally (decided by the user): free, private, works offline.

- **Speech to text: Whisper**, run in the Rust core through whisper.cpp with CUDA (`large-v3-turbo` by default; a
  smaller model as a setting). Push-to-talk: hold the hotkey, record from the default mic, transcribe, put the text
  in the dump box (the user can edit it before submitting, or set auto-submit).
- **Text to speech: Chatterbox**, run as a small local helper process that Engine starts and talks to over
  localhost. It clones a voice from a ~10 s reference clip, which gives Engine one consistent voice. Spoken briefing:
  a short summary (under ~30 s).
- **Fast short replies (only if needed):** if Chatterbox is too slow for one-liners like "Opened Opera", add Kokoro
  (tiny, fast, fixed voices) for those and keep Chatterbox for briefings.
- **GPU sharing:** models load only when voice is used and unload after a few idle minutes, so Engine doesn't hold
  VRAM while the user games or renders.
- **Voice rights:** the reference voice must be one the user owns or has permission to use (their own, a designed
  voice, or a licensed one), never a real person's voice without consent.
- ElevenLabs (STT and TTS) remains an optional fallback if a key is added in Settings.

### Storage and settings

- SQLite in the app data folder: dumps, items, jobs, approvals, audit results, launcher index.
- Secrets (any optional API key, e.g. an ElevenLabs fallback key) in Windows Credential Manager.
- Settings: Projects (name, folder, default runner), Aliases, Knowledge folders, hotkeys, start with Windows, concurrency cap, voice on
  or off and voice choice.

### UI

Dark, quiet, generous spacing, one accent colour, no clutter. Three views: Briefing (home), Schedule, Settings.
Rough layout of the Briefing view:

```
┌ Engine ─────────────────────────────────────────── – □ × ┐
│  ┌──────────────────────────────────────────────┐  (mic)  │
│  │ Dump anything…                               │         │
│  └──────────────────────────────────────────────┘         │
│  NOW        Opened YouTube                                │
│  RUNNING    Claude · Draft 4 video ideas        ▮▮▮▯      │
│             Codex  · App: this week's tasks     ▮▯▯▯      │
│  NEEDS YOU  Approve reply to "Licence question"  [Approve]│
│             Record 3 clips → ~\Videos\inbox               │
│  DONE       7-day study plan for the test       ▸         │
└───────────────────────────────────────────────────────────┘
```

Exact visual design (type, colour, spacing, motion) is produced with `ui-ux-pro-max` and shown to the user as a
mockup before the screens are built.

### Logo

Generate several candidates with an image model in the macOS icon style (rounded-square tile, rendered V8 engine,
soft light, subtle shadow), show them to the user, refine the pick, then produce a 1024 px master and let
`tauri icon` generate every size (`.ico`, `.png`, tray).

## Build order

Each phase ends with something the user can run.

1. **Shell and launcher.** Tauri app, dark UI shell, tray, global hotkey, dump box, launcher with aliases. Done
   when "open Opera", "open Gmail", "open YouTube" work from the hotkey. Logo work runs alongside.
2. **Router and briefing.** Dump splitting, *answer* items, briefing view, SQLite. Done when the reference dump
   produces correctly classified items and a study-plan answer.
3. **Agents, audit, knowledge and schedule.** Claude and Codex jobs, live status, cancel and retry, audit verdicts,
   Projects and Knowledge folders in settings, the Schedule view. Done when the reference dump's study item reads
   the vault and syllabus and puts a 7-day plan in the Schedule and in Obsidian.
4. **Approvals and voice.** Draft-only enforcement, approval cards, Needs-you follow-ups, push-to-talk, spoken
   briefing.
5. **Later, one at a time:** wake word, local model, end-of-day review, folder watching, calendar sync, dedicated integrations (YouTube,
   Gmail, study planner, trend research), optional phone access.

## Calls made by Claude (correct any of these)

- Push-to-talk in v1; wake word in phase 2. Reason: always-on listening needs a wake-word engine and tuning; prove
  the rest first.
- Local voice (decided by the user): Whisper `large-v3-turbo` through whisper.cpp for speech-to-text, Chatterbox
  for text-to-speech, models unloaded when idle. Kokoro only if Chatterbox is too slow for short replies.
- The router uses `claude -p` (the user's plan) rather than an API key. Reason: no new key or bill.
- All outward actions need approval. Reason: a sent email or public upload can't be undone.
- Frontend in Svelte with TypeScript. Reason: compiles small and fast, fits the efficiency goal.
- Default concurrency cap of 3 agents.
- One briefing per day; unfinished items carry over; Done clears at day end (kept in history).
- Items that need the user's own data become jobs, not bare answers.
- Reject discards; Revise re-runs as a new job with the note. No agent-session resume in v1.
- Inside a git project, agents may edit, delete and commit files (git makes it recoverable); `git push` counts as
  outward.
- The end-of-day review moves to phase 5, triggered by a button.
- Draft-only strictness: block known outward tools, accept best-effort risk for arbitrary scripts (the user handed
  this call to Claude).
- Follow-ups after a Needs-you task start from a Done button; folder watching later (the user handed this call to
  Claude).
- A built-in Schedule view instead of Mermaid diagrams, and plans also written into Obsidian (the user handed this
  call to Claude).
- No search index over notes; agents search the vault directly.

## Left to the builder

- Exact crates and Tauri plugins; frontend tooling details.
- How to enumerate and launch Store apps reliably.
- Whether agents can use the user's claude.ai connectors (e.g. Gmail) in headless mode, and the fallback if not.
- Exact JSON schemas for router output and "proposed action" blocks.
- Measuring the memory and start-time targets. (Phase 1 measured: about 166 MB private memory in total, including
  WebView2.)
- Whether Engine reuses the existing Chatterbox environment or gets its own install, and how the Whisper model file
  is downloaded on first use.
