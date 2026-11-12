//! Silence detection via ffmpeg silencedetect — catches the pauses Whisper's
//! text usually swallows (the workflow's exact technique).

use crate::engine::run_capture;
use std::process::Command;

/// Detect silences in a WAV. Returns (start_ms, end_ms) ranges.
pub fn detect(wav: &str, threshold_db: f32, min_duration_s: f32) -> Result<Vec<(u64, u64)>, String> {
    let filter = format!("silencedetect=noise={threshold_db}dB:d={min_duration_s}");
    let mut cmd = Command::new("ffmpeg");
    cmd.args(["-i", wav, "-af", &filter, "-f", "null", "-"]);
    // silencedetect reports on stderr
    let out = cmd.output().map_err(|e| format!("spawn ffmpeg: {e}"))?;
    let stderr = String::from_utf8_lossy(&out.stderr).to_string();
    if !out.status.success() && !stderr.contains("silencedetect") {
        return Err(format!("ffmpeg silencedetect failed: {stderr}"));
    }

    let mut starts: Vec<f64> = Vec::new();
    let mut ranges: Vec<(u64, u64)> = Vec::new();
    let number_after = |line: &str, marker: &str| -> Option<f64> {
        let rest = line.split(marker).nth(1)?;
        let v: String = rest
            .trim_start()
            .chars()
            .take_while(|c| *c != '|' && !c.is_whitespace())
            .collect();
        v.parse::<f64>().ok()
    };
    for line in stderr.lines() {
        if line.contains("silence_start:") {
            if let Some(s) = number_after(line, "silence_start:") {
                starts.push(s);
            }
        } else if line.contains("silence_end:") {
            if let Some(e) = number_after(line, "silence_end:") {
                if let Some(s) = starts.pop() {
                    ranges.push(((s * 1000.0).round() as u64, (e * 1000.0).round() as u64));
                }
            }
        }
    }
    ranges.sort();
    Ok(ranges)
}
