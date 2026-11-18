// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // `engine --guard` is the agents' outward-tool hook: answer and exit before the app (and its
    // single-instance check) starts.
    if std::env::args().nth(1).as_deref() == Some("--guard") {
        std::process::exit(engine_lib::jobs::guard());
    }
    // `engine --mcp` is Engine's own MCP server for agents (schedule, briefing, settings).
    if std::env::args().nth(1).as_deref() == Some("--mcp") {
        engine_lib::mcp::serve();
        return;
    }
    engine_lib::run()
}
