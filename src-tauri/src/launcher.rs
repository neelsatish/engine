//! Opening apps, sites and folders: "open Opera", "open Gmail", "hey Engine, launch Spotify".

use serde::{Deserialize, Serialize};
use std::{fs, path::Path, process::Command};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Target {
    pub name: String,
    /// Handed to explorer.exe: a URL, a folder path, or `shell:AppsFolder\<AppID>`.
    pub target: String,
}

#[derive(Debug, PartialEq)]
pub enum Match {
    One(Target),
    Choices(Vec<Target>),
    Nothing,
}

const WAKE: [&str; 3] = ["hey engine", "ok engine", "engine"];
const VERBS: [&str; 4] = ["open up", "open", "launch", "start"];

/// Strips `prefix` from `s` only when it ends on a word boundary, plus any punctuation after it.
fn strip_word<'a>(s: &'a str, prefix: &str) -> Option<&'a str> {
    let rest = s.strip_prefix(prefix)?;
    match rest.chars().next() {
        Some(c) if c.is_alphanumeric() => None,
        _ => Some(rest.trim_start_matches(|c: char| !c.is_alphanumeric())),
    }
}

/// Pulls the name out of an "open X" request. `None` if the text isn't one.
pub fn parse_open(input: &str) -> Option<String> {
    let lower = input.trim().to_lowercase();
    let mut s = lower.as_str();
    if let Some(rest) = WAKE.iter().find_map(|w| strip_word(s, w)) {
        s = rest;
    }
    if let Some(rest) = strip_word(s, "please") {
        s = rest;
    }
    // "open X" at the start, or after a lead-in ("I'll open X", "hello, can you open? Opera").
    let rest = VERBS.iter().find_map(|v| strip_word(s, v)).or_else(|| {
        VERBS
            .iter()
            .filter_map(|v| {
                s.match_indices(v)
                    .filter(|(i, _)| s[..*i].ends_with(|c: char| !c.is_alphanumeric()))
                    .filter_map(|(i, _)| strip_word(&s[i..], v).map(|r| (i, r)))
                    .last()
            })
            .max_by_key(|(i, _)| *i)
            .map(|(_, r)| r)
    })?;
    let rest = strip_word(rest, "my").or_else(|| strip_word(rest, "the")).unwrap_or(rest);
    let name = rest.trim_end_matches(|c: char| !c.is_alphanumeric());
    let name = name.strip_suffix(" please").unwrap_or(name).trim();
    (!name.is_empty()).then(|| name.to_string())
}

/// Pulls the name out of a "close X" request ("close Spotify", "hey Engine, quit Discord", "can you close my
/// Notepad"). Only at the start, after a wake word or "please"/"can you": "close the deal with Sam" mid-sentence is
/// left to the router.
pub fn parse_close(input: &str) -> Option<String> {
    let lower = input.trim().to_lowercase();
    let mut s = lower.as_str();
    if let Some(rest) = WAKE.iter().find_map(|w| strip_word(s, w)) {
        s = rest;
    }
    for lead in ["please", "can you", "could you"] {
        if let Some(rest) = strip_word(s, lead) {
            s = rest;
        }
    }
    let rest = ["close", "quit", "exit"].iter().find_map(|v| strip_word(s, v))?;
    let rest = strip_word(rest, "my").or_else(|| strip_word(rest, "the")).unwrap_or(rest);
    let name = rest.trim_end_matches(|c: char| !c.is_alphanumeric());
    let name = name.strip_suffix(" please").unwrap_or(name).trim();
    (!name.is_empty()).then(|| name.to_string())
}

