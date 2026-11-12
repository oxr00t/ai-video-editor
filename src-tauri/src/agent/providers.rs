//! Model providers. Two shapes cover most of the market:
//! - "openai": any OpenAI-compatible /chat/completions endpoint
//!   (OpenAI, Ollama http://localhost:11434/v1, OpenRouter, LM Studio, …)
//! - "anthropic": the /v1/messages API
//!
//! Internally the agent keeps messages in OpenAI format; the Anthropic
//! branch converts in and out.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ProviderConfig {
    pub kind: String,
    pub base_url: String,
    pub api_key: String,
    pub model: String,
}

/// One model turn. Returns an OpenAI-format assistant message:
/// `{role:"assistant", content: Option<String>, tool_calls?: [...]}`.
pub async fn call(
    cfg: &ProviderConfig,
    system: &str,
    messages: &[Value],
    tools: &[Value],
) -> Result<Value, String> {
    match cfg.kind.as_str() {
        "anthropic" => anthropic(cfg, system, messages, tools).await,
        _ => openai(cfg, system, messages, tools).await,
    }
}

/// Some routers (e.g. 9Router combo models) return SSE (`data: {...}` lines)
/// even without `stream: true`. Aggregate chunks into one complete message,
/// the way the official SDKs do.
fn parse_completion_body(raw: &str) -> Result<Value, String> {
    if raw.trim_start().starts_with("data:") {
        let mut content = String::new();
        let mut tool_calls: Vec<Value> = Vec::new();
        for line in raw.lines() {
            let Some(payload) = line.strip_prefix("data:") else { continue };
            let payload = payload.trim();
            if payload.is_empty() || payload == "[DONE]" {
                continue;
            }
            let Ok(chunk) = serde_json::from_str::<Value>(payload) else { continue };
            let Some(delta) = chunk["choices"][0].get("delta") else { continue };
            if let Some(c) = delta["content"].as_str() {
                content.push_str(c);
            }
            if let Some(tcs) = delta["tool_calls"].as_array() {
                for tc in tcs {
                    let idx = tc["index"].as_u64().unwrap_or(0) as usize;
                    while tool_calls.len() <= idx {
                        tool_calls.push(json!({"id":"","type":"function","function":{"name":"","arguments":""}}));
                    }
                    let slot = &mut tool_calls[idx];
                    if let Some(id) = tc["id"].as_str() {
                        if !id.is_empty() { slot["id"] = json!(id); }
                    }
                    if let Some(n) = tc["function"]["name"].as_str() {
                        if !n.is_empty() { slot["function"]["name"] = json!(n); }
                    }
                    if let Some(a) = tc["function"]["arguments"].as_str() {
                        let merged = format!("{}{}", slot["function"]["arguments"].as_str().unwrap_or(""), a);
                        slot["function"]["arguments"] = json!(merged);
                    }
                }
            }
        }
        let mut msg = json!({"role": "assistant", "content": content});
        if !tool_calls.is_empty() {
            msg["tool_calls"] = json!(tool_calls);
        }
        return Ok(json!({"choices": [{"message": msg}]}));
    }
    serde_json::from_str(raw).map_err(|e| format!("non-JSON body ({e})"))
}

fn snippet(raw: &str, max: usize) -> String {
    let s: String = raw.chars().take(max).collect();
    if raw.chars().count() > max {
        format!("{s}…")
    } else {
        s
    }
}

/// "data:image/jpeg;base64,AAA…" → ("image/jpeg", "AAA…")
fn split_data_url(url: &str) -> (String, String) {
    match url.strip_prefix("data:").and_then(|r| r.split_once(";base64,")) {
        Some((media, data)) => (media.to_string(), data.to_string()),
        None => ("image/jpeg".into(), url.to_string()),
    }
}

