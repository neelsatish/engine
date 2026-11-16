//! Splits a brain dump into items, each instant, answer, job or outward.

use serde::Deserialize;

#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    /// Open an app, site or folder now.
    Instant,
    /// One model reply, shown in the briefing.
    Answer,
    /// Background agent work.
    Job,
    /// Seen by others or can't be undone: always held for approval.
    Outward,
    /// A reminder at a time, shown as a Windows notification.
    Reminder,
}

impl Kind {
    pub fn as_str(self) -> &'static str {
        match self {
            Kind::Instant => "instant",
            Kind::Answer => "answer",
            Kind::Job => "job",
            Kind::Outward => "outward",
            Kind::Reminder => "reminder",
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
    /// For a reminder: when, as local time "YYYY-MM-DDTHH:MM".
    #[serde(default)]
    pub when: Option<String>,
    /// For an answer the router could give itself (small talk, a quick question): the reply, so no second
    /// model call is needed.
    #[serde(default)]
    pub reply: Option<String>,
}

pub const MODEL: &str = "haiku";
pub const ANSWER_MODEL: &str = "sonnet";

pub const SYSTEM: &str = r#"You sort a person's brain dump for Engine, their desktop assistant. Split it into separate items, in the order given, and classify each one.

Kinds:
- instant: open an app, website or folder on this PC; also media and volume controls (play, pause, next song, volume 30), locking the PC, and playing or searching something on YouTube or Spotify or the web. For those controls, detail is the command as the person said it ("pause the music", "play lofi on spotify"). title: "Open <name>". detail: only the name, without "my" (for example "YouTube" or "Downloads folder"); use one of the person's shortcut names when one fits. url: the https URL when it is a website; leave it out for apps and folders.
- answer: chat, and anything a single written reply can fully deliver with no tools, no web, and none of the person's files or accounts: small talk ("hi", "what's up", "wsp", "how are you", "thanks"), a quick question, an explanation, a plan, ideas, a draft text. detail: the complete question with every fact the person gave, written so it stands alone (use the recent conversation to fill in what "it" or "that" means). reply: when the answer is short and you are sure of it (small talk, a quick fact, a yes or no, a one-line explanation), write the reply itself here, as Engine: friendly, natural and brief, one to three sentences, plain text. Leave reply out for anything longer or that needs care: plans, drafts, detailed explanations.
- job: work that needs tools: the person's files, projects, notes or documents, their email or other accounts, the web, or running code. An AI agent does it on its own: it can search and read the web, read and write files, run commands, and open websites, apps and files when it finishes. detail: complete instructions for that agent, including the person's own words. project: when the job is about one of the person's projects, that project's name exactly as listed; leave it out otherwise.
- reminder: the person wants to be reminded of something at a time ("remind me in 20 minutes to stretch", "remind me at 6pm to call mum"). title: what to remind them of. when: the local time it's due, "YYYY-MM-DDTHH:MM", worked out from Now.
- outward: anything other people will see or that cannot be undone: sending or replying to a message, posting, uploading, publishing, deleting, paying. detail: exactly what would happen.

Rules:
- "Draft" or "prepare" is never outward. Only the actual sending, posting or uploading is. If one request asks for both, make two items.
- Work that ends in opening something ("find the best X and open it") is one job, not a job plus an instant item.
- Work that should only happen after the person does something themselves ("once I've recorded them, write the captions") belongs inside the same job, never as its own item: that agent will ask the person to do their part and hand the rest back as a follow-up.
- Never put a folder or file path in detail unless the person wrote it. You don't know where agents will work, and they know their own folder.
- One item per distinct request. Do not invent, merge or skip requests. A greeting in front of a request ("hey Engine, open Spotify") is ignored; a greeting or small talk on its own is an answer, so Engine replies.
- Only treat something as an app, site or folder when the person clearly asks to open, launch or go to it. Never read slang or abbreviations as app names: "wsp" and "sup" mean "what's up", not WhatsApp.
- title: a short imperative under 60 characters, in the person's language."#;

pub const SCHEMA: &str = r#"{"type":"object","properties":{"items":{"type":"array","items":{"type":"object","properties":{"kind":{"type":"string","enum":["instant","answer","job","outward","reminder"]},"title":{"type":"string"},"detail":{"type":"string"},"url":{"type":"string"},"project":{"type":"string"},"when":{"type":"string"},"reply":{"type":"string"}},"required":["kind","title","detail"]}}},"required":["items"]}"#;

/// The router's message: the time, the person's shortcut and project names, the last few chat exchanges, then
/// their dump.
pub fn prompt(dump: &str, shortcuts: &[String], projects: &[String], now: &str, recent: &str) -> String {
    let projects = if projects.is_empty() { "(none)".to_string() } else { projects.join(", ") };
    let recent = if recent.is_empty() { "(none)" } else { recent };
    format!("Now: {now}\nShortcuts: {}\nProjects: {projects}\n\nRecent conversation:\n{recent}\n\nDump:\n{dump}", shortcuts.join(", "))
}

/// The last few exchanges, oldest first, as "Person: … / Engine: …" lines, each cut short. `pairs` is (said, reply).
pub fn recent(pairs: &[(String, String)]) -> String {
    let cut = |s: &str, n: usize| -> String {
        let s = s.split_whitespace().collect::<Vec<_>>().join(" ");
        match s.char_indices().nth(n) {
            Some((i, _)) => format!("{}…", &s[..i]),
            None => s,
        }
    };
    pairs.iter().map(|(said, reply)| format!("Person: {}\nEngine: {}", cut(said, 300), cut(reply, 400))).collect::<Vec<_>>().join("\n")
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
        "You are Engine, a personal assistant you can also just chat with: warm, natural and to the point. \
         Today is {today}. Answer completely but briefly in text. \
         Write plain text: short lines and \"- \" bullets. No markdown headings, bold, tables or code fences. \
         If the answer is a plan with days and times (a study plan, a schedule), also list each session in schedule: \
         title, start and end as local times \"YYYY-MM-DDTHH:MM\" (pick sensible times, starting from tomorrow unless \
         asked otherwise), and kind (study, deadline, test, recording or other). Leave schedule empty otherwise."
    )
}