/// Closes an app's windows gracefully, the same as clicking its X, so it can still ask to save. Matched by process
/// name or product name, never window title (a browser tab called "Claude" mustn't close the browser). Returns
/// what to tell the person.
pub fn close(t: &Target) -> Result<String, String> {
    // Engine itself, Explorer, and the host every Store app window lives in are never closed.
    // No double quotes in here: they don't survive being passed to powershell.exe as an argument.
    let script = r#"
$q = $env:ENGINE_CLOSE
$skip = @($PID, [int]$env:ENGINE_PID)
$plain = { param($s) ([string]$s -replace '[^a-zA-Z0-9]', '').ToLower() }
$found = Get-Process | Where-Object {
  $_.Id -notin $skip -and $_.ProcessName -notin @('explorer', 'ApplicationFrameHost', 'engine', 'powershell') -and (
    ((& $plain $_.ProcessName) -like ('*' + $q + '*')) -or
    ((& $plain $_.Product) -eq $q) -or
    ((& $plain $_.Description) -eq $q))
}
if (-not $found) { 'none'; exit }
$closed = 0
foreach ($p in $found) { if ($p.MainWindowHandle -ne 0 -and $p.CloseMainWindow()) { $closed++ } }
if ($closed -gt 0) { 'closed' } else { 'hidden' }
"#;
    let q = squash(&t.name);
    if q.len() < 3 {
        return Err(format!("“{}” is too short a name to close safely.", t.name));
    }
    let mut cmd = Command::new("powershell.exe");
    cmd.args(["-NoProfile", "-NonInteractive", "-Command", script])
        .env("ENGINE_CLOSE", &q)
        .env("ENGINE_PID", std::process::id().to_string());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    let out = cmd.output().map_err(|e| format!("Couldn't close {}: {e}", t.name))?;
    match String::from_utf8_lossy(&out.stdout).trim() {
        "closed" => Ok(format!("Closed {}", t.name)),
        "hidden" => Ok(format!("{} has no window open (it may be in the tray)", t.name)),
        _ => Err(format!("{} isn't open", t.name)),
    }
}

/// How well `name` matches `query` (both lowercase): 3 exact, 2 starts with, 1 starts a later word, 0 none.
/// Mid-word matches don't count, so "ord" never opens Discord.
fn score(query: &str, name: &str) -> u8 {
    if name == query {
        3
    } else if name.starts_with(query) {
        2
    } else if name
        .match_indices(query)
        .any(|(i, _)| name[..i].ends_with(|c: char| !c.is_alphanumeric()))
    {
        1
    } else {
        0
    }
}

/// Lowercase letters and digits only, so "Chat GPT" (as speech often comes out) names the ChatGPT app.
fn squash(s: &str) -> String {
    s.chars().filter(|c| c.is_alphanumeric()).flat_map(char::to_lowercase).collect()
}

/// The installed app (first) or shortcut whose name is exactly `query`, ignoring case, spaces and punctuation.
pub fn exact(query: &str, aliases: &[Target], apps: &[Target]) -> Option<Target> {
    let q = squash(query);
    let same = |t: &&Target| squash(&t.name) == q;
    apps.iter().find(same).or_else(|| aliases.iter().find(same)).cloned()
}

/// Finds what to open. An exact name wins, an installed app before an alias, so "open Claude" opens the Claude
/// app rather than its website; otherwise the best match, or the tied choices.
pub fn resolve(query: &str, aliases: &[Target], apps: &[Target]) -> Match {
    let q = query.to_lowercase();
    let exact = |t: &&Target| squash(&t.name) == squash(&q);
    if let Some(a) = apps.iter().find(exact).or_else(|| aliases.iter().find(exact)) {
        return Match::One(a.clone());
    }
    let mut ranked: Vec<(u8, &Target)> = aliases
        .iter()
        .chain(apps)
        .filter(|t| !t.name.to_lowercase().contains("uninstall"))
        .map(|t| (score(&q, &t.name.to_lowercase()), t))
        .filter(|(s, _)| *s > 0)
        .collect();
    // Best score first; among equals, the shortest name is the most specific.
    ranked.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.name.len().cmp(&b.1.name.len())));
    let Some(&(top, best)) = ranked.first() else {
        return Match::Nothing;
    };
    let tied: Vec<Target> = ranked
        .iter()
        .take_while(|(s, _)| *s == top)
        .take(5)
        .map(|(_, t)| (*t).clone())
        .collect();
    if top == 3 || tied.len() == 1 {
        Match::One(best.clone())
    } else {
        Match::Choices(tied)
    }
}

