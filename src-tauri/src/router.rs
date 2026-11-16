//! Splits a brain dump into items, each instant, answer, job or outward.

use serde::Deserialize;

#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    /// Open an app, site or folder now.
    Instant,
    /// One model reply, shown in the briefing.
    Answer,
    /// Background agent work (agents arrive in phase 3).
    Job,
    /// Seen by others or can't be undone: always held for approval.
    Outward,
}

impl Kind {
    pub fn as_str(self) -> &'static str {
        match self {
            Kind::Instant => "instant",
            Kind::Answer => "answer",
            Kind::Job => "job",
            Kind::Outward => "outward",
        }
    }
}

#[derive(Debug, Deserialize, PartialEq)]
pub struct Item {
    pub kind: Kind,
    pub title: String,
    pub detail: String,
    /// For an instant website: its URL, used when no alias or app matches.
    #[serde(default)]
    pub url: Option<String>,
    /// For a job: which of the person's projects it belongs to, by name.
    #[serde(default)]
    pub project: Option<String>,
}

pub const MODEL: &str = "haiku";
pub const ANSWER_MODEL: &str = "sonnet";

pub const SYSTEM: &str = r#"You sort a person's brain dump for Engine, their desktop assistant. Split it into separate items, in the order given, and classify each one.

Kinds:
- instant: open an app, website or folder on this PC. title: "Open <name>". detail: only the name, without "my" (for example "YouTube" or "Downloads folder"); use one of the person's shortcut names when one fits. url: the https URL when it is a website; leave it out for apps and folders.
- answer: something a single written reply can fully deliver with no tools, no web, and none of the person's files or accounts: an explanation, a plan, ideas, a draft text. detail: the complete question with every fact the person gave, written so it stands alone.
- job: work that needs tools: the person's files, projects, notes or documents, their email or other accounts, the web, or running code. An AI agent does it on its own: it can search and read the web, read and write files, run commands, and open websites, apps and files when it finishes. detail: complete instructions for that agent, including the person's own words. project: when the job is about one of the person's projects, that project's name exactly as listed; leave it out otherwise.
- outward: anything other people will see or that cannot be undone: sending or replying to a message, posting, uploading, publishing, deleting, paying. detail: exactly what would happen.

Rules:
- "Draft" or "prepare" is never outward. Only the actual sending, posting or uploading is. If one request asks for both, make two items.
- Work that ends in opening something ("find the best X and open it") is one job, not a job plus an instant item.
- Work that should only happen after the person does something themselves ("once I've recorded them, write the captions") belongs inside the same job, never as its own item: that agent will ask the person to do their part and hand the rest back as a follow-up.
- One item per distinct request. Do not invent, merge or skip requests. Ignore greetings such as "hey Engine".
- title: a short imperative under 60 characters, in the person's language."#;

pub const SCHEMA: &str = r#"{"type":"object","properties":{"items":{"type":"array","items":{"type":"object","properties":{"kind":{"type":"string","enum":["instant","answer","job","outward"]},"title":{"type":"string"},"detail":{"type":"string"},"url":{"type":"string"},"project":{"type":"string"}},"required":["kind","title","detail"]}}},"required":["items"]}"#;

/// The router's message: the person's shortcut and project names, then their dump.
pub fn prompt(dump: &str, shortcuts: &[String], projects: &[String]) -> String {
    let projects = if projects.is_empty() { "(none)".to_string() } else { projects.join(", ") };
    format!("Shortcuts: {}\nProjects: {projects}\n\nDump:\n{dump}", shortcuts.join(", "))
}

pub fn parse(out: serde_json::Value) -> Result<Vec<Item>, String> {
    #[derive(Deserialize)]
    struct Out {
        items: Vec<Item>,
    }
    serde_json::from_value::<Out>(out)
        .map(|o| o.items)
        .map_err(|e| format!("Couldn't read the router's reply: {e}"))
}

pub fn answer_system(today: &str) -> String {
    format!(
        "You are Engine, a personal assistant. Today is {today}. Answer completely but briefly. \
         Write plain text: short lines and \"- \" bullets. No markdown headings, bold, tables or code fences."
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_items() {
        let out = json!({"items": [
            {"kind": "instant", "title": "Open YouTube", "detail": "YouTube", "url": "https://youtube.com"},
            {"kind": "job", "title": "Research trends", "detail": "Research what's trending"}
        ]});
        let items = parse(out).unwrap();
        assert_eq!(items[0].kind, Kind::Instant);
        assert_eq!(items[0].url.as_deref(), Some("https://youtube.com"));
        assert_eq!(items[1].url, None);
        assert!(parse(json!({"items": [{"kind": "other", "title": "x", "detail": "y"}]})).is_err());
    }

    /// Calls the real router through Claude Code. Run with `cargo test -- --ignored`.
    #[test]
    #[ignore]
    fn sorts_reference_dump() {
        let route = |dump: &str| {
            let out = crate::ai::ask(
                &std::env::temp_dir(),
                MODEL,
                SYSTEM,
                &prompt(dump, &["gmail".into(), "youtube".into()], &["my app".into()]),
                Some(SCHEMA),
            );
            let items = parse(out.unwrap()).unwrap();
            println!("{items:#?}");
            items.into_iter().map(|i| i.kind).collect::<Vec<_>>()
        };

        let kinds = route(
            "Hey Engine, open my email and draft replies to anything urgent. Keep going on my app project and tell \
             me what's left this week. Research what's trending in my niche and turn it into a content plan, with a \
             list of what I need to record. I have a test next week: use my notes and the syllabus to plan what to \
             study each day. And open YouTube.",
        );
        assert_eq!(kinds.iter().filter(|k| **k == Kind::Instant).count(), 2, "{kinds:?}");
        assert!(kinds.iter().filter(|k| **k == Kind::Job).count() >= 3, "{kinds:?}");
        assert!(!kinds.contains(&Kind::Outward), "{kinds:?}");

        let kinds = route("Make me a 7-day study plan for a chemistry test on organic reactions.");
        assert_eq!(kinds, [Kind::Answer]);

        let kinds = route("Reply to Sam's email saying yes to Friday.");
        assert!(kinds.contains(&Kind::Outward), "{kinds:?}");
    }
}
