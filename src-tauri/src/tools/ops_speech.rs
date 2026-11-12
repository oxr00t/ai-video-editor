//! Talking-head speech tools: transcription, silence analysis, the
//! remove_ranges cut-table recipe, jump-cut zoom masking, and frame
//! extraction for the vision QA loop. All heavy lifting is local
//! (whisper/ffmpeg) — the model only makes decisions.

use crate::edl::new_id;
use crate::engine::{frames, silence, transcribe};
use crate::state::App;
use crate::tools;
use crate::tools::ops_media::clip_json;
use serde_json::{json, Value};
use std::sync::Arc;
use tauri::AppHandle;

/// Ensure the 16k mono wav for an asset exists (reused by transcribe +
/// silence detection). Returns (wav_path, asset).
fn ensure_wav(app: &Arc<App>, asset_id: &str) -> Result<(std::path::PathBuf, crate::edl::Asset), String> {
    let (asset, wav) = {
        let st = app.state.lock().unwrap();
        let asset = st
            .project
            .asset(asset_id)
            .ok_or_else(|| format!("unknown asset {asset_id}"))?
            .clone();
        let wav = transcribe::wav_path(&app.cache_dir, &asset.id);
        (asset, wav)
    };
    if !wav.exists() {
        transcribe::extract_wav(&asset.path, &wav)?;
    }
    Ok((wav, asset))
}

// ---- transcribe ------------------------------------------------------------

pub fn transcribe_asset(_h: &AppHandle, app: &Arc<App>, args: &Value) -> Result<Value, String> {
    let asset_id = args["asset_id"].as_str().ok_or("asset_id is required")?.to_string();
    let language = args["language"].as_str().map(|s| s.to_string());
    let model = args["model"].as_str().unwrap_or("medium").to_string();
    let initial_prompt = args["initial_prompt"].as_str().map(|s| s.to_string());

    let (wav, asset) = ensure_wav(app, &asset_id)?;
    let out_dir = app.cache_dir.join("transcripts");

    // blocking whisper run — can take minutes on CPU with `medium`
    let out_json = transcribe::run_whisper(&wav, &model, language.as_deref(), initial_prompt.as_deref(), &out_dir)?;
    let raw = std::fs::read_to_string(&out_json).map_err(|e| e.to_string())?;
    let (tr, lines) = transcribe::parse(&raw)?;

    let jp = transcribe::json_path(&app.cache_dir, &asset.id);
    transcribe::store(
        &jp,
        &tr,
        &lines,
        json!({
            "asset_id": asset.id,
            "language": language,
            "model": model,
            "source_duration_ms": asset.duration_ms,
        }),
    )?;

    // quality signals so the agent can sanity-check BEFORE cutting
    let word_ms: u64 = tr.words.iter().map(|(s, e, _)| e.saturating_sub(*s)).sum();
    let coverage_pct = (word_ms as f64 / asset.duration_ms.max(1) as f64 * 100.0).round() as u64;
    let mut line_counts: std::collections::HashMap<String, usize> = Default::default();
    for l in &lines {
        let t = l["t"].as_str().unwrap_or("").trim().to_lowercase();
        *line_counts.entry(t).or_default() += 1;
    }
    let repeated = line_counts.values().filter(|c| **c >= 3).count();
    let mut warnings: Vec<String> = Vec::new();
    if tr.words.is_empty() {
        warnings.push("no words detected — audio may be non-speech or the model failed".into());
    }
    if coverage_pct < 25 && !tr.words.is_empty() {
        warnings.push(format!("words cover only {coverage_pct}% of the audio — transcript may be incomplete"));
    }
    if repeated > 0 {
        warnings.push(format!("{repeated} line(s) repeat 3+ times — likely hallucination loops; try a larger model"));
    }
    if language.as_deref() == Some("fa") && model == "small" {
        warnings.push("model 'small' is unreliable for Persian — prefer 'medium'".into());
    }

    let mut result = json!({
        "ok": true,
        "model": model,
        "word_count": tr.words.len(),
        "lines": lines,
        "quality": {"coverage_pct": coverage_pct},
        "note": "word-level timings stored; remove_ranges snaps cuts to word boundaries automatically",
    });
    if !warnings.is_empty() {
        result["quality"]["warnings"] = json!(warnings);
    }
    Ok(result)
}

