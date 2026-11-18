pub mod ai;
pub mod control;
pub mod jobs;
pub mod launcher;
pub mod mcp;
pub mod narrate;
pub mod ptt;
pub mod quick;
pub mod router;
pub mod schedule;
pub mod settings;
pub mod store;
pub mod sysload;
pub mod voice;
pub mod wake;
pub mod screen;
pub mod winctl;

use launcher::{Match, Target};
use router::Kind;
use serde::Serialize;
use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    sync::{Mutex, MutexGuard, OnceLock},
};
use store::Store;
use tauri::{
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    AppHandle, Emitter, Manager, State, WindowEvent,
};
use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Modifiers, Shortcut, ShortcutState};

struct Engine {
    apps: Mutex<Vec<Target>>,
    aliases_path: PathBuf,
    settings_path: PathBuf,
    store: Mutex<Store>,
    /// Where model calls run, so no project's Claude Code settings apply to them. Jobs get folders under it.
    work_dir: PathBuf,
    /// Running jobs and their agent's process id (0 until it starts).
    jobs: Mutex<HashMap<i64, u32>>,
    /// The microphone, while push-to-talk is held.
    recording: Mutex<Option<voice::Recording>>,
    /// The Whisper/Chatterbox helper, while voice is in use.
    helper: Mutex<Option<voice::Helper>>,
    /// Windows speech in progress, so a new briefing (or Stop) can end it.
    speaking: Mutex<Option<std::process::Child>>,
    /// Acting while the person talks: how many samples have already been acted on, and the loop doing it.
    live: Mutex<Live>,
    /// The last app or site opened, so "close it" and "minimize that" know what "it" is.
    last_opened: Mutex<Option<Target>>,
    load: sysload::Monitor,
}

#[derive(Default)]
struct Live {
    /// Samples of the current recording already transcribed and acted on.
    done: usize,
    on: bool,
    task: Option<std::thread::JoinHandle<()>>,
}

impl Engine {
    /// The Start menu index, built on first use. `refresh` rebuilds it (for apps installed since).
    fn apps(&self, refresh: bool) -> Vec<Target> {
        let mut apps = self.apps.lock().unwrap();
        if refresh || apps.is_empty() {
            *apps = launcher::index_start_apps();
        }
        apps.clone()
    }

    fn aliases(&self) -> Result<Vec<Target>, Reply> {
        launcher::load_aliases(&self.aliases_path).map_err(|e| Reply::Error {
            message: format!("aliases.json has a mistake in it: {e}"),
        })
    }

    /// Matches a name against aliases and apps, re-indexing once if nothing matches.
    fn find(&self, query: &str, aliases: &[Target]) -> Match {
        match launcher::resolve(query, aliases, &self.apps(false)) {
            Match::Nothing => launcher::resolve(query, aliases, &self.apps(true)),
            found => found,
        }
    }

    /// Settings as saved; a broken file counts as empty here (the Settings view reports it).
    fn settings(&self) -> settings::Settings {
        settings::Settings::load(&self.settings_path).unwrap_or_default()
    }

    fn store(&self) -> MutexGuard<'_, Store> {
        self.store.lock().unwrap()
    }
}

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum Reply {
    Opened { name: String },
    Choices { items: Vec<Target> },
    Routed { count: usize },
    Error { message: String },
}

/// Tells the UI to reload the briefing.
fn changed(app: &AppHandle) {
    let _ = app.emit("briefing", ());
}

fn log_err<T>(r: rusqlite::Result<T>) -> Option<T> {
    r.map_err(|e| eprintln!("briefing store: {e}")).ok()
}

/// Opens `t` and records the outcome as an instant item in today's briefing.
fn launch(engine: &Engine, dump: Option<i64>, title: &str, t: &Target) -> Result<(), String> {
    let result = launcher::launch(&t.target).map_err(|e| format!("Couldn't open {}: {e}", t.name));
    if result.is_ok() && !t.target.starts_with("http") {
        *engine.last_opened.lock().unwrap() = Some(t.clone());
    }
    let (status, text) = match &result {
        Ok(()) => ("done", format!("Opened {}", t.name)),
        Err(e) => ("failed", e.clone()),
    };
    log_err(engine.store().add_item(dump, "instant", title, &t.target, status, &text));
    result
}

fn open_reply(app: &AppHandle, dump: Option<i64>, t: Target) -> Reply {
    let reply = match launch(&app.state::<Engine>(), dump, &format!("Open {}", t.name), &t) {
        Ok(()) => Reply::Opened { name: t.name },
        Err(message) => Reply::Error { message },
    };
    changed(app);
    reply
}

/// Runs a quick command (media, volume, YouTube, Spotify, web search, lock) and records it under Now.
fn run_quick(app: &AppHandle, dump: Option<i64>, q: quick::Quick) -> Reply {
    let engine = app.state::<Engine>();
    let spotify_app = engine.apps(false).iter().any(|t| t.name.eq_ignore_ascii_case("spotify"));
    let label = q.label();
    let reply = match q.url(spotify_app) {
        Some(url) => match launch(&engine, dump, &label, &Target { name: label.clone(), target: url }) {
            Ok(()) => Reply::Opened { name: label },
            Err(message) => Reply::Error { message },
        },
        None => {
            let result = quick::press(&q);
            let (status, text) = match &result {
                Ok(()) => ("done", label.clone()),
                Err(e) => ("failed", e.clone()),
            };
            log_err(engine.store().add_item(dump, "instant", &label, "", status, &text));
            match result {
                Ok(()) => Reply::Opened { name: label },
                Err(message) => Reply::Error { message },
            }
        }
    };
    changed(app);
    reply
}

/// Whether a dump talks about the clipboard, so its contents should go along.
fn mentions_clipboard(text: &str) -> bool {
    let t = text.to_lowercase();
    ["clipboard", "what i copied", "i just copied", "i've copied", "the copied", "what i've copied"].iter().any(|w| t.contains(w))
}

