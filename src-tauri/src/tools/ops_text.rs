use crate::edl::{new_id, TextClip};
use crate::state::App;
use crate::tools;
use serde_json::{json, Value};
use std::sync::Arc;
use tauri::AppHandle;

pub fn text_add(_h: &AppHandle, app: &Arc<App>, args: &Value) -> Result<Value, String> {
    let text = args["text"].as_str().ok_or("text is required")?.to_string();
    let start_ms = args["start_ms"].as_u64().ok_or("start_ms is required")?;
    let end_ms = args["end_ms"].as_u64().ok_or("end_ms is required")?;
    let x = args["x"].as_f64().unwrap_or(0.5) as f32;
    let y = args["y"].as_f64().unwrap_or(0.5) as f32;
    let font_size = args["font_size"].as_u64().unwrap_or(64) as u32;
    let color = args["color"].as_str().unwrap_or("#FFFFFF").to_string();

    tools::mutate(app, move |st| {
        let t = TextClip {
            id: new_id(),
            text,
            start_ms,
            end_ms,
            x: x.clamp(0.0, 1.0),
            y: y.clamp(0.0, 1.0),
            font_size,
            color,
        };
        st.project.texts.push(t.clone());
        Ok(json!({"ok": true, "text": t}))
    })
}

pub fn text_update(_h: &AppHandle, app: &Arc<App>, args: &Value) -> Result<Value, String> {
    let id = args["id"].as_str().ok_or("id is required")?.to_string();
    tools::mutate(app, move |st| {
        let p = &mut st.project;
        if p.text(&id).is_none() {
            return Err(format!("unknown text {id}"));
        }
        let t = p.texts.iter_mut().find(|t| t.id == id).unwrap();
        if let Some(v) = args["text"].as_str() {
            t.text = v.to_string();
        }
        if let Some(v) = args["start_ms"].as_u64() {
            t.start_ms = v;
        }
        if let Some(v) = args["end_ms"].as_u64() {
            t.end_ms = v;
        }
        if let Some(v) = args["x"].as_f64() {
            t.x = (v as f32).clamp(0.0, 1.0);
        }
        if let Some(v) = args["y"].as_f64() {
            t.y = (v as f32).clamp(0.0, 1.0);
        }
        if let Some(v) = args["font_size"].as_u64() {
            t.font_size = v as u32;
        }
        if let Some(v) = args["color"].as_str() {
            t.color = v.to_string();
        }
        Ok(json!({"ok": true, "text": t.clone()}))
    })
}

pub fn text_remove(_h: &AppHandle, app: &Arc<App>, args: &Value) -> Result<Value, String> {
    let id = args["id"].as_str().ok_or("id is required")?.to_string();
    tools::mutate(app, move |st| {
        let before = st.project.texts.len();
        st.project.texts.retain(|t| t.id != id);
        if st.project.texts.len() == before {
            return Err(format!("unknown text {id}"));
        }
        Ok(json!({"ok": true}))
    })
}
