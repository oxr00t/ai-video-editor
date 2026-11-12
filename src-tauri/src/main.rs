// Prevents an extra console window on Windows in release builds
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // `--mcp`: run as a headless MCP stdio bridge (for external agents such as
    // Claude Code) instead of launching the GUI app.
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "--mcp") {
        app_media_creator_lib::mcp::run_bridge();
        return;
    }
    app_media_creator_lib::run()
}