#[derive(Deserialize)]
struct StartApp {
    #[serde(rename = "Name")]
    name: String,
    #[serde(rename = "AppID")]
    app_id: String,
}

/// Every app in the Start menu. Get-StartApps lists desktop and Store apps alike, and
/// `shell:AppsFolder\<AppID>` launches both, so one source covers everything.
pub fn index_start_apps() -> Vec<Target> {
    let mut cmd = Command::new("powershell.exe");
    cmd.args([
        "-NoProfile",
        "-NonInteractive",
        "-Command",
        "[Console]::OutputEncoding=[Text.Encoding]::UTF8; Get-StartApps | ConvertTo-Json -Compress",
    ]);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    match cmd.output() {
        Ok(out) => parse_start_apps(&String::from_utf8_lossy(&out.stdout)),
        Err(e) => {
            eprintln!("couldn't list Start menu apps: {e}");
            vec![]
        }
    }
}

fn parse_start_apps(json: &str) -> Vec<Target> {
    // ConvertTo-Json emits a bare object, not an array, when there is exactly one app.
    let apps: Vec<StartApp> = serde_json::from_str(json)
        .or_else(|_| serde_json::from_str::<StartApp>(json).map(|a| vec![a]))
        .unwrap_or_default();
    apps.into_iter()
        .map(|a| Target { name: a.name, target: format!("shell:AppsFolder\\{}", a.app_id) })
        .collect()
}

pub fn launch(target: &str) -> std::io::Result<()> {
    // explorer.exe opens URLs in the default browser, folders in Explorer, and AppsFolder entries as apps.
    Command::new("explorer.exe").arg(target).spawn().map(|_| ())
}

fn default_aliases() -> Vec<Target> {
    [
        ("gmail", "https://mail.google.com"),
        ("youtube", "https://www.youtube.com"),
        ("youtube studio", "https://studio.youtube.com"),
        ("calendar", "https://calendar.google.com"),
        ("drive", "https://drive.google.com"),
        ("chatgpt", "https://chatgpt.com"),
        ("claude", "https://claude.ai"),
        ("github", "https://github.com"),
    ]
    .into_iter()
    .map(|(name, target)| Target { name: name.into(), target: target.into() })
    .collect()
}

