//! Background jobs. Each runs as a headless Claude Code agent on Haiku, in its own folder, draft-only.
//!
//! Agents get the user's own Claude Code setup: CLAUDE.md, skills, plugins, MCP servers and claude.ai connectors.
//! Outward connector tools (send, post, share, delete...) are refused by a PreToolUse hook that runs `engine --guard`.

use serde::Deserialize;
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
approval; Engine refuses outward tools anyway. If the task needs an outward \
step, put it in proposed instead of doing it: action (a short title such as \"Send reply to Sam\"), details (exactly \
what would happen: recipient, subject and full text for a message; file, title and visibility for an upload) and tool \
(the exact tool you would use, such as mcp__claude_ai_Gmail__send_message). If the user has to do something themselves \
(record clips, sign in, make a choice), put it in needs_you: task (exactly what to do and where) and follow_up (what \
an agent should do once they have, if anything). If the task produces a plan with \
dates and times (a study plan, a schedule), also list each session in schedule: title, start and end as local times \
\"YYYY-MM-DDTHH:MM\", and kind (study, deadline, test, recording or other). Engine adds them to the user's calendar. \
Save anything \
you make as files in the current folder. When you finish, return a summary of what you did and found (short, plain \
text, \"- \" bullets, no markdown headings) and an open list of things the user should see now: website URLs, app \
names, or paths of files you made. Leave open empty when nothing needs opening.";

const SCHEMA: &str = r#"{"type":"object","properties":{"summary":{"type":"string"},"open":{"type":"array","items":{"type":"string"}},"proposed":{"type":"array","items":{"type":"object","properties":{"action":{"type":"string"},"details":{"type":"string"},"tool":{"type":"string"}},"required":["action","details"]}},"needs_you":{"type":"array","items":{"type":"object","properties":{"task":{"type":"string"},"follow_up":{"type":"string"}},"required":["task"]}},"schedule":{"type":"array","items":{"type":"object","properties":{"title":{"type":"string"},"start":{"type":"string"},"end":{"type":"string"},"kind":{"type":"string","enum":["study","deadline","test","recording","other"]}},"required":["title","start","end","kind"]}}},"required":["summary","open"]}"#;

/// Outward shell commands Engine knows about. Best effort: an arbitrary script could still reach the network.
const DENIED: &str = "Bash(git push:*) Bash(gh:*) Bash(curl -X POST:*) PowerShell(git push:*) PowerShell(gh:*)";

/// Settings layered over the user's own for agent runs: the outward-tool guard, and superpowers off, because its
/// brainstorming gate waits for a human to approve a design and a background agent has nobody to ask.
fn settings(engine_exe: &Path, approved: Option<(&str, &Path)>) -> String {
    let slash = |p: &Path| p.to_string_lossy().replace('\\', "/");
    let mut guard = format!("\"{}\" --guard", slash(engine_exe));
    if let Some((tool, marker)) = approved {
        guard += &format!(" --allow \"{tool}\" --once \"{}\"", slash(marker));
    }
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

/// Whether the guard lets `tool` through. An approved job may make one outward call: to the approved tool, or
/// any outward tool if `allow` is "*". `used` says whether that call has happened.
pub fn permits(tool: &str, allow: Option<&str>, used: bool) -> bool {
    !outward(tool) || allow.is_some_and(|a| !used && (a == "*" || a == tool))
}

/// `engine --guard [--allow TOOL --once MARKER]`: a Claude Code PreToolUse hook. Reads the hook's JSON on stdin;
/// exit code 2 refuses the tool. With --allow, the first matching outward call goes through and leaves MARKER.
pub fn guard() -> i32 {
    let args: Vec<String> = std::env::args().collect();
    let flag = |name: &str| args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned();
    let (allow, once) = (flag("--allow"), flag("--once"));
    let mut input = String::new();
    let _ = std::io::stdin().read_to_string(&mut input);
    let tool = serde_json::from_str::<Value>(&input).ok().and_then(|v| v["tool_name"].as_str().map(str::to_string));
    let used = once.as_ref().is_some_and(|m| Path::new(m).exists());
    match tool {
        Some(tool) if outward(&tool) && permits(&tool, allow.as_deref(), used) => {
            if let Some(marker) = &once {
                let _ = std::fs::write(marker, &tool);
            }
            0
        }
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

/// The audit: a second, tool-free model call that checks a finished job against what was asked.
pub const AUDIT_SYSTEM: &str = "You check work done by a background AI agent for its user. Given the task and the \
agent's report, decide whether the work does what was asked. Answer good when it does. Answer concern when part of \
the task is missing, the report is vague about what was actually done, it relies on guesses it doesn't flag, or it \
did something outward (sent, posted, deleted) that it shouldn't have. reason: one short sentence the user can act on.";

pub const AUDIT_SCHEMA: &str = r#"{"type":"object","properties":{"verdict":{"type":"string","enum":["good","concern"]},"reason":{"type":"string"}},"required":["verdict","reason"]}"#;

pub fn audit_prompt(task: &str, report: &str, opened: &[String]) -> String {
    let opened = if opened.is_empty() { "nothing".to_string() } else { opened.join(", ") };
    format!("Task:\n{task}\n\nAgent's report:\n{report}\n\nEngine opened for the user: {opened}")
}

/// Reads the audit's structured reply as (verdict, reason).
pub fn audit_verdict(out: &Value) -> Option<(String, String)> {
    let verdict = out["verdict"].as_str().filter(|v| matches!(*v, "good" | "concern"))?;
    Some((verdict.to_string(), out["reason"].as_str().unwrap_or_default().trim().to_string()))
}

#[derive(Debug, PartialEq)]
pub struct Outcome {
    pub summary: String,
    pub open: Vec<String>,
    /// Outward steps the agent prepared but didn't do: each becomes an approval card.
    pub proposed: Vec<Proposal>,
    /// Things only the user can do: each becomes a needs-you card.
    pub needs_you: Vec<UserTask>,
    /// Sessions of a plan the agent made: each becomes a block in the schedule.
    pub schedule: Vec<crate::schedule::Block>,
}

#[derive(Debug, Deserialize, PartialEq)]
pub struct Proposal {
    pub action: String,
    pub details: String,
    #[serde(default)]
    pub tool: String,
}

#[derive(Debug, Deserialize, PartialEq)]
pub struct UserTask {
    pub task: String,
    #[serde(default)]
    pub follow_up: String,
}

pub fn review_system(today: &str) -> String {
    format!(
        "You are Engine, the user's desktop assistant. Today is {today}. Write a short end-of-day review: what got \
         done, what's still open and what to do about it, what failed or looked doubtful, and what tomorrow holds. \
         End with the three most useful things to do first tomorrow. Plain text, short lines, \"- \" bullets, no \
         markdown headings. Under 200 words."
    )
}

/// What the end-of-day review looks at: today's briefing, and today's and tomorrow's schedule.
pub fn review_prompt(items: &[crate::store::Item], today: &[crate::schedule::Block], tomorrow: &[crate::schedule::Block]) -> String {
    let mut text = String::from("Today's briefing:\n");
    for i in items.iter().filter(|i| i.kind != "instant") {
        let verdict = match i.verdict.as_str() {
            "good" => " (checked: looks good)".to_string(),
            "concern" => format!(" (checked: concern: {})", i.verdict_reason),
            _ => String::new(),
        };
        text += &format!("- [{}] {}: {}{verdict}\n", i.status, i.kind, i.title);
    }
    let opened = items.iter().filter(|i| i.kind == "instant").count();
    text += &format!("Opened {opened} apps or sites.\n\nToday's schedule:\n");
    for b in today {
        text += &format!("- {}–{} {} ({})\n", &b.start[11..], &b.end[11..], b.title, b.kind);
    }
    text += "\nTomorrow's schedule:\n";
    for b in tomorrow {
        text += &format!("- {}–{} {} ({})\n", &b.start[11..], &b.end[11..], b.title, b.kind);
    }
    text
}

/// Instructions for the agent that carries out an action the user approved.
pub fn approved_prompt(action: &str, details: &str, tool: &str) -> String {
    let tool = if tool.is_empty() { String::new() } else { format!("\nUse {tool}.") };
    format!(
        "The user approved this action. Do exactly this, once, and nothing else outward, then report what you did.\n\n\
         Action: {action}\n{details}{tool}"
    )
}

/// Instructions for redrafting an approval card with the user's note.
pub fn revise_prompt(action: &str, details: &str, note: &str) -> String {
    format!(
        "Revise this draft for the user and propose it again for approval. Don't do it.\n\nAction: {action}\n{details}\n\n\
         The user's note: {note}"
    )
}

/// Instructions for an outward request straight from a dump: draft it as an approval card.
pub fn prepare_prompt(request: &str) -> String {
    format!(
        "Prepare this for the user's approval. Do not carry it out and do not try the tool that would: Engine refuses \
         it. Look up anything you need (the thread, the recipient's address), draft the exact content, and return it \
         in proposed.\n\n{request}"
    )
}

/// Runs one job to the end in `work` (a project folder, or the job's own folder), with read access to `notes`.
/// `on_spawn` gets the agent's process id (for Cancel); `on_activity` gets each step as (orb process, label).
/// The agent's raw event stream is kept in `agent-log.jsonl` in `log_dir`.
pub fn run(
    work: &Path,
    log_dir: &Path,
    notes: &[String],
    approved: Option<(&str, &Path)>,
    prompt: &str,
    on_spawn: impl FnOnce(u32),
    mut on_activity: impl FnMut(&'static str, String),
) -> Result<Outcome, String> {
    let exe = std::env::current_exe().map_err(|e| format!("Couldn't find Engine's own path for the agent guard: {e}"))?;
    let system = match notes {
        [] => SYSTEM.to_string(),
        [vault, rest @ ..] => {
            let more = if rest.is_empty() { String::new() } else { format!(" Other notes and documents: {}.", rest.join("; ")) };
            format!(
                "{SYSTEM} The user's Obsidian vault is {vault}.{more} Search these when the task mentions their notes, \
                 subjects, syllabus or documents."
            )
        }
    };
    let mut cmd = Command::new("claude");
    cmd.current_dir(work)
        .args(["-p", "--no-session-persistence", "--verbose", "--output-format", "stream-json"])
        .args(["--model", MODEL, "--append-system-prompt", &system, "--json-schema", SCHEMA])
        // Unattended: every tool is allowed except the denied commands, and the guard hook refuses outward ones.
        .args(["--permission-mode", "bypassPermissions", "--permission-prompts", "none"])
        .args(["--disallowedTools", DENIED, "--settings", &settings(&exe, approved)]);
    if !notes.is_empty() {
        cmd.arg("--add-dir").args(notes);
    }
    cmd
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

    let mut log = File::create(log_dir.join("agent-log.jsonl")).ok();
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
    Ok(structured(&event["structured_output"], event["result"].as_str()))
}

/// Builds an outcome from an agent's structured reply; `fallback` is its plain text if the structure is missing.
fn structured(out: &Value, fallback: Option<&str>) -> Outcome {
    let summary = out["summary"].as_str().or(fallback).unwrap_or_default().trim().to_string();
    let open = out["open"]
        .as_array()
        .map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect())
        .unwrap_or_default();
    let list = |key: &str| out.get(key).cloned().unwrap_or(Value::Array(vec![]));
    let proposed = serde_json::from_value(list("proposed")).unwrap_or_default();
    let needs_you = serde_json::from_value(list("needs_you")).unwrap_or_default();
    let schedule = serde_json::from_value(list("schedule")).unwrap_or_default();
    Outcome { summary, open, proposed, needs_you, schedule }
}

/// The result format for Codex. OpenAI's strict schemas need every field required and no extra fields, so empty
/// arrays and strings stand in for "none".
const CODEX_SCHEMA: &str = r#"{"type":"object","additionalProperties":false,"required":["summary","open","proposed","needs_you","schedule"],"properties":{"summary":{"type":"string"},"open":{"type":"array","items":{"type":"string"}},"proposed":{"type":"array","items":{"type":"object","additionalProperties":false,"required":["action","details","tool"],"properties":{"action":{"type":"string"},"details":{"type":"string"},"tool":{"type":"string"}}}},"needs_you":{"type":"array","items":{"type":"object","additionalProperties":false,"required":["task","follow_up"],"properties":{"task":{"type":"string"},"follow_up":{"type":"string"}}}},"schedule":{"type":"array","items":{"type":"object","additionalProperties":false,"required":["title","start","end","kind"],"properties":{"title":{"type":"string"},"start":{"type":"string"},"end":{"type":"string"},"kind":{"type":"string","enum":["study","deadline","test","recording","other"]}}}}}}"#;

/// Runs one job with Codex (`codex exec`), on the user's ChatGPT sign-in. Codex works in its workspace-write
/// sandbox (no network, writes only in `work`) with its MCP servers, apps, plugins, browser and computer use turned
/// off, so it can't reach anything outward; outward steps come back as proposals like any other agent's.
pub fn run_codex(
    work: &Path,
    log_dir: &Path,
    prompt: &str,
    on_spawn: impl FnOnce(u32),
    mut on_activity: impl FnMut(&'static str, String),
) -> Result<Outcome, String> {
    let schema = log_dir.join("codex-schema.json");
    let last = log_dir.join("codex-result.json");
    std::fs::write(&schema, CODEX_SCHEMA).map_err(|e| e.to_string())?;
    let _ = std::fs::remove_file(&last);
    let mut cmd = program("codex");
    cmd.current_dir(work)
        .args(["exec", "--json", "--skip-git-repo-check", "--ephemeral"])
        // The user's config stays (it sets up Codex's Windows sandbox), but nothing that could act outward does:
        // no MCP servers, apps, plugins, browser or computer use.
        .args(["-c", "mcp_servers={}"])
        .args(["--disable", "apps", "--disable", "plugins", "--disable", "computer_use"])
        .args(["--disable", "browser_use", "--disable", "browser_use_external", "--disable", "in_app_browser"])
        .args(["--sandbox", "workspace-write", "--output-schema"])
        .arg(&schema)
        .arg("-o")
        .arg(&last)
        .arg("-")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    let mut child = cmd.spawn().map_err(|e| format!("Couldn't start Codex ({e}). Is the Codex CLI installed?"))?;
    on_spawn(child.id());
    let instructions = format!("{SYSTEM}\n\nThe task:\n{prompt}");
    child
        .stdin
        .take()
        .expect("stdin is piped")
        .write_all(instructions.as_bytes())
        .map_err(|e| format!("Couldn't talk to Codex: {e}"))?;
    let mut stderr = child.stderr.take().expect("stderr is piped");
    let errors = std::thread::spawn(move || {
        let mut s = String::new();
        let _ = stderr.read_to_string(&mut s);
        s
    });

    let mut log = File::create(log_dir.join("agent-log.jsonl")).ok();
    let mut failure = None;
    for line in BufReader::new(child.stdout.take().expect("stdout is piped")).lines() {
        let Ok(line) = line else { break };
        if let Some(log) = log.as_mut() {
            let _ = writeln!(log, "{line}");
        }
        let Ok(event) = serde_json::from_str::<Value>(&line) else { continue };
        if event["type"] == "turn.failed" || event["type"] == "error" {
            failure = event["error"]["message"].as_str().or(event["message"].as_str()).map(str::to_string);
        } else if let Some((process, label)) = codex_activity(&event) {
            on_activity(process, label);
        }
    }
    let _ = child.wait();
    let errors = errors.join().unwrap_or_default();
    if let Some(e) = failure {
        return Err(e);
    }
    let text = std::fs::read_to_string(&last).map_err(|_| {
        errors.lines().rev().find(|l| !l.trim().is_empty()).map_or("Codex stopped before it finished.".into(), |l| l.trim().to_string())
    })?;
    let out: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
    Ok(structured(&out, Some(&text)))
}

/// A command for a CLI on the PATH. npm installs some CLIs (Codex among them) as `.cmd` launchers, which Windows
/// only runs through cmd.exe, so those are wrapped in `cmd /C`.
fn program(name: &str) -> Command {
    let on_path = |file: String| {
        std::env::var_os("PATH").is_some_and(|p| std::env::split_paths(&p).any(|d| d.join(&file).is_file()))
    };
    if cfg!(windows) && !on_path(format!("{name}.exe")) && on_path(format!("{name}.cmd")) {
        let mut cmd = Command::new("cmd");
        cmd.args(["/D", "/C", name]);
        cmd
    } else {
        Command::new(name)
    }
}

/// What Codex is doing, from one `codex exec --json` event.
pub fn codex_activity(event: &Value) -> Option<(&'static str, String)> {
    if event["type"] != "item.started" && event["type"] != "item.completed" {
        return None;
    }
    let item = &event["item"];
    match item["type"].as_str()? {
        "reasoning" | "agent_message" => Some(("thinking", "Thinking".into())),
        "command_execution" => Some(("working", "Running a command".into())),
        "file_change" => Some(("writing", "Editing files".into())),
        "web_search" => Some(("searching", match item["query"].as_str() {
            Some(q) => format!("Searching the web for “{}”", short(q)),
            None => "Searching the web".into(),
        })),
        "mcp_tool_call" => Some(("working", short(&format!("Using {}", item["tool"].as_str().unwrap_or("a tool"))))),
        "todo_list" => Some(("thinking", "Planning".into())),
        _ => None, // "error" items are Codex's own config warnings
    }
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
        let expect = Outcome {
            summary: "Did it.".into(),
            open: vec!["https://a.com".into()],
            proposed: vec![],
            needs_you: vec![],
            schedule: vec![],
        };
        assert_eq!(outcome(&done), Ok(expect));
        let cards = json!({"type": "result", "is_error": false, "structured_output": {"summary": "s", "open": [],
            "proposed": [{"action": "Send reply", "details": "To: Sam", "tool": "mcp__g__send_message"}],
            "needs_you": [{"task": "Record 3 clips"}]}});
        let out = outcome(&cards).unwrap();
        assert_eq!(out.proposed[0].tool, "mcp__g__send_message");
        assert_eq!(out.needs_you[0], UserTask { task: "Record 3 clips".into(), follow_up: String::new() });
        let plan = json!({"type": "result", "is_error": false, "structured_output": {"summary": "s", "open": [],
            "schedule": [{"title": "Faraday", "start": "2026-11-17T16:00", "end": "2026-11-17T17:00", "kind": "study"}]}});
        assert_eq!(outcome(&plan).unwrap().schedule[0].title, "Faraday");
        let failed = json!({"type": "result", "is_error": true, "result": "Not logged in"});
        assert_eq!(outcome(&failed), Err("Not logged in".into()));
    }

    #[test]
    fn reads_codex_events() {
        let cmd = json!({"type": "item.started", "item": {"type": "command_execution", "command": "ls"}});
        assert_eq!(codex_activity(&cmd), Some(("working", "Running a command".into())));
        let edit = json!({"type": "item.completed", "item": {"type": "file_change"}});
        assert_eq!(codex_activity(&edit).unwrap().0, "writing");
        assert_eq!(codex_activity(&json!({"type": "item.completed", "item": {"type": "error", "message": "x"}})), None);
        assert_eq!(codex_activity(&json!({"type": "turn.completed"})), None);
        let out = structured(&json!({"summary": "Did it", "open": [], "proposed": [], "needs_you": [], "schedule": []}), None);
        assert_eq!(out.summary, "Did it");
        let schema: Value = serde_json::from_str(CODEX_SCHEMA).unwrap();
        assert_eq!(schema["required"].as_array().unwrap().len(), 5);
    }

    #[test]
    fn reads_audit_verdicts() {
        let good = json!({"verdict": "good", "reason": " Covers all three. "});
        assert_eq!(audit_verdict(&good), Some(("good".into(), "Covers all three.".into())));
        assert_eq!(audit_verdict(&json!({"verdict": "maybe", "reason": "x"})), None);
        assert!(audit_prompt("do x", "did x", &[]).contains("opened for the user: nothing"));
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
    fn approved_jobs_get_one_outward_call() {
        let send = "mcp__claude_ai_Gmail__send_message";
        assert!(!permits(send, None, false));
        assert!(permits(send, Some(send), false));
        assert!(!permits(send, Some(send), true)); // only once
        assert!(!permits("mcp__claude_ai_Gmail__forward", Some(send), false)); // only the approved tool
        assert!(permits("mcp__claude_ai_Gmail__forward", Some("*"), false));
        assert!(permits("mcp__claude_ai_Gmail__search_threads", None, true)); // reads are always fine
    }

    #[test]
    fn guard_settings_are_valid_json() {
        let s: Value = serde_json::from_str(&settings(Path::new("C:\\Users\\x\\engine.exe"), None)).unwrap();
        let cmd = s["hooks"]["PreToolUse"][0]["hooks"][0]["command"].as_str().unwrap();
        assert_eq!(cmd, "\"C:/Users/x/engine.exe\" --guard");
        let approved = settings(Path::new("C:\\e.exe"), Some(("mcp__a__send", Path::new("C:\\j\\once"))));
        let s: Value = serde_json::from_str(&approved).unwrap();
        let cmd = s["hooks"]["PreToolUse"][0]["hooks"][0]["command"].as_str().unwrap();
        assert_eq!(cmd, "\"C:/e.exe\" --guard --allow \"mcp__a__send\" --once \"C:/j/once\"");
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
