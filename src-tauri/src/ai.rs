//! One-shot model calls through the Claude Code CLI the user is already signed in to, so no API key is needed.

use serde_json::Value;
use std::{
    io::Write,
    path::Path,
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
    use serde_json::json;

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