/// Reads the user's alias file, creating it with defaults the first time. Read on every request so
/// edits apply without a restart. A broken file is reported, never overwritten.
pub fn load_aliases(path: &Path) -> Result<Vec<Target>, String> {
    match fs::read_to_string(path) {
        Ok(s) => serde_json::from_str(&s).map_err(|e| e.to_string()),
        Err(_) => {
            let defaults = default_aliases();
            if let Some(dir) = path.parent() {
                fs::create_dir_all(dir).map_err(|e| e.to_string())?;
            }
            let json = serde_json::to_string_pretty(&defaults).map_err(|e| e.to_string())?;
            fs::write(path, json).map_err(|e| e.to_string())?;
            Ok(defaults)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(name: &str) -> Target {
        Target { name: name.into(), target: format!("x:{name}") }
    }

    #[test]
    fn parses_open_requests() {
        assert_eq!(parse_open("open Opera").as_deref(), Some("opera"));
        assert_eq!(parse_open("Hey Engine, open my Gmail please.").as_deref(), Some("gmail"));
        assert_eq!(parse_open("hey engine open up youtube!").as_deref(), Some("youtube"));
        assert_eq!(parse_open("please launch the spotify").as_deref(), Some("spotify"));
        assert_eq!(parse_open("open upwork").as_deref(), Some("upwork"));
        assert_eq!(parse_open("opener").as_deref(), None);
        assert_eq!(parse_open("open").as_deref(), None);
        assert_eq!(parse_open("plan my day").as_deref(), None);
        assert_eq!(parse_open("engineering notes").as_deref(), None);
        assert_eq!(parse_open("I'll open ChatGPT.").as_deref(), Some("chatgpt"));
        assert_eq!(parse_open("Hello, can you open? Opera.").as_deref(), Some("opera"));
        assert_eq!(parse_open("reopen the case").as_deref(), None);
    }

    #[test]
    fn parses_close_requests() {
        assert_eq!(parse_close("close Spotify").as_deref(), Some("spotify"));
        assert_eq!(parse_close("Hey Engine, can you quit my Discord please.").as_deref(), Some("discord"));
        assert_eq!(parse_close("exit notepad").as_deref(), Some("notepad"));
        assert_eq!(parse_close("closed the tab").as_deref(), None);
        assert_eq!(parse_close("is the shop close by").as_deref(), None);
        assert_eq!(parse_close("close").as_deref(), None);
    }

    #[test]
    fn resolves_names() {
        let apps = [t("Opera GX Browser"), t("Discord"), t("Microsoft Word"), t("Opera GX Uninstaller")];
        let aliases = [t("gmail")];
        assert_eq!(resolve("gmail", &aliases, &apps), Match::One(t("gmail")));
        assert_eq!(resolve("opera", &aliases, &apps), Match::One(t("Opera GX Browser")));
        assert_eq!(resolve("word", &aliases, &apps), Match::One(t("Microsoft Word")));
        assert_eq!(resolve("ord", &aliases, &apps), Match::Nothing);
        assert_eq!(resolve("opera gx browser", &aliases, &apps), Match::One(t("Opera GX Browser")));
    }

    #[test]
    fn installed_apps_beat_website_aliases() {
        let apps = [t("Claude"), t("ChatGPT")];
        let aliases = [t("claude"), t("chatgpt"), t("gmail")];
        assert_eq!(resolve("claude", &aliases, &apps), Match::One(t("Claude")));
        assert_eq!(resolve("Chat GPT", &aliases, &apps), Match::One(t("ChatGPT")));
        assert_eq!(resolve("gmail", &aliases, &apps), Match::One(t("gmail"))); // no app: the alias still works
    }

    #[test]
    fn ties_become_choices() {
        let apps = [t("Visual Studio Code"), t("Visual Studio 2022")];
        assert_eq!(
            resolve("visual", &[], &apps),
            Match::Choices(vec![t("Visual Studio Code"), t("Visual Studio 2022")])
        );
    }

    /// Checks the real Start menu on this PC. Run with `cargo test -- --ignored`.
    #[test]
    #[ignore]
    fn finds_real_apps() {
        let apps = index_start_apps();
        assert!(apps.len() > 10, "only {} Start menu apps found", apps.len());
        for name in ["opera", "notepad", "settings"] {
            assert!(matches!(resolve(name, &[], &apps), Match::One(_)), "{name} didn't resolve to one app");
        }
        assert_eq!(resolve("gmail", &default_aliases(), &apps), Match::One(default_aliases()[0].clone()));
    }

    #[test]
    fn parses_start_apps_json() {
        let many = r#"[{"Name":"Discord","AppID":"com.squirrel.Discord.Discord"},{"Name":"Notepad","AppID":"Microsoft.WindowsNotepad_8wekyb3d8bbwe!App"}]"#;
        let one = r#"{"Name":"Discord","AppID":"com.squirrel.Discord.Discord"}"#;
        assert_eq!(parse_start_apps(many).len(), 2);
        assert_eq!(parse_start_apps(one)[0].target, r"shell:AppsFolder\com.squirrel.Discord.Discord");
        assert!(parse_start_apps("").is_empty());
    }
}
