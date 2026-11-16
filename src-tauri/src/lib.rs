pub mod ai;
pub mod jobs;
pub mod launcher;
pub mod router;
pub mod schedule;
pub mod settings;
pub mod store;
pub mod voice;

use launcher::{Match, Target};
use router::Kind;
use serde::Serialize;
use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    sync::{Mutex, MutexGuard},
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
    let (status, text) = match &result {
        Ok(()) => ("done", format!("Opened {}", t.name)),
        Err(e) => ("failed", e.clone()),
    };
    log_err(engine.store().add_item(dump, "instant", title, &t.target, status, &text));
    result
}

fn open_reply(app: &AppHandle, t: Target) -> Reply {
    let reply = match launch(&app.state::<Engine>(), None, &format!("Open {}", t.name), &t) {
        Ok(()) => Reply::Opened { name: t.name },
        Err(message) => Reply::Error { message },
    };
    changed(app);
    reply
}

fn handle_dump(app: &AppHandle, text: &str) -> Reply {
    let engine = app.state::<Engine>();
    let aliases = match engine.aliases() {
        Ok(a) => a,
        Err(reply) => return reply,
    };
    // Fast path: a plain "open X" skips the model so it feels instant.
    if let Some(query) = launcher::parse_open(text) {
        match engine.find(&query, &aliases) {
            Match::One(t) => return open_reply(app, t),
            Match::Choices(items) => return Reply::Choices { items },
            // Several requests in one, or a site with no alias: the router sorts it out.
            Match::Nothing => {}
        }
    }

    let dump = match engine.store().add_dump(text) {
        Ok(id) => id,
        Err(e) => return Reply::Error { message: format!("Couldn't save the dump: {e}") },
    };
    let shortcuts: Vec<String> = aliases.iter().map(|a| a.name.clone()).collect();
    let projects: Vec<String> = engine.settings().projects.into_iter().map(|p| p.name).collect();
    let prompt = router::prompt(text, &shortcuts, &projects);
    let items = match ai::ask(&engine.work_dir, router::MODEL, router::SYSTEM, &prompt, Some(router::SCHEMA))
        .and_then(router::parse)
    {
        Ok(items) => items,
        Err(message) => return Reply::Error { message },
    };
    let count = items.len();
    for item in items {
        start(app, &engine, dump, &aliases, item);
    }
    pump(app);
    changed(app);
    Reply::Routed { count }
}

