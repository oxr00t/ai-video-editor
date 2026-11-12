//! Local Whisper transcription (openai-whisper CLI): extract 16k mono WAV,
//! run word-level ASR, store the full transcript on disk, and hand models a
//! compact line-grouped view (token-efficient — the full word grid stays on
//! disk for tools like remove_ranges to snap cuts to word boundaries).

use crate::engine::run_capture;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::Command;

pub struct Transcript {
    pub words: Vec<(u64, u64, String)>, // (start_ms, end_ms, word)
}

pub fn wav_path(cache_dir: &Path, asset_id: &str) -> PathBuf {
    cache_dir.join("transcripts").join(format!("{asset_id}.wav"))
}

pub fn json_path(cache_dir: &Path, asset_id: &str) -> PathBuf {
    cache_dir.join("transcripts").join(format!("{asset_id}.json"))
}

/// ffmpeg: 16kHz mono PCM — exactly what whisper wants.
pub fn extract_wav(src: &str, dst: &Path) -> Result<(), String> {
    if let Some(parent) = dst.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let mut cmd = Command::new("ffmpeg");
    cmd.args(["-y", "-loglevel", "error", "-i", src])
        .args(["-vn", "-acodec", "pcm_s16le", "-ar", "16000", "-ac", "1"])
        .arg(dst.to_string_lossy().to_string());
    run_capture(&mut cmd).map(|_| ())
}

/// Resolve the whisper launcher once. `python -m whisper` is preferred over
/// the whisper.exe console shim — the shim proved unreliable when spawned
/// from the GUI process (exited 0 without writing output).
fn whisper_command() -> Command {
    static USE_PYTHON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    let use_python = *USE_PYTHON.get_or_init(|| {
        Command::new("python")
            .args(["-c", "import whisper"])
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    });
    let mut c = if use_python {
        let mut c = Command::new("python");
        c.arg("-m").arg("whisper");
        c
    } else {
        Command::new("whisper")
    };
    // CRITICAL: the GUI process runs under the system codepage (cp1256 on
    // Persian Windows). whisper PRINTS each transcribed line and dies with
    // UnicodeEncodeError on Persian characters — exiting 0 with NO output.
    // Forcing UTF-8 for all Python IO fixes it.
    c.env("PYTHONUTF8", "1").env("PYTHONIOENCODING", "utf-8");
    c
}

/// Blocking whisper run — call from a worker thread. `model` e.g. "medium".
pub fn run_whisper(
    wav: &Path,
    model: &str,
    language: Option<&str>,
    initial_prompt: Option<&str>,
    out_dir: &Path,
) -> Result<PathBuf, String> {
    std::fs::create_dir_all(out_dir).map_err(|e| e.to_string())?;
    let wav_str = wav.to_string_lossy().to_string();
    let mut cmd = whisper_command();
    cmd.arg(&wav_str)
        .args(["--model", model])
        .args(["--word_timestamps", "True"])
        .args(["--output_format", "json"])
        .arg("--output_dir")
        .arg(out_dir.to_string_lossy().to_string())
        .args(["--task", "transcribe"])
        .args(["--fp16", "False"]); // CPU torch
    if let Some(lang) = language {
        cmd.args(["--language", lang]);
    }
    if let Some(p) = initial_prompt.filter(|p| !p.trim().is_empty()) {
        cmd.args(["--initial_prompt", p]);
    }
    let started = std::time::SystemTime::now();
    let (stdout, stderr) = run_capture(&mut cmd)?;

    // postmortem log for exactly this kind of mystery
    let _ = std::fs::write(
        out_dir.join("whisper_last_run.log"),
        format!("CMD: {wav_str} model={model}\n--- stdout ---\n{stdout}\n--- stderr ---\n{stderr}"),
    );

    let stem = wav.file_stem().unwrap_or_default().to_string_lossy().to_string();
    let expected = out_dir.join(format!("{stem}.json"));
    if expected.exists() {
        return Ok(expected);
    }
    // fallback: newest json written DURING this run (naming surprises only —
    // never pick up some older transcript from another asset)
    if let Ok(entries) = std::fs::read_dir(out_dir) {
        let mut best: Option<(std::time::SystemTime, PathBuf)> = None;
        for e in entries.flatten() {
            let p = e.path();
            if p.extension().and_then(|x| x.to_str()) == Some("json") {
                if let Ok(t) = e.metadata().and_then(|m| m.modified()) {
                    if t >= started && best.as_ref().map(|(bt, _)| t > *bt).unwrap_or(true) {
                        best = Some((t, p));
                    }
                }
            }
        }
        if let Some((_, p)) = best {
            return Ok(p);
        }
    }
    let tail: String = stderr
        .lines()
        .rev()
        .take(6)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect::<Vec<_>>()
        .join("\n");
    Err(format!(
        "whisper exited ok but wrote no json. last output: {}",
        if tail.is_empty() { stdout.lines().last().unwrap_or("").to_string() } else { tail }
    ))
}