// ---- silence_detect ---------------------------------------------------------

pub fn silence_detect(_h: &AppHandle, app: &Arc<App>, args: &Value) -> Result<Value, String> {
    let asset_id = args["asset_id"].as_str().ok_or("asset_id is required")?.to_string();
    let threshold_db = args["threshold_db"].as_f64().unwrap_or(-26.0) as f32;
    let min_duration_s = args["min_duration_s"].as_f64().unwrap_or(0.6) as f32;

    let (wav, _asset) = ensure_wav(app, &asset_id)?;
    let ranges = silence::detect(&wav.to_string_lossy(), threshold_db, min_duration_s)?;
    let silences: Vec<Value> = ranges
        .iter()
        .map(|(s, e)| json!({"start_ms": s, "end_ms": e, "duration_ms": e - s}))
        .collect();
    Ok(json!({
        "ok": true,
        "threshold_db": threshold_db,
        "min_duration_s": min_duration_s,
        "silences": silences,
    }))
}

// ---- remove_ranges (the cut-table recipe) -----------------------------------

/// Snap a boundary to a word edge so cuts fall BETWEEN words - but ONLY
/// within the tolerance window (point +/- tol). A boundary never jumps
/// farther than tol: with a garbage/sparse transcript, raw times win
/// (better a slightly off cut than deleting half the video).
fn snap(point_ms: u64, words: &[(u64, u64, String)], is_start: bool, tol_ms: u64) -> u64 {
    if words.is_empty() {
        return point_ms;
    }
    let mut bounds: Vec<u64> = Vec::with_capacity(words.len() * 2);
    for (s, e, _) in words {
        bounds.push(*s);
        bounds.push(*e);
    }
    bounds.sort_unstable();
    let lo = point_ms.saturating_sub(tol_ms);
    let hi = point_ms + tol_ms;
    if is_start {
        // latest boundary inside the window = the removed word's own start
        bounds.iter().rev().find(|b| **b >= lo && **b <= hi).copied().unwrap_or(point_ms)
    } else {
        // earliest boundary inside the window = the removed word's end
        bounds.iter().find(|b| **b >= lo && **b <= hi).copied().unwrap_or(point_ms)
    }
}