fn handle_dump(app: &AppHandle, text: &str) -> Reply {
    let engine = app.state::<Engine>();
    let aliases = match engine.aliases() {
        Ok(a) => a,
        Err(reply) => return reply,
    };
    // Saved first, so the chat shows what was said next to whatever Engine did about it.
    let dump = match engine.store().add_dump(text) {
        Ok(id) => id,
        Err(e) => return Reply::Error { message: format!("Couldn't save the dump: {e}") },
    };
    if quick::stop_all(text) {
        return stop_all(app, dump);
    }
    if quick::hush(text) {
        stop_speech(&engine);
        log_err(engine.store().add_item(Some(dump), "instant", "Stop talking", "", "done", "Quiet now"));
        changed(app);
        return Reply::Opened { name: "Quiet now".into() };
    }
    // An app's name on its own ("Google Chrome", "Claude") opens it, before quick commands can read "google …" as
    // a web search.
    if let Some(t) = named_app(&engine, text, &aliases) {
        return open_reply(app, Some(dump), t);
    }
    if let Some(reply) = quick::small_talk(text) {
        log_err(engine.store().add_item(Some(dump), "answer", text, text, "done", reply));
        changed(app);
        return Reply::Routed { count: 1 };
    }
    if let Some(minutes) = quick::timer(text) {
        let (now, _) = engine.store().now().map_err(|e| e.to_string()).unwrap_or_default();
        let at = engine.store().add_minutes(&now, minutes as i64).unwrap_or_default();
        let end = engine.store().add_minutes(&at, 1).unwrap_or_else(|_| at.clone());
        let label = format!("{minutes} minute timer");
        let block = schedule::Block { title: label.clone(), start: at.clone(), end, kind: "reminder".into(), ..Default::default() };
        let done = match log_err(engine.store().save_block(block)) {
            Some(_) => format!("Timer set: {minutes} min, done at {}", at.get(11..16).unwrap_or(&at)),
            None => "Couldn't set the timer".into(),
        };
        log_err(engine.store().add_item(Some(dump), "instant", &label, "", "done", &done));
        let _ = app.emit("schedule", ());
        changed(app);
        return Reply::Opened { name: done };
    }
    if quick::clear_briefing(text) {
        let n = log_err(engine.store().clear_briefing()).unwrap_or(0);
        let done = format!("Cleared {n} item{} from the briefing", if n == 1 { "" } else { "s" });
        log_err(engine.store().add_item(Some(dump), "instant", "Clear the briefing", "", "done", &done));
        changed(app);
        return Reply::Opened { name: done };
    }
    // "Close X" first: "close the open file" mustn't reach the open fast path below.
    if let Some(query) = launcher::parse_close(text) {
        if let Some(t) = it(&engine, &query).or_else(|| resolve_one(&engine, &query, &aliases)) {
            return close_reply(app, Some(dump), &t);
        }
    }
    // "What's on my screen?": a screenshot (with Engine out of the way) for an agent to look at.
    if screen::asks_about_screen(text) {
        let shot = engine.work_dir.join("screens").join(format!("screen-{dump}.png"));
        let window = app.get_webview_window("main").filter(|w| w.is_visible().unwrap_or(false));
        if let Some(w) = &window {
            let _ = w.hide();
            std::thread::sleep(std::time::Duration::from_millis(250));
        }
        let taken = screen::capture(&shot);
        if let Some(w) = &window {
            let _ = w.show();
        }
        if let Err(e) = taken {
            return Reply::Error { message: e };
        }
        let prompt = format!(
            "The person asked: \"{text}\"\n\nA screenshot of their screen, taken just now, is at {}. Read that image \
             first, then answer them directly and briefly in your summary. Look only; don't change anything.",
            shot.display()
        );
        log_err(engine.store().add_job(Some(dump), "Look at my screen", &prompt, "", "", ""));
        pump(app);
        changed(app);
        return Reply::Routed { count: 1 };
    }
    // Window control: "switch to Discord", "snap Chrome left", "minimize everything".
    if let Some(request) = winctl::parse(text) {
        let shown = match &request {
            winctl::Request::One(_, q) => it(&engine, q).or_else(|| resolve_one(&engine, q, &aliases)).map(|t| t.name).unwrap_or_else(|| q.clone()),
            winctl::Request::MinimizeAll => String::new(),
        };
        let request = match request {
            winctl::Request::One(a, _) => winctl::Request::One(a, shown.clone()),
            all => all,
        };
        let (status, said) = match winctl::act(&request, &shown) {
            Ok(m) => ("done", m),
            Err(e) => ("failed", e),
        };
        log_err(engine.store().add_item(Some(dump), "instant", &said, "", status, &said));
        changed(app);
        return if status == "done" { Reply::Opened { name: said } } else { Reply::Error { message: said } };
    }
    // Fastest path: media, volume, YouTube, Spotify, web search, lock. No model.
    if let Some(q) = quick::parse(text) {
        return run_quick(app, Some(dump), q);
    }
    // Fast path: a plain "open X" skips the model so it feels instant.
    if let Some(query) = launcher::parse_open(text) {
        match engine.find(&query, &aliases) {
            Match::One(t) => return open_reply(app, Some(dump), t),
            Match::Choices(items) => return Reply::Choices { items },
            // Several requests in one, or a site with no alias: the router sorts it out.
            Match::Nothing => {}
        }
    }

    let shortcuts: Vec<String> = aliases.iter().map(|a| a.name.clone()).collect();
    let projects: Vec<String> = engine.settings().projects.into_iter().map(|p| p.name).collect();
    let now = engine.store().now().map(|n| n.1).unwrap_or_default();
    // The last few exchanges, so a follow-up ("and tomorrow?") makes sense, like a normal chat.
    let mut pairs: Vec<(String, String)> = log_err(engine.store().briefing())
        .unwrap_or_default()
        .into_iter()
        .filter(|i| matches!(i.kind.as_str(), "answer" | "job") && i.status == "done" && !i.said.is_empty())
        .map(|i| (i.said, i.result))
        .collect();
    pairs.drain(..pairs.len().saturating_sub(4));
    // Today's and tomorrow's schedule, so "what's on today?" gets real times.
    let schedule = engine
        .store()
        .today_range()
        .ok()
        .and_then(|(from, to)| {
            let after = format!("{}T00:00", engine.store().day_after(&to[..10]).ok()?);
            engine.store().blocks(&from, &after).ok()
        })
        .unwrap_or_default()
        .iter()
        .map(|b| format!("- {} to {}: {} ({})", b.start, b.end.get(11..).unwrap_or(&b.end), b.title, b.kind))
        .collect::<Vec<_>>()
        .join("\n");
    let prompt = router::prompt(text, &shortcuts, &projects, &now, &router::recent(&pairs), &schedule);
    // The clipboard goes straight to the answers and jobs that need it, rather than through the router.
    let clip = mentions_clipboard(text)
        .then(|| {
            use tauri_plugin_clipboard_manager::ClipboardExt;
            app.clipboard().read_text().ok()
        })
        .flatten()
        .filter(|c| !c.trim().is_empty())
        .map(|c| c.chars().take(20_000).collect::<String>());
    // Local AI sorts (about 3x faster than Claude), except anything that might send, post or delete: Claude
    // sorts those, since a misread outward request would become an agent that can act.
    let local = engine.settings().local_ai && !router::sounds_outward(text);
    let items = match ai::ask_any(&engine.work_dir, router::MODEL, router::SYSTEM, &prompt, Some(router::SCHEMA), local)
        .and_then(router::parse)
    {
        Ok(items) => router::tidy(items, text, &now, |t, m| engine.store().add_minutes(t, m).ok()),
        Err(message) => return Reply::Error { message },
    };
    let count = items.len();
    let folder = engine.settings().folder;
    let voice = Voice::for_dump(text);
    for mut item in items {
        if let Some(clip) = clip.as_ref().filter(|_| matches!(item.kind, Kind::Answer | Kind::Job)) {
            item.detail += &format!("\n\nThe person's clipboard contains:\n{clip}");
        }
        start(app, &engine, dump, &aliases, &folder, item, voice);
    }
    pump(app);
    changed(app);
    Reply::Routed { count }
}

/// How much Engine says out loud about a request.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Voice {
    /// "Tell me…", "could you tell me…", "just say…": the answer is spoken, and agents talk through their work.
    Speak,
    /// A question asked without that: a short "Writing to you now", and the answer only in the chat.
    Cue,
    /// Anything else: silent.
    Quiet,
}

impl Voice {
    fn for_dump(text: &str) -> Voice {
        let t = text.trim().to_lowercase();
        let starts = |w: &[&str]| w.iter().any(|p| t.starts_with(p));
        if ["tell me", "just say", "say it", "read it to me", "read me"].iter().any(|p| t.contains(p)) {
            Voice::Speak
        } else if t.ends_with('?')
            || starts(&["what", "who", "why", "how", "when", "where", "which", "is ", "are ", "can ", "could ", "do ", "does ", "should "])
        {
            Voice::Cue
        } else {
            Voice::Quiet
        }
    }

    /// Says an answer as this request wants: in full, as a cue, or not at all.
    fn answer(self, text: &str) {
        match self {
            Voice::Speak => tell(narrate::Line::Overview(narrate::overview(text))),
            Voice::Cue => tell(narrate::Line::Cue("Writing to you now")),
            Voice::Quiet => {}
        }
    }
}

/// Whether the person asked to open, play or go to something, or named the thing itself, so an instant item from
/// the router is really wanted.
fn asked_to_open(said: &str, target: &str) -> bool {
    let s = said.to_lowercase();
    let squash = |t: &str| t.chars().filter(|c| c.is_alphanumeric()).flat_map(char::to_lowercase).collect::<String>();
    let name = squash(target);
    ["open", "launch", "start", "go to", "pull up", "bring up", "show me", "play", "pause", "search", "watch", "volume", "mute", "lock", "next", "skip"]
        .iter()
        .any(|v| s.contains(v))
        || (name.len() >= 3 && squash(&s).contains(&name))
}

/// "It", "that", "this": the app opened last.
fn it(engine: &Engine, query: &str) -> Option<Target> {
    ["it", "that", "this", "that app", "this app", "that one"]
        .contains(&query.trim())
        .then(|| engine.last_opened.lock().unwrap().clone())
        .flatten()
}

/// One app or shortcut for `query`, the way "open X" picks it.
fn resolve_one(engine: &Engine, query: &str, aliases: &[Target]) -> Option<Target> {
    match engine.find(query, aliases) {
        Match::One(t) => Some(t),
        _ => None,
    }
}

/// Closes an app and records it in the chat.
fn close_reply(app: &AppHandle, dump: Option<i64>, t: &Target) -> Reply {
    let engine = app.state::<Engine>();
    let (status, text) = match launcher::close(t) {
        Ok(m) => ("done", m),
        Err(e) => ("failed", e),
    };
    log_err(engine.store().add_item(dump, "instant", &format!("Close {}", t.name), "", status, &text));
    changed(app);
    if status == "done" { Reply::Opened { name: text } } else { Reply::Error { message: text } }
}

