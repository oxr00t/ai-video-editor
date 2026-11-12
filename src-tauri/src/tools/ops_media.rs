use crate::edl::{new_id, Clip};
use crate::engine::{export, probe, proxy};
use crate::state::App;
use crate::tools;
use serde_json::{json, Value};
use std::sync::Arc;
use tauri::{AppHandle, Emitter};

pub fn asset_import(handle: &AppHandle, app: &Arc<App>, args: &Value) -> Result<Value, String> {
    let paths: Vec<String> = args["paths"]
        .as_array()
        .ok_or("paths must be an array of file paths")?
        .iter()
        .filter_map(|v| v.as_str().map(|s| s.to_string()))
        .collect();
    if paths.is_empty() {
        return Err("paths is empty".into());
    }

    let mut assets = Vec::new();
    let mut errors = Vec::new();
    for path in &paths {
        match probe::probe(path) {
            Ok(a) => assets.push(a),
            Err(e) => errors.push(e),
        }
    }

    // Kick off proxy generation per asset in the background; the UI gets a
    // project_changed event again when each proxy lands.
    for asset in &assets {
        let app = Arc::clone(app);
        let handle = handle.clone();
        let asset_id = asset.id.clone();
        let src = asset.path.clone();
        let has_audio = asset.has_audio;
        let cache_dir = app.cache_dir.clone();
        std::thread::spawn(move || {
            let dst = proxy::proxy_path(&cache_dir, &asset_id);
            match proxy::generate(&src, &dst, has_audio) {
                Ok(p) => {
                    let mut st = app.state.lock().unwrap();
                    if let Some(a) = st.project.assets.iter_mut().find(|a| a.id == asset_id) {
                        a.proxy_path = Some(p);
                    }
                    let project = st.project.clone();
                    drop(st);
                    app.broadcast_project(&project);
                    let _ = handle.emit("project_changed", &project);
                }
                Err(e) => {
                    eprintln!("proxy generation failed for {src}: {e}");
                }
            }
        });
    }

    let imported = assets.clone();
    tools::mutate(app, move |st| {
        let mut created = Vec::new();
        for a in imported {
            let v = asset_json(&a);
            st.project.assets.push(a);
            created.push(v);
        }
        Ok(json!({"ok": true, "assets": created, "errors": errors}))
    })
}

pub fn asset_list(_h: &AppHandle, app: &Arc<App>, _a: &Value) -> Result<Value, String> {
    tools::read(app, |st| {
        Ok(json!(st
            .project
            .assets
            .iter()
            .map(asset_json)
            .collect::<Vec<_>>()))
    })
}

pub fn assemble_sequence(_h: &AppHandle, app: &Arc<App>, args: &Value) -> Result<Value, String> {
    let asset_ids: Vec<String> = args["asset_ids"]
        .as_array()
        .ok_or("asset_ids must be an array")?
        .iter()
        .filter_map(|v| v.as_str().map(|s| s.to_string()))
        .collect();
    if asset_ids.is_empty() {
        return Err("asset_ids is empty".into());
    }

    tools::mutate(app, move |st| {
        let p = &mut st.project;
        let mut clip_ids = Vec::new();
        for aid in &asset_ids {
            let asset = p
                .asset(aid)
                .ok_or_else(|| format!("unknown asset {aid}"))?
                .clone();
            let start = p.track_end_ms("v1");
            let clip = Clip {
                id: new_id(),
                asset_id: asset.id,
                track_id: "v1".into(),
                start_ms: start,
                in_ms: 0,
                out_ms: asset.duration_ms,
                zoom: 1.0,
            };
            clip_ids.push(clip.id.clone());
            p.clips.push(clip);
        }
        Ok(json!({
            "ok": true,
            "clip_ids": clip_ids,
            "project_duration_ms": p.duration_ms(),
        }))
    })
}

