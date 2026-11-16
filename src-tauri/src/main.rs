// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // `engine --guard` is the agents' outward-tool hook: answer and exit before the app (and its
    // single-instance check) starts.
    if std::env::args().nth(1).as_deref() == Some("--guard") {
        std::process::exit(engine_lib::jobs::guard());
    }
    engine_lib::run()
}