/// Acts on one routed item: open it, answer it, or hold it in the briefing.
fn start(app: &AppHandle, engine: &Engine, dump: i64, aliases: &[Target], folder: &str, item: router::Item, voice: Voice) {
    let add = |status: &str, result: &str| {
        log_err(engine.store().add_item(Some(dump), item.kind.as_str(), &item.title, &item.detail, status, result))
    };
    match item.kind {
        Kind::Instant if quick::parse(&item.detail).is_some() => {
            run_quick(app, Some(dump), quick::parse(&item.detail).expect("checked above"));
        }
        Kind::Control => {
            let setting = item.setting.clone().unwrap_or_default();
            let (status, said) = match control(app, &setting, item.value.as_deref().unwrap_or_default()) {
                Ok(m) => ("done", m),
                Err(e) => ("failed", e),
            };
            log_err(engine.store().add_item(Some(dump), "instant", &item.title, "", status, &said));
        }
        Kind::Schedule => {
            let start: String = item.when.clone().unwrap_or_default().chars().take(16).collect();
            if !schedule::valid_time(&start) {
                add("failed", "Couldn't work out when. Try \"gym tomorrow at 6 for an hour\".");
                return;
            }
            let minutes = item.minutes.unwrap_or(60).clamp(5, 24 * 60);
            let end = engine.store().add_minutes(&start, minutes).unwrap_or_else(|_| start.clone());
            let kind = if schedule::KINDS.contains(&item.detail.as_str()) { item.detail.clone() } else { "other".into() };
            let block = schedule::Block { title: item.title.clone(), start: start.clone(), end: end.clone(), kind, ..Default::default() };
            if log_err(engine.store().save_block(block)).is_some() {
                let text = format!("Added to your schedule: {} {}–{}", item.title, start.replace('T', " "), end.get(11..).unwrap_or(&end));
                log_err(engine.store().add_item(Some(dump), "instant", &item.title, "", "done", &text));
                let _ = app.emit("schedule", ());
            }
        }
        Kind::Reminder => {
            let Some(when) = item.when.clone().filter(|w| schedule::valid_time(&w.chars().take(16).collect::<String>())) else {
                add("failed", "Couldn't work out when to remind you. Try \"remind me at 6pm to…\".");
                return;
            };
            let when: String = when.chars().take(16).collect();
            let end = engine.store().add_minutes(&when, 5).unwrap_or_else(|_| when.clone());
            let block = schedule::Block { title: item.title.clone(), start: when.clone(), end, kind: "reminder".into(), ..Default::default() };
            if log_err(engine.store().save_block(block)).is_some() {
                let day = if engine.store().today_range().is_ok_and(|(from, _)| when.starts_with(&from[..10])) { "today" } else { &when[..10] };
                let text = format!("Reminder set for {} {day}: {}", &when[11..], item.title);
                log_err(engine.store().add_item(Some(dump), "instant", &item.title, "", "done", &text));
                let _ = app.emit("schedule", ());
            }
        }
        Kind::Instant if launcher::parse_close(&item.title).is_some() => {
            match resolve_one(engine, &item.detail, aliases) {
                Some(t) => {
                    close_reply(app, Some(dump), &t);
                }
                None => {
                    add("failed", &format!("Couldn't find “{}” to close.", item.detail));
                }
            }
        }
        // The router named something to open, but the person never asked to open anything ("wsp" isn't WhatsApp):
        // answer them instead.
        Kind::Instant if !asked_to_open(&engine.store().dump_text(dump).unwrap_or_default(), &item.detail) => {
            let said = engine.store().dump_text(dump).unwrap_or_default();
            let chat = router::Item { kind: Kind::Answer, title: said.chars().take(60).collect(), detail: said, ..item };
            start(app, engine, dump, aliases, folder, chat, voice);
        }
        Kind::Instant => {
            let site = item.url.clone().filter(|u| u.starts_with("https://") || u.starts_with("http://"));
            match (engine.find(&item.detail, aliases), site) {
                (Match::One(t), _) => {
                    let _ = launch(engine, Some(dump), &item.title, &t);
                }
                (Match::Nothing, Some(url)) => {
                    let _ = launch(engine, Some(dump), &item.title, &Target { name: item.detail.clone(), target: url });
                }
                (Match::Choices(found), _) => {
                    let names: Vec<_> = found.iter().map(|t| t.name.as_str()).collect();
                    add("failed", &format!("Several apps match “{}”: {}. Say which one.", item.detail, names.join(", ")));
                }
                (Match::Nothing, None) => {
                    add("failed", &format!("Couldn't find “{}”. Add it as a shortcut in Settings › General.", item.detail));
                }
            }
        }
        // Small talk and quick questions: the router already wrote the reply, so it shows (and is said) at once.
        Kind::Answer if item.reply.as_deref().is_some_and(|r| !r.trim().is_empty()) => {
            let reply = item.reply.clone().unwrap_or_default().trim().to_string();
            add("done", &reply);
            voice.answer(&reply);
        }
        Kind::Answer => {
            let Some(id) = add("running", "") else { return };
            let (app, question, title) = (app.clone(), item.detail.clone(), item.title.clone());
            if voice == Voice::Cue {
                voice.answer(""); // the cue now, while the answer is written
            }
            std::thread::spawn(move || {
                let engine = app.state::<Engine>();
                let today = engine.store().today().unwrap_or_default();
                let system = router::answer_system(&today);
                let model = engine.settings().models.answer;
                let local = engine.settings().local_ai;
                let reply = ai::ask_any(&engine.work_dir, &model, &system, &question, Some(router::ANSWER_SCHEMA), local);
                let (status, result) = match reply {
                    Ok(out) => {
                        let (text, plan) = router::parse_answer(&out);
                        if voice == Voice::Speak {
                            voice.answer(&text);
                        }
                        let added = add_plan(&app, &title, &text, plan);
                        ("done", if added > 0 { format!("{text}\n\nAdded {added} blocks to your schedule.") } else { text })
                    }
                    Err(e) => ("failed", e),
                };
                log_err(engine.store().finish(id, status, &result));
                changed(&app);
            });
        }
        Kind::Job => {
            let project = item.project.as_deref().unwrap_or_default();
            log_err(engine.store().add_job(Some(dump), &item.title, &item.detail, project, "", folder));
        }
        Kind::Outward => {
            // Nothing outward runs from a dump: an agent drafts it, and the draft comes back as an approval card.
            let project = item.project.as_deref().unwrap_or_default();
            let title = format!("Draft: {}", item.title);
            let prompt = jobs::prepare_prompt(&item.detail);
            log_err(engine.store().add_job(Some(dump), &title, &prompt, project, "", folder));
        }
    }
}

/// Starts queued jobs while fewer than `jobs::MAX_RUNNING` are running.
fn pump(app: &AppHandle) {
    let engine = app.state::<Engine>();
    loop {
        let mut running = engine.jobs.lock().unwrap();
        if running.len() >= jobs::MAX_RUNNING {
            return;
        }
        let Some(job) = log_err(engine.store().claim_job()).flatten() else { return };
        running.insert(job.id, 0);
        drop(running);
        let app = app.clone();
        std::thread::spawn(move || run_job(&app, job));
    }
}

fn run_job(app: &AppHandle, job: store::Job) {
    let engine = app.state::<Engine>();
    let (id, dump) = (job.id, job.dump);
    let dir = engine.work_dir.join("jobs").join(id.to_string());
    let settings = engine.settings();
    // A job for a known project works in that project's folder; anything else gets its own folder.
    let project = settings.project(&job.project);
    // Where it works: the folder chosen in the dump box, else the project's folder, else its own job folder.
    let work = Some(PathBuf::from(&job.folder))
        .filter(|f| !job.folder.is_empty() && f.is_dir())
        .or_else(|| project.map(|p| PathBuf::from(&p.folder)).filter(|f| f.is_dir()))
        .unwrap_or_else(|| dir.clone());
    // Approved actions always go to Claude: the one-call guard and the user's connectors live there.
    let codex_model = settings::codex_model(&settings.models.agent);
    let codex = job.tool.is_empty() && (codex_model.is_some() || project.is_some_and(|p| p.runner == "codex"));
    let notes = settings.knowledge_dirs();
    let marker = dir.join("approved.once");
    let approved = (!job.tool.is_empty()).then(|| (job.tool.as_str(), marker.as_path()));
    changed(app);
    // Agents talk through their work only when asked to ("tell me…").
    let talk = dump.is_some_and(|d| Voice::for_dump(&engine.store().dump_text(d).unwrap_or_default()) == Voice::Speak);
    if talk {
        tell(narrate::Line::Word("On it"));
    }
    let on_spawn = |pid| {
        engine.jobs.lock().unwrap().insert(id, pid);
    };
    let on_activity = |process: &'static str, label: String| {
        if let Some(word) = narrate::word(process, &label).filter(|_| talk) {
            tell(narrate::Line::Word(word));
        }
        log_err(engine.store().set_activity(id, process, &label));
        changed(app);
    };
    let outcome = fs::create_dir_all(&dir).map_err(|e| format!("Couldn't make the job folder: {e}")).and_then(|()| {
        if codex {
            jobs::run_codex(&work, &dir, codex_model, &job.prompt, on_spawn, on_activity)
        } else {
            // An approved action with a ChatGPT agent model still runs on Claude, so Haiku stands in for it.
            let model = if codex_model.is_some() { "haiku" } else { settings.models.agent.as_str() };
            let tune = jobs::Tune { effort: settings.models.effort.clone(), advisor: settings.models.advisor.clone() };
            jobs::run(&work, &dir, &notes, approved, model, &tune, &job.prompt, on_spawn, on_activity)
        }
    });
    let report = match outcome {
        Ok(done) => {
            let aliases = engine.aliases().unwrap_or_default();
            for wanted in &done.open {
                open_for_job(&engine, dump, &work, wanted, &aliases);
            }
            // Point at the job folder only when the agent worked there and left something besides Engine's own files.
            let ours = |name: &str| name == "agent-log.jsonl" || name.starts_with("codex-") || name == "approved.once";
            let made = fs::read_dir(&dir)
                .map_or(0, |d| d.flatten().filter(|f| !ours(&f.file_name().to_string_lossy())).count());
            let saved = if work == dir && made > 0 { format!("\n\nSaved in {}", dir.display()) } else { String::new() };
            let summary = format!("{}{saved}", done.summary);
            for p in &done.proposed {
                log_err(engine.store().add_held("outward", dump, &p.action, &p.details, &p.tool, &job.project, &job.folder));
            }
            for t in &done.needs_you {
                log_err(engine.store().add_held("task", dump, &t.task, &t.follow_up, "", &job.project, &job.folder));
            }
            let title = job_title(&engine, id);
            let added = add_plan(app, &title, &done.summary, done.schedule);
            let summary = if added > 0 { format!("{summary}\n\nAdded {added} blocks to your schedule.") } else { summary };
            log_err(engine.store().finish(id, "done", &summary));
            if talk {
                tell(narrate::Line::Overview(format!("Done: {title}. {}", narrate::overview(&done.summary))));
            }
            Some((summary, done.open))
        }
        Err(e) => {
            log_err(engine.store().finish(id, "failed", &e));
            if talk {
                tell(narrate::Line::Overview(format!("{} didn't finish. It's in your briefing.", job_title(&engine, id))));
            }
            None
        }
    };
    engine.jobs.lock().unwrap().remove(&id);
    changed(app);
    pump(app); // free the slot before the audit, which is quick and doesn't count against the cap

    if let Some((summary, opened)) = report {
        audit(app, id, &job.prompt, &summary, &opened);
    }
}

