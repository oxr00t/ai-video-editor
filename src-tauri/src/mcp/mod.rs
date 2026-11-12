//! MCP stdio bridge (`app.exe --mcp`).
//!
//! Speaks the Model Context Protocol (JSON-RPC 2.0, one message per line on
//! stdin/stdout) and forwards tool calls to the running app's WS control
//! server — so an external agent edits the live project. Hand-rolled on
//! purpose: the skeleton only needs initialize / tools/list / tools/call.

use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use std::io::{BufRead, Write};
use tokio_tungstenite::tungstenite::Message;

const PROTOCOL_VERSION: &str = "2024-11-05";
const CALL_TIMEOUT_SECS: u64 = 15 * 60; // exports can be slow

pub fn run_bridge() {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("tokio runtime");

    let stdin = std::io::stdin();
    for line in stdin.lock().lines() {
        let line = match line {
            Ok(l) => l,
            Err(_) => break,
        };
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let req: Value = match serde_json::from_str(trimmed) {
            Ok(v) => v,
            Err(e) => {
                respond(json!({
                    "jsonrpc": "2.0", "id": Value::Null,
                    "error": {"code": -32700, "message": format!("parse error: {e}")}
                }));
                continue;
            }
        };

        // notifications get no response
        if req.get("id").is_none() || req["id"].is_null() {
            continue;
        }
        let id = req["id"].clone();
        let method = req["method"].as_str().unwrap_or("");
        let params = req.get("params").cloned().unwrap_or(json!({}));

        let result = match method {
            "initialize" => Ok(json!({
                "protocolVersion": PROTOCOL_VERSION,
                "capabilities": {"tools": {}},
                "serverInfo": {"name": "media-agent", "version": "0.1.0"}
            })),
            "ping" => Ok(json!({})),
            "tools/list" => Ok(json!({"tools": crate::tools::mcp_tools_json()})),
            "tools/call" => rt.block_on(call_tool(&params)),
            other => Err(json!({
                "code": -32601, "message": format!("method not found: {other}")
            })),
        };

        match result {
            Ok(r) => respond(json!({"jsonrpc": "2.0", "id": id, "result": r})),
            Err(e) => respond(json!({"jsonrpc": "2.0", "id": id, "error": e})),
        }
    }
}

async fn call_tool(params: &Value) -> Result<Value, Value> {
    let name = params["name"].as_str().unwrap_or("").to_string();
    let args = params.get("arguments").cloned().unwrap_or(json!({}));

    // find the running app's control port
    let mut last_err = String::new();
    for port in 7333..=7340 {
        let url = format!("ws://127.0.0.1:{port}");
        let connect = tokio_tungstenite::connect_async(url.clone());
        let ws = match tokio::time::timeout(std::time::Duration::from_secs(3), connect).await {
            Ok(Ok((ws, _))) => ws,
            Ok(Err(e)) => {
                last_err = format!("{url}: {e}");
                continue;
            }
            Err(_) => {
                last_err = format!("{url}: timeout");
                continue;
            }
        };
        let (mut sink, mut source) = ws.split();

        let req_id = chrono::Utc::now().timestamp_millis();
        let msg = json!({"id": req_id, "tool": name, "args": args});
        if sink.send(Message::Text(msg.to_string())).await.is_err() {
            last_err = format!("{url}: send failed");
            continue;
        }

        let deadline = std::time::Duration::from_secs(CALL_TIMEOUT_SECS);
        loop {
            let next = tokio::time::timeout(deadline, source.next()).await;
            match next {
                Ok(Some(Ok(Message::Text(t)))) => {
                    let v: Value = match serde_json::from_str(&t) {
                        Ok(v) => v,
                        Err(_) => continue,
                    };
                    if v.get("id").and_then(|i| i.as_i64()) == Some(req_id) {
                        let text = if v["ok"].as_bool().unwrap_or(false) {
                            // compact serialization — pretty-printing doubles the
                            // tokens every external agent pays per tool result
                            serde_json::to_string(&v["result"]).unwrap_or_default()
                        } else {
                            v["error"].as_str().unwrap_or("unknown error").to_string()
                        };
                        return Ok(json!({
                            "content": [{"type": "text", "text": text}],
                            "isError": !v["ok"].as_bool().unwrap_or(false),
                        }));
                    }
                    // project_changed broadcasts — ignore
                }
                Ok(Some(Ok(_))) => continue,
                Ok(Some(Err(e))) => {
                    return Err(json!({"code": -32000, "message": format!("ws read: {e}")}));
                }
                Ok(None) => break,
                Err(_) => {
                    return Err(json!({"code": -32000, "message": "tool call timed out"}));
                }
            }
        }
        last_err = format!("{url}: connection closed before response");
    }

    Err(json!({
        "code": -32000,
        "message": format!(
            "Media Agent app is not running (no control server found). \
             Start the Media Agent desktop app first, then retry. ({last_err})"
        )
    }))
}

fn respond(msg: Value) {
    let mut out = std::io::stdout().lock();
    let _ = writeln!(out, "{msg}");
    let _ = out.flush();
}
