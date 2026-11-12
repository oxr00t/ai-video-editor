//! ffmpeg orchestration: probe, proxy generation, transcription, export.

pub mod export;
pub mod frames;
pub mod probe;
pub mod proxy;
pub mod silence;
pub mod transcribe;

use std::process::Command;

/// Runs a command, capturing stdout; returns (stdout, stderr, exit_ok).
pub fn run_capture(cmd: &mut Command) -> Result<(String, String), String> {
    let out = cmd
        .output()
        .map_err(|e| format!("failed to spawn {:?}: {e}", cmd.get_program()))?;
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    let stderr = String::from_utf8_lossy(&out.stderr).to_string();
    if !out.status.success() {
        return Err(format!(
            "{} exited with {:?}\n--- stderr ---\n{}",
            cmd.get_program().to_string_lossy(),
            out.status.code(),
            stderr
        ));
    }
    Ok((stdout, stderr))
}