/// Adds a plan's sessions to the schedule and, when a vault is set, writes the plan as an Obsidian note.
/// Returns how many blocks were added. Blocks with unreadable times are skipped.
fn add_plan(app: &AppHandle, title: &str, summary: &str, plan: Vec<schedule::Block>) -> usize {
    let blocks: Vec<schedule::Block> = plan.into_iter().filter_map(|b| b.tidy().ok()).collect();
    if blocks.is_empty() {
        return 0;
    }
    let engine = app.state::<Engine>();
    let note = engine
        .settings()
        .knowledge_dirs()
        .first()
        .and_then(|vault| schedule::write_note(Path::new(vault), title, summary, &blocks).map_err(|e| eprintln!("{e}")).ok())
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_default();
    let added = blocks
        .into_iter()
        .filter_map(|b| log_err(engine.store().save_block(schedule::Block { note: note.clone(), ..b })))
        .count();
    let _ = app.emit("schedule", ());
    added
}

/// A job's title, for naming its plan note.
fn job_title(engine: &Engine, id: i64) -> String {
    engine.store().title(id).ok().flatten().unwrap_or_else(|| "Plan".into())
}

/// Checks a finished job against its task and records "good" or "concern" with a reason.
fn audit(app: &AppHandle, id: i64, task: &str, report: &str, opened: &[String]) {
    let engine = app.state::<Engine>();
    log_err(engine.store().set_verdict(id, "checking", ""));
    changed(app);
    let prompt = jobs::audit_prompt(task, report, opened);
    let (verdict, reason) = ai::ask_any(&engine.work_dir, jobs::AUDIT_MODEL, jobs::AUDIT_SYSTEM, &prompt, Some(jobs::AUDIT_SCHEMA), engine.settings().local_ai)
        .ok()
        .and_then(|out| jobs::audit_verdict(&out))
        .unwrap_or_else(|| (String::new(), String::new())); // no verdict beats a wrong one
    log_err(engine.store().set_verdict(id, &verdict, &reason));
    changed(app);
}

/// Opens what a finished agent asked for: a web address, a file it made in its folder, or an app or alias by name.
fn open_for_job(engine: &Engine, dump: Option<i64>, dir: &Path, wanted: &str, aliases: &[Target]) {
    let target = if wanted.starts_with("https://") || wanted.starts_with("http://") {
        Some(Target { name: jobs::host(wanted), target: wanted.to_string() })
    } else if let Some(path) = jobs::own_file(dir, wanted) {
        let name = path.file_name().map_or(wanted.to_string(), |f| f.to_string_lossy().into_owned());
        Some(Target { name, target: path.to_string_lossy().into_owned() })
    } else {
        match engine.find(wanted, aliases) {
            Match::One(t) => Some(t),
            _ => None,
        }
    };
    match target {
        Some(t) => {
            let _ = launch(engine, dump, &format!("Open {}", t.name), &t);
        }
        None => {
            let msg = format!("An agent asked to open “{wanted}”, but Engine couldn't find it.");
            log_err(engine.store().add_item(dump, "instant", &format!("Open {wanted}"), wanted, "failed", &msg));
        }
    }
}

#[tauri::command]
async fn submit(text: String, app: AppHandle) -> Reply {
    tauri::async_runtime::spawn_blocking(move || handle_dump(&app, &text))
        .await
        .unwrap_or_else(|e| Reply::Error { message: e.to_string() })
}

#[tauri::command]
fn open_choice(choice: Target, app: AppHandle) -> Reply {
    let engine = app.state::<Engine>();
    let aliases = match engine.aliases() {
        Ok(a) => a,
        Err(reply) => return reply,
    };
    // Only open what Engine itself offered, so the UI can't launch arbitrary targets.
    if aliases.contains(&choice) || engine.apps(false).contains(&choice) {
        open_reply(&app, None, choice)
    } else {
        Reply::Error { message: format!("{} isn't in the app list any more.", choice.name) }
    }
}

#[tauri::command]
fn briefing(engine: State<Engine>) -> Result<Vec<store::Item>, String> {
    engine.store().briefing().map_err(|e| e.to_string())
}

#[tauri::command]
fn dismiss(id: i64, app: AppHandle) -> Result<(), String> {
    app.state::<Engine>().store().dismiss(id).map_err(|e| e.to_string())?;
    changed(&app);
    Ok(())
}

#[tauri::command]
fn cancel(id: i64, app: AppHandle) -> Result<(), String> {
    let engine = app.state::<Engine>();
    if engine.store().cancel(id).map_err(|e| e.to_string())? {
        if let Some(&pid) = engine.jobs.lock().unwrap().get(&id).filter(|&&pid| pid != 0) {
            kill_tree(pid);
        }
    }
    changed(&app);
    Ok(())
}

/// "Stop everything": ends every running agent, drops queued ones, and stops Engine talking.
fn stop_all(app: &AppHandle, dump: i64) -> Reply {
    let engine = app.state::<Engine>();
    stop_speech(&engine);
    let busy: Vec<(i64, String)> = log_err(engine.store().briefing())
        .unwrap_or_default()
        .into_iter()
        .filter(|i| matches!(i.status.as_str(), "queued" | "running") && i.kind != "instant")
        .map(|i| (i.id, i.status))
        .collect();
    for (id, status) in &busy {
        let _ = if status == "queued" { dismiss(*id, app.clone()) } else { cancel(*id, app.clone()) };
    }
    let text = match busy.len() {
        0 => "Nothing was running.".to_string(),
        1 => "Stopped 1 task.".to_string(),
        n => format!("Stopped {n} tasks."),
    };
    log_err(engine.store().add_item(Some(dump), "instant", "Stop everything", "", "done", &text));
    changed(app);
    Reply::Opened { name: text }
}

/// Quit from the tray. With agents still working, Engine asks first (quitting stops them).
fn quit(app: &AppHandle) {
    let running = app.state::<Engine>().jobs.lock().unwrap().len();
    if running == 0 {
        app.exit(0);
    } else {
        show(app);
        let _ = app.emit("confirm-quit", running);
    }
}

/// Stops every running agent, then quits.
#[tauri::command]
fn quit_now(app: AppHandle) {
    let engine = app.state::<Engine>();
    let running: Vec<(i64, u32)> = engine.jobs.lock().unwrap().iter().map(|(&id, &pid)| (id, pid)).collect();
    for (id, pid) in running {
        log_err(engine.store().finish(id, "failed", "Stopped when Engine quit. Retry to run it again."));
        if pid != 0 {
            kill_tree(pid);
        }
    }
    app.exit(0);
}

/// Ends an agent and everything it started.
fn kill_tree(pid: u32) {
    let mut cmd = std::process::Command::new("taskkill");
    cmd.args(["/PID", &pid.to_string(), "/T", "/F"]);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    let _ = cmd.status();
}

#[tauri::command]
fn retry(id: i64, app: AppHandle) -> Result<(), String> {
    app.state::<Engine>().store().retry(id).map_err(|e| e.to_string())?;
    pump(&app);
    changed(&app);
    Ok(())
}

/// Approve an approval card: an agent carries out exactly that action, with that one outward call allowed.
#[tauri::command]
fn approve(id: i64, app: AppHandle) -> Result<(), String> {
    let engine = app.state::<Engine>();
    let held = engine.store().take_held(id, "done").map_err(|e| e.to_string())?;
    if let Some(h) = held.filter(|h| h.kind == "outward") {
        let tool = if h.tool.is_empty() { "*" } else { h.tool.as_str() };
        let prompt = jobs::approved_prompt(&h.title, &h.detail, &h.tool);
        engine.store().add_job(h.dump, &h.title, &prompt, &h.project, tool, &h.folder).map_err(|e| e.to_string())?;
        pump(&app);
    }
    changed(&app);
    Ok(())
}

/// Revise an approval card: an agent redrafts it with the user's note and proposes it again.
#[tauri::command]
fn revise(id: i64, note: String, app: AppHandle) -> Result<(), String> {
    let engine = app.state::<Engine>();
    let held = engine.store().take_held(id, "dismissed").map_err(|e| e.to_string())?;
    if let Some(h) = held.filter(|h| h.kind == "outward") {
        let prompt = jobs::revise_prompt(&h.title, &h.detail, note.trim());
        let title = format!("Revise: {}", h.title);
        engine.store().add_job(h.dump, &title, &prompt, &h.project, "", &h.folder).map_err(|e| e.to_string())?;
        pump(&app);
    }
    changed(&app);
    Ok(())
}

/// The user did a needs-you task. If it had a follow-up, an agent picks it up now.
#[tauri::command]
fn task_done(id: i64, app: AppHandle) -> Result<(), String> {
    let engine = app.state::<Engine>();
    let held = engine.store().take_held(id, "done").map_err(|e| e.to_string())?;
    if let Some(h) = held.filter(|h| h.kind == "task" && !h.detail.trim().is_empty()) {
        let prompt = format!("The user has done this: {}\n\nNow: {}", h.title, h.detail);
        engine.store().add_job(h.dump, &h.detail, &prompt, &h.project, "", &h.folder).map_err(|e| e.to_string())?;
        pump(&app);
    }
    changed(&app);
    Ok(())
}

