//! Engine's own MCP server (`engine --mcp`), so agents can work Engine itself: read and add to the schedule, set
//! reminders, read and clear the briefing and chat, and change settings. JSON-RPC over stdin and stdout, one message
//! per line. It works on the same database and settings file as the app, then leaves a note so the open app
//! refreshes.

use crate::{schedule::Block, settings::Settings, store::Store};
use serde_json::{json, Value};
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};

/// Where the app keeps its database and settings (the same folder Tauri gives it).
fn data_dir() -> PathBuf {
    PathBuf::from(std::env::var("APPDATA").unwrap_or_default()).join("com.engine.desktop")
}

/// The note the app watches for: touched after any change, so the open window reloads.
pub fn change_note(dir: &Path) -> PathBuf {
    dir.join("mcp-changed")
}

fn tools() -> Value {
    let s = |props: Value, required: &[&str]| json!({"type": "object", "properties": props, "required": required});
    json!([
        {"name": "schedule_list", "description": "The person's schedule between two local times (\"YYYY-MM-DDTHH:MM\"). Without times: today and tomorrow.",
         "inputSchema": s(json!({"from": {"type": "string"}, "to": {"type": "string"}}), &[])},
        {"name": "schedule_add", "description": "Put something on the person's schedule. start is local time \"YYYY-MM-DDTHH:MM\"; minutes is how long (default 60); kind is study, deadline, test, recording or other.",
         "inputSchema": s(json!({"title": {"type": "string"}, "start": {"type": "string"}, "minutes": {"type": "integer"}, "kind": {"type": "string"}}), &["title", "start"])},
        {"name": "remind", "description": "Remind the person at a local time \"YYYY-MM-DDTHH:MM\" (a Windows notification, also spoken).",
         "inputSchema": s(json!({"title": {"type": "string"}, "when": {"type": "string"}}), &["title", "when"])},
        {"name": "briefing_list", "description": "Today's briefing: what's running, waiting on the person, and done.",
         "inputSchema": s(json!({}), &[])},
        {"name": "briefing_clear", "description": "Clear the briefing: removes finished and waiting items (running ones stay).",
         "inputSchema": s(json!({}), &[])},
        {"name": "chat_clear", "description": "Clear Engine's chat: removes finished and failed messages.",
         "inputSchema": s(json!({}), &[])},
        {"name": "change_setting", "description": "Change one of Engine's settings. setting: theme (velvet or mono), model (haiku, sonnet, opus, fable, gpt-6.1-sol, gpt-6-astra, gpt-6.1-terra, gpt-6-luna), advisor (none, fable, opus), effort (default, low, medium, high, xhigh, max), voice (kokoro, chatterbox), wake (on, off), local_ai (on, off).",
         "inputSchema": s(json!({"setting": {"type": "string"}, "value": {"type": "string"}}), &["setting", "value"])}
    ])
}

/// Runs one tool. Ok is the text for the agent; Err is an error it should see.
fn call(dir: &Path, name: &str, args: &Value) -> Result<String, String> {
    let store = Store::open(&dir.join("engine.db")).map_err(|e| e.to_string())?;
    let text = |k: &str| args[k].as_str().unwrap_or_default().trim().to_string();
    let changed = || {
        let _ = std::fs::write(change_note(dir), "");
    };
    match name {
        "schedule_list" => {
            let (from, to) = match (text("from"), text("to")) {
                (f, t) if !f.is_empty() && !t.is_empty() => (f, t),
                _ => {
                    let (from, to) = store.today_range().map_err(|e| e.to_string())?;
                    let after = store.day_after(&to[..10]).map_err(|e| e.to_string())?;
                    (from, format!("{after}T00:00"))
                }
            };
            let blocks = store.blocks(&from, &to).map_err(|e| e.to_string())?;
            Ok(if blocks.is_empty() {
                "Nothing on the schedule then.".into()
            } else {
                blocks.iter().map(|b| format!("{} to {}: {} ({})", b.start, b.end, b.title, b.kind)).collect::<Vec<_>>().join("\n")
            })
        }
        "schedule_add" | "remind" => {
            let title = text("title");
            let start: String = text(if name == "remind" { "when" } else { "start" }).chars().take(16).collect();
            if title.is_empty() || !crate::schedule::valid_time(&start) {
                return Err("Needs a title and a local time like 2026-11-20T18:00.".into());
            }
            let minutes = if name == "remind" { 5 } else { args["minutes"].as_i64().unwrap_or(60).clamp(5, 24 * 60) };
            let end = store.add_minutes(&start, minutes).map_err(|e| e.to_string())?;
            let kind = match (name, text("kind").as_str()) {
                ("remind", _) => "reminder".to_string(),
                (_, k) if crate::schedule::KINDS.contains(&k) => k.to_string(),
                _ => "other".to_string(),
            };
            store.save_block(Block { title: title.clone(), start: start.clone(), end: end.clone(), kind, ..Default::default() }).map_err(|e| e.to_string())?;
            changed();
            Ok(format!("{} {title} at {start}.", if name == "remind" { "Reminder set:" } else { "Added" }))
        }
        "briefing_list" => {
            let items = store.briefing().map_err(|e| e.to_string())?;
            Ok(items
                .iter()
                .filter(|i| i.kind != "instant")
                .map(|i| format!("[{}] {} ({}){}", i.status, i.title, i.kind, if i.result.is_empty() { String::new() } else { format!(": {}", i.result.chars().take(160).collect::<String>()) }))
                .collect::<Vec<_>>()
                .join("\n"))
        }
        "briefing_clear" | "chat_clear" => {
            let n = if name == "chat_clear" { store.clear_chat() } else { store.clear_briefing() }.map_err(|e| e.to_string())?;
            changed();
            Ok(format!("Cleared {n} items."))
        }
        "change_setting" => {
            let path = dir.join("settings.json");
            let mut s = Settings::load(&path)?;
            let said = crate::control::tweak(&mut s, &text("setting"), &text("value"))?;
            s.save(&path)?;
            changed();
            Ok(said)
        }
        other => Err(format!("No tool called {other}.")),
    }
}

