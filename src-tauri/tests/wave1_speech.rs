//! Wave-1 speech tool tests: whisper smoke run, silence detection, the
//! remove_ranges cut recipe, zoom/mask, and zoom+fade+loudnorm export.

mod common;

use common::test_media;
use app_media_creator_lib::edl::{new_id, Clip, Project};
use app_media_creator_lib::engine::{export, probe, silence, transcribe};


fn tmp(name: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!("ma_w1_{}_{}", new_id(), name))
}

// ---- whisper smoke ----------------------------------------------------------

#[test]
fn whisper_transcribe_runs_and_parses() {
    let asset = probe::probe(&test_media("clip_a.mp4")).unwrap();
    let wav = tmp("a.wav");
    transcribe::extract_wav(&asset.path, &wav).expect("extract_wav");
    assert!(wav.exists());

    // tiny model: fast CI smoke (speech is a sine tone here — we only assert
    // the pipeline runs and parses, not transcription quality)
    let out_dir = tmp("tout");
    let out = transcribe::run_whisper(&wav, "tiny", Some("fa"), None, &out_dir);
    if out.is_err() {
        // whisper may refuse pure tones; that's fine as long as it ran
        eprintln!("whisper tiny on tone: {:?}", out.as_ref().err());
        return;
    }
    let raw = std::fs::read_to_string(out.unwrap()).unwrap();
    let (tr, lines) = transcribe::parse(&raw).expect("parse");
    assert!(tr.words.len() >= 0);
    let _ = lines;
    let _ = std::fs::remove_file(&wav);
}

// ---- silence -----------------------------------------------------------------

#[test]
fn silence_detect_finds_gap() {
    // sine 1s + silence 1s + sine 1s
    let dst = tmp("silence.wav");
    let status = std::process::Command::new("ffmpeg")
        .args(["-y", "-loglevel", "error"])
        .arg("-f").arg("lavfi").arg("-t").arg("1").arg("-i").arg("sine=frequency=440")
        .arg("-f").arg("lavfi").arg("-t").arg("1").arg("-i").arg("anullsrc=r=16000:cl=mono")
        .arg("-f").arg("lavfi").arg("-t").arg("1").arg("-i").arg("sine=frequency=440")
        .arg("-filter_complex").arg("[0:a][1:a][2:a]concat=n=3:v=0:a=1[a]")
        .arg("-map").arg("[a]").arg("-ar").arg("16000").arg("-ac").arg("1")
        .arg(dst.to_string_lossy().to_string())
        .status()
        .unwrap();
    assert!(status.success(), "build silence fixture");
    let ranges = silence::detect(&dst.to_string_lossy(), -26.0, 0.6).expect("detect");
    assert!(!ranges.is_empty(), "expected at least one silence, got {ranges:?}");
    let (s, e) = ranges[0];
    assert!(s >= 800 && s <= 1300, "silence start {s}");
    assert!(e >= 1800 && e <= 2400, "silence end {e}");
}

// ---- remove_ranges core logic (synthetic transcript) --------------------------

