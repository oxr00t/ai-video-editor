//! ToolRegistry — the single tool surface of the app.
//!
//! Every client (the UI, the internal agent, the MCP bridge for external
//! agents) calls tools through `execute`. Mutations snapshot into history,
//! validate, then broadcast `project_changed` to the UI and WS clients —
//! so an edit made by Claude Code moves the timeline you're watching.

mod ops_clip;
pub mod ops_files;
mod ops_media;
mod ops_project;
mod ops_speech;
mod ops_text;

use crate::state::{App, EditorState};
use serde_json::{json, Value};
use std::sync::Arc;
use tauri::{AppHandle, Emitter};

pub type Handler = fn(&AppHandle, &Arc<App>, &Value) -> Result<Value, String>;

pub struct ToolDef {
    pub name: &'static str,
    pub description: &'static str,
    pub params: Value,
    pub mutates: bool,
    pub handler: Handler,
}

pub fn all_tools() -> Vec<ToolDef> {
    vec![
        // ---- project -----------------------------------------------------
        ToolDef {
            name: "project_get",
            description: "Get compact project state: assets (no file paths), clips (timeline start/end + source in/out), texts, undo flags. Tracks are fixed: v2=video(top), v1=video, a1=audio. Call this first to see what you are working with.",
            params: json!({"type":"object","properties":{},"required":[]}),
            mutates: false,
            handler: ops_project::project_get,
        },
        ToolDef {
            name: "project_new",
            description: "Reset to a fresh empty project. Optional: name, fps (default 30), width/height (default 1920x1080). Do this before starting a new edit.",
            params: json!({"type":"object","properties":{
                "name":{"type":"string"},"fps":{"type":"integer"},
                "width":{"type":"integer"},"height":{"type":"integer"}
            },"required":[]}),
            mutates: true,
            handler: ops_project::project_new,
        },
        // ---- assets ------------------------------------------------------
        ToolDef {
            name: "asset_import",
            description: "Import media files (video) by absolute path. Probes each file and starts generating a preview proxy in the background. Returns the created assets with ids, durations and resolutions.",
            params: json!({"type":"object","properties":{
                "paths":{"type":"array","items":{"type":"string"},"description":"absolute file paths"}
            },"required":["paths"]}),
            mutates: true,
            handler: ops_media::asset_import,
        },
        ToolDef {
            name: "asset_list",
            description: "List imported assets: id, name, duration_ms, size, audio.",
            params: json!({"type":"object","properties":{},"required":[]}),
            mutates: false,
            handler: ops_media::asset_list,
        },
        // ---- clips (primitives — full control for large models) -----------
        ToolDef {
            name: "clip_add",
            description: "Place a clip from an asset onto a track. Defaults: track v1, start = end of that track (append), in/out = full asset. Videos land on video tracks (v1 bottom, v2 above), audio-only usage goes to a1.",
            params: json!({"type":"object","properties":{
                "asset_id":{"type":"string"},
                "track_id":{"type":"string","enum":["v1","v2","a1"]},
                "start_ms":{"type":"integer"},
                "in_ms":{"type":"integer"},
                "out_ms":{"type":"integer"}
            },"required":["asset_id"]}),
            mutates: true,
            handler: ops_clip::clip_add,
        },
        ToolDef {
            name: "clip_cut",
            description: "Split one clip into two at a TIMELINE position (at_ms between the clip's start and end). Does not delete anything.",
            params: json!({"type":"object","properties":{
                "clip_id":{"type":"string"},"at_ms":{"type":"integer"}
            },"required":["clip_id","at_ms"]}),
            mutates: true,
            handler: ops_clip::clip_cut,
        },
        ToolDef {
            name: "clip_move",
            description: "Move a clip to a new timeline position (and optionally another track). Rejects overlaps on the destination track.",
            params: json!({"type":"object","properties":{
                "clip_id":{"type":"string"},"start_ms":{"type":"integer"},"track_id":{"type":"string"}
            },"required":["clip_id","start_ms"]}),
            mutates: true,
            handler: ops_clip::clip_move,
        },
        ToolDef {
            name: "clip_trim",
            description: "Change a clip's source range and/or timeline position in one call — the precise fixer for near-miss cuts. in_ms/out_ms bound the source window; start_ms (usually only when moving the left edge) sets the timeline position. Rejects empty ranges, out-of-asset points, and overlaps.",
            params: json!({"type":"object","properties":{
                "clip_id":{"type":"string"},
                "in_ms":{"type":"integer"},
                "out_ms":{"type":"integer"},
                "start_ms":{"type":"integer"}
            },"required":["clip_id"]}),
            mutates: true,
            handler: ops_clip::clip_trim,
        },
        ToolDef {
            name: "clip_remove",
            description: "Delete a clip from the timeline (the source asset stays imported). ripple=true also slides later clips on the track left to close the gap.",
            params: json!({"type":"object","properties":{
                "clip_id":{"type":"string"},
                "ripple":{"type":"boolean","default":false}
            },"required":["clip_id"]}),
            mutates: true,
            handler: ops_clip::clip_remove,
        },
        // ---- text --------------------------------------------------------
        ToolDef {
            name: "text_add",
            description: "Add a text overlay (title, caption). x/y are the CENTER position as 0..1 fractions (0.5,0.5 = middle of frame; 0.5,0.85 = lower third caption). Defaults: white, font_size 64.",
            params: json!({"type":"object","properties":{
                "text":{"type":"string"},
                "start_ms":{"type":"integer"},"end_ms":{"type":"integer"},
                "x":{"type":"number"},"y":{"type":"number"},
                "font_size":{"type":"integer"},"color":{"type":"string","description":"hex like #FFFFFF"}
            },"required":["text","start_ms","end_ms"]}),
            mutates: true,
            handler: ops_text::text_add,
        },
        ToolDef {
            name: "text_update",
            description: "Change any field of an existing text overlay (text, timing, position, size, color).",
            params: json!({"type":"object","properties":{
                "id":{"type":"string"},
                "text":{"type":"string"},
                "start_ms":{"type":"integer"},"end_ms":{"type":"integer"},
                "x":{"type":"number"},"y":{"type":"number"},
                "font_size":{"type":"integer"},"color":{"type":"string"}
            },"required":["id"]}),
            mutates: true,
            handler: ops_text::text_update,
        },
        ToolDef {
            name: "text_remove",
            description: "Delete a text overlay by id.",
            params: json!({"type":"object","properties":{
                "id":{"type":"string"}
            },"required":["id"]}),
            mutates: true,
            handler: ops_text::text_remove,
        },
        // ---- recipes (one call, sane defaults — ideal for small models) ---
        ToolDef {
            name: "assemble_sequence",
            description: "RECIPE — assemble the given assets into one continuous sequence on track v1, back to back in the given order, each at full length. The simplest way to build an edit: import assets, then call this.",
            params: json!({"type":"object","properties":{
                "asset_ids":{"type":"array","items":{"type":"string"}}
            },"required":["asset_ids"]}),
            mutates: true,
            handler: ops_media::assemble_sequence,
        },
        // ---- filesystem discovery ----------------------------------------
        ToolDef {
            name: "list_dir",
            description: "List a directory so you can find files yourself. Empty path or '~' = home directory. Set media_only=true to show just media files and folders, depth up to 4 for a quick recursive peek.",
            params: json!({"type":"object","properties":{
                "path":{"type":"string"},
                "media_only":{"type":"boolean"},
                "depth":{"type":"integer","description":"1 = this folder only (default), up to 4"}
            },"required":[]}),
            mutates: false,
            handler: ops_files::list_dir,
        },
        ToolDef {
            name: "find_media",
            description: "Recursively find media files (video/audio/image) under a folder, newest first — use this to locate clips when the user doesn't give exact paths. Empty root or '~' = home directory.",
            params: json!({"type":"object","properties":{
                "root":{"type":"string"}
            },"required":[]}),
            mutates: false,
            handler: ops_files::find_media,
        },
        // ---- speech / talking-head (wave 1) --------------------------------
        ToolDef {
            name: "transcribe",
            description: "Transcribe an asset's speech with LOCAL Whisper (word-level timings). Model: 'medium' = accurate but slow on CPU (minutes), 'small' = fast draft. Optional initial_prompt seeds domain vocabulary (names, jargon). Returns compact line-grouped text with timings; word boundaries are stored for cut snapping. Audio is analyzed locally — nothing leaves the machine.",
            params: json!({"type":"object","properties":{
                "asset_id":{"type":"string"},
                "language":{"type":"string","description":"ISO code like fa, en (omit = auto)"},
                "model":{"type":"string","enum":["tiny","base","small","medium","large"],"description":"default medium"},
                "initial_prompt":{"type":"string"}
            },"required":["asset_id"]}),
            mutates: false,
            handler: ops_speech::transcribe_asset,
        },
        ToolDef {
            name: "silence_detect",
            description: "Find silences in an asset (ffmpeg silencedetect). Catches the pauses Whisper's text swallows. Defaults: -26dB threshold, min 0.6s. Returns start_ms/end_ms ranges.",
            params: json!({"type":"object","properties":{
                "asset_id":{"type":"string"},
                "threshold_db":{"type":"number"},
                "min_duration_s":{"type":"number"}
            },"required":["asset_id"]}),
            mutates: false,
            handler: ops_speech::silence_detect,
        },
        ToolDef {
            name: "remove_ranges",
            description: "RECIPE — apply a cut table in ONE call: remove the given SOURCE-time ranges from an asset's clips on a track. Cuts snap to word boundaries (±250ms) when the asset was transcribed. Audio fades (30ms) at export; pieces butt together (ripple). SAFETY: refuses to remove >60% of the clip coverage unless allow_aggressive=true — check your cut table if it refuses.",
            params: json!({"type":"object","properties":{
                "asset_id":{"type":"string"},
                "ranges":{"type":"array","items":{"type":"object","properties":{
                    "start_ms":{"type":"integer"},"end_ms":{"type":"integer"}
                },"required":["start_ms","end_ms"]}},
                "track_id":{"type":"string","default":"v1"},
                "snap_ms":{"type":"integer","default":250},
                "allow_aggressive":{"type":"boolean","default":false}
            },"required":["asset_id","ranges"]}),
            mutates: true,
            handler: ops_speech::remove_ranges,
        },
        ToolDef {
            name: "clip_zoom",
            description: "Set a punch-in zoom on one clip (1.0 = none, 1.08 = subtle jump-cut cover, up to 2.0). Applied at export with lanczos scaling.",
            params: json!({"type":"object","properties":{
                "clip_id":{"type":"string"},"factor":{"type":"number"}
            },"required":["clip_id","factor"]}),
            mutates: true,
            handler: ops_speech::clip_zoom,
        },
        ToolDef {
            name: "mask_jump_cuts",
            description: "RECIPE — alternate punch-in zoom over the clips of a track (1.0, 1.08, 1.0, …) to hide the face-jump of cuts. Run it after remove_ranges.",
            params: json!({"type":"object","properties":{
                "track_id":{"type":"string","default":"v1"},
                "factor":{"type":"number","default":1.08}
            },"required":[]}),
            mutates: true,
            handler: ops_speech::mask_jump_cuts,
        },
        ToolDef {
            name: "extract_frames",
            description: "Extract JPEG frames from an asset at given times_ms (or every_ms) for visual QA. VISION MODELS: pass with_images=true and up to 4 frames are attached to the conversation as images so you can see them. Text-only clients get file paths to open instead.",
            params: json!({"type":"object","properties":{
                "asset_id":{"type":"string"},
                "times_ms":{"type":"array","items":{"type":"integer"}},
                "every_ms":{"type":"integer"},
                "with_images":{"type":"boolean","default":false}
            },"required":["asset_id"]}),
            mutates: false,
            handler: ops_speech::extract_frames,
        },
        // ---- history -----------------------------------------------------
        ToolDef {
            name: "undo",
            description: "Undo the last edit (any edit — yours, another agent's, or the human's).",
            params: json!({"type":"object","properties":{},"required":[]}),
            mutates: true,
            handler: ops_project::undo,
        },
        ToolDef {
            name: "redo",
            description: "Redo the last undone edit.",
            params: json!({"type":"object","properties":{},"required":[]}),
            mutates: true,
            handler: ops_project::redo,
        },
        // ---- output ------------------------------------------------------
        ToolDef {
            name: "export_video",
            description: "Render the project to an mp4 with ffmpeg (topmost video per moment + text overlays + audio track). Optional out_path; default is the app export folder. Returns the written file path. Can take a while — call it once at the end.",
            params: json!({"type":"object","properties":{
                "out_path":{"type":"string"}
            },"required":[]}),
            mutates: false,
            handler: ops_media::export_video,
        },
        // ---- skills ------------------------------------------------------
        ToolDef {
            name: "skills_list",
            description: "List available editing skill guides (technique documentation you can read to edit better).",
            params: json!({"type":"object","properties":{},"required":[]}),
            mutates: false,
            handler: ops_media::skills_list,
        },
        ToolDef {
            name: "skill_read",
            description: "Read one skill guide by name (returns its full markdown). Read a skill before using its techniques.",
            params: json!({"type":"object","properties":{
                "name":{"type":"string"}
            },"required":["name"]}),
            mutates: false,
            handler: ops_media::skill_read,
        },
    ]
}

