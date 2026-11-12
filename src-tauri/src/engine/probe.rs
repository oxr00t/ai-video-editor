//! ffprobe: read media metadata into an Asset.

use crate::edl::Asset;
use crate::engine::run_capture;
use serde_json::Value;
use std::path::Path;
use std::process::Command;

pub fn probe(path: &str) -> Result<Asset, String> {
    let p = Path::new(path);
    if !p.exists() {
        return Err(format!("file not found: {path}"));
    }

    let mut cmd = Command::new("ffprobe");
    cmd.args([
        "-v", "error",
        "-print_format", "json",
        "-show_format",
        "-show_streams",
        path,
    ]);
    let (stdout, _) = run_capture(&mut cmd)?;
    let v: Value = serde_json::from_str(&stdout).map_err(|e| format!("ffprobe output parse: {e}"))?;

    let format = &v["format"];
    let duration_s = format["duration"]
        .as_str()
        .and_then(|s| s.parse::<f64>().ok())
        .or_else(|| format["duration"].as_f64())
        .ok_or_else(|| format!("no duration reported for {path}"))?;
    if !(duration_s.is_finite() && duration_s > 0.0) {
        return Err(format!("invalid duration for {path}"));
    }

    let mut width = 0u32;
    let mut height = 0u32;
    let mut fps = 0.0f64;
    let mut has_audio = false;
    let mut has_video = false;

    if let Some(streams) = v["streams"].as_array() {
        for s in streams {
            match s["codec_type"].as_str() {
                Some("video") if !has_video => {
                    has_video = true;
                    width = s["width"].as_u64().unwrap_or(0) as u32;
                    height = s["height"].as_u64().unwrap_or(0) as u32;
                    // avg_frame_rate is "num/den"
                    if let Some(r) = s["avg_frame_rate"].as_str() {
                        let parts: Vec<&str> = r.split('/').collect();
                        if parts.len() == 2 {
                            if let (Ok(n), Ok(d)) = (parts[0].parse::<f64>(), parts[1].parse::<f64>()) {
                                if d > 0.0 {
                                    fps = n / d;
                                }
                            }
                        }
                    }
                }
                Some("audio") => has_audio = true,
                _ => {}
            }
        }
    }

    if !has_video {
        return Err(format!("no video stream in {path} (audio-only import is v1)"));
    }

    Ok(Asset {
        id: crate::edl::new_id(),
        name: p
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "asset".into()),
        path: p
            .canonicalize()
            .map(|c| c.to_string_lossy().to_string())
            .unwrap_or_else(|_| path.to_string()),
        proxy_path: None,
        duration_ms: (duration_s * 1000.0).round() as u64,
        width,
        height,
        fps: if fps > 0.0 { fps } else { 30.0 },
        has_audio,
    })
}
