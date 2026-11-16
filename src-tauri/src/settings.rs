//! The user's settings: projects (named working folders for agents) and knowledge folders (their notes).
//! Kept as JSON next to aliases.json so it's easy to read and back up.

use serde::{Deserialize, Serialize};
use std::{fs, path::Path};

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(default)]
pub struct Settings {
    pub projects: Vec<Project>,
    /// Folders agents may read when a job touches the user's notes. The first is the Obsidian vault.
    pub knowledge: Vec<String>,
    pub voice: Voice,
    pub models: Models,
    /// The folder new jobs work in, chosen in the dump box like a working folder in Claude. Empty: none.
    pub folder: String,
    /// Folders chosen recently, newest first.
    pub recent: Vec<String>,
    /// Local AI power: Engine's own thinking runs on the local model (Qwen3 through Ollama) instead of Haiku.
    pub local_ai: bool,
}

pub const MODELS: [&str; 4] = ["haiku", "sonnet", "opus", "fable"];
/// ChatGPT models for agents, run through Codex. Stored with a "codex:" prefix.
pub const CODEX_MODELS: [&str; 4] = ["gpt-6.1-sol", "gpt-6-astra", "gpt-6.1-terra", "gpt-6-luna"];

/// The Codex model an agent model setting names, if it's a ChatGPT one.
pub fn codex_model(agent: &str) -> Option<&str> {
    agent.strip_prefix("codex:").filter(|m| CODEX_MODELS.contains(m))
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(default)]
pub struct Models {
    /// Background agents (jobs).
    pub agent: String,
    /// Answers and the end-of-day review.
    pub answer: String,
}

impl Default for Models {
    fn default() -> Self {
        Self { agent: "haiku".into(), answer: "sonnet".into() }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(default)]
pub struct Voice {
    /// The Whisper model for speech to text: "small" (fast), "base.en" or "large-v3-turbo" (best, larger download).
    pub model: String,
    /// Send what was heard straight away instead of leaving it in the dump box to check.
    pub auto_submit: bool,
    /// Read the briefing aloud each time Engine is summoned.
    pub speak_on_summon: bool,
    /// "windows" (built-in voice) or "chatterbox" (the user's cloned voice).
    pub engine: String,
    /// The reference clip Chatterbox clones, about 10 seconds of a voice the user owns or may use.
    pub clip: String,
    /// Hold the key under Esc (`) to talk; a tap still types it.
    pub hold_key: bool,
}

impl Default for Voice {
    fn default() -> Self {
        Self {
            model: "large-v3-turbo".into(),
            auto_submit: false,
            speak_on_summon: false,
            engine: "windows".into(),
            clip: String::new(),
            hold_key: true,
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(default)]
pub struct Project {
    /// What the user calls it in a dump: "my app", "the channel".
    pub name: String,
    pub folder: String,
    /// Who works on it: "claude" (Haiku agents with the user's skills and connectors) or "codex".
    pub runner: String,
}

impl Settings {
    pub fn load(path: &Path) -> Result<Self, String> {
        match fs::read_to_string(path) {
            Ok(text) => serde_json::from_str(&text).map_err(|e| format!("settings.json has a mistake in it: {e}")),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(format!("Couldn't read settings.json: {e}")),
        }
    }

    /// Saves after tidying: trims names and paths, drops empty or duplicate entries.
    pub fn save(mut self, path: &Path) -> Result<Self, String> {
        let mut seen = std::collections::HashSet::new();
        self.projects.retain_mut(|p| {
            p.name = p.name.trim().to_string();
            p.folder = p.folder.trim().to_string();
            if p.runner != "codex" {
                p.runner = "claude".into();
            }
            !p.name.is_empty() && !p.folder.is_empty() && seen.insert(p.name.to_lowercase())
        });
        if !["small", "base.en", "large-v3-turbo"].contains(&self.voice.model.as_str()) {
            self.voice.model = "large-v3-turbo".into();
        }
        if !["windows", "chatterbox"].contains(&self.voice.engine.as_str()) {
            self.voice.engine = "windows".into();
        }
        let defaults = Models::default();
        if !MODELS.contains(&self.models.agent.as_str()) && codex_model(&self.models.agent).is_none() {
            self.models.agent = defaults.agent;
        }
        if !MODELS.contains(&self.models.answer.as_str()) {
            self.models.answer = defaults.answer;
        }
        // The current folder leads the recent list; six is plenty.
        self.folder = self.folder.trim().to_string();
        if !self.folder.is_empty() {
            self.recent.insert(0, self.folder.clone());
        }
        let mut seen = std::collections::HashSet::new();
        self.recent.retain(|f| !f.trim().is_empty() && seen.insert(f.trim().to_lowercase()));
        self.recent.truncate(6);
        let mut seen = std::collections::HashSet::new();
        self.knowledge.retain_mut(|k| {
            *k = k.trim().to_string();
            !k.is_empty() && seen.insert(k.to_lowercase())
        });
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        let text = serde_json::to_string_pretty(&self).map_err(|e| e.to_string())?;
        fs::write(path, text).map_err(|e| format!("Couldn't save settings: {e}"))?;
        Ok(self)
    }

    /// The project a routed job named, matched without case.
    pub fn project(&self, name: &str) -> Option<&Project> {
        let name = name.trim();
        self.projects.iter().find(|p| p.name.eq_ignore_ascii_case(name))
    }

    /// Knowledge folders that exist right now.
    pub fn knowledge_dirs(&self) -> Vec<String> {
        self.knowledge.iter().filter(|k| Path::new(k).is_dir()).cloned().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saves_tidy_and_loads() {
        let path = std::env::temp_dir().join("engine-settings-test").join("settings.json");
        let _ = fs::remove_file(&path);
        assert_eq!(Settings::load(&path).unwrap(), Settings::default());

        let messy = Settings {
            projects: vec![
                Project { name: " My App ".into(), folder: " C:\\code\\app ".into(), runner: "codex".into() },
                Project { name: "my app".into(), folder: "C:\\dupe".into(), runner: String::new() },
                Project { name: "".into(), folder: "C:\\x".into(), runner: String::new() },
            ],
            knowledge: vec!["C:\\Notes".into(), " c:\\notes ".into(), "".into()],
            voice: Voice { model: "huge".into(), ..Voice::default() },
            local_ai: true,
            models: Models { agent: "opus".into(), answer: "gpt".into() },
            folder: "C:\\work".into(),
            recent: vec!["C:\\old".into(), "c:\\WORK".into()],
        };
        let saved = messy.save(&path).unwrap();
        let app = Project { name: "My App".into(), folder: "C:\\code\\app".into(), runner: "codex".into() };
        assert_eq!(saved.projects, [app]);
        assert_eq!(saved.knowledge, ["C:\\Notes"]);
        assert_eq!(saved.voice.model, "large-v3-turbo"); // unknown models fall back
        assert_eq!((saved.models.agent.as_str(), saved.models.answer.as_str()), ("opus", "sonnet"));
        assert_eq!(codex_model("codex:gpt-6.1-sol"), Some("gpt-6.1-sol"));
        assert_eq!(codex_model("codex:gpt-unknown"), None);
        assert_eq!(codex_model("opus"), None);
        assert_eq!(saved.recent, ["C:\\work", "C:\\old"]); // current first, no case-duplicates
        assert_eq!(Settings::load(&path).unwrap(), saved);
        assert_eq!(saved.project("my app").map(|p| p.folder.as_str()), Some("C:\\code\\app"));
        assert!(saved.project("other").is_none());

        fs::write(&path, "{ not json").unwrap();
        assert!(Settings::load(&path).is_err());
    }
}