pub const ANSWER_SCHEMA: &str = r#"{"type":"object","properties":{"text":{"type":"string"},"schedule":{"type":"array","items":{"type":"object","properties":{"title":{"type":"string"},"start":{"type":"string"},"end":{"type":"string"},"kind":{"type":"string","enum":["study","deadline","test","recording","other"]}},"required":["title","start","end","kind"]}}},"required":["text"]}"#;

/// Reads a structured answer as its text and any plan sessions.
pub fn parse_answer(out: &serde_json::Value) -> (String, Vec<crate::schedule::Block>) {
    let text = out["text"].as_str().or(out.as_str()).unwrap_or_default().trim().to_string();
    let schedule = out.get("schedule").cloned().and_then(|s| serde_json::from_value(s).ok()).unwrap_or_default();
    (text, schedule)
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
        let chat = parse(json!({"items": [{"kind": "answer", "title": "Say hi", "detail": "wsp", "reply": "Not much! You?"}]}));
        assert_eq!(chat.unwrap()[0].reply.as_deref(), Some("Not much! You?"));
    }

    #[test]
    fn recent_chat_is_short_and_in_order() {
        let pairs = vec![("hi".to_string(), "Hey!".to_string()), ("long".to_string(), "x ".repeat(600))];
        let text = recent(&pairs);
        assert!(text.starts_with("Person: hi\nEngine: Hey!\nPerson: long"));
        assert!(text.ends_with('…') && text.len() < 900);
        assert!(prompt("yo", &[], &[], "now", "").contains("Recent conversation:\n(none)"));
    }

    #[test]
    fn parses_answers_with_plans() {
        let (text, plan) = parse_answer(&json!({"text": " Plan: ", "schedule": [
            {"title": "Organic", "start": "2026-11-17T16:00", "end": "2026-11-17T17:00", "kind": "study"}]}));
        assert_eq!((text.as_str(), plan.len()), ("Plan:", 1));
        assert!(parse_answer(&json!({"text": "Hi"})).1.is_empty());
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
                &prompt(dump, &["gmail".into(), "youtube".into()], &["my app".into()], "Monday 2026-11-16T15:00", ""),
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

        // Plain chat is answered, never opened: "wsp" is "what's up", not WhatsApp.
        for chat in ["wsp", "what's the capital of France?", "thanks!"] {
            assert_eq!(route(chat), [Kind::Answer], "{chat}");
        }

        let kinds = route("Reply to Sam's email saying yes to Friday.");
        assert!(kinds.contains(&Kind::Outward), "{kinds:?}");
    }
}
