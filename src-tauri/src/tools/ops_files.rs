//! Filesystem discovery tools — let agents locate media themselves instead
//! of demanding absolute paths from the user.

use crate::state::App;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::UNIX_EPOCH;
use tauri::AppHandle;

const MEDIA_EXTS: &[&str] = &[
    "mp4", "mov", "mkv", "avi", "webm", "m4v", "mpg", "mpeg", "wmv", "flv",
    "mp3", "wav", "aac", "flac", "ogg", "m4a", "opus",
    "jpg", "jpeg", "png", "gif", "webp", "bmp",
];

/// Directory names never worth descending into when searching for media.
const SKIP_DIRS: &[&str] = &[
    "node_modules", "target", "AppData", "Windows", "Program Files",
    "Program Files (x86)", "ProgramData", "$Recycle.Bin", "System Volume Information",
];

fn is_media(p: &Path) -> bool {
    p.extension()
        .and_then(|e| e.to_str())
        .map(|e| MEDIA_EXTS.contains(&e.to_ascii_lowercase().as_str()))
        .unwrap_or(false)
}

fn home_dir() -> PathBuf {
    std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("."))
}

fn resolve(path: &str) -> PathBuf {
    let trimmed = path.trim();
    if trimmed.is_empty() || trimmed == "~" {
        home_dir()
    } else {
        PathBuf::from(trimmed)
    }
}

fn modified_ms(md: &std::fs::Metadata) -> u64 {
    md.modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// List one directory (optionally a few levels deep). Returns (entries, truncated).
pub fn list_dir_impl(root: &Path, media_only: bool, depth: usize) -> Result<(Vec<Value>, bool), String> {
    if !root.is_dir() {
        return Err(format!("not a directory: {}", root.display()));
    }
    const CAP: usize = 500;
    let mut out = Vec::new();
    let mut truncated = false;
    let mut queue: Vec<(PathBuf, usize)> = vec![(root.to_path_buf(), 0)];
    while let Some((dir, level)) = queue.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for e in entries.flatten() {
            let path = e.path();
            let Ok(md) = e.metadata() else { continue };
            let name = e.file_name().to_string_lossy().to_string();
            let is_dir = md.is_dir();
            let media = !is_dir && is_media(&path);
            if media_only && !(media || is_dir) {
                continue;
            }
            if out.len() >= CAP {
                truncated = true;
                break;
            }
            out.push(json!({
                "name": name,
                "path": path.to_string_lossy(),
                "kind": if is_dir { "dir" } else { "file" },
                "media": media,
                "size": if is_dir { Value::Null } else { json!(md.len()) },
            }));
            // recurse into non-hidden dirs only
            if is_dir && level + 1 < depth {
                let skip = name.starts_with('.') || name.starts_with('$') || SKIP_DIRS.contains(&name.as_str());
                if !skip {
                    queue.push((path, level + 1));
                }
            }
        }
        if truncated {
            break;
        }
    }
    Ok((out, truncated))
}

/// Recursively find media files under root, newest first.
pub fn find_media_impl(root: &Path) -> Result<Vec<Value>, String> {
    if !root.is_dir() {
        return Err(format!("not a directory: {}", root.display()));
    }
    const CAP: usize = 200;
    const MAX_DEPTH: usize = 6;
    let mut found: Vec<(u64, Value)> = Vec::new(); // (modified_ms, entry)
    let mut stack: Vec<(PathBuf, usize)> = vec![(root.to_path_buf(), 0)];
    while let Some((dir, level)) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for e in entries.flatten() {
            let path = e.path();
            let Ok(md) = e.metadata() else { continue };
            if md.is_dir() {
                let name = e.file_name().to_string_lossy().to_string();
                if level + 1 <= MAX_DEPTH
                    && !name.starts_with('.')
                    && !name.starts_with('$')
                    && !SKIP_DIRS.contains(&name.as_str())
                {
                    stack.push((path, level + 1));
                }
            } else if is_media(&path) {
                let m = modified_ms(&md);
                found.push((
                    m,
                    json!({
                        "path": path.to_string_lossy(),
                        "name": e.file_name().to_string_lossy(),
                        "size": md.len(),
                        "modified_ms": m,
                    }),
                ));
            }
        }
    }
    found.sort_by(|a, b| b.0.cmp(&a.0));
    Ok(found.into_iter().take(CAP).map(|(_, v)| v).collect())
}

pub fn list_dir(_h: &AppHandle, _app: &Arc<App>, args: &Value) -> Result<Value, String> {
    let path = args["path"].as_str().unwrap_or("");
    let media_only = args["media_only"].as_bool().unwrap_or(false);
    let depth = args["depth"].as_u64().unwrap_or(1).clamp(1, 4) as usize;
    let (entries, truncated) = list_dir_impl(&resolve(path), media_only, depth)?;
    Ok(json!({"entries": entries, "truncated": truncated}))
}

pub fn find_media(_h: &AppHandle, _app: &Arc<App>, args: &Value) -> Result<Value, String> {
    let root = args["root"].as_str().unwrap_or("");
    let files = find_media_impl(&resolve(root))?;
    Ok(json!({"files": files}))
}
