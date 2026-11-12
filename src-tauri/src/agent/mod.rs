//! Internal BYOM agent: a plain tool-calling loop over the same
//! ToolRegistry the MCP bridge exposes. Progress is streamed to the UI
//! as `agent_event` Tauri events.

pub mod providers;

use crate::state::App;
use crate::tools;
use providers::ProviderConfig;
use serde_json::{json, Value};
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter};

pub struct ChatState {
    /// OpenAI-format message history (role/content/tool_calls/tool_call_id).
    pub history: Mutex<Vec<Value>>,
    pub busy: AtomicBool,
}

const MAX_STEPS: usize = 24;
const KEEP_TOOL_RESULTS: usize = 6;

/// Keep this many recent tool results verbatim in context; older ones are
/// squashed to a one-line marker so long sessions don't balloon. User
/// messages that carry extracted frame images get squashed too (base64 is
/// huge — the frames stay on disk, the tool can re-send them).
fn squash_old_tool_results(history: &mut Vec<Value>) {
    let tool_positions: Vec<usize> = history
        .iter()
        .enumerate()
        .filter(|(_, m)| m["role"] == "tool")
        .map(|(i, _)| i)
        .collect();
    if tool_positions.len() <= KEEP_TOOL_RESULTS {
        return;
    }
    let cutoff_idx = tool_positions[tool_positions.len() - KEEP_TOOL_RESULTS];
    for m in history.iter_mut().take(cutoff_idx) {
        if m["role"] == "tool" {
            m["content"] = Value::String(
                "[result cleared to save context — re-call the tool if needed]".into(),
            );
        } else if m["role"] == "user" && m["content"].is_array() {
            m["content"] = Value::String(
                "[earlier extracted frames removed from context — call extract_frames again if needed]".into(),
            );
        }
    }
}

/// Split a tool result into (clean payload for context/UI, attached images).
fn take_images(mut payload: Value) -> (Value, Vec<Value>) {
    let mut images = Vec::new();
    if let Some(obj) = payload.as_object_mut() {
        if let Some(imgs) = obj.remove("_images") {
            if let Some(arr) = imgs.as_array() {
                images = arr.clone();
            }
        }
    }
    (payload, images)
}

pub async fn run(handle: AppHandle, app: Arc<App>, chat: Arc<ChatState>, user_message: String) {
    let emit = |v: Value| {
        let _ = handle.emit("agent_event", v);
    };

    let cfg = match load_config(&app) {
        Some(c) => c,
        None => {
            emit(json!({"type":"error","text":"No model configured — open Settings and set a provider (OpenAI-compatible or Anthropic)."}));
            return;
        }
    };

    chat.history.lock().unwrap().push(json!({
        "role": "user", "content": user_message
    }));

    emit(json!({"type":"status","text":"thinking"}));

    for _step in 0..MAX_STEPS {
        squash_old_tool_results(&mut chat.history.lock().unwrap());
        let (system, tools) = system_prompt_and_tools(&app);
        let history = chat.history.lock().unwrap().clone();

        let assistant = match providers::call(&cfg, &system, &history, &tools).await {
            Ok(m) => m,
            Err(e) => {
                emit(json!({"type":"error","text": format!("model call failed: {e}")}));
                return;
            }
        };

        let tool_calls: Vec<Value> = assistant
            .get("tool_calls")
            .and_then(|t| t.as_array())
            .cloned()
            .unwrap_or_default();

        chat.history.lock().unwrap().push(assistant.clone());

        if tool_calls.is_empty() {
            let text = assistant["content"].as_str().unwrap_or("").to_string();
            emit(json!({"type":"message","text": text}));
            return;
        }

        for tc in &tool_calls {
            let name = tc["function"]["name"].as_str().unwrap_or("").to_string();
            let tool_name = name.clone();
            let args: Value = serde_json::from_str(tc["function"]["arguments"].as_str().unwrap_or("{}"))
                .unwrap_or(json!({}));
            let id = tc["id"].as_str().unwrap_or("").to_string();

            emit(json!({"type":"status","text": format!("running {tool_name}")}));
            emit(json!({"type":"tool","name": tool_name, "args": args}));

            let h = handle.clone();
            let a = Arc::clone(&app);
            let (ok, payload) = match tokio::task::spawn_blocking(move || {
                tools::execute(&h, &a, &name, &args)
            })
            .await
            {
                Ok(Ok(v)) => (true, v),
                Ok(Err(e)) => (false, json!({"error": e})),
                Err(e) => (false, json!({"error": format!("join: {e}")})),
            };

            // images (extract_frames with_images) go to the model as a user
            // message right after the tool result — tool messages are text-only
            let (clean, images) = if ok { take_images(payload) } else { (payload, Vec::new()) };

            emit(json!({"type":"tool_result","name": tool_name, "ok": ok, "result": clean}));

            let mut tool_content = serde_json::to_string(&clean).unwrap_or_else(|_| "{}".into());
            if !images.is_empty() {
                tool_content.truncate(400);
                tool_content.push_str(" …frames attached as images in the next message");
            }
            chat.history.lock().unwrap().push(json!({
                "role": "tool",
                "tool_call_id": id,
                "content": tool_content,
            }));

            if !images.is_empty() {
                let mut content: Vec<Value> = vec![json!({
                    "type": "text",
                    "text": format!("Frames from {tool_name} (chronological order):")
                })];
                for img in &images {
                    let url = format!("data:{};base64,{}", img["media_type"].as_str().unwrap_or("image/jpeg"), img["data"].as_str().unwrap_or(""));
                    content.push(json!({"type": "image_url", "image_url": {"url": url}}));
                }
                chat.history.lock().unwrap().push(json!({
                    "role": "user",
                    "content": content,
                }));
            }
        }
        emit(json!({"type":"status","text":"thinking"}));
    }

    emit(json!({"type":"error","text": format!("stopped after {MAX_STEPS} tool steps — the edit may be incomplete. Ask me to continue.")}));
}