async fn openai(cfg: &ProviderConfig, system: &str, messages: &[Value], tools: &[Value]) -> Result<Value, String> {
    let url = format!("{}/chat/completions", cfg.base_url.trim_end_matches('/'));
    let client = reqwest::Client::new();
    let mut all_messages = vec![json!({"role":"system","content": system})];
    all_messages.extend(messages.iter().cloned());
    let mut req = client
        .post(&url)
        .timeout(std::time::Duration::from_secs(600))
        .json(&json!({
            "model": cfg.model,
            "messages": all_messages,
            "tools": tools,
            "tool_choice": "auto",
        }));
    if !cfg.api_key.is_empty() {
        req = req.bearer_auth(&cfg.api_key);
    }
    let resp = req.send().await.map_err(|e| format!("{e}"))?;
    let status = resp.status();
    let raw = resp
        .text()
        .await
        .map_err(|e| format!("read body: {e}"))?;

    let body = parse_completion_body(&raw)
        .map_err(|e| format!("{} {e} — body: «{}»", status.as_u16(), snippet(&raw, 200)))?;

    if !status.is_success() {
        let msg = body["error"]["message"]
            .as_str()
            .map(|s| s.to_string())
            .unwrap_or_else(|| snippet(&raw, 200));
        return Err(format!("{} {msg}", status.as_u16()));
    }
    let mut msg = body["choices"][0]["message"].clone();
    if msg.is_null() {
        return Err(format!("no message in response — body: «{}»", snippet(&raw, 200)));
    }
    if msg.get("content").map(|c| c.is_null()).unwrap_or(true) {
        msg["content"] = json!("");
    }
    Ok(msg)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sse_response_is_aggregated() {
        // exact shape 9Router returns for combo/alias models without stream:true
        let raw = "data: {\"id\":\"x\",\"choices\":[{\"index\":0,\"delta\":{\"role\":\"assistant\"}}]}\n\n\
data: {\"id\":\"x\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"Hi \"}}]}\n\
data: {\"id\":\"x\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"there\"}}]}\n\
data: {\"id\":\"x\",\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_1\",\"type\":\"function\",\"function\":{\"name\":\"ping\",\"arguments\":\"\"}}]}}]}\n\
data: {\"id\":\"x\",\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"{\\\"a\\\":1}\"}}]}}]}\n\
data: [DONE]\n";
        let body = parse_completion_body(raw).expect("sse parse");
        let msg = &body["choices"][0]["message"];
        assert_eq!(msg["content"].as_str().unwrap(), "Hi there");
        assert_eq!(msg["tool_calls"][0]["id"].as_str().unwrap(), "call_1");
        assert_eq!(msg["tool_calls"][0]["function"]["name"].as_str().unwrap(), "ping");
        assert_eq!(msg["tool_calls"][0]["function"]["arguments"].as_str().unwrap(), "{\"a\":1}");
    }

    #[test]
    fn plain_json_still_works() {
        let raw = r#"{"choices":[{"message":{"role":"assistant","content":"OK"}}]}"#;
        let body = parse_completion_body(raw).expect("json parse");
        assert_eq!(body["choices"][0]["message"]["content"].as_str().unwrap(), "OK");
    }

    #[test]
    fn bad_body_reports_error() {
        assert!(parse_completion_body("<html>oops</html>").is_err());
    }
}

async fn anthropic(cfg: &ProviderConfig, system: &str, messages: &[Value], tools: &[Value]) -> Result<Value, String> {
    let url = format!("{}/v1/messages", cfg.base_url.trim_end_matches('/'));
    let client = reqwest::Client::new();

    let a_tools: Vec<Value> = tools
        .iter()
        .map(|t| {
            json!({
                "name": t["function"]["name"],
                "description": t["function"]["description"],
                "input_schema": t["function"]["parameters"],
            })
        })
        .collect();
    // cache breakpoint at the end of the tools block
    let mut a_tools = a_tools;
    if let Some(last) = a_tools.last_mut() {
        last["cache_control"] = json!({"type": "ephemeral"});
    }

    let mut a_messages: Vec<Value> = Vec::new();
    for m in messages {
        match m["role"].as_str() {
            Some("user") => {
                let content: Vec<Value> = if let Some(blocks) = m["content"].as_array() {
                    // multimodal user message (text + image_url) → anthropic blocks
                    blocks
                        .iter()
                        .map(|b| {
                            if b["type"] == "image_url" {
                                let url = b["image_url"]["url"].as_str().unwrap_or("");
                                let (media, data) = split_data_url(url);
                                json!({"type": "image", "source": {"type": "base64", "media_type": media, "data": data}})
                            } else {
                                json!({"type": "text", "text": b["text"].as_str().unwrap_or("")})
                            }
                        })
                        .collect()
                } else {
                    vec![json!({"type": "text", "text": m["content"].as_str().unwrap_or("")})]
                };
                a_messages.push(json!({"role": "user", "content": content}));
            }
            Some("assistant") => {
                let mut blocks = Vec::new();
                if let Some(t) = m["content"].as_str() {
                    if !t.is_empty() {
                        blocks.push(json!({"type":"text","text": t}));
                    }
                }
                if let Some(calls) = m["tool_calls"].as_array() {
                    for c in calls {
                        let input: Value = serde_json::from_str(
                            c["function"]["arguments"].as_str().unwrap_or("{}"),
                        )
                        .unwrap_or(json!({}));
                        blocks.push(json!({
                            "type": "tool_use",
                            "id": c["id"],
                            "name": c["function"]["name"],
                            "input": input,
                        }));
                    }
                }
                a_messages.push(json!({"role":"assistant","content": blocks}));
            }
            Some("tool") => a_messages.push(json!({
                "role": "user",
                "content": [{
                    "type": "tool_result",
                    "tool_use_id": m["tool_call_id"],
                    "content": m["content"],
                }]
            })),
            _ => {}
        }
    }

    let mut req = client
        .post(&url)
        .timeout(std::time::Duration::from_secs(600))
        .header("anthropic-version", "2023-06-01")
        .json(&json!({
            "model": cfg.model,
            "max_tokens": 4096,
            // ephemeral prompt caching: system+tools are identical every turn,
            // so they bill at cache-read rates instead of full input price
            "system": [{
                "type": "text",
                "text": system,
                "cache_control": {"type": "ephemeral"}
            }],
            "messages": a_messages,
            "tools": a_tools,
        }));
    if !cfg.api_key.is_empty() {
        req = req.header("x-api-key", &cfg.api_key);
    }
    let resp = req.send().await.map_err(|e| format!("{e}"))?;
    let status = resp.status();
    let raw = resp
        .text()
        .await
        .map_err(|e| format!("read body: {e}"))?;
    let body: Value = serde_json::from_str(&raw)
        .map_err(|e| format!("{} non-JSON body ({e}) — body: «{}»", status.as_u16(), snippet(&raw, 200)))?;
    if !status.is_success() {
        let msg = body["error"]["message"]
            .as_str()
            .map(|s| s.to_string())
            .unwrap_or_else(|| snippet(&raw, 200));
        return Err(format!("{} {msg}", status.as_u16()));
    }

    // convert back to OpenAI shape
    let mut content = String::new();
    let mut tool_calls = Vec::new();
    if let Some(blocks) = body["content"].as_array() {
        for b in blocks {
            match b["type"].as_str() {
                Some("text") => content.push_str(b["text"].as_str().unwrap_or("")),
                Some("tool_use") => tool_calls.push(json!({
                    "id": b["id"],
                    "type": "function",
                    "function": {
                        "name": b["name"],
                        "arguments": serde_json::to_string(&b["input"]).unwrap_or_else(|_| "{}".into()),
                    }
                })),
                _ => {}
            }
        }
    }
    let mut msg = json!({"role":"assistant","content": content});
    if !tool_calls.is_empty() {
        msg["tool_calls"] = json!(tool_calls);
    }
    Ok(msg)
}
