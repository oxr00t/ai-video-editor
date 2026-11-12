//! Export: compile the EDL into a single ffmpeg command.
//!
//! v0 model — honest about what the skeleton supports:
//! - visible-video = topmost clip at any moment (higher track wins: v2 over v1)
//! - gaps become black segments; texts are drawtext filters with time windows
//! - audio = audio-track clips resampled to 48k stereo, gaps silenced

use crate::edl::{Project, KIND_VIDEO};
use crate::engine::run_capture;
use std::process::Command;

#[derive(Debug, Clone)]
struct Seg {
    start_ms: u64,
    end_ms: u64,
    /// The clip visible during this segment (None = black gap).
    clip_id: Option<String>,
}

fn ms_s(ms: u64) -> String {
    format!("{:.3}", ms as f64 / 1000.0)
}

/// Compute the visible (non-overlapping, topmost-wins) segment list for
/// video-track clips. Bottom track first; each higher track paints over.
fn visible_segments(project: &Project) -> Vec<Seg> {
    let mut video_tracks: Vec<&crate::edl::Track> = project
        .tracks
        .iter()
        .filter(|t| t.kind == KIND_VIDEO)
        .collect();
    // paint order bottom→top: "v1" sorts before "v2"
    video_tracks.sort_by_key(|t| t.id.clone());

    let mut covered: Vec<Seg> = Vec::new();
    for track in video_tracks {
        let mut clips: Vec<&crate::edl::Clip> = project
            .clips
            .iter()
            .filter(|c| c.track_id == track.id)
            .collect();
        clips.sort_by_key(|c| c.start_ms);
        for c in clips {
            let (s, e) = (c.start_ms, c.end_ms());
            let mut pieces: Vec<Seg> = Vec::new();
            for seg in &covered {
                if seg.end_ms <= s || seg.start_ms >= e {
                    pieces.push(seg.clone());
                } else {
                    if seg.start_ms < s {
                        pieces.push(Seg { start_ms: seg.start_ms, end_ms: s, clip_id: seg.clip_id.clone() });
                    }
                    if seg.end_ms > e {
                        pieces.push(Seg { start_ms: e, end_ms: seg.end_ms, clip_id: seg.clip_id.clone() });
                    }
                }
            }
            pieces.push(Seg { start_ms: s, end_ms: e, clip_id: Some(c.id.clone()) });
            covered = pieces;
        }
    }
    covered.sort_by_key(|s| s.start_ms);
    covered
}

