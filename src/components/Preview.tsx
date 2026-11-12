import { useEffect, useRef } from "react";
import { convertFileSrc } from "@tauri-apps/api/core";
import { useStore } from "../store/project";
import { formatTime } from "../types";

/**
 * Preview: plays the active clip's proxy at the playhead via <video>, and
 * overlays text clips as DOM (matching export's relative positioning).
 * No render engine — a viewer over the same data the exporter uses.
 */
export function Preview() {
  const videoRef = useRef<HTMLVideoElement>(null);
  const project = useStore((s) => s.project);
  const playheadMs = useStore((s) => s.playhead.ms);
  const setPlayhead = useStore((s) => s.setPlayhead);
  const playing = useStore((s) => s.playing);
  const setPlaying = useStore((s) => s.setPlaying);

  // active clip = topmost video clip covering the playhead
  const videoTrackIds = project.tracks
    .filter((t) => t.kind === "video")
    .map((t) => t.id);
  const activeClip = project.clips
    .filter(
      (c) =>
        videoTrackIds.includes(c.track_id) &&
        playheadMs >= c.start_ms &&
        playheadMs < c.start_ms + (c.out_ms - c.in_ms),
    )
    .sort((a, b) => b.track_id.localeCompare(a.track_id))[0];
  const activeAsset = activeClip
    ? project.assets.find((a) => a.id === activeClip.asset_id)
    : undefined;
  const src = activeAsset?.proxy_path ?? null;
  const offsetInClip = activeClip ? playheadMs - activeClip.start_ms : 0;

  // seek when playhead jumps or source changes
  useEffect(() => {
    const v = videoRef.current;
    if (!v || !src) return;
    const target = (activeClip!.in_ms + offsetInClip) / 1000;
    if (Math.abs(v.currentTime - target) > 0.08) {
      v.currentTime = target;
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [playheadMs, src]);

  // play/pause
  useEffect(() => {
    const v = videoRef.current;
    if (!v) return;
    if (playing) v.play().catch(() => setPlaying(false));
    else v.pause();
  }, [playing, src, setPlaying]);

  // clock: advance playhead while playing
  useEffect(() => {
    if (!playing) return;
    const iv = setInterval(() => {
      const v = videoRef.current;
      if (!v || v.paused) return;
      const ms = Math.round(v.currentTime * 1000);
      const globalMs = activeClip ? activeClip.start_ms + (ms - activeClip.in_ms) : ms;
      setPlayhead(globalMs);
      const projDur = Math.max(
        1,
        ...project.clips.map((c) => c.start_ms + (c.out_ms - c.in_ms)),
      );
      if (globalMs >= projDur) setPlaying(false);
    }, 40);
    return () => clearInterval(iv);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [playing, activeClip?.id, project.clips]);

  const visibleTexts = project.texts.filter(
    (t) => playheadMs >= t.start_ms && playheadMs < t.end_ms,
  );

  const togglePlay = () => {
    if (!src) return;
    setPlaying(!playing);
  };

  return (
    <div className="preview">
      <div className="preview-stage" style={{ aspectRatio: `${project.width}/${project.height}` }}>
        {src ? (
          <video
            ref={videoRef}
            src={convertFileSrc(src)}
            onEnded={() => setPlaying(false)}
            preload="auto"
          />
        ) : (
          <div className="preview-empty">no clip at playhead</div>
        )}
        {visibleTexts.map((t) => (
          <div
            key={t.id}
            className="preview-text"
            style={{
              left: `${t.x * 100}%`,
              top: `${t.y * 100}%`,
              fontSize: `${(t.font_size / project.height) * 100}cqh`,
              color: t.color,
            }}
          >
            {t.text}
          </div>
        ))}
      </div>
      <div className="preview-bar">
        <button onClick={togglePlay} disabled={!src} className="play-btn">
          {playing ? "⏸" : "▶"}
        </button>
        <span className="tc">{formatTime(playheadMs)}</span>
        <div className="toolbar-spacer" />
        <span className="tc dim">
          {activeAsset ? activeAsset.name : "—"}
        </span>
      </div>
    </div>
  );
}