pub fn remove_ranges(_h: &AppHandle, app: &Arc<App>, args: &Value) -> Result<Value, String> {
    let asset_id = args["asset_id"].as_str().ok_or("asset_id is required")?.to_string();
    let track_id = args["track_id"].as_str().unwrap_or("v1").to_string();
    let snap_ms = args["snap_ms"].as_u64().unwrap_or(250);
    let allow_aggressive = args["allow_aggressive"].as_bool().unwrap_or(false);

    let mut ranges: Vec<(u64, u64)> = args["ranges"]
        .as_array()
        .ok_or("ranges must be an array of {start_ms,end_ms}")?
        .iter()
        .filter_map(|r| Some((r["start_ms"].as_u64()?, r["end_ms"].as_u64()?)))
        .collect();
    if ranges.is_empty() {
        return Err("ranges is empty".into());
    }
    ranges.sort();
    // merge overlaps
    let mut merged: Vec<(u64, u64)> = Vec::new();
    for (s, e) in ranges {
        if e <= s {
            continue;
        }
        match merged.last_mut() {
            Some(last) if s <= last.1 => last.1 = last.1.max(e),
            _ => merged.push((s, e)),
        }
    }

    // snap to word boundaries if a transcript exists
    let words = transcribe::load_words(&transcribe::json_path(&app.cache_dir, &asset_id));
    let snapped: Vec<(u64, u64)> = merged
        .iter()
        .map(|(s, e)| {
            let (s2, e2) = match &words {
                Some(w) => (snap(*s, w, true, snap_ms), snap(*e, w, false, snap_ms)),
                None => (*s, *e),
            };
            if e2 > s2 {
                (s2, e2)
            } else {
                (*s, *e)
            }
        })
        .collect();

    tools::mutate(app, move |st| {
        let p = &mut st.project;
        if p.asset(&asset_id).is_none() {
            return Err(format!("unknown asset {asset_id}"));
        }
        if p.track(&track_id).is_none() {
            return Err(format!("unknown track {track_id}"));
        }

        // split every clip of this asset on the track into kept pieces
        let mut kept: Vec<crate::edl::Clip> = Vec::new();
        let mut removed_total = 0u64;
        for clip in p.clips.iter().filter(|c| c.asset_id == asset_id && c.track_id == track_id) {
            let mut piece_start = clip.in_ms;
            for (rs, re) in &snapped {
                // overlap of the removal range with this clip's source window
                let s = (*rs).max(piece_start);
                let e = (*re).min(clip.out_ms);
                if e > s {
                    if s > piece_start {
                        kept.push(crate::edl::Clip {
                            id: new_id(),
                            asset_id: clip.asset_id.clone(),
                            track_id: clip.track_id.clone(),
                            start_ms: 0, // relayed below
                            in_ms: piece_start,
                            out_ms: s,
                            zoom: clip.zoom,
                        });
                    }
                    removed_total += e - s;
                    piece_start = e;
                }
            }
            if clip.out_ms > piece_start {
                kept.push(crate::edl::Clip {
                    id: new_id(),
                    asset_id: clip.asset_id.clone(),
                    track_id: clip.track_id.clone(),
                    start_ms: 0,
                    in_ms: piece_start,
                    out_ms: clip.out_ms,
                    zoom: clip.zoom,
                });
            }
        }
        if kept.is_empty() && removed_total == 0 {
            return Err("no clip of this asset on this track — place it first (clip_add)".into());
        }

        // safety guard: refusing to nuke the whole take by accident (e.g. a
        // hallucinated transcript led to over-broad ranges)
        let coverage: u64 = p
            .clips
            .iter()
            .filter(|c| c.asset_id == asset_id && c.track_id == track_id)
            .map(|c| c.duration_ms())
            .sum();
        if !allow_aggressive && removed_total > coverage * 60 / 100 {
            return Err(format!(
                "refusing to remove {}ms of {}ms coverage (>60%). If this is really intended (e.g. keeping only one best take), re-call with allow_aggressive=true",
                removed_total, coverage
            ));
        }

        // relay: butt the kept pieces together from the earliest original start
        let base_start = p
            .clips
            .iter()
            .filter(|c| c.asset_id == asset_id && c.track_id == track_id)
            .map(|c| c.start_ms)
            .min()
            .unwrap_or(0);
        let mut cursor = base_start;
        for c in kept.iter_mut() {
            c.start_ms = cursor;
            cursor += c.out_ms - c.in_ms;
        }

        // replace: drop old clips of this asset/track, insert kept
        p.clips
            .retain(|c| !(c.asset_id == asset_id && c.track_id == track_id));
        let kept_snapshot = kept.clone();
        p.clips.extend(kept_snapshot);

        p.validate()?;
        Ok(json!({
            "ok": true,
            "removed_ms": removed_total,
            "removed_ranges": snapped.iter().map(|(s,e)| json!({"from_ms": s, "to_ms": e})).collect::<Vec<_>>(),
            "new_clip_count": kept.len(),
            "project_duration_ms": p.duration_ms(),
            "snapped_to_words": words.is_some(),
        }))
    })
}

// ---- zoom / jump-cut masking -------------------------------------------------

