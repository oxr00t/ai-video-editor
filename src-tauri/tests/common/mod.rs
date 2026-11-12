//! Shared test fixture helper: test clips are NOT committed to git — they
//! are generated deterministically with ffmpeg on first use, so `cargo test`
//! works on a fresh clone with zero setup (ffmpeg on PATH is already a
//! project prerequisite).

use std::path::PathBuf;
use std::process::Command;

pub fn test_media_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../test-media")
}

/// Path to a test fixture, generating the standard set if missing.
pub fn test_media(name: &str) -> String {
    ensure_test_media();
    test_media_dir().join(name).to_string_lossy().to_string()
}

pub fn ensure_test_media() {
    let dir = test_media_dir();
    let a = dir.join("clip_a.mp4");
    let b = dir.join("clip_b.mp4");
    if a.exists() && b.exists() {
        return;
    }
    std::fs::create_dir_all(&dir).expect("create test-media dir");
    gen(&a, "testsrc2=size=1280x720:rate=30:duration=4", 440, 4);
    gen(&b, "smptebars=size=1280x720:rate=30:duration=3", 880, 3);
}

fn gen(dst: &std::path::Path, video_src: &str, freq: u32, dur: u32) {
    let status = Command::new("ffmpeg")
        .args(["-y", "-loglevel", "error"])
        .arg("-f").arg("lavfi").arg("-i").arg(video_src)
        .arg("-f").arg("lavfi")
        .arg("-i").arg(format!("sine=frequency={freq}:duration={dur}"))
        .args(["-c:v", "libx264", "-preset", "veryfast", "-c:a", "aac", "-shortest"])
        .arg(dst.to_string_lossy().to_string())
        .status();
    assert!(
        status.map(|s| s.success()).unwrap_or(false),
        "failed to generate fixture {} — is ffmpeg on PATH?",
        dst.display()
    );
}
