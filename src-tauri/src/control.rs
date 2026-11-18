//! Engine changing its own settings, asked in words ("switch to black and white", "use Opus with a Fable advisor")
//! or by an agent through Engine's MCP server. One place, so both read the same values the same way.

use crate::settings::{Settings, CODEX_MODELS, MODELS};

/// Applies one setting change to `s`. Returns what to tell the person, or why it couldn't be done.
pub fn tweak(s: &mut Settings, setting: &str, value: &str) -> Result<String, String> {
    let v = value.trim().to_lowercase();
    let on = |v: &str| matches!(v, "on" | "true" | "yes" | "enable" | "enabled");
    let off = |v: &str| matches!(v, "off" | "false" | "no" | "disable" | "disabled");
    match setting.trim() {
        "theme" => {
            s.theme = if v.contains("mono") || v.contains("black") || v == "b&w" { "mono".into() } else if v.contains("velvet") || v.contains("purple") { "velvet".into() } else { return Err(format!("No theme called “{value}”: velvet or mono.")) };
            Ok(format!("Theme set to {}", if s.theme == "mono" { "black and white" } else { "velvet" }))
        }
        "model" => {
            let v = v.trim_start_matches("codex:").trim_start_matches("claude ").replace(' ', "-");
            s.models.agent = if MODELS.contains(&v.as_str()) {
                v
            } else if let Some(m) = CODEX_MODELS.iter().find(|m| **m == v || m.ends_with(&v)) {
                format!("codex:{m}")
            } else {
                return Err(format!("No model called “{value}”."));
            };
            Ok(format!("Agents now use {}", s.models.agent.trim_start_matches("codex:")))
        }
        "advisor" => {
            s.models.advisor = match v.as_str() {
                "none" | "off" | "" => String::new(),
                "fable" | "opus" => v,
                _ => return Err(format!("No advisor called “{value}”: none, fable or opus.")),
            };
            Ok(if s.models.advisor.is_empty() { "Advisor off".into() } else { format!("Advisor set to {}", s.models.advisor) })
        }
        "effort" => {
            s.models.effort = match v.as_str() {
                "default" | "" | "normal" => String::new(),
                "low" | "medium" | "high" | "xhigh" | "max" => v,
                _ => return Err(format!("No effort level called “{value}”.")),
            };
            Ok(format!("Effort set to {}", if s.models.effort.is_empty() { "default" } else { &s.models.effort }))
        }
        "voice" => {
            s.voice.engine = match v.as_str() {
                "kokoro" | "british" | "chatterbox" => if v == "british" { "kokoro".into() } else { v },
                _ => return Err(format!("No voice called “{value}”: kokoro or chatterbox.")),
            };
            Ok(format!("Voice set to {}", s.voice.engine))
        }
        "wake" | "local_ai" => {
            let flag = if on(&v) { true } else if off(&v) { false } else { return Err(format!("“{value}” isn't on or off.")) };
            if setting.trim() == "wake" {
                s.voice.wake = flag;
                Ok(format!("Wake word {}", if flag { "on" } else { "off" }))
            } else {
                s.local_ai = flag;
                Ok(format!("Local AI {}", if flag { "on" } else { "off" }))
            }
        }
        other => Err(format!("Engine has no setting called “{other}”.")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn changes_settings_from_words() {
        let mut s = Settings::default();
        assert!(tweak(&mut s, "theme", "black and white").is_ok() && s.theme == "mono");
        assert!(tweak(&mut s, "model", "opus").is_ok() && s.models.agent == "opus");
        assert!(tweak(&mut s, "model", "gpt-6.1-sol").is_ok() && s.models.agent == "codex:gpt-6.1-sol");
        assert!(tweak(&mut s, "advisor", "fable").is_ok() && s.models.advisor == "fable");
        assert!(tweak(&mut s, "effort", "max").is_ok() && s.models.effort == "max");
        assert!(tweak(&mut s, "wake", "off").is_ok() && !s.voice.wake);
        assert!(tweak(&mut s, "model", "gpt-9").is_err());
        assert!(tweak(&mut s, "colour", "red").is_err());
    }
}
