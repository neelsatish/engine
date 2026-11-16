//! One-shot model calls through the Claude Code CLI the user is already signed in to, so no API key is needed.

use serde_json::{json, Value};
use std::{
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

/// Asks Claude once, with no tools and none of the user's Claude Code customizations (`--safe-mode`).
/// With `schema` it returns the structured output; otherwise the reply text as a JSON string.
pub fn ask(cwd: &Path, model: &str, system: &str, prompt: &str, schema: Option<&str>) -> Result<Value, String> {
    let mut cmd = Command::new("claude");
    cmd.current_dir(cwd)
        .args(["-p", "--safe-mode", "--no-session-persistence", "--output-format", "json", "--tools", ""])
        .args(["--model", model, "--system-prompt", system])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(schema) = schema {
        cmd.args(["--json-schema", schema]);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }

    let mut child = cmd
        .spawn()
        .map_err(|e| format!("Couldn't start Claude Code ({e}). Is it installed and on PATH?"))?;
    child
        .stdin
        .take()
        .expect("stdin is piped")
        .write_all(prompt.as_bytes())
        .map_err(|e| format!("Couldn't talk to Claude Code: {e}"))?;
    // shortcut: no timeout, so a hung CLI leaves its item running until Engine restarts. Add one if that shows up.
    let out = child.wait_with_output().map_err(|e| format!("Claude Code stopped: {e}"))?;
    parse(&String::from_utf8_lossy(&out.stdout), schema.is_some()).map_err(|e| {
        let stderr = String::from_utf8_lossy(&out.stderr);
        stderr.lines().rev().find(|l| !l.trim().is_empty()).map_or(e, |l| l.trim().to_string())
    })
}

/// The local model, run by Ollama on this PC. It loads on first use and unloads after Ollama's idle timeout.
pub const LOCAL_MODEL: &str = "qwen3:8b";
const OLLAMA: &str = "http://127.0.0.1:11434";

/// Engine's own model calls (sorting, answers, audits, reviews). With Local AI on they run on the local model,
/// falling back to Claude if it fails; with it off they use `model` through Claude Code.
pub fn ask_any(cwd: &Path, model: &str, system: &str, prompt: &str, schema: Option<&str>, local: bool) -> Result<Value, String> {
    if local {
        ask_local(system, prompt, schema).or_else(|_| ask(cwd, model, system, prompt, schema))
    } else {
        ask(cwd, model, system, prompt, schema)
    }
}

fn no_window(cmd: &mut Command) -> &mut Command {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    cmd
}

/// Ollama's program: its usual install folder, else whatever `ollama` is on the PATH.
fn ollama_exe() -> PathBuf {
    let installed = std::env::var_os("LOCALAPPDATA")
        .map(|d| PathBuf::from(d).join("Programs").join("Ollama").join("ollama.exe"))
        .filter(|p| p.is_file());
    installed.unwrap_or_else(|| PathBuf::from("ollama"))
}

fn ollama_up() -> bool {
    ureq::get(&format!("{OLLAMA}/api/version")).timeout(std::time::Duration::from_secs(1)).call().is_ok()
}

/// Turns Local AI on: starts Ollama if it isn't running and checks the model is there. The model itself loads on
/// first use (or warm-up) and unloads after Ollama's idle timeout.
pub fn start_local() -> Result<(), String> {
    if !ollama_up() {
        no_window(Command::new(ollama_exe()).arg("serve"))
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| format!("Couldn't start Ollama ({e}). Is it installed?"))?;
        let ready = (0..40).any(|_| {
            std::thread::sleep(std::time::Duration::from_millis(500));
            ollama_up()
        });
        if !ready {
            return Err("Ollama didn't start.".into());
        }
    }
    if local_ready() { Ok(()) } else { Err("Ollama is running, but Qwen3 8B hasn't finished downloading yet.".into()) }
}

/// Loads the model ahead of the first question, kept for 10 idle minutes.
pub fn warm_local() {
    let Some(model) = local_model() else { return };
    let body = json!({"model": model, "prompt": "", "keep_alive": "10m"});
    let _ = ureq::post(&format!("{OLLAMA}/api/generate")).timeout(std::time::Duration::from_secs(120)).send_json(body);
}

