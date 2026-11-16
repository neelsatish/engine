# Engine

A personal assistant for Windows that ties your AI tools together. Built with Tauri 2 (Rust core, Svelte UI).

Design: [docs/superpowers/specs/2026-11-16-engine-design.md](docs/superpowers/specs/2026-11-16-engine-design.md).
UI tokens: [design-system/engine/MASTER.md](design-system/engine/MASTER.md).

## What works now (phase 1)

- Open apps, sites and folders: "open Opera", "hey Engine, open my Gmail", "launch Spotify".
  Engine knows every app in your Start menu. Sites and folders come from your aliases.
- `Ctrl+Shift+Space` shows or hides Engine from anywhere; `Esc` hides it.
- Closing the window hides it to the tray. Quit from the tray icon's menu.

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

Tests: `cargo test` in `src-tauri` (add `-- --include-ignored` to also check your real Start menu).
Release build: `npm run tauri build`.
