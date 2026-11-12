//! Local WebSocket control server (127.0.0.1, port 7333..7340).
//!
//! The MCP bridge process (`app --mcp`) connects here and forwards tool
//! calls, so external agents (Claude Code, ZCode, Cursor, …) edit the same
//! live project the user is watching. Every project change is also pushed
//! to connected clients as a `project_changed` message.

use crate::state::App;
use crate::tools;
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use std::sync::Arc;
use tauri::AppHandle;
use tokio::net::{TcpListener, TcpStream};
use tokio_tungstenite::tungstenite::Message;

pub async fn start(handle: AppHandle, app: Arc<App>) {
    let mut bound: Option<u16> = None;
    for port in 7333..=7340 {
        if let Ok(listener) = TcpListener::bind(("127.0.0.1", port)).await {
            bound = Some(port);
            let _ = listener;
            // rebind below in the spawned task; just detect availability
            break;
        }
    }
    let port = match bound {
        Some(p) => p,
        None => {
            eprintln!("control server: no free port in 7333..7340");
            return;
        }
    };
    app.control_port
        .store(port, std::sync::atomic::Ordering::SeqCst);
    println!("control server: ws://127.0.0.1:{port}");

    let listener = match TcpListener::bind(("127.0.0.1", port)).await {
        Ok(l) => l,
        Err(e) => {
            eprintln!("control server bind failed: {e}");
            return;
        }
    };

    loop {
        if let Ok((stream, _)) = listener.accept().await {
            let handle = handle.clone();
            let app = Arc::clone(&app);
            tokio::spawn(async move {
                if let Err(e) = serve_connection(stream, handle, app).await {
                    eprintln!("control connection error: {e}");
                }
            });
        }
    }
}

async fn serve_connection(stream: TcpStream, handle: AppHandle, app: Arc<App>) -> Result<(), String> {
    let ws = tokio_tungstenite::accept_async(stream)
        .await
        .map_err(|e| format!("ws handshake: {e}"))?;
    let (mut sink, mut source) = ws.split();
    let mut rx = app.ws_broadcast.subscribe();

    loop {
        tokio::select! {
            // incoming requests from the bridge / other clients
            msg = source.next() => {
                let msg = match msg {
                    Some(Ok(Message::Text(t))) => t,
                    Some(Ok(_)) => continue, // ping/pong/binary handled by lib
                    Some(Err(e)) => return Err(format!("ws read: {e}")),
                    None => return Ok(()), // client closed
                };
                let req: Value = match serde_json::from_str(&msg) {
                    Ok(v) => v,
                    Err(e) => {
                        let _ = sink
                            .send(Message::Text(json!({"error": format!("bad json: {e}")}).to_string()))
                            .await;
                        continue;
                    }
                };
                let tool = req["tool"].as_str().unwrap_or("").to_string();
                let args = req["args"].clone();
                let req_id = req["id"].clone();
                let handle = handle.clone();
                let app = Arc::clone(&app);
                let resp = match tokio::task::spawn_blocking(move || {
                    tools::execute(&handle, &app, &tool, &args)
                })
                .await
                {
                    Ok(Ok(result)) => json!({"id": req_id, "ok": true, "result": result}),
                    Ok(Err(e)) => json!({"id": req_id, "ok": false, "error": e}),
                    Err(e) => json!({"id": req_id, "ok": false, "error": format!("join error: {e}")}),
                };
                let _ = sink.send(Message::Text(resp.to_string())).await;
            }
            // project change broadcasts
            push = rx.recv() => {
                match push {
                    Ok(project) => {
                        let msg = json!({"type": "project_changed", "project": project});
                        if sink.send(Message::Text(msg.to_string())).await.is_err() {
                            return Ok(());
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(_) => return Ok(()),
                }
            }
        }
    }
}
