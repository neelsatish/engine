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
}

impl Default for Voice {
    fn default() -> Self {
        Self {
            model: "small".into(),
            auto_submit: false,
            speak_on_summon: false,
            engine: "windows".into(),
            clip: String::new(),
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
            self.voice.model = "small".into();
        }
        if !["windows", "chatterbox"].contains(&self.voice.engine.as_str()) {
            self.voice.engine = "windows".into();
        }
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
        };
        let saved = messy.save(&path).unwrap();
        let app = Project { name: "My App".into(), folder: "C:\\code\\app".into(), runner: "codex".into() };
        assert_eq!(saved.projects, [app]);
        assert_eq!(saved.knowledge, ["C:\\Notes"]);
        assert_eq!(saved.voice.model, "small"); // unknown models fall back
        assert_eq!(Settings::load(&path).unwrap(), saved);
        assert_eq!(saved.project("my app").map(|p| p.folder.as_str()), Some("C:\\code\\app"));
        assert!(saved.project("other").is_none());

        fs::write(&path, "{ not json").unwrap();
        assert!(Settings::load(&path).is_err());
    }
}