/// Turns Local AI off: unloads the model from the GPU, then stops Ollama so it uses nothing.
pub fn stop_local() {
    if let Some(model) = local_model() {
        let body = json!({"model": model, "keep_alive": 0});
        let _ = ureq::post(&format!("{OLLAMA}/api/generate")).timeout(std::time::Duration::from_secs(10)).send_json(body);
    }
    for image in ["ollama app.exe", "ollama.exe"] {
        let _ = no_window(Command::new("taskkill").args(["/IM", image, "/F", "/T"])).output();
    }
}

/// Whether Local AI can be used right now: Ollama is installed, whether it's running, and whether it has the model.
pub fn local_status() -> Value {
    let installed = ollama_exe().is_file() || Command::new("where").arg("ollama").output().is_ok_and(|o| o.status.success());
    let running = ollama_up();
    json!({"installed": installed, "running": running, "model": running && local_ready()})
}

/// Whether Ollama is running and has the local model.
pub fn local_ready() -> bool {
    local_model().is_some()
}

/// The name Ollama knows the local model by, if it has it.
fn local_model() -> Option<String> {
    let tags = ureq::get(&format!("{OLLAMA}/api/tags")).timeout(std::time::Duration::from_secs(2)).call().ok()?;
    let names: Vec<String> = tags.into_json::<Value>().ok()?["models"]
        .as_array()?
        .iter()
        .filter_map(|m| m["name"].as_str().map(str::to_string))
        .collect();
    pick_local(&names)
}

/// Prefers `qwen3:8b` from Ollama's library, else any Qwen3 8B build, such as the Hugging Face one
/// (`hf.co/Qwen/Qwen3-8B-GGUF:Q4_K_M`).
fn pick_local(names: &[String]) -> Option<String> {
    names
        .iter()
        .find(|n| n.starts_with(LOCAL_MODEL))
        .or_else(|| names.iter().find(|n| n.to_lowercase().contains("qwen3-8b")))
        .cloned()
}

/// One reply from the local model. With `schema`, Ollama constrains the reply to it and it's parsed as JSON.
pub fn ask_local(system: &str, prompt: &str, schema: Option<&str>) -> Result<Value, String> {
    let model = local_model().ok_or("Qwen3 8B isn't in Ollama yet.")?;
    let mut body = json!({
        "model": model,
        "stream": false,
        "think": false,
        "options": {"temperature": 0.2},
        "messages": [{"role": "system", "content": system}, {"role": "user", "content": prompt}],
    });
    if let Some(schema) = schema {
        body["format"] = serde_json::from_str(schema).map_err(|e| e.to_string())?;
    }
    let reply: Value = ureq::post(&format!("{OLLAMA}/api/chat"))
        .timeout(std::time::Duration::from_secs(180))
        .send_json(body)
        .map_err(|e| format!("The local model isn't available ({e}). Is Ollama running?"))?
        .into_json()
        .map_err(|e| format!("Couldn't read the local model's reply: {e}"))?;
    let text = reply["message"]["content"].as_str().unwrap_or_default().trim().to_string();
    if schema.is_some() {
        serde_json::from_str(&text).map_err(|e| format!("The local model's reply wasn't the expected shape: {e}"))
    } else {
        Ok(Value::String(text))
    }
}

/// One thing agents can use: a claude.ai connector, a plugin's server, or a local MCP server.
#[derive(Debug, PartialEq, serde::Serialize)]
pub struct Connection {
    pub name: String,
    /// "connector", "plugin" or "local".
    pub kind: String,
    /// "connected", "needs auth" or "failed".
    pub status: String,
}

/// Lists what Claude Code agents can connect to (`claude mcp list`, which also checks each one's health).
pub fn connections(cwd: &Path) -> Result<Vec<Connection>, String> {
    let mut cmd = Command::new("claude");
    cmd.current_dir(cwd).args(["mcp", "list"]);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    let out = cmd.output().map_err(|e| format!("Couldn't run Claude Code ({e})."))?;
    Ok(parse_connections(&String::from_utf8_lossy(&out.stdout)))
}