/// Answers one JSON-RPC message, or None for a notification.
fn answer(dir: &Path, msg: &Value) -> Option<Value> {
    let id = msg.get("id")?.clone();
    let result = match msg["method"].as_str().unwrap_or_default() {
        "initialize" => json!({
            "protocolVersion": msg["params"]["protocolVersion"].as_str().unwrap_or("2025-06-18"),
            "capabilities": {"tools": {}},
            "serverInfo": {"name": "engine", "version": env!("CARGO_PKG_VERSION")}
        }),
        "tools/list" => json!({"tools": tools()}),
        "tools/call" => {
            let (text, failed) = match call(dir, msg["params"]["name"].as_str().unwrap_or_default(), &msg["params"]["arguments"]) {
                Ok(t) => (t, false),
                Err(e) => (e, true),
            };
            json!({"content": [{"type": "text", "text": text}], "isError": failed})
        }
        "ping" => json!({}),
        other => return Some(json!({"jsonrpc": "2.0", "id": id, "error": {"code": -32601, "message": format!("Unknown method {other}")}})),
    };
    Some(json!({"jsonrpc": "2.0", "id": id, "result": result}))
}

/// Serves MCP on stdin and stdout until the agent hangs up.
pub fn serve() {
    let dir = data_dir();
    let stdout = std::io::stdout();
    for line in std::io::stdin().lock().lines().map_while(Result::ok) {
        let Ok(msg) = serde_json::from_str::<Value>(line.trim_start_matches('\u{feff}')) else { continue };
        if let Some(reply) = answer(&dir, &msg) {
            let mut out = stdout.lock();
            let _ = writeln!(out, "{reply}");
            let _ = out.flush();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn speaks_mcp_and_works_the_schedule() {
        let dir = std::env::temp_dir().join(format!("engine-mcp-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let init = answer(&dir, &json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {"protocolVersion": "2025-06-18"}})).unwrap();
        assert_eq!(init["result"]["serverInfo"]["name"], "engine");
        assert!(answer(&dir, &json!({"jsonrpc": "2.0", "method": "notifications/initialized"})).is_none());
        let list = answer(&dir, &json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"})).unwrap();
        assert_eq!(list["result"]["tools"].as_array().unwrap().len(), 7);

        let add = json!({"jsonrpc": "2.0", "id": 3, "method": "tools/call",
            "params": {"name": "schedule_add", "arguments": {"title": "Gym", "start": "2026-11-20T18:00", "minutes": 60}}});
        assert_eq!(answer(&dir, &add).unwrap()["result"]["isError"], false);
        let listed = call(&dir, "schedule_list", &json!({"from": "2026-11-20T00:00", "to": "2026-11-21T00:00"})).unwrap();
        assert!(listed.contains("Gym") && listed.contains("2026-11-20T19:00"));
        assert!(change_note(&dir).is_file());
        assert!(call(&dir, "schedule_add", &json!({"title": "x", "start": "soon"})).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
