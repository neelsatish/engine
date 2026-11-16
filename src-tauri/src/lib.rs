pub mod launcher;

use launcher::{Match, Target};
use serde::Serialize;
use std::{path::PathBuf, sync::Mutex};
use tauri::{
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    AppHandle, Emitter, Manager, State, WindowEvent,
};
use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Modifiers, Shortcut, ShortcutState};

struct Engine {
    apps: Mutex<Vec<Target>>,
    aliases_path: PathBuf,
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
}

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum Reply {
    Opened { name: String },
    Choices { items: Vec<Target> },
    NotFound { query: String },
    Unsupported,
    Error { message: String },
}

fn open(t: Target) -> Reply {
    match launcher::launch(&t.target) {
        Ok(()) => Reply::Opened { name: t.name },
        Err(e) => Reply::Error { message: format!("Couldn't open {}: {e}", t.name) },
    }
}

#[tauri::command]
fn submit(text: String, engine: State<Engine>) -> Reply {
    let Some(query) = launcher::parse_open(&text) else {
        return Reply::Unsupported;
    };
    let aliases = match engine.aliases() {
        Ok(a) => a,
        Err(reply) => return reply,
    };
    let mut found = launcher::resolve(&query, &aliases, &engine.apps(false));
    if found == Match::Nothing {
        found = launcher::resolve(&query, &aliases, &engine.apps(true));
    }
    match found {
        Match::One(t) => open(t),
        Match::Choices(items) => Reply::Choices { items },
        Match::Nothing => Reply::NotFound { query },
    }
}

#[tauri::command]
fn open_choice(choice: Target, engine: State<Engine>) -> Reply {
    let aliases = match engine.aliases() {
        Ok(a) => a,
        Err(reply) => return reply,
    };
    // Only open what Engine itself offered, so the UI can't launch arbitrary targets.
    if aliases.contains(&choice) || engine.apps(false).contains(&choice) {
        open(choice)
    } else {
        Reply::Error { message: format!("{} isn't in the app list any more.", choice.name) }
    }
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
            app.manage(Engine { apps: Mutex::new(vec![]), aliases_path });

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
        .invoke_handler(tauri::generate_handler![submit, open_choice, edit_aliases])
        .run(tauri::generate_context!())
        .expect("error while running Engine");
}
