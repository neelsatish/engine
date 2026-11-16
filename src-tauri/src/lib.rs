pub mod ai;
pub mod launcher;
pub mod router;
pub mod store;

use launcher::{Match, Target};
use router::Kind;
use serde::Serialize;
use std::{
    fs,
    path::PathBuf,
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
    store: Mutex<Store>,
    /// Where model calls run, so no project's Claude Code settings apply to them.
    work_dir: PathBuf,
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
    let prompt = router::prompt(text, &shortcuts);
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
            let (app, question) = (app.clone(), item.detail.clone());
            std::thread::spawn(move || {
                let engine = app.state::<Engine>();
                let today = engine.store().today().unwrap_or_default();
                let system = router::answer_system(&today);
                let (status, result) = match ai::ask(&engine.work_dir, router::ANSWER_MODEL, &system, &question, None) {
                    Ok(reply) => ("done", reply.as_str().unwrap_or_default().trim().to_string()),
                    Err(e) => ("failed", e),
                };
                log_err(engine.store().set(id, status, &result));
                changed(&app);
            });
        }
        Kind::Job => {
            add("queued", "Waiting for agents, which arrive in the next update.");
        }
        Kind::Outward => {
            add("held", "Held for your approval. Approval cards arrive in a later update.");
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

    tauri::Builder::default()
        // Must be first: a second launch just brings the running Engine forward.
        .plugin(tauri_plugin_single_instance::init(|app, _, _| show(app)))
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(move |app, shortcut, event| {
                    if shortcut == &summon && event.state() == ShortcutState::Pressed {
                        toggle(app);
                    }
                })
                .build(),
        )
        .setup(move |app| {
            let aliases_path = app.path().app_config_dir()?.join("aliases.json");
            let work_dir = app.path().app_data_dir()?;
            fs::create_dir_all(&work_dir)?;
            let store = Mutex::new(Store::open(&work_dir.join("engine.db"))?);
            app.manage(Engine { apps: Mutex::new(vec![]), aliases_path, store, work_dir });

            // Build the app index in the background so the first "open X" is instant.
            let handle = app.handle().clone();
            std::thread::spawn(move || {
                handle.state::<Engine>().apps(false);
            });

            if let Err(e) = app.global_shortcut().register(summon) {
                eprintln!("couldn't register Ctrl+Shift+Space: {e}");
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
                    "quit" => app.exit(0),
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
        .invoke_handler(tauri::generate_handler![submit, open_choice, briefing, dismiss, edit_aliases])
        .run(tauri::generate_context!())
        .expect("error while running Engine");
}
