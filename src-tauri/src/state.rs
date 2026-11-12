//! Central editor state, managed by Tauri and shared by every client:
//! the UI (Tauri commands), the internal agent, and the MCP bridge (WS).

use crate::edl::history::History;
use crate::edl::Project;
use std::path::PathBuf;
use std::sync::Mutex;

pub struct EditorState {
    pub project: Project,
    pub history: History,
    pub project_path: Option<PathBuf>,
    /// Absolute path of the last export, if any.
    pub last_export: Option<String>,
}

impl EditorState {
    pub fn new() -> Self {
        EditorState {
            project: Project::new("Untitled", 30, 1920, 1080),
            history: History::new(),
            project_path: None,
            last_export: None,
        }
    }

    pub fn save(&self) -> Result<(), String> {
        let path = self
            .project_path
            .clone()
            .unwrap_or_else(|| PathBuf::from(format!("{}.meproj.json", self.project.name)));
        let json = serde_json::to_string_pretty(&self.project).map_err(|e| e.to_string())?;
        std::fs::write(&path, json).map_err(|e| format!("save to {}: {e}", path.display()))
    }
}

/// Managed Tauri state. All mutation goes through `state.lock()`.
pub struct App {
    pub state: Mutex<EditorState>,
    /// Dir for proxies and other generated media.
    pub cache_dir: PathBuf,
    /// Dir containing SKILL.md bundles.
    pub skills_dir: PathBuf,
    /// Broadcasts the full project JSON to WS clients after every change.
    pub ws_broadcast: tokio::sync::broadcast::Sender<serde_json::Value>,
    /// Port the WS control server bound to (0 = not running).
    pub control_port: std::sync::atomic::AtomicU16,
}

impl App {
    pub fn broadcast_project(&self, project: &Project) {
        let _ = serde_json::to_value(project).map(|v| {
            let _ = self.ws_broadcast.send(v);
        });
    }
}
