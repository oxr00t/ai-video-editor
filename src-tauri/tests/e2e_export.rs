//! End-to-end engine test (no GUI): probe test clips → build a project →
//! export through the real ffmpeg binary. Run with `cargo test`.

mod common;

use common::test_media;
use app_media_creator_lib::edl::{new_id, Clip, Project, TextClip};
use app_media_creator_lib::engine::{export, probe, proxy};


#[test]
fn probe_reads_metadata() {
    let asset = probe::probe(&test_media("clip_a.mp4")).expect("probe failed");
    assert_eq!(asset.width, 1280);
    assert_eq!(asset.height, 720);
    assert!(asset.duration_ms >= 3900 && asset.duration_ms <= 4100, "duration {}ms", asset.duration_ms);
    assert!(asset.has_audio);
}

#[test]
fn export_two_clips_with_text() {
    let mut project = Project::new("e2e", 30, 1280, 720);
    let a = probe::probe(&test_media("clip_a.mp4")).unwrap();
    let b = probe::probe(&test_media("clip_b.mp4")).unwrap();
    let (a_id, b_id) = (a.id.clone(), b.id.clone());
    let (a_path, b_path) = (a.path.clone(), b.path.clone());
    project.assets.push(a);
    project.assets.push(b);

    // cut the first clip in half: keep only its first 2s on the timeline
    project.clips.push(Clip {
        id: new_id(),
        asset_id: a_id.clone(),
        track_id: "v1".into(),
        start_ms: 0,
        in_ms: 500,
                out_ms: 2500,
        zoom: 1.0,
    });
    project.clips.push(Clip {
        id: new_id(),
        asset_id: b_id.clone(),
        track_id: "v1".into(),
        start_ms: 2000,
        in_ms: 0,
                out_ms: 3000,
        zoom: 1.0,
    });
    // overlay text during the second clip
    project.texts.push(TextClip {
        id: new_id(),
        text: "E2E TEST".into(),
        start_ms: 2200,
        end_ms: 4200,
        x: 0.5,
        y: 0.2,
        font_size: 96,
        color: "#FFD400".into(),
    });

    project.validate().expect("project invalid");
    assert_eq!(project.duration_ms(), 5000);

    let out = std::env::temp_dir().join(format!("ma_e2e_{}.mp4", new_id()));
    let out_str = out.to_string_lossy().to_string();
    export::export(&project, &out_str).expect("export failed");

    // verify the output through ffprobe
    let out_asset = probe::probe(&out_str).expect("output not probeable");
    assert_eq!(out_asset.width, 1280);
    assert_eq!(out_asset.height, 720);
    assert!(
        out_asset.duration_ms >= 4900 && out_asset.duration_ms <= 5200,
        "output duration {}ms (expected ~5000)",
        out_asset.duration_ms
    );
    assert!(out_asset.has_audio);
    println!("export ok: {out_str}");
    let _ = std::fs::remove_file(&out_str);
}

#[test]
fn proxy_generates_smaller_file() {
    let asset = probe::probe(&test_media("clip_a.mp4")).unwrap();
    let dst = std::env::temp_dir().join(format!("ma_proxy_{}.mp4", new_id()));
    let p = proxy::generate(&asset.path, &dst, asset.has_audio).expect("proxy failed");
    let src_len = std::fs::metadata(&asset.path).unwrap().len();
    let dst_len = std::fs::metadata(&p).unwrap().len();
    assert!(dst_len < src_len, "proxy {} not smaller than source {}", dst_len, src_len);
    let pa = probe::probe(&p).unwrap();
    assert_eq!(pa.height, 360);
    let _ = std::fs::remove_file(&p);
}

#[test]
fn overlap_validation_rejects() {
    let mut project = Project::new("ovl", 30, 1280, 720);
    let a = probe::probe(&test_media("clip_a.mp4")).unwrap();
    let a_id = a.id.clone();
    project.assets.push(a);
    project.clips.push(Clip {
        id: new_id(),
        asset_id: a_id.clone(),
        track_id: "v1".into(),
        start_ms: 0,
        in_ms: 0,
                out_ms: 2000,
        zoom: 1.0,
    });
    project.clips.push(Clip {
        id: new_id(),
        asset_id: a_id,
        track_id: "v1".into(),
        start_ms: 1000, // overlaps the first clip
        in_ms: 0,
                out_ms: 2000,
        zoom: 1.0,
    });
    assert!(project.validate().is_err());
}

#[test]
fn overlap_check_helper_reports() {
    let mut project = Project::new("ovl2", 30, 1280, 720);
    let a = probe::probe(&test_media("clip_b.mp4")).unwrap();
    let a_id = a.id.clone();
    project.assets.push(a);
    project.clips.push(Clip {
        id: new_id(),
        asset_id: a_id,
        track_id: "v1".into(),
        start_ms: 0,
        in_ms: 0,
                out_ms: 2000,
        zoom: 1.0,
    });
    assert!(project.check_overlap("v1", 1000, 3000, None).is_err());
    assert!(project.check_overlap("v1", 2000, 3000, None).is_ok());
    assert!(project.check_overlap("v2", 0, 60000, None).is_ok());
}

#[test]
fn file_discovery_tools_find_test_clips() {
    use app_media_creator_lib::tools::ops_files::{find_media_impl, list_dir_impl};
    let clip_a = test_media("clip_a.mp4");
    let dir = std::path::Path::new(&clip_a).parent().unwrap().to_path_buf();

    let (entries, truncated) = list_dir_impl(&dir, true, 1).expect("list_dir failed");
    assert!(!truncated);
    let names: Vec<&str> = entries
        .iter()
        .filter_map(|e| e["name"].as_str())
        .collect();
    assert!(names.contains(&"clip_a.mp4"), "entries: {entries:?}");
    assert!(names.contains(&"clip_b.mp4"));

    let found = find_media_impl(&dir).expect("find_media failed");
    assert!(found.len() >= 2);
    assert!(found
        .iter()
        .all(|f| f["path"].as_str().unwrap().contains("clip_")));

    // media_only never returns more entries than the unfiltered listing
    let (all, _) = list_dir_impl(&dir, false, 1).unwrap();
    assert!(all.len() >= entries.len());
}
