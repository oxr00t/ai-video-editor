pub mod agent;
mod commands;
mod control;
pub mod edl;
pub mod engine;
pub mod mcp;
mod state;
pub mod tools;

use state::{App, EditorState};
use std::sync::atomic::AtomicU16;
use std::sync::{Arc, Mutex};
use tauri::Manager;

fn resolve_skills_dir(app_data: &std::path::Path) -> std::path::PathBuf {
    // Dev: the repo's skills/ dir (cwd is src-tauri under `tauri dev`).
    // Env var wins; packaged builds fall back to appdata/skills.
    if let Ok(dir) = std::env::var("MA_SKILLS_DIR") {
        let p = std::path::PathBuf::from(dir);
        if p.is_dir() {
            return p;
        }
    }
    for cand in ["skills", "../skills"] {
        let p = std::path::PathBuf::from(cand);
        if p.join("cuts-and-pace").join("SKILL.md").exists() {
            return std::fs::canonicalize(&p).unwrap_or(p);
        }
    }
    let fallback = app_data.join("skills");
    let _ = std::fs::create_dir_all(&fallback);
    fallback
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let app_data = app
                .path()
                .app_local_data_dir()
                .expect("no app local data dir");
            let cache_dir = app_data.join("cache");
            std::fs::create_dir_all(&cache_dir).expect("cannot create cache dir");
            let skills_dir = resolve_skills_dir(&app_data);

            let (tx, _rx) = tokio::sync::broadcast::channel(32);
            let shared = Arc::new(App {
                state: Mutex::new(EditorState::new()),
                cache_dir,
                skills_dir,
                ws_broadcast: tx,
                control_port: AtomicU16::new(0),
            });
            app.manage(Arc::clone(&shared));
            app.manage(Arc::new(agent::ChatState {
                history: Mutex::new(Vec::new()),
                busy: std::sync::atomic::AtomicBool::new(false),
            }));

            println!("skills dir: {}", shared.skills_dir.display());

            let handle = app.handle().clone();
            tauri::async_runtime::spawn(control::start(handle, Arc::clone(&shared)));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_project,
            commands::tool_invoke,
            commands::get_provider_config,
            commands::set_provider_config,
            commands::list_models,
            commands::test_provider,
            commands::agent_send,
            commands::agent_clear_chat,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
