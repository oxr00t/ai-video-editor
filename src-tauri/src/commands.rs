//! Tauri commands — the UI's entry points. Thin wrappers over the same
//! `tools::execute` the MCP bridge and internal agent use.

use crate::agent::providers::ProviderConfig;
use crate::agent::{self, ChatState};
use crate::state::App;
use crate::tools;
use serde_json::Value;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use tauri::{AppHandle, State};

/// Full EDL for the UI only — model-facing `project_get` stays compact.
#[tauri::command]
pub fn get_project(app: State<'_, Arc<App>>) -> Result<crate::edl::Project, String> {
    Ok(app.state.lock().unwrap().project.clone())
}

#[tauri::command]
pub async fn tool_invoke(
    handle: AppHandle,
    app: State<'_, Arc<App>>,
    name: String,
    args: Value,
) -> Result<Value, String> {
    let app = Arc::clone(app.inner());
    tokio::task::spawn_blocking(move || tools::execute(&handle, &app, &name, &args))
        .await
        .map_err(|e| format!("join error: {e}"))?
}

/// One tiny round-trip with the given config: verifies URL/key/model AND
/// that the model can do tool-calling. Returns the model's reply.
#[tauri::command]
pub async fn test_provider(config: crate::agent::providers::ProviderConfig) -> Result<String, String> {
    let tools = vec![serde_json::json!({
        "type": "function",
        "function": {
            "name": "ping",
            "description": "reply pong",
            "parameters": {"type": "object", "properties": {}, "required": []},
        }
    })];
    let messages = vec![serde_json::json!({
        "role": "user",
        "content": "Reply with exactly: OK (do not call any tool)",
    })];
    let msg = crate::agent::providers::call(&config, "You are a connection test.", &messages, &tools)
        .await?;
    Ok(msg["content"].as_str().unwrap_or("(empty reply)").to_string())
}

#[tauri::command]
pub fn get_provider_config(app: State<'_, Arc<App>>) -> Value {
    agent::read_config(&app)
}

#[tauri::command]
pub fn set_provider_config(app: State<'_, Arc<App>>, config: ProviderConfig) -> Result<(), String> {
    agent::save_config(&app, &config)
}

/// Query a router/provider for its available model ids (OpenAI-compatible
/// `/models`, or Anthropic's `/v1/models`). Used by the Settings dialog.
#[tauri::command]
pub async fn list_models(kind: String, base_url: String, api_key: String) -> Result<Vec<String>, String> {
    let client = reqwest::Client::new();
    let (url, is_anthropic) = match kind.as_str() {
        "anthropic" => (format!("{}/v1/models", base_url.trim_end_matches('/')), true),
        _ => (format!("{}/models", base_url.trim_end_matches('/')), false),
    };
    let mut req = client
        .get(&url)
        .timeout(std::time::Duration::from_secs(15));
    if is_anthropic {
        req = req.header("anthropic-version", "2023-06-01");
        if !api_key.is_empty() {
            req = req.header("x-api-key", &api_key);
        }
    } else if !api_key.is_empty() {
        req = req.bearer_auth(&api_key);
    }
    let resp = req.send().await.map_err(|e| format!("request to {url}: {e}"))?;
    let status = resp.status();
    let body: Value = resp.json().await.map_err(|e| format!("decode: {e}"))?;
    if !status.is_success() {
        let msg = body["error"]["message"]
            .as_str()
            .unwrap_or(&body.to_string())
            .to_string();
        return Err(format!("{} {msg}", status.as_u16()));
    }
    Ok(body["data"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|m| m["id"].as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default())
}

#[tauri::command]
pub fn agent_clear_chat(chat: State<'_, Arc<ChatState>>) {
    chat.history.lock().unwrap().clear();
}

#[tauri::command]
pub async fn agent_send(
    handle: AppHandle,
    app: State<'_, Arc<App>>,
    chat: State<'_, Arc<ChatState>>,
    message: String,
) -> Result<(), String> {
    if chat.busy.swap(true, Ordering::SeqCst) {
        return Err("agent is already working — wait for it to finish".into());
    }
    let app = Arc::clone(app.inner());
    let chat = Arc::clone(chat.inner());
    let h = handle.clone();
    tokio::spawn(async move {
        agent::run(h, app, chat.clone(), message).await;
        chat.busy.store(false, Ordering::SeqCst);
    });
    Ok(())
}
