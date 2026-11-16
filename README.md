# Engine

A personal assistant for Windows that ties your AI tools together. Built with Tauri 2 (Rust core, Svelte UI).

Design: [docs/superpowers/specs/2026-11-16-engine-design.md](docs/superpowers/specs/2026-11-16-engine-design.md).
UI tokens: [design-system/engine/MASTER.md](design-system/engine/MASTER.md).

## What works now (phases 1 and 2)

- Open apps, sites and folders: "open Opera", "hey Engine, open my Gmail", "launch Spotify".
  Engine knows every app in your Start menu. Sites and folders come from your aliases. A plain "open X" never
  waits on a model.
- Dump anything else and Engine sorts it into items:
  - *instant*: opened right away;
  - *answer*: answered by Claude, shown under Done (click to expand);
  - *job*: queued for agents (phase 3);
  - *outward* (send, post, upload, delete): held under Needs you, never run (approvals are phase 4).
- The briefing has four sections: Now, Running, Needs you, Done. There is one briefing per day, and unfinished items
  carry over to the next day. Dismiss removes an item.
- `Ctrl+Shift+Space` shows or hides Engine from anywhere; `Esc` hides it.
- Closing the window hides it to the tray. Quit from the tray icon's menu.

Sorting and answers use the Claude Code CLI you're already signed in to (`claude` on your PATH), so there's no API
key. The calls run with `--safe-mode`, so your own Claude Code plugins, hooks and CLAUDE.md don't change them. The
briefing is stored in `%APPDATA%\com.engine.desktop\engine.db`.

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

## Develop

```bash
npm install
npm run tauri dev
```

Tests: `cargo test` in `src-tauri`. Add `-- --include-ignored` to also check your real Start menu and run the
router on sample dumps through Claude Code (this uses your plan).
Release build: `npm run tauri build`.