/// Run a mutation under the state lock with snapshot + validation + rollback.
pub fn mutate<F>(app: &App, f: F) -> Result<Value, String>
where
    F: FnOnce(&mut EditorState) -> Result<Value, String>,
{
    let mut st = app.state.lock().unwrap();
    {
        // destructure the guard once so disjoint field borrows are allowed
        let EditorState { history, project, .. } = &mut *st;
        history.push(project.clone());
    }
    let mut result = f(&mut st);
    if result.is_ok() {
        if let Err(e) = st.project.validate() {
            result = Err(format!("validation failed: {e}"));
        }
    }
    match result {
        Ok(v) => {
            app.broadcast_project(&st.project);
            let _ = st.save();
            Ok(v)
        }
        Err(e) => {
            let EditorState { history, project, .. } = &mut *st;
            history.rollback(project);
            Err(e)
        }
    }
}

/// Run a read-only operation under the state lock.
pub fn read<F>(app: &App, f: F) -> Result<Value, String>
where
    F: FnOnce(&EditorState) -> Result<Value, String>,
{
    let st = app.state.lock().unwrap();
    f(&st)
}

/// Central entry point used by every client. Executes the tool and, for
/// mutating tools, notifies the UI (Tauri event) after success.
pub fn execute(handle: &AppHandle, app: &Arc<App>, name: &str, args: &Value) -> Result<Value, String> {
    let tool = all_tools()
        .into_iter()
        .find(|t| t.name == name)
        .ok_or_else(|| {
            format!(
                "unknown tool '{name}'. Available: {}",
                all_tools().iter().map(|t| t.name).collect::<Vec<_>>().join(", ")
            )
        })?;
    let result = (tool.handler)(handle, app, args)?;
    if tool.mutates {
        let project = app.state.lock().unwrap().project.clone();
        let _ = handle.emit("project_changed", &project);
    }
    Ok(result)
}

/// JSON-Schema list for MCP tools/list.
pub fn mcp_tools_json() -> Value {
    json!(all_tools()
        .into_iter()
        .map(|t| {
            json!({
                "name": t.name,
                "description": t.description,
                "inputSchema": t.params,
            })
        })
        .collect::<Vec<_>>())
}