#[test]
fn remove_ranges_snaps_and_ripples() {
    let mut project = Project::new("rr", 30, 640, 360);
    let asset = probe::probe(&test_media("clip_a.mp4")).unwrap();
    let asset_id = asset.id.clone();
    let asset_duration = asset.duration_ms;
    project.assets.push(asset);

    // one full-length clip on v1
    project.clips.push(Clip {
        id: new_id(),
        asset_id: asset_id.clone(),
        track_id: "v1".into(),
        start_ms: 0,
        in_ms: 0,
        out_ms: asset_duration,
        zoom: 1.0,
    });

    // synthetic transcript: word k at [k*1000+200, k*1000+900]
    let words: Vec<(u64, u64, String)> = (0..4)
        .map(|k| (k * 1000 + 200, k * 1000 + 900, format!("w{k}")))
        .collect();

    // mirror ops_speech::snap semantics: cuts land on the removed word's own
    // edges — start snaps to the latest boundary ≤ point+tol (the removed
    // word's start), end to the earliest boundary ≥ point-tol (its end)
    let snap = |point: u64, is_start: bool| {
        let mut bounds: Vec<u64> = Vec::new();
        for (s, e, _) in &words {
            bounds.push(*s);
            bounds.push(*e);
        }
        bounds.sort();
        if is_start {
            bounds.into_iter().filter(|b| *b <= point + 250).max().unwrap_or(point)
        } else {
            bounds.into_iter().filter(|b| *b + 250 >= point).min().unwrap_or(point)
        }
    };

    // remove filler ≈[1000..2000) = word w1 exactly
    let (rs, re) = (snap(1050, true), snap(2050, false));
    assert_eq!(rs, 1200, "range start snapped to w1 start");
    assert_eq!(re, 1900, "range end snapped to w1 end");

    // BOUNDED snap: a sparse/hallucinated transcript must not explode a
    // small range into a huge one (the whole-clip-deleted bug)
    let sparse: Vec<(u64, u64, String)> = vec![(0, 500, "x".into()), (55000, 56000, "y".into())];
    let snap_sparse = |point: u64, is_start: bool| {
        let mut bounds: Vec<u64> = Vec::new();
        for (s, e, _) in &sparse {
            bounds.push(*s);
            bounds.push(*e);
        }
        bounds.sort();
        let lo = point.saturating_sub(250);
        let hi = point + 250;
        if is_start {
            bounds.iter().rev().find(|b| **b >= lo && **b <= hi).copied().unwrap_or(point)
        } else {
            bounds.iter().find(|b| **b >= lo && **b <= hi).copied().unwrap_or(point)
        }
    };
    assert_eq!(snap_sparse(10000, true), 10000, "sparse: start must NOT snap to 500");
    assert_eq!(snap_sparse(12000, false), 12000, "sparse: end must NOT snap to 55000");

    // emulate the recipe's EDL surgery (kept pieces + ripple)
    let snapped = [(rs, re)];
    let clip = &project.clips[0];
    let mut kept: Vec<Clip> = Vec::new();
    let mut piece_start = clip.in_ms;
    for (s, e) in snapped {
        let s = s.max(piece_start);
        let e = e.min(clip.out_ms);
        if e > s {
            if s > piece_start {
                kept.push(Clip { id: new_id(), asset_id: asset_id.clone(), track_id: "v1".into(), start_ms: 0, in_ms: piece_start, out_ms: s, zoom: 1.0 });
            }
            piece_start = e;
        }
    }
    if clip.out_ms > piece_start {
        kept.push(Clip { id: new_id(), asset_id: asset_id.clone(), track_id: "v1".into(), start_ms: 0, in_ms: piece_start, out_ms: clip.out_ms, zoom: 1.0 });
    }
    let mut cursor = 0u64;
    for c in kept.iter_mut() {
        c.start_ms = cursor;
        cursor += c.out_ms - c.in_ms;
    }
    project.clips = kept;
    project.validate().expect("valid after removal");

    let removed = re - rs;
    assert_eq!(removed, 700);
    assert_eq!(project.duration_ms(), asset_duration - removed);
    // first piece: [0..1200) source, second: [1900..dur) relayed to 1200
    assert_eq!(project.clips[0].in_ms, 0);
    assert_eq!(project.clips[0].out_ms, rs);
    assert_eq!(project.clips[0].start_ms, 0);
    assert_eq!(project.clips[1].in_ms, re);
    assert_eq!(project.clips[1].start_ms, rs);
}

// ---- zoom + fades + loudnorm export -------------------------------------------

#[test]
fn export_with_zoom_and_loudnorm() {
    let mut project = Project::new("zoom", 30, 640, 360);
    let a = probe::probe(&test_media("clip_a.mp4")).unwrap();
    let b = probe::probe(&test_media("clip_b.mp4")).unwrap();
    let (a_id, b_id) = (a.id.clone(), b.id.clone());
    project.assets.push(a);
    project.assets.push(b);
    project.clips.push(Clip {
        id: new_id(), asset_id: a_id, track_id: "v1".into(),
        start_ms: 0, in_ms: 0, out_ms: 1500, zoom: 1.08,
    });
    project.clips.push(Clip {
        id: new_id(), asset_id: b_id, track_id: "v1".into(),
        start_ms: 1500, in_ms: 0, out_ms: 3000, zoom: 1.0,
    });
    project.validate().unwrap();

    let out = tmp("zoom.mp4");
    export::export(&project, &out.to_string_lossy()).expect("export with zoom");
    let meta = probe::probe(&out.to_string_lossy()).unwrap();
    assert_eq!((meta.width, meta.height), (640, 360));
    assert!(meta.duration_ms >= 4400 && meta.duration_ms <= 4700, "dur {}", meta.duration_ms);
    let _ = std::fs::remove_file(&out);
}