/// End-of-day review: a short look back at today and ahead to tomorrow, added to Done like an answer.
#[tauri::command]
fn review_day(app: AppHandle) -> Result<(), String> {
    let engine = app.state::<Engine>();
    let items = engine.store().briefing().map_err(|e| e.to_string())?;
    let (from, to) = engine.store().today_range().map_err(|e| e.to_string())?;
    let day_after = format!("{}T00:00", engine.store().day_after(&to[..10]).map_err(|e| e.to_string())?);
    let today = engine.store().blocks(&from, &to).map_err(|e| e.to_string())?;
    let tomorrow = engine.store().blocks(&to, &day_after).map_err(|e| e.to_string())?;
    let id = engine
        .store()
        .add_item(None, "answer", "Review of today", "", "running", "")
        .map_err(|e| e.to_string())?;
    changed(&app);
    let prompt = jobs::review_prompt(&items, &today, &tomorrow);
    std::thread::spawn(move || {
        let engine = app.state::<Engine>();
        let today = engine.store().today().unwrap_or_default();
        let model = engine.settings().models.answer;
        let (status, text) = match ai::ask_any(&engine.work_dir, &model, &jobs::review_system(&today), &prompt, None, engine.settings().local_ai) {
            Ok(reply) => ("done", reply.as_str().unwrap_or_default().trim().to_string()),
            Err(e) => ("failed", e),
        };
        log_err(engine.store().finish(id, status, &text));
        changed(&app);
    });
    Ok(())
}

// ---------- voice ----------

impl Engine {
    fn voice_env(&self) -> PathBuf {
        self.work_dir.join("voice-env")
    }

    /// Runs `f` with the voice helper, starting it if needed.
    fn with_helper<T>(&self, f: impl FnOnce(&mut voice::Helper) -> Result<T, String>) -> Result<T, String> {
        let mut helper = self.helper.lock().unwrap();
        if helper.is_none() {
            *helper = Some(voice::Helper::start(&voice::python(&self.voice_env()), &self.work_dir.join("voice"))?);
        }
        let result = f(helper.as_mut().unwrap());
        if result.as_ref().is_err_and(|e| e.contains("helper stopped")) {
            *helper = None; // start fresh next time
        }
        result
    }
}

fn voice_event(app: &AppHandle, state: &str, message: &str) {
    let _ = app.emit("voice", serde_json::json!({"state": state, "message": message}));
}

fn start_listening(app: &AppHandle) -> Result<(), String> {
    let engine = app.state::<Engine>();
    let mut rec = engine.recording.lock().unwrap();
    if rec.is_some() {
        return Ok(());
    }
    *rec = Some(voice::record()?);
    drop(rec);
    stop_speech(&engine); // talking to Engine cuts off whatever it was saying
    voice::beep(&engine.work_dir.join("voice")); // so you know it's listening without looking
    voice_event(app, "listening", "");
    warm_voice(app);
    let live = engine.settings().voice.live;
    let mut state = engine.live.lock().unwrap();
    *state = Live { done: 0, on: true, task: None };
    let app = app.clone();
    state.task = Some(std::thread::spawn(move || listen_loop(&app, live)));
    Ok(())
}

/// While the mic is open: send the voice level to the UI often, and (when acting live) transcribe about once a
/// second and act on every phrase that's finished, so "open Spotify" happens while the person keeps talking.
fn listen_loop(app: &AppHandle, live: bool) {
    let engine = app.state::<Engine>();
    let model = engine.settings().voice.model;
    let wav = engine.work_dir.join("voice").join("live.wav");
    let mut last_pass = std::time::Instant::now();
    loop {
        std::thread::sleep(std::time::Duration::from_millis(70));
        if !engine.live.lock().unwrap().on {
            break;
        }
        let (level, rate) = match engine.recording.lock().unwrap().as_ref() {
            Some(r) => (r.level(), r.rate),
            None => break,
        };
        let _ = app.emit("voice-level", level);
        // A pass takes ~0.1-0.7 s on a GPU depending on the model; checking this often is what makes it feel instant.
        if !live || last_pass.elapsed() < std::time::Duration::from_millis(200) {
            continue;
        }
        last_pass = std::time::Instant::now();
        let from = engine.live.lock().unwrap().done;
        let Some(audio) = engine.recording.lock().unwrap().as_ref().map(|r| r.since(from)) else { break };
        if audio.len() < rate as usize * 6 / 10 {
            continue; // too little new sound to hold even "pause"
        }
        if voice::write_wav(&wav, &audio, rate).is_err() {
            continue;
        }
        let Ok(segments) = engine.with_helper(|h| h.segments(&wav, &model)) else { continue };
        let (mut phrases, mut until) = voice::finished(&segments, audio.len() as f32 / rate as f32);
        // A complete instant command ("open Claude", "pause") runs the moment it's heard, without waiting for
        // the pause after it.
        if let Some(last) = segments.last().filter(|s| s.1 > until && instant_command(&engine, &s.2)) {
            phrases = segments.iter().filter(|s| s.1 > until).map(|s| s.2.clone()).collect();
            until = last.1;
        }
        if phrases.is_empty() {
            continue;
        }
        engine.live.lock().unwrap().done = from + (until * rate as f32) as usize;
        act_on(app, &phrases.join(" "));
    }
    let _ = fs::remove_file(&wav);
}

/// Shows what was heard and acts on it straight away, like a dump sent from the box.
fn act_on(app: &AppHandle, text: &str) {
    let text = text.trim().to_string();
    if !worth_acting(&text) {
        ptt::log(&format!("ignored: {text}"));
        return;
    }
    ptt::log(&format!("acting on: {text}"));
    let _ = app.emit("heard-part", &text);
    let app = app.clone();
    std::thread::spawn(move || {
        if let Reply::Error { message } = handle_dump(&app, &text) {
            voice_event(&app, "error", &message);
        }
    });
}

/// Drops what an open mic catches that isn't meant for Engine: no words at all, filler, and the phrases Whisper
/// invents from silence ("Thank you."). Everything else counts, so "hello" gets a reply like any chat.
fn worth_acting(text: &str) -> bool {
    const FILLER: [&str; 16] = [
        "thank you", "thanks for watching", "thank you for watching", "you", "bye", "oh", "uh", "um", "hmm", "ah",
        "okay", "ok", "yeah", "so", "and", "open",
    ];
    let words: Vec<String> = text
        .split_whitespace()
        .map(|w| w.trim_matches(|c: char| !c.is_alphanumeric() && c != '\'').to_lowercase())
        .filter(|w| w.chars().any(char::is_alphabetic))
        .collect();
    !words.is_empty() && !FILLER.contains(&words.join(" ").as_str())
}

/// Whether a phrase is something Engine does instantly with no model: a quick command, "stop everything", or
/// opening one exact app or shortcut. Those run the moment they're heard.
fn instant_command(engine: &Engine, text: &str) -> bool {
    if quick::parse(text).is_some() || quick::stop_all(text) || quick::hush(text) || winctl::parse(text).is_some() {
        return true;
    }
    let aliases = engine.aliases().unwrap_or_default();
    launcher::parse_close(text).is_some_and(|q| resolve_one(engine, &q, &aliases).is_some())
        || launcher::parse_open(text).is_some_and(|q| matches!(engine.find(&q, &aliases), Match::One(_)))
        || named_app(engine, text, &aliases).is_some()
}

/// An installed app or shortcut named on its own ("Claude", "Spotify"), so saying a name opens it.
fn named_app(engine: &Engine, text: &str, aliases: &[Target]) -> Option<Target> {
    let name = text.trim().trim_matches(|c: char| !c.is_alphanumeric());
    if name.is_empty() || name.split_whitespace().count() > 3 {
        return None;
    }
    launcher::exact(name, aliases, &engine.apps(false))
}

/// Brings Engine up so its logo visibly listens, even from the tray. No focus steal and no "summon" (which could
/// read the briefing aloud over the person talking).
fn reveal(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.unminimize();
    }
}

/// Whether push-to-talk has the microphone, so the wake word listener should ignore what it hears. Engine talking
/// doesn't count: "Engine, stop" must still be heard over it. (A busy voice helper used to count too, which made
/// the listener go deaf for whole replies and right after "Yes sir?".)
fn voice_busy(engine: &Engine) -> bool {
    engine.recording.lock().unwrap().is_some()
}