pub fn clip_zoom(_h: &AppHandle, app: &Arc<App>, args: &Value) -> Result<Value, String> {
    let clip_id = args["clip_id"].as_str().ok_or("clip_id is required")?.to_string();
    let factor = args["factor"].as_f64().unwrap_or(1.0).clamp(0.5, 3.0) as f32;
    tools::mutate(app, move |st| {
        let p = &mut st.project;
        let c = p
            .clips
            .iter_mut()
            .find(|c| c.id == clip_id)
            .ok_or_else(|| format!("unknown clip {clip_id}"))?;
        c.zoom = factor;
        let out = clip_json(c);
        Ok(json!({"ok": true, "clip": out}))
    })
}

pub fn mask_jump_cuts(_h: &AppHandle, app: &Arc<App>, args: &Value) -> Result<Value, String> {
    let track_id = args["track_id"].as_str().unwrap_or("v1").to_string();
    let factor = args["factor"].as_f64().unwrap_or(1.08).clamp(1.0, 2.0) as f32;
    tools::mutate(app, move |st| {
        let p = &mut st.project;
        let mut ordered: Vec<(u64, String)> = p
            .clips
            .iter()
            .filter(|c| c.track_id == track_id)
            .map(|c| (c.start_ms, c.id.clone()))
            .collect();
        ordered.sort_by_key(|(s, _)| *s);
        if ordered.len() < 2 {
            return Err(format!("need at least 2 clips on {track_id} to mask jump cuts"));
        }
        for (i, (_, id)) in ordered.iter().enumerate() {
            if let Some(c) = p.clips.iter_mut().find(|c| &c.id == id) {
                c.zoom = if i % 2 == 1 { factor } else { 1.0 };
            }
        }
        Ok(json!({"ok": true, "clips": ordered.len(), "alternating_zoom": factor}))
    })
}

// ---- extract_frames ----------------------------------------------------------

pub fn extract_frames(_h: &AppHandle, app: &Arc<App>, args: &Value) -> Result<Value, String> {
    let asset_id = args["asset_id"].as_str().ok_or("asset_id is required")?.to_string();
    let with_images = args["with_images"].as_bool().unwrap_or(false);
    let mut times: Vec<u64> = args["times_ms"]
        .as_array()
        .map(|a| a.iter().filter_map(|v| v.as_u64()).collect())
        .unwrap_or_default();
    // or periodic sampling
    if times.is_empty() {
        if let Some(every) = args["every_ms"].as_u64() {
            let dur = {
                let st = app.state.lock().unwrap();
                st.project
                    .asset(&asset_id)
                    .ok_or_else(|| format!("unknown asset {asset_id}"))?
                    .duration_ms
            };
            let mut t = 0;
            while t < dur {
                times.push(t);
                t += every;
            }
        }
    }
    if times.is_empty() {
        return Err("provide times_ms array or every_ms".into());
    }
    if times.len() > 12 {
        times.truncate(12);
    }
    let (src, asset_id2) = {
        let st = app.state.lock().unwrap();
        let a = st
            .project
            .asset(&asset_id)
            .ok_or_else(|| format!("unknown asset {asset_id}"))?
            .clone();
        (a.path.clone(), a.id)
    };
    let out_dir = app.cache_dir.join("frames").join(&asset_id2);
    let paths = frames::extract(&src, &times, &out_dir)?;

    // attach images for the internal agent's vision (max 4, ≤512px jpegs)
    let mut images: Vec<Value> = Vec::new();
    if with_images {
        for p in paths.iter().take(4) {
            if let Ok(data) = frames::encode_small_b64(p) {
                images.push(json!({"data": data, "media_type": "image/jpeg"}));
            }
        }
    }

    let mut result = json!({
        "ok": true,
        "frames": paths
            .iter()
            .zip(times.iter())
            .map(|(p, t)| json!({"time_ms": t, "path": p.to_string_lossy()}))
            .collect::<Vec<_>>(),
        "note": if with_images {
            "frames attached as images in the next message — look at them"
        } else {
            "open the image files and inspect them yourself (vision QA)"
        },
    });
    if !images.is_empty() {
        result["_images"] = json!(images);
    }
    Ok(result)
}
