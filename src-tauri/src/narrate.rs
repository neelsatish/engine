//! Engine talking you through its work: a word or two as an agent moves between steps ("Writing", "Sending",
//! "Almost done"), then a short spoken overview when a task is done.

use std::time::{Duration, Instant};

/// Something to say. Progress words are dropped when newer ones replace them; overviews are always said.
#[derive(Debug, PartialEq)]
pub enum Line {
    Word(&'static str),
    Overview(String),
}

/// Every short phrase Engine says, recorded once in the British voice so each plays instantly.
pub const PHRASES: [&str; 9] =
    ["Yes sir?", "On it", "Writing", "Reading", "Searching", "Sending", "Working on it", "Almost done", "Writing to you now"];

/// The word for an agent's step, from its orb process and label (see `jobs::activity`). `None` for steps too
/// small to mention.
pub fn word(process: &str, label: &str) -> Option<&'static str> {
    let label = label.to_lowercase();
    if label == "writing up" {
        return Some("Almost done"); // the agent is writing its final report
    }
    if ["send", "reply", "post", "upload", "publish"].iter().any(|w| label.contains(w)) {
        return Some("Sending");
    }
    match process {
        "writing" => Some("Writing"),
        "viewing" => Some("Reading"),
        "searching" => Some("Searching"),
        "working" => Some("Working on it"),
        _ => None, // thinking happens between every step: saying it would be constant chatter
    }
}

/// Keeps progress words from piling up: a word is said only when it's new, and not within a few seconds of the
/// last thing said.
#[derive(Default)]
pub struct Pacer {
    last: Option<&'static str>,
    at: Option<Instant>,
}

const GAP: Duration = Duration::from_secs(5);

impl Pacer {
    pub fn allow(&mut self, word: &'static str, now: Instant) -> bool {
        if self.last == Some(word) || self.at.is_some_and(|t| now.duration_since(t) < GAP) {
            return false;
        }
        (self.last, self.at) = (Some(word), Some(now));
        true
    }

    /// Something longer was just said: wait a moment before the next word, and let any word come next.
    pub fn spoke(&mut self, now: Instant) {
        (self.last, self.at) = (None, Some(now));
    }
}

/// Text split into sentences for speaking one at a time; very short ones ride along with the next.
pub fn sentences(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        current.push(c);
        // A sentence ends at . ! ? or a line break followed by a space, so "rain.txt" stays whole.
        let ends = matches!(c, '.' | '!' | '?' | '\n') && chars.peek().is_none_or(|n| n.is_whitespace());
        if ends && current.trim().len() >= 16 {
            out.push(current.trim().to_string());
            current.clear();
        }
    }
    if !current.trim().is_empty() {
        match out.last_mut() {
            Some(last) if current.trim().len() < 12 => *last = format!("{last} {}", current.trim()),
            _ => out.push(current.trim().to_string()),
        }
    }
    out
}

/// A report cut down for speaking: no markdown, links or file paths, and only its first sentences.
pub fn overview(report: &str) -> String {
    const LIMIT: usize = 200; // about ten seconds spoken
    let mut out = String::new();
    for line in report.lines() {
        let line = line.trim().trim_start_matches(['-', '*', '#', '>', ' ']).trim();
        let spoken: Vec<&str> = line
            .split_whitespace()
            .filter(|w| !w.contains("://") && !w.contains(":\\") && !w.starts_with('/'))
            .collect();
        let line = spoken.join(" ").replace(['*', '`', '_', '#'], "");
        if line.is_empty() || line.starts_with("Saved in") {
            continue;
        }
        for sentence in line.split_inclusive(['.', '!', '?']) {
            if out.len() + sentence.len() > LIMIT && !out.is_empty() {
                return out.trim().to_string();
            }
            out.push_str(sentence.trim());
            out.push(' ');
        }
        if !out.trim_end().ends_with(['.', '!', '?']) {
            out = format!("{}. ", out.trim_end());
        }
    }
    out.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn steps_become_words() {
        assert_eq!(word("writing", "Writing up"), Some("Almost done"));
        assert_eq!(word("working", "gmail: send message"), Some("Sending"));
        assert_eq!(word("writing", "Writing plan.md"), Some("Writing"));
        assert_eq!(word("viewing", "Reading notes.md"), Some("Reading"));
        assert_eq!(word("thinking", "Thinking"), None);
    }

    #[test]
    fn words_are_paced() {
        let t = Instant::now();
        let mut p = Pacer::default();
        assert!(p.allow("Writing", t));
        assert!(!p.allow("Reading", t + Duration::from_secs(2))); // too soon
        assert!(!p.allow("Writing", t + Duration::from_secs(9))); // not new
        assert!(p.allow("Reading", t + Duration::from_secs(9)));
    }

    #[test]
    fn speaks_a_sentence_at_a_time() {
        let s = sentences("Done: Write a poem. It's saved as rain.txt in the job folder. Ok.");
        assert_eq!(s, vec!["Done: Write a poem.", "It's saved as rain.txt in the job folder. Ok."]);
        assert_eq!(sentences("Hi."), vec!["Hi."]);
    }

    #[test]
    fn overviews_are_short_and_plain() {
        let report = "## Study plan\n- **Monday**: read chapter 3.\n- Tuesday: practice problems at https://x.com/a\n\nSaved in C:\\Users\\me\\jobs\\4";
        assert_eq!(overview(report), "Study plan. Monday: read chapter 3. Tuesday: practice problems at.");
        let long = "One. ".repeat(100);
        assert!(overview(&long).len() <= 200);
    }
}
