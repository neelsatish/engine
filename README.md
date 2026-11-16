# Engine

A personal assistant for Windows that ties your AI tools together. Built with Tauri 2 (Rust core, Svelte UI).

Design: [docs/superpowers/specs/2026-11-16-engine-design.md](docs/superpowers/specs/2026-11-16-engine-design.md).
UI tokens: [design-system/engine/MASTER.md](design-system/engine/MASTER.md).

## What works now

- Open apps, sites and folders: "open Opera", "hey Engine, open my Gmail", "launch Spotify".
  Engine knows every app in your Start menu. Sites and folders come from your aliases. A plain "open X" never
  waits on a model.
- Dump anything else and Engine sorts it into items:
  - *instant*: opened right away;
  - *answer*: answered by Claude, shown under Done (click to expand);
  - *job*: handed to a background agent (see below);
  - *outward* (send, post, upload, delete): held under Needs you, never run (approvals are phase 4).
- The briefing has four sections: Now, Running, Needs you, Done. There is one briefing per day, and unfinished items
  carry over to the next day. Dismiss removes an item.

### Agents

Each job runs as a headless Claude Code agent on Haiku, up to 3 at once (the rest queue). An agent works on its own:
it searches and reads the web, reads and writes files, and runs commands. Running shows what it's doing right now,
with an orb for the kind of step (thinking, viewing, searching, writing, working). When it finishes, its summary
goes under Done and Engine opens anything it asked for: a website, an app or alias, or a file it made.

Agents get your own Claude Code setup: your CLAUDE.md, skills, plugins, MCP servers and claude.ai connectors (Gmail,
Google Drive, Notion and so on). The one exception is superpowers, which is turned off for agents because its
brainstorming step waits for a person to approve a design, and a background agent has nobody to ask.

Agents are draft-only. Before every connector call, Claude Code runs `engine --guard`, which refuses outward tools:
sending, replying, forwarding, posting, uploading, sharing, deleting, calendar invites and similar. Drafts and new
documents are allowed. The outward shell commands Engine knows about (`git push`, `gh`) are blocked too. That's best
effort, not a sandbox: an arbitrary script could still reach the network. Each job gets its own folder under
`%APPDATA%\com.engine.desktop\jobs\`, with the agent's full log in `agent-log.jsonl`. Cancel stops a running agent
and everything it started; Retry re-runs a failed job.
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