/// Acts on one routed item: open it, answer it, or hold it in the briefing.
fn start(app: &AppHandle, engine: &Engine, dump: i64, aliases: &[Target], item: router::Item) {
    let add = |status: &str, result: &str| {
        log_err(engine.store().add_item(Some(dump), item.kind.as_str(), &item.title, &item.detail, status, result))
    };
    match item.kind {
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
                    add("failed", &format!("Couldn't find “{}”. Add it under Edit aliases.", item.detail));
                }
            }
        }
        Kind::Answer => {
            let Some(id) = add("running", "") else { return };
            let (app, question, title) = (app.clone(), item.detail.clone(), item.title.clone());
            std::thread::spawn(move || {
                let engine = app.state::<Engine>();
                let today = engine.store().today().unwrap_or_default();
                let system = router::answer_system(&today);
                let reply = ai::ask(&engine.work_dir, router::ANSWER_MODEL, &system, &question, Some(router::ANSWER_SCHEMA));
                let (status, result) = match reply {
                    Ok(out) => {
                        let (text, plan) = router::parse_answer(&out);
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
            log_err(engine.store().add_job(Some(dump), &item.title, &item.detail, project, ""));
        }
        Kind::Outward => {
            // Nothing outward runs from a dump: an agent drafts it, and the draft comes back as an approval card.
            let project = item.project.as_deref().unwrap_or_default();
            let title = format!("Draft: {}", item.title);
            log_err(engine.store().add_job(Some(dump), &title, &jobs::prepare_prompt(&item.detail), project, ""));
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
    let work = project.map(|p| PathBuf::from(&p.folder)).filter(|f| f.is_dir()).unwrap_or_else(|| dir.clone());
    // Approved actions always go to Claude: the one-call guard and the user's connectors live there.
    let codex = job.tool.is_empty() && project.is_some_and(|p| p.runner == "codex");
    let notes = settings.knowledge_dirs();
    let marker = dir.join("approved.once");
    let approved = (!job.tool.is_empty()).then(|| (job.tool.as_str(), marker.as_path()));
    changed(app);
    let on_spawn = |pid| {
        engine.jobs.lock().unwrap().insert(id, pid);
    };
    let on_activity = |process: &'static str, label: String| {
        log_err(engine.store().set_activity(id, process, &label));
        changed(app);
    };
    let outcome = fs::create_dir_all(&dir).map_err(|e| format!("Couldn't make the job folder: {e}")).and_then(|()| {
        if codex {
            jobs::run_codex(&work, &dir, &job.prompt, on_spawn, on_activity)
        } else {
            jobs::run(&work, &dir, &notes, approved, &job.prompt, on_spawn, on_activity)
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
                log_err(engine.store().add_held("outward", dump, &p.action, &p.details, &p.tool, &job.project));
            }
            for t in &done.needs_you {
                log_err(engine.store().add_held("task", dump, &t.task, &t.follow_up, "", &job.project));
            }
            let added = add_plan(app, &job_title(&engine, id), &done.summary, done.schedule);
            let summary = if added > 0 { format!("{summary}\n\nAdded {added} blocks to your schedule.") } else { summary };
            log_err(engine.store().finish(id, "done", &summary));
            Some((summary, done.open))
        }
        Err(e) => {
            log_err(engine.store().finish(id, "failed", &e));
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
    let (verdict, reason) = ai::ask(&engine.work_dir, jobs::MODEL, jobs::AUDIT_SYSTEM, &prompt, Some(jobs::AUDIT_SCHEMA))
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
        open_reply(&app, choice)
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
        engine.store().add_job(h.dump, &h.title, &prompt, &h.project, tool).map_err(|e| e.to_string())?;
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
        engine.store().add_job(h.dump, &title, &prompt, &h.project, "").map_err(|e| e.to_string())?;
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
        engine.store().add_job(h.dump, &h.detail, &prompt, &h.project, "").map_err(|e| e.to_string())?;
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
        let (status, text) = match ai::ask(&engine.work_dir, router::ANSWER_MODEL, &jobs::review_system(&today), &prompt, None) {
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
    if rec.is_none() {
        *rec = Some(voice::record()?);
        voice_event(app, "listening", "");
    }
    Ok(())
}

/// Stops the microphone and transcribes what was said. Empty when nothing was caught.
fn stop_listening(app: &AppHandle) -> Result<String, String> {
    let engine = app.state::<Engine>();
    let Some(rec) = engine.recording.lock().unwrap().take() else { return Ok(String::new()) };
    let wav = engine.work_dir.join("voice").join("heard.wav");
    let Some(wav) = rec.finish(&wav)? else {
        voice_event(app, "idle", "");
        return Ok(String::new());
    };
    voice_event(app, "transcribing", "");
    let model = engine.settings().voice.model;
    let text = engine.with_helper(|h| h.transcribe(&wav, &model));
    let _ = fs::remove_file(&wav); // the words are kept, the recording isn't
    voice_event(app, "idle", "");
    text
}

/// Push-to-talk released: transcribe, then hand the words to the UI (which may send them straight away).
fn heard(app: &AppHandle) {
    match stop_listening(app) {
        Ok(text) if !text.is_empty() => {
            show(app);
            let _ = app.emit("heard", serde_json::json!({"text": text, "submit": app.state::<Engine>().settings().voice.auto_submit}));
        }
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
    let v = engine.settings().voice;
    if v.engine == "chatterbox" {
        let out = engine.work_dir.join("voice").join("spoken.wav");
        let request = serde_json::json!({"cmd": "speak", "text": text, "clip": v.clip, "out": out});
        return engine.with_helper(|h| h.call(&request)).map(|_| ());
    }
    *engine.speaking.lock().unwrap() = Some(voice::speak_windows(text)?);
    Ok(())
}

fn stop_speech(engine: &Engine) {
    if let Some(mut child) = engine.speaking.lock().unwrap().take() {
        let _ = child.kill();
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
fn get_settings(engine: State<Engine>) -> Result<settings::Settings, String> {
    settings::Settings::load(&engine.settings_path)
}

#[tauri::command]
fn save_settings(settings: settings::Settings, engine: State<Engine>) -> Result<settings::Settings, String> {
    settings.save(&engine.settings_path)
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
            });

            // Free the voice models after a few idle minutes, so Engine doesn't hold memory while unused.
            let reaper = app.handle().clone();
            std::thread::spawn(move || loop {
                std::thread::sleep(std::time::Duration::from_secs(60));
                let engine = reaper.state::<Engine>();
                let mut helper = engine.helper.lock().unwrap();
                if helper.as_ref().is_some_and(|h| h.last_used.elapsed() > voice::IDLE) {
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
            get_settings,
            save_settings,
            edit_aliases
        ])
        .run(tauri::generate_context!())
        .expect("error while running Engine");
}