/// Parse whisper's JSON into words + compact lines.
pub fn parse(raw: &str) -> Result<(Transcript, Vec<Value>), String> {
    let v: Value = serde_json::from_str(raw).map_err(|e| format!("whisper json parse: {e}"))?;
    let mut words = Vec::new();
    if let Some(segs) = v["segments"].as_array() {
        for seg in segs {
            if let Some(ws) = seg["words"].as_array() {
                for w in ws {
                    let text = w["word"].as_str().unwrap_or("").trim().to_string();
                    if text.is_empty() {
                        continue;
                    }
                    let s = w["start"].as_f64().unwrap_or(0.0);
                    let e = w["end"].as_f64().unwrap_or(s);
                    words.push((
                        (s * 1000.0).round() as u64,
                        (e * 1000.0).round() as u64,
                        text,
                    ));
                }
            }
        }
    }
    let lines = compact_lines(&words);
    Ok((Transcript { words }, lines))
}

/// Group words into readable lines: break on >0.6s gaps or sentence enders.
fn compact_lines(words: &[(u64, u64, String)]) -> Vec<Value> {
    const GAP_MS: u64 = 600;
    const ENDERS: [&str; 6] = [".", "!", "?", "؟", "۔", ":"];
    let mut lines: Vec<Value> = Vec::new();
    let mut cur_text = String::new();
    let mut cur_start: Option<u64> = None;
    let mut cur_end = 0u64;
    let mut prev_end: Option<u64> = None;

    let flush = |lines: &mut Vec<Value>, text: &mut String, s: Option<u64>, e: u64| {
        let t = text.trim().to_string();
        if !t.is_empty() {
            lines.push(json!({"s": s.unwrap_or(0), "e": e, "t": t}));
        }
        text.clear();
    };

    for (s, e, w) in words {
        let break_line = prev_end
            .map(|pe| s.saturating_sub(pe) > GAP_MS)
            .unwrap_or(false)
            || cur_text.chars().count() > 80;
        if break_line {
            flush(&mut lines, &mut cur_text, cur_start, cur_end);
            cur_start = None;
        }
        if cur_start.is_none() {
            cur_start = Some(*s);
        }
        cur_text.push_str(w);
        cur_text.push(' ');
        cur_end = *e;
        prev_end = Some(*e);
        if ENDERS.iter().any(|x| w.ends_with(x)) && cur_text.chars().count() > 24 {
            flush(&mut lines, &mut cur_text, cur_start, cur_end);
            cur_start = None;
            prev_end = None;
        }
    }
    flush(&mut lines, &mut cur_text, cur_start, cur_end);
    lines
}

/// Full transcript file the tools read back (words stay on disk, not in context).
pub fn store(
    json_path: &Path,
    transcript: &Transcript,
    lines: &[Value],
    meta: Value,
) -> Result<(), String> {
    let words: Vec<Value> = transcript
        .words
        .iter()
        .map(|(s, e, t)| json!({"s": s, "e": e, "t": t}))
        .collect();
    let doc = json!({
        "meta": meta,
        "lines": lines,
        "words": words,
    });
    std::fs::write(json_path, serde_json::to_vec(&doc).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())
}

/// Load stored transcript words for an asset (None if never transcribed).
pub fn load_words(json_path: &Path) -> Option<Vec<(u64, u64, String)>> {
    let raw = std::fs::read_to_string(json_path).ok()?;
    let v: Value = serde_json::from_str(&raw).ok()?;
    let words = v["words"].as_array()?;
    Some(
        words
            .iter()
            .filter_map(|w| {
                Some((
                    w["s"].as_u64()?,
                    w["e"].as_u64()?,
                    w["t"].as_str()?.to_string(),
                ))
            })
            .collect(),
    )
}