pub fn export(project: &Project, out_path: &str) -> Result<String, String> {
    if project.clips.is_empty() && project.texts.is_empty() {
        return Err("project is empty — nothing to export".into());
    }

    let segs = visible_segments(project);
    let total_ms = project
        .duration_ms()
        .max(segs.last().map(|s| s.end_ms).unwrap_or(0));

    let w = project.width;
    let h = project.height;
    let fps = project.fps;

    // ---- video inputs, in timeline order (gaps inline as black) -----------
    let mut cmd_args: Vec<String> = vec!["-y".into(), "-loglevel".into(), "error".into()];
    // (is_gap, zoom) per input in push order
    let mut video_inputs: Vec<(bool, f32)> = Vec::new();
    let mut prev_end = 0u64;

    for seg in &segs {
        if seg.start_ms > prev_end {
            cmd_args.extend(["-f".into(), "lavfi".into(), "-t".into(), ms_s(seg.start_ms - prev_end), "-i".into()]);
            cmd_args.push(format!("color=black:s={w}x{h}:r={fps}"));
            video_inputs.push((true, 1.0));
        }
        let clip = project
            .clip(seg.clip_id.as_deref().unwrap_or(""))
            .ok_or("internal: segment without a clip")?;
        let asset = project
            .asset(&clip.asset_id)
            .ok_or_else(|| format!("missing asset {}", clip.asset_id))?;
        cmd_args.extend(["-ss".into(), ms_s(clip.in_ms), "-t".into(), ms_s(seg.end_ms - seg.start_ms), "-i".into()]);
        cmd_args.push(asset.path.clone());
        video_inputs.push((false, clip.zoom));
        prev_end = seg.end_ms;
    }
    if total_ms > prev_end {
        cmd_args.extend(["-f".into(), "lavfi".into(), "-t".into(), ms_s(total_ms - prev_end), "-i".into()]);
        cmd_args.push(format!("color=black:s={w}x{h}:r={fps}"));
        video_inputs.push((true, 1.0));
    }

    if video_inputs.is_empty() {
        return Err("no visible video segments".into());
    }

    // ---- filter graph ------------------------------------------------------
    let mut filters = String::new();

    let even = |v: u32| if v % 2 == 1 { v + 1 } else { v };
    for (i, (is_gap, zoom)) in video_inputs.iter().enumerate() {
        if *is_gap {
            filters.push_str(&format!("[{i}:v]format=yuv420p,fps={fps}[v{i}];"));
        } else {
            // punch-in: scale up around center then crop back — jump-cut masking
            // setsar after crop resets the DAR-preserving SAR scale introduces
            let zoom_part = if *zoom > 1.001 {
                let zw = even((w as f32 * zoom).round() as u32);
                let zh = even((h as f32 * zoom).round() as u32);
                format!(",scale={zw}:{zh}:flags=lanczos,crop={w}:{h},setsar=1")
            } else {
                String::new()
            };
            filters.push_str(&format!(
                "[{i}:v]scale={w}:{h}:force_original_aspect_ratio=decrease,pad={w}:{h}:(ow-iw)/2:(oh-ih)/2,setsar=1{zoom_part},fps={fps},format=yuv420p[v{i}];"
            ));
        }
    }

    let joined: String = (0..video_inputs.len()).map(|i| format!("[v{i}]")).collect();
    let has_texts = !project.texts.is_empty();
    let vout_pre = if has_texts { "vcat" } else { "vout" };
    filters.push_str(&format!(
        "{joined}concat=n={}:v=1:a=0[{vout_pre}];",
        video_inputs.len()
    ));

    if has_texts {
        let mut texts: Vec<&crate::edl::TextClip> = project.texts.iter().collect();
        texts.sort_by_key(|t| t.start_ms);
        let last = texts.len() - 1;
        let mut cur = vout_pre.to_string();
        for (n, t) in texts.iter().enumerate() {
            let next = if n == last { "vout".to_string() } else { format!("vt{n}") };
            filters.push_str(&format!(
                "[{cur}]drawtext=fontfile='C\\:/Windows/Fonts/arial.ttf':text='{}':fontsize={}:fontcolor=0x{}:x='w*{:.4}-(text_w/2)':y='h*{:.4}-(text_h/2)':enable='between(t,{},{})'[{next}];",
                escape_text(&t.text),
                t.font_size,
                t.color.trim_start_matches('#'),
                t.x,
                t.y,
                ms_s(t.start_ms),
                ms_s(t.end_ms),
            ));
            cur = next;
        }
    }

    // ---- audio -------------------------------------------------------------
    // Audio-track clips win; if none exist, fall back to the audio of the
    // visible video segments (a video clip carries its own sound, like
    // Premiere's linked audio). Build an ordered list of audible windows:
    // (source_in_ms, timeline_start_ms, timeline_end_ms, path)
    let mut audio_windows: Vec<(u64, u64, u64, String)> = Vec::new();

    let audio_track_clips: Vec<&crate::edl::Clip> = project
        .clips
        .iter()
        .filter(|c| {
            project
                .track(&c.track_id)
                .map(|t| t.kind == "audio")
                .unwrap_or(false)
        })
        .collect();

    if !audio_track_clips.is_empty() {
        for c in &audio_track_clips {
            if let Some(asset) = project.asset(&c.asset_id) {
                if asset.has_audio {
                    audio_windows.push((c.in_ms, c.start_ms, c.end_ms(), asset.path.clone()));
                }
            }
        }
    } else {
        for seg in &segs {
            let Some(clip_id) = seg.clip_id.as_deref() else { continue };
            let Some(clip) = project.clip(clip_id) else { continue };
            if let Some(asset) = project.asset(&clip.asset_id) {
                if asset.has_audio {
                    let src_in = clip.in_ms + (seg.start_ms - clip.start_ms);
                    audio_windows.push((src_in, seg.start_ms, seg.end_ms, asset.path.clone()));
                }
            }
        }
    }

    let has_audio = !audio_windows.is_empty();
    if has_audio {
        let mut a_inputs: Vec<String> = Vec::new(); // labels in order
        let mut gap_n = 0usize;
        let mut prev = 0u64;
        for (src_in, start, end, path) in &audio_windows {
            if *start > prev {
                cmd_args.extend(["-f".into(), "lavfi".into(), "-t".into(), ms_s(start - prev), "-i".into()]);
                cmd_args.push("anullsrc=r=48000:cl=stereo".into());
                a_inputs.push(format!("ag{gap_n}"));
                gap_n += 1;
            }
            cmd_args.extend(["-ss".into(), ms_s(*src_in), "-t".into(), ms_s(end - start), "-i".into()]);
            cmd_args.push(path.clone());
            a_inputs.push(format!("as{}", a_inputs.len()));
            prev = *end;
        }
        if total_ms > prev {
            cmd_args.extend(["-f".into(), "lavfi".into(), "-t".into(), ms_s(total_ms - prev), "-i".into()]);
            cmd_args.push("anullsrc=r=48000:cl=stereo".into());
            a_inputs.push(format!("ag{gap_n}"));
        }

        let first_audio_input = video_inputs.len();
        // 30ms fades at every segment edge kill cut clicks (workflow rule);
        // gaps are silent so they don't need fades
        let fade_s = 0.030f64;
        for (i, label) in a_inputs.iter().enumerate() {
            let idx = first_audio_input + i;
            let is_gap = label.starts_with("ag");
            let fades = if is_gap {
                String::new()
            } else {
                // find this input's duration back from the windows/gaps order
                let n_gaps_before = a_inputs[..i].iter().filter(|l| l.starts_with("ag")).count();
                let real_i = i - n_gaps_before;
                let (_, start, end, _) = audio_windows[real_i];
                let dur = (end - start) as f64 / 1000.0;
                if dur > 0.1 {
                    format!(
                        "afade=t=in:st=0:d={fade_s:.3},afade=t=out:st={:.3}:d={fade_s:.3},",
                        (dur - fade_s).max(0.0)
                    )
                } else {
                    String::new()
                }
            };
            filters.push_str(&format!(
                "[{idx}:a]{fades}aresample=48000,aformat=channel_layouts=stereo[a{i}];"
            ));
        }
        let joined_a: String = (0..a_inputs.len()).map(|i| format!("[a{i}]")).collect();
        let lufs = project.loudness_lufs;
        let a_final = if lufs != 0 {
            // EBU R128 target (default -14 for YouTube/Instagram)
            format!("[aout]loudnorm=I={lufs}:LRA=7:tp=-1.5,aresample=48000[afinal];")
        } else {
            String::new()
        };
        filters.push_str(&format!(
            "{joined_a}concat=n={}:v=0:a=1[aout];{a_final}",
            a_inputs.len()
        ));
    }

    // ---- run ---------------------------------------------------------------
    let filter_str = filters.trim_end_matches(';').to_string();
    let mut cmd = Command::new("ffmpeg");
    cmd.args(&cmd_args);
    cmd.arg("-filter_complex").arg(&filter_str);
    cmd.arg("-map").arg("[vout]");
    if has_audio {
        let audio_label = if project.loudness_lufs != 0 { "[afinal]" } else { "[aout]" };
        cmd.arg("-map").arg(audio_label);
    }
    cmd.args(["-c:v", "libx264", "-preset", "medium", "-crf", "20", "-pix_fmt", "yuv420p"]);
    if has_audio {
        cmd.args(["-c:a", "aac", "-b:a", "160k"]);
    }
    cmd.args(["-movflags", "+faststart", out_path]);

    run_capture(&mut cmd)?;
    Ok(out_path.to_string())
}

/// Escape text for a single-quoted drawtext argument.
fn escape_text(s: &str) -> String {
    s.replace('\\', "\\\\").replace('\'', "\\'")
}