/// The wake word listener: while it's on, the microphone stays open and each burst of speech is transcribed.
/// "Engine, open Spotify" acts at once; "Engine" alone answers "Yes sir?" and acts on the next thing said.
fn wake_loop(app: &AppHandle) {
    use std::time::{Duration, Instant};
    const STEP_MS: u32 = 100;
    let engine = app.state::<Engine>();
    let wav = engine.work_dir.join("voice").join("wake.wav");
    let mut mic: Option<voice::Recording> = None;
    let mut gate = wake::Gate::default();
    let mut follow: Option<Instant> = None; // after "Yes sir?": until when the next words count without "Engine"
    let mut warmed = false;
    let mut said = String::new(); // what's been said since "Yes sir?"
    loop {
        std::thread::sleep(Duration::from_millis(STEP_MS as u64));
        if !wake::ENABLED.load(std::sync::atomic::Ordering::Relaxed) {
            mic = None; // closes the microphone
            continue;
        }
        let Some(rec) = &mic else {
            match voice::record() {
                Ok(r) => (mic, gate) = (Some(r), wake::Gate::default()),
                Err(e) => {
                    ptt::log(&format!("wake: {e}"));
                    std::thread::sleep(Duration::from_secs(30));
                }
            }
            continue;
        };
        if !warmed {
            // Make the reply clip (and load Whisper) off this thread, so listening starts straight away.
            warmed = true;
            let app = app.clone();
            std::thread::spawn(move || {
                let engine = app.state::<Engine>();
                let model = engine.settings().voice.model;
                let _ = engine.with_helper(|h| h.call(&serde_json::json!({"cmd": "warm", "model": model})));
            });
        }
        if voice_busy(&engine) {
            rec.clear();
            gate.reset();
            continue;
        }
        let following = follow.is_some_and(|t| Instant::now() < t || gate.in_speech());
        if follow.is_some() && !following {
            follow = None; // nothing more was said: act on whatever came, if anything
            voice_event(app, "idle", "");
            if worth_acting(&said) {
                act_on(app, &std::mem::take(&mut said));
            }
            said.clear();
        }
        let level = rec.level();
        if following {
            let _ = app.emit("voice-level", level);
        }
        let Some((from, to)) = gate.push(level, rec.len(), rec.rate, STEP_MS) else {
            if !gate.in_speech() && rec.len() > rec.rate as usize * 10 {
                rec.clear(); // keep memory flat between bursts
            }
            continue;
        };
        let audio: Vec<f32> = rec.since(from).into_iter().take(to - from).collect();
        rec.clear();
        gate.reset();
        // While Engine is talking, the mic hears Engine too: a burst then only counts as "Engine, stop".
        let over_speech = voice::PLAYING.load(std::sync::atomic::Ordering::Relaxed);
        if following {
            voice_event(app, "transcribing", "");
        }
        let model = engine.settings().voice.model;
        let heard = voice::write_wav(&wav, &audio, rec.rate).and_then(|()| engine.with_helper(|h| h.transcribe(&wav, &model)));
        let _ = fs::remove_file(&wav); // the words are used, the recording isn't kept
        let heard = heard.unwrap_or_default();
        if over_speech {
            // Never a request (it may be Engine's own words), never a follow-up: only an interruption.
            if let Some(rest) = wake::command(&heard) {
                if rest.is_empty() || quick::hush(&rest) || quick::stop_all(&rest) {
                    ptt::log("wake: interrupted");
                    stop_speech(&engine);
                    if quick::stop_all(&rest) {
                        act_on(app, &rest);
                    }
                }
            }
            continue;
        }
        if following {
            // After "Yes sir?", pieces split by a pause ("open…" "Opera") are joined until the request is whole.
            let piece = wake::command(&heard).unwrap_or(heard); // "Engine" said again is fine too
            said = format!("{said} {piece}").trim().to_string();
            if voice::dangling(&said) || !worth_acting(&said) {
                follow = Some(Instant::now() + Duration::from_secs(4));
                voice_event(app, "listening", "");
                continue;
            }
            follow = None;
            voice_event(app, "idle", "");
            act_on(app, &std::mem::take(&mut said));
            continue;
        }
        said.clear();
        let Some(rest) = wake::command(&heard) else { continue }; // talk not meant for Engine: forgotten
        ptt::log(&format!("wake: heard {heard:?}"));
        stop_speech(&engine); // talking to Engine cuts off whatever it was saying
        reveal(app);
        if worth_acting(&rest) && !voice::dangling(&rest) {
            act_on(app, &rest);
            continue;
        }
        said = rest; // "Engine, can you open…": the rest comes after "Yes sir?"
        // Only the ready-made clip: making it now would leave the person waiting in silence.
        let clip = said_clip(&engine, "Yes sir?");
        if clip.is_file() {
            voice::play(&clip);
        }
        rec.clear(); // don't hear our own reply
        gate.reset();
        follow = Some(Instant::now() + Duration::from_secs(8));
        voice_event(app, "listening", "");
    }
}

/// Stops the microphone and transcribes what was said. Empty when nothing was caught. When acting live, only the
/// part not yet acted on is transcribed.
fn stop_listening(app: &AppHandle) -> Result<String, String> {
    let engine = app.state::<Engine>();
    // Stop the live loop first and let any pass in progress finish, so nothing is acted on twice.
    let task = {
        let mut live = engine.live.lock().unwrap();
        live.on = false;
        live.task.take()
    };
    if let Some(task) = task {
        let _ = task.join();
    }
    let done = engine.live.lock().unwrap().done;
    let Some(rec) = engine.recording.lock().unwrap().take() else { return Ok(String::new()) };
    let rest = rec.since(done);
    let rate = rec.rate;
    let wav = engine.work_dir.join("voice").join("heard.wav");
    let _ = rec.finish(&engine.work_dir.join("voice").join("all.wav"));
    let _ = fs::remove_file(engine.work_dir.join("voice").join("all.wav"));
    if rest.len() < rate as usize * 3 / 10 {
        voice_event(app, "idle", "");
        return Ok(String::new());
    }
    voice::write_wav(&wav, &rest, rate)?;
    voice_event(app, "transcribing", "");
    let model = engine.settings().voice.model;
    let text = engine.with_helper(|h| h.transcribe(&wav, &model));
    let _ = fs::remove_file(&wav); // the words are kept, the recording isn't
    voice_event(app, "idle", "");
    text
}

/// Push-to-talk released: transcribe what's left. Acting live, it runs straight away like the rest; otherwise the
/// words go to the dump box (sent at once if auto-submit is on).
fn heard(app: &AppHandle) {
    let voice = app.state::<Engine>().settings().voice;
    let result = stop_listening(app);
    ptt::log(&format!("released: {:?}", result.as_ref().map(|t| t.chars().take(80).collect::<String>())));
    match result {
        Ok(text) if !text.is_empty() && voice.live => act_on(app, &text),
        Ok(text) if !text.is_empty() => {
            show(app);
            let _ = app.emit("heard", serde_json::json!({"text": text, "submit": voice.auto_submit}));
        }
        Ok(_) if voice.live => {}
        Ok(_) => voice_event(app, "error", "Didn't catch that. Hold the key while you speak."),
        Err(e) => voice_event(app, "error", &e),
    }
}

#[tauri::command]
fn voice_start(app: AppHandle) -> Result<(), String> {
    start_listening(&app)
}

#[tauri::command]
async fn voice_stop(app: AppHandle) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || stop_listening(&app)).await.map_err(|e| e.to_string())?
}

/// Reads the briefing aloud in the chosen voice.
#[tauri::command]
async fn speak_briefing(app: AppHandle) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let engine = app.state::<Engine>();
        let items = engine.store().briefing().map_err(|e| e.to_string())?;
        let (from, to) = engine.store().today_range().map_err(|e| e.to_string())?;
        let today = engine.store().blocks(&from, &to).map_err(|e| e.to_string())?;
        speak(&app, &voice::briefing_text(&items, &today))
    })
    .await
    .map_err(|e| e.to_string())?
}

fn speak(app: &AppHandle, text: &str) -> Result<(), String> {
    let engine = app.state::<Engine>();
    stop_speech(&engine);
    let mut v = engine.settings().voice;
    if v.engine != "chatterbox" {
        v.engine = "kokoro".into(); // Kokoro for everything else; Windows' own voice is never used
    }
    // Kokoro or Chatterbox: each sentence is made while the one before plays, so speech starts after the first
    // sentence instead of the whole reply. Playback is here, not in the helper, so talking over it cuts it off.
    use std::sync::atomic::Ordering;
    let turn = SPEECH.fetch_add(1, Ordering::SeqCst) + 1; // a newer reply or a hush ends this one
    let (queue, played) = std::sync::mpsc::channel::<PathBuf>();
    let player = std::thread::spawn(move || {
        for wav in played {
            if SPEECH.load(Ordering::SeqCst) == turn {
                voice::play(&wav);
            }
            let _ = fs::remove_file(&wav);
        }
    });
    for (n, sentence) in narrate::sentences(text).iter().enumerate() {
        if SPEECH.load(Ordering::SeqCst) != turn {
            break;
        }
        let out = engine.work_dir.join("voice").join(format!("spoken-{turn}-{n}.wav"));
        let request = serde_json::json!({"cmd": "say", "text": sentence, "out": out, "engine": v.engine, "clip": v.clip});
        if let Err(e) = engine.with_helper(|h| h.call(&request)) {
            drop(queue);
            let _ = player.join();
            return Err(e);
        }
        let _ = queue.send(out);
    }
    drop(queue);
    let _ = player.join();
    Ok(())
}

/// Counts replies spoken, so stopping or starting a new one ends the one before mid-sentence.
static SPEECH: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

fn stop_speech(engine: &Engine) {
    SPEECH.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    voice::hush();
    if let Some(mut child) = engine.speaking.lock().unwrap().take() {
        let _ = child.kill();
    }
}

static NARRATOR: OnceLock<std::sync::mpsc::Sender<narrate::Line>> = OnceLock::new();

/// Queues something for Engine to say while it works (when "Talk me through tasks" is on).
fn tell(line: narrate::Line) {
    if let Some(n) = NARRATOR.get() {
        let _ = n.send(line);
    }
}