pub fn parse_connections(text: &str) -> Vec<Connection> {
    text.lines()
        .filter_map(|line| {
            let (name, rest) = line.split_once(": ")?;
            let (_, state) = rest.rsplit_once(" - ")?;
            let status = if state.contains("Connected") {
                "connected"
            } else if state.to_lowercase().contains("auth") {
                "needs auth"
            } else {
                "failed"
            };
            let (kind, name) = if let Some(n) = name.strip_prefix("claude.ai ") {
                ("connector", n.to_string())
            } else if let Some(n) = name.strip_prefix("plugin:") {
                ("plugin", n.rsplit(':').next().unwrap_or(n).to_string())
            } else {
                ("local", name.to_string())
            };
            Some(Connection { name, kind: kind.into(), status: status.into() })
        })
        .collect()
}

/// Reads the `--output-format json` result line.
fn parse(stdout: &str, structured: bool) -> Result<Value, String> {
    let line = stdout
        .lines()
        .rev()
        .find(|l| l.trim_start().starts_with('{'))
        .ok_or("Claude Code gave no reply.")?;
    let v: Value = serde_json::from_str(line).map_err(|e| format!("Couldn't read Claude Code's reply: {e}"))?;
    if v["is_error"] != Value::Bool(false) {
        return Err(v["result"].as_str().unwrap_or("Claude Code reported an error.").to_string());
    }
    match &v[if structured { "structured_output" } else { "result" }] {
        Value::Null => Err("Claude Code's reply was empty.".into()),
        out => Ok(out.clone()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Asks the real local model through Ollama. Run with `cargo test -- --ignored` once Ollama has qwen3:8b.
    #[test]
    #[ignore]
    fn local_model_answers_in_shape() {
        assert!(local_ready(), "Ollama isn't running or doesn't have {LOCAL_MODEL}");
        let schema = r#"{"type":"object","properties":{"kind":{"type":"string","enum":["answer","job"]}},"required":["kind"]}"#;
        let out = ask_local("Classify the request.", "What is 2 + 2?", Some(schema)).unwrap();
        assert!(matches!(out["kind"].as_str(), Some("answer" | "job")), "{out}");
    }

    #[test]
    fn finds_the_local_model_by_any_name() {
        let names = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(pick_local(&names(&["llama3:8b", "qwen3:8b"])), Some("qwen3:8b".into()));
        let hf = "hf.co/Qwen/Qwen3-8B-GGUF:Q4_K_M";
        assert_eq!(pick_local(&names(&[hf])), Some(hf.into()));
        assert_eq!(pick_local(&names(&[hf, "qwen3:8b"])), Some("qwen3:8b".into()));
        assert_eq!(pick_local(&names(&["qwen3:4b"])), None);
    }

    #[test]
    fn reads_connections() {
        let text = "Checking MCP server health…\n\nclaude.ai Gmail: https://gmailmcp.googleapis.com/mcp/v1 - ✔ Connected\n\
                    plugin:context7:context7: https://mcp.context7.com/mcp (HTTP) - ! Needs authentication\n\
                    blender: uvx mcp-for-blender - ✗ Failed to connect\n";
        let c = parse_connections(text);
        assert_eq!(c.len(), 3);
        assert_eq!(c[0], Connection { name: "Gmail".into(), kind: "connector".into(), status: "connected".into() });
        assert_eq!((c[1].name.as_str(), c[1].status.as_str()), ("context7", "needs auth"));
        assert_eq!((c[2].kind.as_str(), c[2].status.as_str()), ("local", "failed"));
    }

    #[test]
    fn reads_results() {
        let ok = r#"{"type":"result","is_error":false,"result":"hi","structured_output":{"text":"hi"}}"#;
        assert_eq!(parse(ok, false), Ok(json!("hi")));
        assert_eq!(parse(&format!("noise\n{ok}\n"), true), Ok(json!({"text": "hi"})));

        let failed = r#"{"type":"result","is_error":true,"result":"Not logged in"}"#;
        assert_eq!(parse(failed, false), Err("Not logged in".into()));
        assert!(parse("", false).is_err());
        assert!(parse(r#"{"is_error":false}"#, true).is_err());
    }
}
