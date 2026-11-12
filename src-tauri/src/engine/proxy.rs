//! Proxy generation: a small 360p mp4 per asset so the preview pane can seek
//! instantly without touching the heavy source file.

use crate::engine::run_capture;
use std::path::{Path, PathBuf};
use std::process::Command;

pub fn proxy_path(cache_dir: &Path, asset_id: &str) -> PathBuf {
    cache_dir.join("proxies").join(format!("{asset_id}.mp4"))
}

/// Returns the proxy path on success (blocking — call from a worker thread).
pub fn generate(src: &str, dst: &Path, has_audio: bool) -> Result<String, String> {
    if let Some(parent) = dst.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let dst_str = dst.to_string_lossy().to_string();

    let mut cmd = Command::new("ffmpeg");
    cmd.args(["-y", "-loglevel", "error", "-i", src])
        .args(["-vf", "scale=-2:360"])
        .args(["-c:v", "libx264", "-preset", "veryfast", "-crf", "28"])
        .args(["-c:a", "aac", "-b:a", "96k"]);
    if !has_audio {
        cmd.arg("-an");
    }
    cmd.arg(&dst_str);
    run_capture(&mut cmd)?;
    Ok(dst_str)
}