/// Says queued progress words and overviews one at a time, never over the person talking.
fn narrator(app: AppHandle, lines: std::sync::mpsc::Receiver<narrate::Line>) {
    use narrate::Line;
    use std::time::{Duration, Instant};
    let engine = app.state::<Engine>();
    let mut pacer = narrate::Pacer::default();
    while let Ok(first) = lines.recv() {
        let mut batch = vec![first];
        batch.extend(lines.try_iter());
        // "Talk me through tasks" off silences the progress words only: answers asked for aloud and reminders
        // are still said.
        if !engine.settings().voice.narrate {
            batch.retain(|l| !matches!(l, Line::Word(_)));
            if batch.is_empty() {
                continue;
            }
        }
        // Cues first, unpaced: they were asked for.
        for cue in batch.iter().filter_map(|l| if let Line::Cue(c) = l { Some(*c) } else { None }) {
            say_short(&engine, cue);
            pacer.spoke(Instant::now());
        }
        batch.retain(|l| !matches!(l, Line::Cue(_)));
        if batch.is_empty() {
            continue;
        }
        // Wait while the person is talking to Engine (up to a minute), then say what's still worth saying.
        for _ in 0..300 {
            if engine.recording.lock().unwrap().is_none() {
                break;
            }
            std::thread::sleep(Duration::from_millis(200));
        }
        batch.extend(lines.try_iter());
        let overviews: Vec<String> = batch.iter().filter_map(|l| if let Line::Overview(t) = l { Some(t.clone()) } else { None }).collect();
        if overviews.is_empty() {
            // Only the newest progress word still matters.
            let word = batch.iter().rev().find_map(|l| if let Line::Word(w) = l { Some(*w) } else { None });
            if let Some(w) = word.filter(|w| pacer.allow(w, Instant::now())) {
                ptt::log(&format!("said: {w}"));
                say_short(&engine, w);
            }
            continue;
        }
        for text in overviews {
            ptt::log(&format!("said overview: {} chars", text.len()));
            if let Err(e) = speak(&app, &text) {
                eprintln!("narrator: {e}");
            }
            while engine.speaking.lock().unwrap().as_mut().is_some_and(|c| matches!(c.try_wait(), Ok(None))) {
                std::thread::sleep(Duration::from_millis(100));
            }
            pacer.spoke(Instant::now());
        }
    }
}

/// The voice short phrases are recorded in: Chatterbox when it's Engine's voice, otherwise Kokoro's British George.
fn phrase_voice(engine: &Engine) -> (String, String) {
    let v = engine.settings().voice;
    if v.engine == "chatterbox" { ("chatterbox".into(), v.clip) } else { ("kokoro".into(), String::new()) }
}

/// Where a short phrase's cached recording lives ("Yes sir?" in Chatterbox is voice/said/chatterbox/yes-sir.wav).
fn said_clip(engine: &Engine, text: &str) -> PathBuf {
    let name: String = text.to_lowercase().split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).collect::<Vec<_>>().join("-");
    engine.work_dir.join("voice").join("said").join(phrase_voice(engine).0).join(format!("{name}.wav"))
}

/// A short phrase in Engine's voice, made once and kept so it plays at once. None when it can't be made.
fn cached_clip(engine: &Engine, text: &str) -> Option<PathBuf> {
    let clip = said_clip(engine, text);
    if !clip.is_file() {
        let _ = fs::create_dir_all(clip.parent()?);
        let (voice, reference) = phrase_voice(engine);
        let request = serde_json::json!({"cmd": "say", "text": text, "out": clip, "engine": voice, "clip": reference});
        if let Err(e) = engine.with_helper(|h| h.call(&request)) {
            ptt::log(&format!("voice: couldn't make {text:?}: {e}"));
        }
    }
    clip.is_file().then_some(clip)
}

/// Says a short phrase: the cached British clip when there is one, otherwise Windows' voice. Waits until done.
fn say_short(engine: &Engine, text: &str) {
    // Without the voice pack there's no Kokoro: stay quiet rather than fall back to Windows' voice.
    if let Some(clip) = cached_clip(engine, text) {
        voice::play(&clip);
    }
}

#[tauri::command]
fn stop_speaking(engine: State<Engine>) {
    stop_speech(&engine);
}

#[tauri::command]
async fn voice_status(app: AppHandle) -> serde_json::Value {
    tauri::async_runtime::spawn_blocking(move || {
        let engine = app.state::<Engine>();
        let mut status = voice::status(&voice::python(&engine.voice_env()));
        status["own_env"] = serde_json::Value::Bool(engine.voice_env().join("Scripts").join("python.exe").is_file());
        status
    })
    .await
    .unwrap_or_default()
}

