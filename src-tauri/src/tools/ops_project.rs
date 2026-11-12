use crate::edl::Project;
use crate::state::{App, EditorState};
use crate::tools;
use crate::tools::ops_media::{asset_json, clip_json, text_json};
use serde_json::{json, Value};
use std::sync::Arc;
use tauri::AppHandle;

/// Compact state for models: no file paths, no fixed track layout, short
/// field names where derivable — every call to this tool lands in the model's
/// context, so size directly drives token cost.
pub fn project_get(_h: &AppHandle, app: &Arc<App>, _a: &Value) -> Result<Value, String> {
    tools::read(app, |st| {
        let p = &st.project;
        Ok(json!({
            "name": p.name,
            "fps": p.fps,
            "size": format!("{}x{}", p.width, p.height),
            "duration_ms": p.duration_ms(),
            "tracks": "v2=video(top) v1=video a1=audio",
            "can_undo": st.history.can_undo(),
            "can_redo": st.history.can_redo(),
            "assets": p.assets.iter().map(asset_json).collect::<Vec<_>>(),
            "clips": p.clips.iter().map(clip_json).collect::<Vec<_>>(),
            "texts": p.texts.iter().map(text_json).collect::<Vec<_>>(),
        }))
    })
}

pub fn project_new(_h: &AppHandle, app: &Arc<App>, args: &Value) -> Result<Value, String> {
    let name = args["name"].as_str().unwrap_or("Untitled").to_string();
    let fps = args["fps"].as_u64().unwrap_or(30).clamp(1, 120) as u32;
    let width = args["width"].as_u64().unwrap_or(1920).clamp(16, 7680) as u32;
    let height = args["height"].as_u64().unwrap_or(1080).clamp(16, 4320) as u32;
    tools::mutate(app, move |st| {
        st.project = Project::new(&name, fps, width, height);
        st.last_export = None;
        Ok(json!({"ok": true, "project_id": st.project.id, "fps": fps, "width": width, "height": height}))
    })
}

/// undo/redo manage their own history movement, so they skip `mutate`'s
/// snapshot (it would pollute the stacks with duplicate states).
fn history_op(app: &Arc<App>, op: fn(&mut EditorState) -> bool) -> Result<Value, String> {
    let mut st = app.state.lock().unwrap();
    let ok = op(&mut st);
    app.broadcast_project(&st.project);
    let _ = st.save();
    Ok(json!({"ok": ok, "can_undo": st.history.can_undo(), "can_redo": st.history.can_redo()}))
}

pub fn undo(_h: &AppHandle, app: &Arc<App>, _a: &Value) -> Result<Value, String> {
    history_op(app, |st| {
        let mut scratch = std::mem::replace(&mut st.project, Project::new("_", 30, 16, 16));
        let ok = st.history.undo(&mut scratch);
        st.project = scratch;
        ok
    })
}

pub fn redo(_h: &AppHandle, app: &Arc<App>, _a: &Value) -> Result<Value, String> {
    history_op(app, |st| {
        let mut scratch = std::mem::replace(&mut st.project, Project::new("_", 30, 16, 16));
        let ok = st.history.redo(&mut scratch);
        st.project = scratch;
        ok
    })
}
