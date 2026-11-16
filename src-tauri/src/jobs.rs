//! Background jobs. Each runs as a headless Claude Code agent on Haiku, in its own folder, draft-only.
//!
//! Agents get the user's own Claude Code setup: CLAUDE.md, skills, plugins, MCP servers and claude.ai connectors.
//! Outward connector tools (send, post, share, delete...) are refused by a PreToolUse hook that runs `engine --guard`.

use serde_json::Value;
use std::{
    fs::File,
    io::{BufRead, BufReader, Read, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

pub const MODEL: &str = "haiku";
/// Jobs past this many wait in the queue.
pub const MAX_RUNNING: usize = 3;

const SYSTEM: &str = "You are a background agent working for Engine, the user's desktop assistant. Work on your own: \
never ask the user questions or wait for approval; make sensible assumptions and say what they were. You have the \
user's skills, connectors (such as Gmail, Google Drive, Notion, Canva, Microsoft 365) and MCP tools: use them when \
they help. You are draft-only: never send, reply to, forward, post, upload, share or publish anything, never pay for \
anything, never git push, and never delete anything outside the current folder. Creating drafts (email drafts, new \
documents, new pages) is fine. If the task needs an outward step, prepare it and say it is ready for the user's \
approval; Engine refuses outward tools anyway. Save anything \
you make as files in the current folder. When you finish, return a summary of what you did and found (short, plain \
text, \"- \" bullets, no markdown headings) and an open list of things the user should see now: website URLs, app \
names, or paths of files you made. Leave open empty when nothing needs opening.";

const SCHEMA: &str = r#"{"type":"object","properties":{"summary":{"type":"string"},"open":{"type":"array","items":{"type":"string"}}},"required":["summary","open"]}"#;

/// Outward shell commands Engine knows about. Best effort: an arbitrary script could still reach the network.
const DENIED: &str = "Bash(git push:*) Bash(gh:*) Bash(curl -X POST:*) PowerShell(git push:*) PowerShell(gh:*)";

/// Settings layered over the user's own for agent runs: the outward-tool guard, and superpowers off, because its
/// brainstorming gate waits for a human to approve a design and a background agent has nobody to ask.
fn settings(engine_exe: &Path) -> String {
    let guard = format!("\"{}\" --guard", engine_exe.to_string_lossy().replace('\\', "/"));
    serde_json::json!({
        "hooks": {"PreToolUse": [{"matcher": "mcp__.*", "hooks": [{"type": "command", "command": guard}]}]},
        "enabledPlugins": {"superpowers@claude-plugins-official": false}
    })
    .to_string()
}

/// Whether an MCP tool acts outward: others would see it, or it can't be undone. Matched on whole words of the
/// tool's own name, so `list-comments`, `sharepoint_search` and `create_reply_draft` stay allowed.
pub fn outward(tool: &str) -> bool {
    let Some((_, name)) = tool.strip_prefix("mcp__").and_then(|t| t.split_once("__")) else { return false };
    let name = name.to_lowercase();
    let words: Vec<&str> = name.split(['_', '-']).filter(|w| !w.is_empty()).collect();
    let has = |list: &[&str]| words.iter().any(|w| list.contains(w));
    let service = ["outlook", "sharepoint", "notion", "nvapp", "client", "batch"];
    let first = words.iter().copied().find(|w| !service.contains(w));

    if matches!(first, Some("list" | "get" | "search" | "read" | "fetch" | "query" | "show" | "view" | "download")) {
        return false;
    }
    let destroys = has(&["send", "delete", "trash", "untrash"]);
    if has(&["draft"]) && !destroys {
        return false; // drafts are the point of draft-only
    }
    let changes_event = has(&["event", "events"]) && has(&["create", "update", "cancel", "delete", "respond"]);
    changes_event
        || destroys
        || has(&[
            "reply", "forward", "post", "publish", "upload", "share", "invite", "comment", "respond", "cancel", "spam",
            "move", "rename", "merge", "resolve", "resolved", "set", "pay", "purchase",
        ])
}

/// `engine --guard`: a Claude Code PreToolUse hook. Reads the hook's JSON on stdin; exit code 2 refuses the tool.
pub fn guard() -> i32 {
    let mut input = String::new();
    let _ = std::io::stdin().read_to_string(&mut input);
    let tool = serde_json::from_str::<Value>(&input).ok().and_then(|v| v["tool_name"].as_str().map(str::to_string));
    match tool {
        Some(tool) if outward(&tool) => {
            eprintln!(
                "Engine blocked {tool}: background agents are draft-only. Prepare it instead (a draft, a file, the \
                 exact text) and say it is ready for the user's approval."
            );
            2
        }
        _ => 0,
    }
}

#[derive(Debug, PartialEq)]
pub struct Outcome {
    pub summary: String,
    pub open: Vec<String>,
}

/// Runs one job to the end. `on_spawn` gets the agent's process id (for Cancel); `on_activity` gets each step
/// as (orb process, label). The agent's raw event stream is kept in `agent-log.jsonl` in `dir`.
pub fn run(
    dir: &Path,
    prompt: &str,
    on_spawn: impl FnOnce(u32),
    mut on_activity: impl FnMut(&'static str, String),
) -> Result<Outcome, String> {
    let exe = std::env::current_exe().map_err(|e| format!("Couldn't find Engine's own path for the agent guard: {e}"))?;
    let mut cmd = Command::new("claude");
    cmd.current_dir(dir)
        .args(["-p", "--no-session-persistence", "--verbose", "--output-format", "stream-json"])
        .args(["--model", MODEL, "--append-system-prompt", SYSTEM, "--json-schema", SCHEMA])
        // Unattended: every tool is allowed except the denied commands, and the guard hook refuses outward ones.
        .args(["--permission-mode", "bypassPermissions", "--permission-prompts", "none"])
        .args(["--disallowedTools", DENIED, "--settings", &settings(&exe)])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    let mut child = cmd
        .spawn()
        .map_err(|e| format!("Couldn't start Claude Code ({e}). Is it installed and on PATH?"))?;
    on_spawn(child.id());
    child
        .stdin
        .take()
        .expect("stdin is piped")
        .write_all(prompt.as_bytes())
        .map_err(|e| format!("Couldn't talk to Claude Code: {e}"))?;

    // Drain stderr on the side so a chatty agent can't fill the pipe and stall.
    let mut stderr = child.stderr.take().expect("stderr is piped");
    let errors = std::thread::spawn(move || {
        let mut s = String::new();
        let _ = stderr.read_to_string(&mut s);
        s
    });

    let mut log = File::create(dir.join("agent-log.jsonl")).ok();
    let mut result = None;
    for line in BufReader::new(child.stdout.take().expect("stdout is piped")).lines() {
        let Ok(line) = line else { break };
        if let Some(log) = log.as_mut() {
            let _ = writeln!(log, "{line}");
        }
        let Ok(event) = serde_json::from_str::<Value>(&line) else { continue };
        if event["type"] == "result" {
            result = Some(event);
        } else if let Some((process, label)) = activity(&event) {
            on_activity(process, label);
        }
    }
    let _ = child.wait();
    let errors = errors.join().unwrap_or_default();
    match result {
        Some(event) => outcome(&event),
        None => Err(errors
            .lines()
            .rev()
            .find(|l| !l.trim().is_empty())
            .map_or("The agent stopped before it finished.".into(), |l| l.trim().to_string())),
    }
}

/// Reads the agent's final `result` event.
pub fn outcome(event: &Value) -> Result<Outcome, String> {
    if event["is_error"] != Value::Bool(false) {
        return Err(event["result"].as_str().unwrap_or("The agent reported an error.").to_string());
    }
    let out = &event["structured_output"];
    let summary = out["summary"].as_str().or(event["result"].as_str()).unwrap_or_default().trim().to_string();
    let open = out["open"]
        .as_array()
        .map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect())
        .unwrap_or_default();
    Ok(Outcome { summary, open })
}

/// What the agent is doing, from one stream event: the orb to show and a short label.
pub fn activity(event: &Value) -> Option<(&'static str, String)> {
    if event["type"] != "assistant" {
        return None;
    }
    let block = event["message"]["content"].as_array()?.last()?;
    match block["type"].as_str()? {
        "thinking" | "text" => Some(("thinking", "Thinking".into())),
        "tool_use" => Some(tool(block["name"].as_str()?, &block["input"])),
        _ => None,
    }
}

fn tool(name: &str, input: &Value) -> (&'static str, String) {
    let text = |key: &str| input[key].as_str().unwrap_or_default();
    let file = |key: &str| Path::new(text(key)).file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_default();
    match name {
        "Read" => ("viewing", format!("Reading {}", file("file_path"))),
        "Glob" | "Grep" => ("searching", "Looking through files".into()),
        "WebSearch" => ("searching", format!("Searching the web for “{}”", short(text("query")))),
        "WebFetch" => ("viewing", format!("Reading {}", host(text("url")))),
        "Edit" | "Write" | "NotebookEdit" => ("writing", format!("Writing {}", file("file_path"))),
        "Bash" | "PowerShell" => ("working", match text("description") {
            "" => "Running a command".into(),
            d => short(d),
        }),
        "TodoWrite" => ("thinking", "Planning".into()),
        "StructuredOutput" => ("writing", "Writing up".into()),
        "Skill" => ("thinking", format!("Using the {} skill", text("skill"))),
        other if other.starts_with("mcp__") => {
            let mut parts = other.trim_start_matches("mcp__").splitn(2, "__");
            let server = parts.next().unwrap_or_default().trim_start_matches("claude_ai_").replace('_', " ");
            let action = parts.next().unwrap_or_default().replace(['_', '-'], " ");
            ("working", short(&format!("{server}: {action}")))
        }
        other => ("working", format!("Using {other}")),
    }
}

fn short(s: &str) -> String {
    match s.char_indices().nth(60) {
        Some((i, _)) => format!("{}…", &s[..i]),
        None => s.to_string(),
    }
}

/// "https://www.example.com/a/b" to "example.com".
pub fn host(url: &str) -> String {
    let rest = url.split_once("://").map_or(url, |(_, r)| r);
    let host = rest.split(['/', '?', '#']).next().unwrap_or(rest);
    host.strip_prefix("www.").unwrap_or(host).to_string()
}

/// A file the agent made, if `wanted` names one inside `dir`. Agents can only open their own files this way.
pub fn own_file(dir: &Path, wanted: &str) -> Option<PathBuf> {
    let path = dir.join(wanted); // an absolute `wanted` replaces `dir`, and is then checked like any other
    let (real, root) = (path.canonicalize().ok()?, dir.canonicalize().ok()?);
    (real.starts_with(&root) && real.is_file()).then_some(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn step(block: Value) -> Option<(&'static str, String)> {
        activity(&json!({"type": "assistant", "message": {"content": [block]}}))
    }

    #[test]
    fn labels_activity() {
        let read = json!({"type": "tool_use", "name": "Read", "input": {"file_path": "C:\\notes\\plan.md"}});
        assert_eq!(step(read), Some(("viewing", "Reading plan.md".into())));
        let search = json!({"type": "tool_use", "name": "WebSearch", "input": {"query": "light linux distros"}});
        assert_eq!(step(search), Some(("searching", "Searching the web for “light linux distros”".into())));
        let fetch = json!({"type": "tool_use", "name": "WebFetch", "input": {"url": "https://www.example.com/x?y"}});
        assert_eq!(step(fetch).unwrap().1, "Reading example.com");
        let cmd = json!({"type": "tool_use", "name": "Bash", "input": {"command": "ls"}});
        assert_eq!(step(cmd), Some(("working", "Running a command".into())));
        assert_eq!(step(json!({"type": "text", "text": "hi"})).unwrap().0, "thinking");
        assert_eq!(activity(&json!({"type": "user"})), None);
    }

    #[test]
    fn reads_outcomes() {
        let done = json!({"type": "result", "is_error": false, "structured_output": {"summary": " Did it. ", "open": ["https://a.com"]}});
        assert_eq!(outcome(&done), Ok(Outcome { summary: "Did it.".into(), open: vec!["https://a.com".into()] }));
        let failed = json!({"type": "result", "is_error": true, "result": "Not logged in"});
        assert_eq!(outcome(&failed), Err("Not logged in".into()));
    }

    #[test]
    fn guards_outward_tools() {
        let blocked = [
            "mcp__claude_ai_Gmail__send_message",
            "mcp__claude_ai_Gmail__reply",
            "mcp__claude_ai_Gmail__forward",
            "mcp__claude_ai_Gmail__trash_thread",
            "mcp__claude_ai_Gmail__mark_message_spam",
            "mcp__claude_ai_Gmail__delete_draft",
            "mcp__claude_ai_Google_Drive__share_file",
            "mcp__claude_ai_Microsoft_365__outlook_send_draft",
            "mcp__claude_ai_Microsoft_365__outlook_create_event",
            "mcp__claude_ai_Microsoft_365__outlook_batch_delete_messages",
            "mcp__claude_ai_Microsoft_365__sharepoint_upload_file",
            "mcp__claude_ai_Notion__notion-create-comment",
            "mcp__claude_ai_Notion__notion-calendar-create-events",
            "mcp__claude_ai_Notion__notion-delete-pages",
            "mcp__claude_ai_Canva__publish-brand-template",
            "mcp__Roblox_Studio__upload_image",
            "mcp__nvidia-app__nvapp_client_set_laptop_feature",
        ];
        let allowed = [
            "mcp__claude_ai_Gmail__create_draft",
            "mcp__claude_ai_Gmail__update_draft",
            "mcp__claude_ai_Gmail__search_threads",
            "mcp__claude_ai_Gmail__list_labels",
            "mcp__claude_ai_Microsoft_365__outlook_create_reply_draft",
            "mcp__claude_ai_Microsoft_365__sharepoint_search",
            "mcp__claude_ai_Microsoft_365__sharepoint_folder_search",
            "mcp__claude_ai_Google_Drive__list_shared_drives",
            "mcp__claude_ai_Google_Drive__create_file",
            "mcp__claude_ai_Notion__notion-create-pages",
            "mcp__claude_ai_Notion__notion-get-comments",
            "mcp__claude_ai_Canva__list-comments",
            "mcp__claude_ai_Canva__generate-design",
            "mcp__claude_ai_Canva__remove-background",
            "mcp__Roblox_Studio__script_read",
            "Bash",
        ];
        for t in blocked {
            assert!(outward(t), "should block {t}");
        }
        for t in allowed {
            assert!(!outward(t), "should allow {t}");
        }
    }

    #[test]
    fn guard_settings_are_valid_json() {
        let s: Value = serde_json::from_str(&settings(Path::new("C:\\Users\\x\\engine.exe"))).unwrap();
        let cmd = s["hooks"]["PreToolUse"][0]["hooks"][0]["command"].as_str().unwrap();
        assert_eq!(cmd, "\"C:/Users/x/engine.exe\" --guard");
        assert_eq!(s["enabledPlugins"]["superpowers@claude-plugins-official"], false);
    }

    #[test]
    fn opens_only_own_files() {
        let dir = std::env::temp_dir().join("engine-own-file-test");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("report.md"), "x").unwrap();
        assert!(own_file(&dir, "report.md").is_some());
        assert!(own_file(&dir, &dir.join("report.md").to_string_lossy()).is_some());
        assert!(own_file(&dir, "missing.md").is_none());
        assert!(own_file(&dir, "..\\..").is_none());
        assert!(own_file(&dir, "C:\\Windows\\notepad.exe").is_none());
    }
}
