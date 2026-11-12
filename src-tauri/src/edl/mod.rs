//! EDL — the Edit Decision List: the single source of truth of an edit.
//!
//! The project is plain data (serde JSON). Agents mutate it through tools,
//! the UI is a live view over it, export renders it with ffmpeg.

pub mod history;

use serde::{Deserialize, Serialize};

pub const KIND_VIDEO: &str = "video";
pub const KIND_AUDIO: &str = "audio";

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Project {
    pub id: String,
    pub name: String,
    pub fps: u32,
    pub width: u32,
    pub height: u32,
    /// Target integrated loudness for export (EBU R128). 0 = skip loudnorm.
    #[serde(default = "default_lufs")]
    pub loudness_lufs: i32,
    pub assets: Vec<Asset>,
    pub tracks: Vec<Track>,
    pub clips: Vec<Clip>,
    pub texts: Vec<TextClip>,
}

fn default_lufs() -> i32 {
    -14
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Asset {
    pub id: String,
    pub name: String,
    /// Absolute path to the source media file.
    pub path: String,
    /// Low-res proxy used by the preview pane (generated in background).
    pub proxy_path: Option<String>,
    pub duration_ms: u64,
    pub width: u32,
    pub height: u32,
    pub fps: f64,
    pub has_audio: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Track {
    pub id: String,
    pub kind: String,
    pub name: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Clip {
    pub id: String,
    pub asset_id: String,
    pub track_id: String,
    /// Position on the timeline.
    pub start_ms: u64,
    /// In-point inside the source asset.
    pub in_ms: u64,
    /// Out-point inside the source asset (exclusive).
    pub out_ms: u64,
    /// Punch-in factor (1.0 = none). Used for jump-cut masking (1.08) and
    /// Ken Burns-style life on static shots.
    #[serde(default = "default_zoom")]
    pub zoom: f32,
}

fn default_zoom() -> f32 {
    1.0
}

impl Clip {
    pub fn duration_ms(&self) -> u64 {
        self.out_ms.saturating_sub(self.in_ms)
    }
    pub fn end_ms(&self) -> u64 {
        self.start_ms + self.duration_ms()
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct TextClip {
    pub id: String,
    pub text: String,
    pub start_ms: u64,
    pub end_ms: u64,
    /// Center position, relative (0..1).
    pub x: f32,
    pub y: f32,
    pub font_size: u32,
    /// Hex color, e.g. "#FFFFFF".
    pub color: String,
}

impl TextClip {}

pub fn new_id() -> String {
    // short ids keep every tool result (and every model context) lean;
    // 8 hex chars ≈ 4B values — collision-free at project scale
    uuid::Uuid::new_v4().simple().to_string()[..8].to_string()
}

impl Project {
    pub fn new(name: &str, fps: u32, width: u32, height: u32) -> Self {
        Project {
            id: new_id(),
            name: name.to_string(),
            fps,
            width,
            height,
            loudness_lufs: -14,
            assets: Vec::new(),
            tracks: vec![
                Track {
                    id: "v2".into(),
                    kind: KIND_VIDEO.into(),
                    name: "V2".into(),
                },
                Track {
                    id: "v1".into(),
                    kind: KIND_VIDEO.into(),
                    name: "V1".into(),
                },
                Track {
                    id: "a1".into(),
                    kind: KIND_AUDIO.into(),
                    name: "A1".into(),
                },
            ],
            clips: Vec::new(),
            texts: Vec::new(),
        }
    }

    pub fn asset(&self, id: &str) -> Option<&Asset> {
        self.assets.iter().find(|a| a.id == id)
    }

    pub fn clip(&self, id: &str) -> Option<&Clip> {
        self.clips.iter().find(|c| c.id == id)
    }

    pub fn text(&self, id: &str) -> Option<&TextClip> {
        self.texts.iter().find(|t| t.id == id)
    }

    pub fn track(&self, id: &str) -> Option<&Track> {
        self.tracks.iter().find(|t| t.id == id)
    }

    /// Total timeline length in ms (clips and texts included).
    pub fn duration_ms(&self) -> u64 {
        let c = self.clips.iter().map(|c| c.end_ms()).max().unwrap_or(0);
        let t = self.texts.iter().map(|t| t.end_ms).max().unwrap_or(0);
        c.max(t)
    }

    /// End of the last clip on a given track.
    pub fn track_end_ms(&self, track_id: &str) -> u64 {
        self.clips
            .iter()
            .filter(|c| c.track_id == track_id)
            .map(|c| c.end_ms())
            .max()
            .unwrap_or(0)
    }

    /// Rejects overlaps on the same track (v0 keeps tracks gap-tolerant but
    /// overlap-free — same rule Premiere applies on insert).
    pub fn check_overlap(&self, track_id: &str, start_ms: u64, end_ms: u64, ignore_clip: Option<&str>) -> Result<(), String> {
        for c in &self.clips {
            if c.track_id == track_id && Some(c.id.as_str()) != ignore_clip {
                let s = c.start_ms;
                let e = c.end_ms();
                if start_ms < e && s < end_ms {
                    return Err(format!(
                        "overlap on track {track_id}: new [{start_ms}..{end_ms}] clips with existing [{s}..{e}] (clip {})",
                        c.id
                    ));
                }
            }
        }
        Ok(())
    }

    /// Validation run after every mutation.
    pub fn validate(&self) -> Result<(), String> {
        for c in &self.clips {
            if self.asset(&c.asset_id).is_none() {
                return Err(format!("clip {} references missing asset {}", c.id, c.asset_id));
            }
            if self.track(&c.track_id).is_none() {
                return Err(format!("clip {} references missing track {}", c.id, c.track_id));
            }
            if c.out_ms <= c.in_ms {
                return Err(format!("clip {} has empty range (in={}, out={})", c.id, c.in_ms, c.out_ms));
            }
            let asset = self.asset(&c.asset_id).unwrap();
            if c.out_ms > asset.duration_ms + 1 {
                return Err(format!(
                    "clip {} out-point {} exceeds asset {} duration {}",
                    c.id, c.out_ms, asset.id, asset.duration_ms
                ));
            }
        }
        // no two clips may overlap on the same track
        let mut by_track: std::collections::HashMap<&str, Vec<&Clip>> = std::collections::HashMap::new();
        for c in &self.clips {
            by_track.entry(c.track_id.as_str()).or_default().push(c);
        }
        for (track_id, mut clips) in by_track {
            clips.sort_by_key(|c| c.start_ms);
            for pair in clips.windows(2) {
                if pair[1].start_ms < pair[0].end_ms() {
                    return Err(format!(
                        "overlap on track {track_id}: clips {} and {} intersect",
                        pair[0].id, pair[1].id
                    ));
                }
            }
        }
        for t in &self.texts {
            if t.end_ms <= t.start_ms {
                return Err(format!("text {} has empty range", t.id));
            }
        }
        Ok(())
    }
}