/// Installs Engine's own voice environment (Chatterbox and faster-whisper). Started by the user from Settings.
#[tauri::command]
async fn install_voice(app: AppHandle) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let env = app.state::<Engine>().voice_env();
        *app.state::<Engine>().helper.lock().unwrap() = None; // the old helper may be using the old Python
        voice::install(&env, |line| {
            let _ = app.emit("voice-install", line);
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
fn blocks(from: String, to: String, engine: State<Engine>) -> Result<Vec<schedule::Block>, String> {
    engine.store().blocks(&from, &to).map_err(|e| e.to_string())
}

#[tauri::command]
fn save_block(block: schedule::Block, app: AppHandle) -> Result<schedule::Block, String> {
    let saved = app.state::<Engine>().store().save_block(block.tidy()?).map_err(|e| e.to_string())?;
    let _ = app.emit("schedule", ());
    Ok(saved)
}

#[tauri::command]
fn delete_block(id: i64, app: AppHandle) -> Result<(), String> {
    app.state::<Engine>().store().delete_block(id).map_err(|e| e.to_string())?;
    let _ = app.emit("schedule", ());
    Ok(())
}

#[tauri::command]
async fn connections(app: AppHandle) -> Result<Vec<ai::Connection>, String> {
    tauri::async_runtime::spawn_blocking(move || ai::connections(&app.state::<Engine>().work_dir))
        .await
        .map_err(|e| e.to_string())?
}

/// Opens claude.ai's connector settings, where Gmail, Drive and the rest are added.
/// Whether Ollama is installed and running, and whether it has the local model.
#[tauri::command]
async fn local_status() -> serde_json::Value {
    tauri::async_runtime::spawn_blocking(ai::local_status).await.unwrap_or_default()
}

/// The Local AI power button. On: start Ollama, warm the model, and route Engine's own thinking to it.
/// Off: unload the model, stop Ollama, and go back to Haiku.
#[tauri::command]
async fn set_local_ai(on: bool, app: AppHandle) -> Result<bool, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let engine = app.state::<Engine>();
        if on {
            ai::start_local()?;
            std::thread::spawn(ai::warm_local);
        } else {
            ai::stop_local();
        }
        let mut settings = engine.settings();
        settings.local_ai = on;
        let saved = settings.save(&engine.settings_path)?;
        let _ = app.emit("settings", &saved);
        Ok(on)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Engine changing itself, asked in words: clearing the briefing or chat, or a setting (theme, model, advisor,
/// effort, voice, wake word, Local AI). Returns what to tell the person.
fn control(app: &AppHandle, setting: &str, value: &str) -> Result<String, String> {
    let engine = app.state::<Engine>();
    let plural = |n: usize| if n == 1 { "" } else { "s" };
    match setting {
        "clear_briefing" => {
            let n = engine.store().clear_briefing().map_err(|e| e.to_string())?;
            changed(app);
            Ok(format!("Cleared {n} item{} from the briefing", plural(n)))
        }
        "clear_chat" => {
            let n = engine.store().clear_chat().map_err(|e| e.to_string())?;
            changed(app);
            Ok(format!("Cleared {n} message{} from the chat", plural(n)))
        }
        _ => {
            let mut s = engine.settings();
            let was_local = s.local_ai;
            let said = control::tweak(&mut s, setting, value)?;
            if s.local_ai != was_local {
                if s.local_ai {
                    ai::start_local()?;
                    std::thread::spawn(ai::warm_local);
                } else {
                    ai::stop_local();
                }
            }
            save_settings(s, app.clone())?;
            Ok(said)
        }
    }
}

#[tauri::command]
fn manage_connectors() -> Result<(), String> {
    launcher::launch("https://claude.ai/settings/connectors").map_err(|e| e.to_string())
}

/// CPU, RAM and GPU load for the corner readout.
#[tauri::command]
fn system_load(engine: State<Engine>) -> sysload::Load {
    engine.load.read()
}

#[tauri::command]
fn get_settings(engine: State<Engine>) -> Result<settings::Settings, String> {
    settings::Settings::load(&engine.settings_path)
}

#[tauri::command]
fn save_settings(settings: settings::Settings, app: AppHandle) -> Result<settings::Settings, String> {
    let saved = settings.save(&app.state::<Engine>().settings_path)?;
    ptt::ENABLED.store(saved.voice.hold_key, std::sync::atomic::Ordering::Relaxed);
    wake::ENABLED.store(saved.voice.wake, std::sync::atomic::Ordering::Relaxed);
    let _ = app.emit("settings", &saved); // keeps the dump box chips and the Settings view in step
    Ok(saved)
}

#[tauri::command]
fn edit_aliases(engine: State<Engine>) -> Result<(), String> {
    launcher::load_aliases(&engine.aliases_path)?;
    launcher::launch(&engine.aliases_path.to_string_lossy()).map_err(|e| e.to_string())
}

fn show(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
        let _ = w.emit("summon", ());
    }
    warm_voice(app);
}

/// Loads Whisper in the background, so the first thing said after summoning Engine is caught at full speed.
fn warm_voice(app: &AppHandle) {
    let app = app.clone();
    std::thread::spawn(move || {
        let engine = app.state::<Engine>();
        let model = engine.settings().voice.model;
        let _ = engine.with_helper(|h| h.call(&serde_json::json!({"cmd": "warm", "model": model})));
    });
}

fn toggle(app: &AppHandle) {
    let Some(w) = app.get_webview_window("main") else { return };
    if w.is_visible().unwrap_or(false) && w.is_focused().unwrap_or(false) {
        let _ = w.hide();
    } else {
        show(app);
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let summon = Shortcut::new(Some(Modifiers::CONTROL | Modifiers::SHIFT), Code::Space);
    // Hold to talk.
    let talk = Shortcut::new(Some(Modifiers::CONTROL | Modifiers::ALT), Code::Space);

    tauri::Builder::default()
        // Must be first: a second launch just brings the running Engine forward.
        .plugin(tauri_plugin_single_instance::init(|app, _, _| show(app)))
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        // Started at sign-in with --autostart, Engine stays hidden in the tray.
        .plugin(tauri_plugin_autostart::init(tauri_plugin_autostart::MacosLauncher::LaunchAgent, Some(vec!["--autostart"])))
        .plugin(tauri_plugin_process::init())
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(move |app, shortcut, event| {
                    if shortcut == &summon && event.state() == ShortcutState::Pressed {
                        toggle(app);
                    } else if shortcut == &talk {
                        match event.state() {
                            ShortcutState::Pressed => {
                                if let Err(e) = start_listening(app) {
                                    voice_event(app, "error", &e);
                                }
                            }
                            ShortcutState::Released => {
                                let app = app.clone();
                                std::thread::spawn(move || heard(&app));
                            }
                        }
                    }
                })
                .build(),
        )
        .setup(move |app| {
            let aliases_path = app.path().app_config_dir()?.join("aliases.json");
            let settings_path = app.path().app_config_dir()?.join("settings.json");
            let work_dir = app.path().app_data_dir()?;
            fs::create_dir_all(&work_dir)?;
            let store = Mutex::new(Store::open(&work_dir.join("engine.db"))?);
            app.manage(Engine {
                apps: Mutex::new(vec![]),
                aliases_path,
                settings_path,
                store,
                work_dir,
                jobs: Mutex::default(),
                recording: Mutex::default(),
                helper: Mutex::default(),
                speaking: Mutex::default(),
                live: Mutex::default(),
                last_opened: Mutex::default(),
                load: sysload::Monitor::new(),
            });

            // Local AI left on last time: make sure Ollama is running (the model loads on first use).
            if app.state::<Engine>().settings().local_ai {
                std::thread::spawn(|| {
                    match ai::start_local() {
                        Ok(_) => ai::warm_local(), // load the model now: a cold first call takes ~9 s
                        Err(e) => eprintln!("local AI: {e}"),
                    }
                });
            }

            // Hold ` to talk; a tap still types it.
            let _ = fs::create_dir_all(app.state::<Engine>().work_dir.join("voice"));
            ptt::log_to(app.state::<Engine>().work_dir.join("voice").join("ptt.log"));
            ptt::ENABLED.store(app.state::<Engine>().settings().voice.hold_key, std::sync::atomic::Ordering::Relaxed);
            wake::ENABLED.store(app.state::<Engine>().settings().voice.wake, std::sync::atomic::Ordering::Relaxed);
            let waker = app.handle().clone();
            std::thread::spawn(move || wake_loop(&waker));
            let (lines, queue) = std::sync::mpsc::channel();
            let _ = NARRATOR.set(lines);
            let speaker = app.handle().clone();
            std::thread::spawn(move || {
                // Load Chatterbox now when it's Engine's voice (its first reply would otherwise take ~35 s), then
                // record the short phrases once, so none waits later.
                let engine = speaker.state::<Engine>();
                let v = engine.settings().voice;
                if v.engine == "chatterbox" {
                    let warm = serde_json::json!({"cmd": "warm_tts", "clip": v.clip});
                    if let Err(e) = engine.with_helper(|h| h.call(&warm)) {
                        ptt::log(&format!("voice: couldn't load Chatterbox: {e}"));
                    }
                }
                if v.wake || v.narrate {
                    for phrase in narrate::PHRASES {
                        cached_clip(&engine, phrase);
                    }
                }
                narrator(speaker.clone(), queue)
            });
            let (on_hold, on_release) = (app.handle().clone(), app.handle().clone());
            ptt::install(
                move || match start_listening(&on_hold) {
                    Ok(()) => reveal(&on_hold),
                    Err(e) => {
                        ptt::log(&format!("listen error: {e}"));
                        voice_event(&on_hold, "error", &e);
                    }
                },
                move || {
                    let app = on_release.clone();
                    std::thread::spawn(move || heard(&app));
                },
            );

            // An agent changed something through Engine's MCP server: reload the briefing, schedule and settings.
            let watcher = app.handle().clone();
            std::thread::spawn(move || {
                let note = mcp::change_note(&watcher.state::<Engine>().work_dir);
                loop {
                    std::thread::sleep(std::time::Duration::from_secs(2));
                    if fs::remove_file(&note).is_ok() {
                        let engine = watcher.state::<Engine>();
                        let saved = engine.settings();
                        wake::ENABLED.store(saved.voice.wake, std::sync::atomic::Ordering::Relaxed);
                        let _ = watcher.emit("settings", &saved);
                        let _ = watcher.emit("schedule", ());
                        changed(&watcher);
                    }
                }
            });

            // Due reminders become Windows notifications.
            let ticker = app.handle().clone();
            std::thread::spawn(move || loop {
                std::thread::sleep(std::time::Duration::from_secs(15));
                let engine = ticker.state::<Engine>();
                let Ok((now, _)) = engine.store().now() else { continue };
                for r in log_err(engine.store().take_due_reminders(&now)).unwrap_or_default() {
                    use tauri_plugin_notification::NotificationExt;
                    let _ = ticker.notification().builder().title("Engine reminder").body(&r.title).show();
                    let _ = ticker.emit("schedule", ());
                    // Spoken too, since the person set it: "Sir, your 5 minute timer is done."
                    let line = if r.title.ends_with("timer") { format!("Sir, your {} is done.", r.title) } else { format!("Sir, a reminder: {}.", r.title) };
                    tell(narrate::Line::Overview(line));
                }
            });

            // Free the voice models after a few idle minutes, so Engine doesn't hold memory while unused.
            let reaper = app.handle().clone();
            std::thread::spawn(move || loop {
                std::thread::sleep(std::time::Duration::from_secs(60));
                let engine = reaper.state::<Engine>();
                let mut helper = engine.helper.lock().unwrap();
                // Listening for the wake word needs Whisper ready at any moment, so it stays loaded then.
                let idle = helper.as_ref().is_some_and(|h| h.last_used.elapsed() > voice::IDLE);
                if idle && !wake::ENABLED.load(std::sync::atomic::Ordering::Relaxed) {
                    *helper = None;
                }
            });
            pump(app.handle()); // jobs queued before Engine last closed

            if !std::env::args().any(|a| a == "--autostart") {
                show(app.handle());
            }

            // Build the app index in the background so the first "open X" is instant.
            let handle = app.handle().clone();
            std::thread::spawn(move || {
                handle.state::<Engine>().apps(false);
            });

            if let Err(e) = app.global_shortcut().register(summon) {
                eprintln!("couldn't register Ctrl+Shift+Space: {e}");
            }
            if let Err(e) = app.global_shortcut().register(talk) {
                eprintln!("couldn't register Ctrl+Alt+Space: {e}");
            }

            let show_item = MenuItem::with_id(app, "show", "Show Engine", true, None::<&str>)?;
            let quit_item = MenuItem::with_id(app, "quit", "Quit Engine", true, None::<&str>)?;
            TrayIconBuilder::new()
                .icon(app.default_window_icon().unwrap().clone())
                .tooltip("Engine")
                .menu(&Menu::with_items(app, &[&show_item, &quit_item])?)
                .show_menu_on_left_click(false)
                .on_menu_event(|app, e| match e.id.as_ref() {
                    "show" => show(app),
                    "quit" => quit(app),
                    _ => {}
                })
                .on_tray_icon_event(|tray, e| {
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    } = e
                    {
                        toggle(tray.app_handle());
                    }
                })
                .build(app)?;
            Ok(())
        })
        // Closing the window hides it to the tray; Quit lives in the tray menu.
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .invoke_handler(tauri::generate_handler![
            submit,
            open_choice,
            briefing,
            dismiss,
            cancel,
            retry,
            approve,
            revise,
            task_done,
            review_day,
            quit_now,
            voice_start,
            voice_stop,
            speak_briefing,
            stop_speaking,
            voice_status,
            install_voice,
            blocks,
            save_block,
            delete_block,
            connections,
            system_load,
            local_status,
            set_local_ai,
            manage_connectors,
            get_settings,
            save_settings,
            edit_aliases
        ])
        .run(tauri::generate_context!())
        .expect("error while running Engine");
}

#[cfg(test)]
mod tests {
    use super::Voice;

    #[test]
    fn speaks_only_when_asked() {
        assert_eq!(Voice::for_dump("Can you tell me what's on today"), Voice::Speak);
        assert_eq!(Voice::for_dump("just say the weather"), Voice::Speak);
        assert_eq!(Voice::for_dump("What's the capital of France?"), Voice::Cue);
        assert_eq!(Voice::for_dump("how do magnets work"), Voice::Cue);
        assert_eq!(Voice::for_dump("write a poem about rain"), Voice::Quiet);
    }

    #[test]
    fn filler_is_not_acted_on() {
        for filler in ["Oh.", "Thank you.", "Open.", "1 . 2 3 7 7 8", ""] {
            assert!(!super::worth_acting(filler), "{filler:?} should be ignored");
        }
        for meant in ["Open Opera", "pause", "Hello.", "Claude.", "open google docs", "what's on my calendar today"] {
            assert!(super::worth_acting(meant), "{meant:?} should be acted on");
        }
    }
}