fn load_config(app: &Arc<App>) -> Option<ProviderConfig> {
    let path = app.cache_dir.join("settings.json");
    let v: Value = serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()?;
    let model = v["model"].as_str()?.to_string();
    if model.is_empty() {
        return None;
    }
    Some(ProviderConfig {
        kind: v["kind"].as_str().unwrap_or("openai").to_string(),
        base_url: v["base_url"].as_str().unwrap_or("https://api.openai.com/v1").to_string(),
        api_key: v["api_key"].as_str().unwrap_or("").to_string(),
        model,
    })
}

pub fn save_config(app: &Arc<App>, cfg: &ProviderConfig) -> Result<(), String> {
    std::fs::create_dir_all(&app.cache_dir).map_err(|e| e.to_string())?;
    let path = app.cache_dir.join("settings.json");
    std::fs::write(
        path,
        serde_json::to_string_pretty(cfg).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())
}

pub fn read_config(app: &Arc<App>) -> Value {
    load_config(app)
        .map(|c| serde_json::to_value(c).unwrap_or(json!({})))
        .unwrap_or(json!({"kind":"openai","base_url":"https://api.openai.com/v1","api_key":"","model":""}))
}

fn system_prompt_and_tools(app: &Arc<App>) -> (String, Vec<Value>) {
    let st = app.state.lock().unwrap();
    let p = &st.project;
    let summary = format!(
        "Project '{}': {}x{} @ {}fps, duration {}ms, {} assets, {} clips, {} text overlays.",
        p.name,
        p.width,
        p.height,
        p.fps,
        p.duration_ms(),
        p.assets.len(),
        p.clips.len(),
        p.texts.len()
    );
    let system = format!(
        "You are a video editing agent working inside the Media Agent editor. \
The user watches a live timeline — every tool call visibly changes it. \
All times are integer milliseconds. \
{summary}\
\nGuidance: for a quick assembly prefer assemble_sequence; for finer control use clip_add/clip_cut/clip_move. \
Talking-head cleanup flow: transcribe → CHECK TRANSCRIPT QUALITY → silence_detect → decide the cut table → remove_ranges (one call) → mask_jump_cuts → export. \
TRANSCRIPT QUALITY: after transcribe, read the quality field. If it has warnings (hallucination loops, low coverage, wrong model for the language) re-run transcribe with a bigger model BEFORE cutting — Persian needs 'medium'; 'small' hallucinates on fa. Never cut based on a suspicious transcript; tell the user instead. \
If you support vision, you can LOOK at the video: call extract_frames with with_images=true and the frames arrive as images — use this to understand the footage, verify edits, and describe content. \
Call project_get to see current state before complex edits. \
If the user doesn't give exact file paths, find the media yourself with find_media / list_dir instead of asking. \
For technique guidance (captions, pacing) call skills_list and skill_read. \
When the edit is done, tell the user; export only when asked. \
Keep your replies brief — never restate the project state in prose; the tools already show it."
    );
    let tools: Vec<Value> = tools::all_tools()
        .into_iter()
        .map(|t| {
            json!({
                "type": "function",
                "function": {
                    "name": t.name,
                    "description": t.description,
                    "parameters": t.params,
                }
            })
        })
        .collect();
    (system, tools)
}
