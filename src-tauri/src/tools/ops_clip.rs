use crate::edl::{new_id, Clip};
use crate::state::App;
use crate::tools;
use crate::tools::ops_media::clip_json;
use serde_json::{json, Value};
use std::sync::Arc;
use tauri::AppHandle;

pub fn clip_add(_h: &AppHandle, app: &Arc<App>, args: &Value) -> Result<Value, String> {
    let asset_id = args["asset_id"]
        .as_str()
        .ok_or("asset_id is required")?
        .to_string();
    let track_id = args["track_id"].as_str().unwrap_or("v1").to_string();
    let start_ms = args["start_ms"].as_u64();
    let in_ms = args["in_ms"].as_u64();
    let out_ms = args["out_ms"].as_u64();

    tools::mutate(app, move |st| {
        let p = &mut st.project;
        let asset = p
            .asset(&asset_id)
            .ok_or_else(|| format!("unknown asset {asset_id}"))?
            .clone();
        p.track(&track_id)
            .ok_or_else(|| format!("unknown track {track_id}"))?;
        let in_ms = in_ms.unwrap_or(0).min(asset.duration_ms.saturating_sub(1));
        let out_ms = out_ms
            .unwrap_or(asset.duration_ms)
            .clamp(in_ms + 1, asset.duration_ms);
        let start_ms = start_ms.unwrap_or_else(|| p.track_end_ms(&track_id));

        p.check_overlap(&track_id, start_ms, start_ms + (out_ms - in_ms), None)?;

        let clip = Clip {
            id: new_id(),
            asset_id: asset.id.clone(),
            track_id: track_id.clone(),
            start_ms,
            in_ms,
            out_ms,
            zoom: 1.0,
        };
        p.clips.push(clip.clone());
        Ok(json!({"ok": true, "clip": clip_json(&clip), "project_duration_ms": p.duration_ms()}))
    })
}

pub fn clip_cut(_h: &AppHandle, app: &Arc<App>, args: &Value) -> Result<Value, String> {
    let clip_id = args["clip_id"].as_str().ok_or("clip_id is required")?.to_string();
    let at_ms = args["at_ms"].as_u64().ok_or("at_ms is required (timeline ms)")?;

    tools::mutate(app, move |st| {
        let p = &mut st.project;
        let clip = p
            .clip(&clip_id)
            .ok_or_else(|| format!("unknown clip {clip_id}"))?
            .clone();
        if at_ms <= clip.start_ms || at_ms >= clip.end_ms() {
            return Err(format!(
                "at_ms {} must be strictly inside the clip range [{}..{}]",
                at_ms,
                clip.start_ms,
                clip.end_ms()
            ));
        }
        let split_in = clip.in_ms + (at_ms - clip.start_ms);
        let mut left = clip.clone();
        left.out_ms = split_in;
        let right = Clip {
            id: new_id(),
            asset_id: clip.asset_id.clone(),
            track_id: clip.track_id.clone(),
            start_ms: at_ms,
            in_ms: split_in,
            out_ms: clip.out_ms,
            zoom: clip.zoom,
        };
        p.clips.retain(|c| c.id != clip_id);
        p.clips.push(left.clone());
        p.clips.push(right.clone());
        Ok(json!({"ok": true, "left": clip_json(&left), "right": clip_json(&right)}))
    })
}

pub fn clip_move(_h: &AppHandle, app: &Arc<App>, args: &Value) -> Result<Value, String> {
    let clip_id = args["clip_id"].as_str().ok_or("clip_id is required")?.to_string();
    let start_ms = args["start_ms"].as_u64().ok_or("start_ms is required")?;
    let track_id = args["track_id"].as_str().map(|s| s.to_string());

    tools::mutate(app, move |st| {
        let p = &mut st.project;
        let clip = p
            .clip(&clip_id)
            .ok_or_else(|| format!("unknown clip {clip_id}"))?
            .clone();
        let track_id = track_id.unwrap_or_else(|| clip.track_id.clone());
        if p.track(&track_id).is_none() {
            return Err(format!("unknown track {track_id}"));
        }
        let dur = clip.duration_ms();
        p.check_overlap(&track_id, start_ms, start_ms + dur, Some(&clip_id))?;

        let stored = p.clips.iter_mut().find(|c| c.id == clip_id).unwrap();
        stored.start_ms = start_ms;
        stored.track_id = track_id;
        let out = clip_json(stored);
        Ok(json!({"ok": true, "clip": out}))
    })
}

/// Trim a clip's edges (human fixing the agent's cuts, or the agent refining).
/// Absolute values: in_ms/out_ms are source-range bounds; start_ms only
/// changes when the LEFT edge is dragged (material before it is exposed).
pub fn clip_trim(_h: &AppHandle, app: &Arc<App>, args: &Value) -> Result<Value, String> {
    let clip_id = args["clip_id"].as_str().ok_or("clip_id is required")?.to_string();
    let in_ms = args["in_ms"].as_u64();
    let out_ms = args["out_ms"].as_u64();
    let start_ms = args["start_ms"].as_u64();
    if in_ms.is_none() && out_ms.is_none() && start_ms.is_none() {
        return Err("provide at least one of in_ms/out_ms/start_ms".into());
    }

    tools::mutate(app, move |st| {
        let p = &mut st.project;
        let clip = p
            .clip(&clip_id)
            .ok_or_else(|| format!("unknown clip {clip_id}"))?
            .clone();
        let asset = p.asset(&clip.asset_id).unwrap().clone();

        let new_in = in_ms.unwrap_or(clip.in_ms);
        let new_out = out_ms.unwrap_or(clip.out_ms);
        let new_start = start_ms.unwrap_or(clip.start_ms);

        if new_out <= new_in {
            return Err(format!("empty range: in {new_in} >= out {new_out}"));
        }
        if new_in >= asset.duration_ms {
            return Err(format!("in {new_in} beyond asset duration {}", asset.duration_ms));
        }
        if new_out > asset.duration_ms {
            return Err(format!("out {new_out} beyond asset duration {}", asset.duration_ms));
        }

        p.check_overlap(&clip.track_id, new_start, new_start + (new_out - new_in), Some(&clip_id))?;

        let stored = p.clips.iter_mut().find(|c| c.id == clip_id).unwrap();
        stored.in_ms = new_in;
        stored.out_ms = new_out;
        stored.start_ms = new_start;
        let out = clip_json(stored);
        Ok(json!({"ok": true, "clip": out, "project_duration_ms": p.duration_ms()}))
    })
}

/// Remove a clip; with ripple=true the later clips on the track slide left
/// to close the hole (like remove_ranges does).
pub fn clip_remove(_h: &AppHandle, app: &Arc<App>, args: &Value) -> Result<Value, String> {
    let clip_id = args["clip_id"].as_str().ok_or("clip_id is required")?.to_string();
    let ripple = args["ripple"].as_bool().unwrap_or(false);
    tools::mutate(app, move |st| {
        let p = &mut st.project;
        let clip = match p.clip(&clip_id) {
            Some(c) => c.clone(),
            None => return Err(format!("unknown clip {clip_id}")),
        };
        let dur = clip.duration_ms();
        let end = clip.end_ms();
        p.clips.retain(|c| c.id != clip_id);
        if ripple {
            for c in p.clips.iter_mut() {
                if c.track_id == clip.track_id && c.start_ms >= end {
                    c.start_ms = c.start_ms.saturating_sub(dur);
                }
            }
        }
        p.validate()?;
        Ok(json!({"ok": true, "ripple": ripple, "project_duration_ms": p.duration_ms()}))
    })
}
