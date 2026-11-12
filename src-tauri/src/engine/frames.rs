//! Frame extraction for the vision QA loop: the model picks moments
//! (element in/out points, or every N ms) and either reads the JPEGs itself
//! (MCP path) or gets them attached as base64 images (internal agent).

use crate::engine::run_capture;
use std::path::{Path, PathBuf};
use std::process::Command;

pub fn extract(src: &str, times_ms: &[u64], out_dir: &Path) -> Result<Vec<PathBuf>, String> {
    std::fs::create_dir_all(out_dir).map_err(|e| e.to_string())?;
    let mut paths = Vec::new();
    for (i, t) in times_ms.iter().enumerate() {
        let out = out_dir.join(format!("frame_{i:03}_{}.jpg", ms_s(*t)));
        let mut cmd = Command::new("ffmpeg");
        cmd.args(["-y", "-loglevel", "error", "-ss", &ms_s(*t), "-i", src])
            .args(["-frames:v", "1", "-q:v", "2"])
            .arg(out.to_string_lossy().to_string());
        run_capture(&mut cmd)?;
        paths.push(out);
    }
    Ok(paths)
}

/// Small (≤512px) JPEG copy of a frame, base64-encoded for model vision.
pub fn encode_small_b64(src: &Path) -> Result<String, String> {
    let small = src.with_extension("small.jpg");
    let mut cmd = Command::new("ffmpeg");
    cmd.args(["-y", "-loglevel", "error", "-i"])
        .arg(src.to_string_lossy().to_string())
        .args(["-vf", "scale=512:-2", "-q:v", "5"])
        .arg(small.to_string_lossy().to_string());
    run_capture(&mut cmd)?;
    let bytes = std::fs::read(&small).map_err(|e| e.to_string())?;
    let _ = std::fs::remove_file(&small);
    Ok(base64::Engine::encode(&base64::engine::general_purpose::STANDARD, bytes))
}

fn ms_s(ms: u64) -> String {
    format!("{:.3}", ms as f64 / 1000.0)
}