pub fn export_video(_h: &AppHandle, app: &Arc<App>, args: &Value) -> Result<Value, String> {
    let (project, name_for_file) = {
        let st = app.state.lock().unwrap();
        (st.project.clone(), st.project.name.clone())
    };
    if project.clips.is_empty() && project.texts.is_empty() {
        return Err("project is empty — nothing to export".into());
    }

    let out_path = match args["out_path"].as_str() {
        Some(p) => p.to_string(),
        None => {
            let stamp = chrono::Local::now().format("%Y%m%d_%H%M%S");
            app.cache_dir
                .join("exports")
                .join(format!("{}_{}.mp4", sanitize(&name_for_file), stamp))
                .to_string_lossy()
                .to_string()
        }
    };
    if let Some(parent) = std::path::Path::new(&out_path).parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }

    export::export(&project, &out_path)?;

    app.state.lock().unwrap().last_export = Some(out_path.clone());
    Ok(json!({"ok": true, "path": out_path}))
}

fn sanitize(s: &str) -> String {
    s.chars()
        .map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
        .collect()
}

// ---- compact serializers shared across tools -------------------------------
// These shapes are what models see in their context — keep them minimal.

pub fn asset_json(a: &crate::edl::Asset) -> Value {
    json!({
        "id": a.id, "name": a.name, "duration_ms": a.duration_ms,
        "size": format!("{}x{}", a.width, a.height), "audio": a.has_audio,
    })
}

pub fn clip_json(c: &crate::edl::Clip) -> Value {
    json!({
        "id": c.id, "track": c.track_id, "asset": c.asset_id,
        "start_ms": c.start_ms, "end_ms": c.end_ms(),
        "in_ms": c.in_ms, "out_ms": c.out_ms,
    })
}

pub fn text_json(t: &crate::edl::TextClip) -> Value {
    json!({
        "id": t.id, "text": t.text,
        "start_ms": t.start_ms, "end_ms": t.end_ms,
        "x": t.x, "y": t.y, "size": t.font_size, "color": t.color,
    })
}

// ---- skills ----------------------------------------------------------------

fn parse_skill(dir: &std::path::Path) -> Option<Value> {
    let skill_file = dir.join("SKILL.md");
    let content = std::fs::read_to_string(&skill_file).ok()?;
    let name = dir
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    let mut description = String::new();
    if let Some(rest) = content.strip_prefix("---") {
        if let Some(end) = rest.find("---") {
            for line in rest[..end].lines() {
                if let Some(d) = line.strip_prefix("description:") {
                    description = d.trim().to_string();
                }
            }
        }
    }
    if description.is_empty() {
        description = content
            .lines()
            .find(|l| !l.trim().is_empty() && !l.starts_with('#'))
            .unwrap_or("")
            .trim()
            .chars()
            .take(120)
            .collect();
    }
    Some(json!({"name": name, "description": description}))
}

pub fn skills_list(_h: &AppHandle, app: &Arc<App>, _a: &Value) -> Result<Value, String> {
    let mut out = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&app.skills_dir) {
        for e in entries.flatten() {
            if e.path().is_dir() {
                if let Some(v) = parse_skill(&e.path()) {
                    out.push(v);
                }
            }
        }
    }
    out.sort_by(|a, b| a["name"].as_str().cmp(&b["name"].as_str()));
    Ok(json!({"skills": out}))
}

pub fn skill_read(_h: &AppHandle, app: &Arc<App>, args: &Value) -> Result<Value, String> {
    let name = args["name"].as_str().ok_or("name is required")?;
    // no path traversal
    if name.contains("..") || name.contains('/') || name.contains('\\') {
        return Err("invalid skill name".into());
    }
    let path = app.skills_dir.join(name).join("SKILL.md");
    let content = std::fs::read_to_string(&path)
        .map_err(|_| format!("skill '{name}' not found"))?;
    Ok(json!({"name": name, "content": content}))
}
