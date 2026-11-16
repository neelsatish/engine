//! The schedule: time blocks (study sessions, deadlines, tests, recording sessions) and the Obsidian note
//! Engine writes for each plan an agent or answer produces.

use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
};

pub const KINDS: [&str; 6] = ["study", "deadline", "test", "recording", "reminder", "other"];

/// One block. Times are local, as "YYYY-MM-DDTHH:MM", so they sort and compare as plain strings.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(default)]
pub struct Block {
    /// 0 for a block that isn't saved yet.
    pub id: i64,
    pub title: String,
    pub start: String,
    pub end: String,
    pub kind: String,
    /// Path of the Obsidian note for the plan this block came from, if any.
    pub note: String,
}

/// Whether `s` is a local time in Engine's format ("2026-11-17T16:00").
pub fn valid_time(s: &str) -> bool {
    let b = s.as_bytes();
    let digits = |r: std::ops::Range<usize>| b[r].iter().all(u8::is_ascii_digit);
    b.len() == 16
        && b[4] == b'-'
        && b[7] == b'-'
        && b[10] == b'T'
        && b[13] == b':'
        && digits(0..4)
        && digits(5..7)
        && digits(8..10)
        && digits(11..13)
        && digits(14..16)
        && (1..=12).contains(&s[5..7].parse::<u8>().unwrap_or(0))
        && (1..=31).contains(&s[8..10].parse::<u8>().unwrap_or(0))
        && s[11..13].parse::<u8>().unwrap_or(99) < 24
        && s[14..16].parse::<u8>().unwrap_or(99) < 60
}

impl Block {
    /// Trims and checks a block before it's saved: a title, valid times, end after start, a known kind.
    pub fn tidy(mut self) -> Result<Self, String> {
        self.title = self.title.trim().to_string();
        self.start = self.start.trim().chars().take(16).collect();
        self.end = self.end.trim().chars().take(16).collect();
        if self.title.is_empty() {
            return Err("A block needs a title.".into());
        }
        if !valid_time(&self.start) || !valid_time(&self.end) {
            return Err(format!("“{}” has a time Engine can't read.", self.title));
        }
        if self.end <= self.start {
            return Err(format!("“{}” ends before it starts.", self.title));
        }
        if !KINDS.contains(&self.kind.as_str()) {
            self.kind = "other".into();
        }
        Ok(self)
    }
}

/// Writes a plan into `<vault>/Engine/` as Markdown, so it also shows in Obsidian. Returns the note's path.
pub fn write_note(vault: &Path, title: &str, summary: &str, blocks: &[Block]) -> Result<PathBuf, String> {
    let dir = vault.join("Engine");
    fs::create_dir_all(&dir).map_err(|e| format!("Couldn't make the Engine folder in the vault: {e}"))?;
    let day = blocks.iter().map(|b| &b.start[..10]).min().unwrap_or("plan");
    let safe: String = title
        .chars()
        .map(|c| if "<>:\"/\\|?*".contains(c) || c.is_control() { ' ' } else { c })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    let name = format!("{day} {}", safe.chars().take(80).collect::<String>());
    // Never overwrite a note the user may have edited: number a new one instead.
    let mut path = dir.join(format!("{name}.md"));
    let mut n = 2;
    while path.exists() {
        path = dir.join(format!("{name} ({n}).md"));
        n += 1;
    }

    let mut text = format!("# {title}\n\n");
    if !summary.trim().is_empty() {
        text += &format!("{}\n\n", summary.trim());
    }
    text += "## Schedule\n\n";
    for b in blocks {
        text += &format!("- {} {}–{}: {} ({})\n", &b.start[..10], &b.start[11..], &b.end[11..], b.title, b.kind);
    }
    text += "\n_Made by Engine._\n";
    fs::write(&path, text).map_err(|e| format!("Couldn't write the plan note: {e}"))?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn block(title: &str, start: &str, end: &str, kind: &str) -> Block {
        Block { title: title.into(), start: start.into(), end: end.into(), kind: kind.into(), ..Default::default() }
    }

    #[test]
    fn checks_blocks() {
        assert!(valid_time("2026-11-17T16:00"));
        assert!(!valid_time("2026-13-17T16:00"));
        assert!(!valid_time("2026-11-17 16:00"));
        assert!(!valid_time("2026-11-17T24:00"));

        let ok = block(" Faraday ", "2026-11-17T16:00:00", "2026-11-17T17:00", "study").tidy().unwrap();
        assert_eq!((ok.title.as_str(), ok.start.as_str()), ("Faraday", "2026-11-17T16:00"));
        assert_eq!(block("x", "2026-11-17T16:00", "2026-11-17T17:00", "party").tidy().unwrap().kind, "other");
        assert!(block("x", "2026-11-17T17:00", "2026-11-17T16:00", "study").tidy().is_err());
        assert!(block("", "2026-11-17T16:00", "2026-11-17T17:00", "study").tidy().is_err());
    }

    #[test]
    fn writes_plan_notes_without_overwriting() {
        let vault = std::env::temp_dir().join("engine-vault-test");
        let _ = fs::remove_dir_all(&vault);
        let blocks = [
            block("Faraday's law", "2026-11-18T16:00", "2026-11-18T17:00", "study"),
            block("Test", "2026-11-17T09:00", "2026-11-17T10:00", "test"),
        ];
        let first = write_note(&vault, "Physics: 7-day plan", "Focus on EM.", &blocks).unwrap();
        assert_eq!(first.file_name().unwrap(), "2026-11-17 Physics 7-day plan.md");
        let text = fs::read_to_string(&first).unwrap();
        assert!(text.contains("- 2026-11-18 16:00–17:00: Faraday's law (study)"));
        let second = write_note(&vault, "Physics: 7-day plan", "", &blocks).unwrap();
        assert_eq!(second.file_name().unwrap(), "2026-11-17 Physics 7-day plan (2).md");
    }
}
