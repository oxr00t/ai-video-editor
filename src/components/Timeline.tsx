import { useEffect, useRef, useState, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useStore } from "../store/project";
import { formatTime } from "../types";
import type { Clip, TextClip, Project } from "../types";

const TRACK_H = 40;
const RULER_H = 26;
const TEXT_LANE_H = 26;
const PAD_LEFT = 8;
const MIN_CLIP_MS = 100;
const EDGE_PX = 7; // grab zone for trim handles
const SNAP_PX = 8; // magnetic snap tolerance

// Track display order (top to bottom): v2, v1, then text lane
const TRACK_ORDER = ["v2", "v1"];

interface DragState {
  kind: "move" | "trimL" | "trimR" | "text";
  id: string;
  grabOffsetMs: number;
  origStart: number;
  origEnd: number; // timeline end
  origIn: number;
  origOut: number;
}

const clipEnd = (c: Clip) => c.start_ms + (c.out_ms - c.in_ms);

export function Timeline() {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const wrapRef = useRef<HTMLDivElement>(null);
  const project = useStore((s) => s.project);
  const playheadMs = useStore((s) => s.playhead.ms);
  const setPlayhead = useStore((s) => s.setPlayhead);
  const selectedClipId = useStore((s) => s.selectedClipId);
  const selectedTextId = useStore((s) => s.selectedTextId);
  const select = useStore((s) => s.select);
  const tool = useStore((s) => s.tool);
  const setPlaying = useStore((s) => s.setPlaying);
  const playing = useStore((s) => s.playing);

  const [pxPerMs, setPxPerMs] = useState(0.05);
  const [scrollX, setScrollX] = useState(0);
  const [drag, setDrag] = useState<DragState | null>(null);

  const projectDur = Math.max(
    1,
    project.clips.reduce((a, c) => Math.max(a, clipEnd(c)), 0),
    project.texts.reduce((a, t) => Math.max(a, t.end_ms), 0),
  );
  const viewMs = projectDur * 1.25 + 8000;

  const xOf = useCallback(
    (ms: number) => PAD_LEFT + (ms - scrollX) * pxPerMs,
    [pxPerMs, scrollX],
  );
  const msOf = useCallback(
    (x: number) => (x - PAD_LEFT) / pxPerMs + scrollX,
    [pxPerMs, scrollX],
  );

  // fit zoom to project whenever it changes a lot
  useEffect(() => {
    const w = wrapRef.current?.clientWidth ?? 800;
    const fit = (w - PAD_LEFT * 2) / viewMs;
    if (fit > 0.005 && fit < 0.6) setPxPerMs(Math.max(0.01, fit));
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [projectDur]);

  // ---- snapping -------------------------------------------------------------
  const snapMs = useCallback(
    (candidate: number, ignoreId: string) => {
      const tol = SNAP_PX / pxPerMs;
      const points: number[] = [0, playheadMs];
      for (const c of project.clips) {
        if (c.id === ignoreId) continue;
        points.push(c.start_ms, clipEnd(c));
      }
      let best = candidate;
      let bestDist = tol;
      for (const p of points) {
        const d = Math.abs(p - candidate);
        if (d < bestDist) {
          bestDist = d;
          best = p;
        }
      }
      return Math.max(0, Math.round(best));
    },
    [playheadMs, project.clips, pxPerMs],
  );

  // ---- drawing ----------------------------------------------------------------
  const draw = useCallback(() => {
    const canvas = canvasRef.current;
    const wrap = wrapRef.current;
    if (!canvas || !wrap) return;
    const dpr = window.devicePixelRatio || 1;
    const w = wrap.clientWidth;
    const h = wrap.clientHeight;
    canvas.width = w * dpr;
    canvas.height = h * dpr;
    canvas.style.width = `${w}px`;
    canvas.style.height = `${h}px`;
    const ctx = canvas.getContext("2d");
    if (!ctx) return;
    ctx.scale(dpr, dpr);

    const trackCount = TRACK_ORDER.length;
    const contentH = RULER_H + trackCount * TRACK_H + TEXT_LANE_H + 8;
    const lanesTop = RULER_H;

    ctx.fillStyle = "#191919";
    ctx.fillRect(0, 0, w, h);

    const targetTicks = 12;
    const stepCandidates = [100, 250, 500, 1000, 2000, 5000, 10000, 30000, 60000, 120000, 300000];
    const step = stepCandidates.find((s) => s * pxPerMs > w / targetTicks) ?? 600000;

    // ruler
    ctx.fillStyle = "#202020";
    ctx.fillRect(0, 0, w, RULER_H);
    ctx.font = "10px 'Segoe UI', sans-serif";
    ctx.textBaseline = "middle";
    const firstTick = Math.floor(scrollX / step) * step;
    for (let t = firstTick; xOf(t) < w; t += step) {
      const x = xOf(t);
      if (x < PAD_LEFT - 1) continue;
      ctx.strokeStyle = "#3a3a3a";
      ctx.beginPath();
      ctx.moveTo(x, RULER_H - 6);
      ctx.lineTo(x, RULER_H);
      ctx.stroke();
      ctx.fillStyle = "#8a8a8a";
      ctx.fillText(formatTime(t), x + 4, RULER_H / 2 - 2);
      ctx.strokeStyle = "#222";
      ctx.beginPath();
      ctx.moveTo(x, RULER_H);
      ctx.lineTo(x, contentH);
      ctx.stroke();
    }

    // track lanes
    for (let i = 0; i < trackCount; i++) {
      const y = lanesTop + i * TRACK_H;
      ctx.fillStyle = i % 2 === 0 ? "#1e1e1e" : "#1a1a1a";
      ctx.fillRect(0, y, w, TRACK_H);
      ctx.fillStyle = "#666";
      ctx.font = "bold 10px 'Segoe UI', sans-serif";
      ctx.save();
      ctx.translate(4, y + TRACK_H / 2);
      ctx.rotate(-Math.PI / 2);
      ctx.textAlign = "center";
      ctx.fillText(TRACK_ORDER[i], 0, 4);
      ctx.restore();
    }
    const textY = lanesTop + trackCount * TRACK_H;
    ctx.fillStyle = "#161616";
    ctx.fillRect(0, textY, w, TEXT_LANE_H);
    ctx.fillStyle = "#555";
    ctx.font = "bold 10px 'Segoe UI', sans-serif";
    ctx.fillText("T", 4, textY + TEXT_LANE_H / 2);

    // clips
    TRACK_ORDER.forEach((trackId, i) => {
      const y = lanesTop + i * TRACK_H;
      for (const c of project.clips) {
        if (c.track_id !== trackId) continue;
        const x = xOf(c.start_ms);
        const cw = Math.max(2, (c.out_ms - c.in_ms) * pxPerMs);
        const asset = project.assets.find((a) => a.id === c.asset_id);
        const selected = c.id === selectedClipId;
        ctx.fillStyle = selected ? "#7c5cff" : asset ? "#2f6f4f" : "#444";
        ctx.beginPath();
        ctx.roundRect(x, y + 5, cw, TRACK_H - 10, 3);
        ctx.fill();
        if (cw > 12) {
          ctx.save();
          ctx.beginPath();
          ctx.roundRect(x, y + 5, cw, TRACK_H - 10, 3);
          ctx.clip();
          ctx.strokeStyle = "rgba(255,255,255,0.08)";
          for (let sx = x + 4; sx < x + cw - 2; sx += 5) {
            ctx.beginPath();
            ctx.moveTo(sx, y + 7);
            ctx.lineTo(sx, y + TRACK_H - 7);
            ctx.stroke();
          }
          ctx.restore();
        }
        if (cw > 30) {
          ctx.fillStyle = "#e8e8e8";
          ctx.font = "11px 'Segoe UI', sans-serif";
          ctx.textAlign = "left";
          const label = asset?.name ?? c.asset_id;
          ctx.fillText(selected ? `${label} ✂` : label, x + 5, y + TRACK_H / 2);
        }
        // trim handles on the selected clip
        if (selected && cw > 14) {
          ctx.fillStyle = "#fff";
          ctx.fillRect(x, y + 5, 3, TRACK_H - 10);
          ctx.fillRect(x + cw - 3, y + 5, 3, TRACK_H - 10);
        }
      }
    });

    // text clips
    for (const t of project.texts) {
      const x = xOf(t.start_ms);
      const cw = Math.max(2, (t.end_ms - t.start_ms) * pxPerMs);
      const selected = t.id === selectedTextId;
      ctx.fillStyle = selected ? "#e8a13c" : "#8a6a2f";
      ctx.beginPath();
      ctx.roundRect(x, textY + 4, cw, TEXT_LANE_H - 8, 3);
      ctx.fill();
      if (cw > 24) {
        ctx.fillStyle = "#fff";
        ctx.font = "10px 'Segoe UI', sans-serif";
        ctx.textAlign = "left";
        ctx.fillText(t.text, x + 4, textY + TEXT_LANE_H / 2);
      }
    }

    // playhead
    const phX = xOf(playheadMs);
    if (phX >= PAD_LEFT - 1) {
      ctx.strokeStyle = "#ff5a5a";
      ctx.lineWidth = 1.5;
      ctx.beginPath();
      ctx.moveTo(phX, 0);
      ctx.lineTo(phX, contentH);
      ctx.stroke();
      ctx.fillStyle = "#ff5a5a";
      ctx.beginPath();
      ctx.moveTo(phX - 5, 0);
      ctx.lineTo(phX + 5, 0);
      ctx.lineTo(phX, 8);
      ctx.closePath();
      ctx.fill();
      ctx.lineWidth = 1;
    }

    ctx.textAlign = "right";
    ctx.fillStyle = "#777";
    ctx.fillText(formatTime(playheadMs), w - 6, RULER_H / 2 - 2);
  }, [project, pxPerMs, scrollX, playheadMs, selectedClipId, selectedTextId, xOf]);

  useEffect(() => {
    draw();
  }, [draw]);

  useEffect(() => {
    const wrap = wrapRef.current;
    if (!wrap) return;
    const ro = new ResizeObserver(() => draw());
    ro.observe(wrap);
    return () => ro.disconnect();
  }, [draw]);

  // ---- hit testing ------------------------------------------------------------
  const clipAt = useCallback(
    (x: number, y: number): { clip: Clip; edge: "left" | "right" | null } | null => {
      const lanesTop = RULER_H;
      for (let i = 0; i < TRACK_ORDER.length; i++) {
        const ty = lanesTop + i * TRACK_H;
        if (y < ty || y >= ty + TRACK_H) continue;
        const hits: Clip[] = [];
        for (const c of project.clips) {
          if (c.track_id !== TRACK_ORDER[i]) continue;
          const cx = xOf(c.start_ms);
          const cw = Math.max(2, (c.out_ms - c.in_ms) * pxPerMs);
          if (x >= cx && x <= cx + cw) hits.push(c);
        }
        if (!hits.length) return null;
        const c = hits[hits.length - 1];
        const cx = xOf(c.start_ms);
        const cw = Math.max(2, (c.out_ms - c.in_ms) * pxPerMs);
        if (x - cx < EDGE_PX && cw > 24) return { clip: c, edge: "left" };
        if (cx + cw - x < EDGE_PX && cw > 24) return { clip: c, edge: "right" };
        return { clip: c, edge: null };
      }
      return null;
    },
    [project.clips, pxPerMs, xOf],
  );

  const textAt = useCallback(
    (x: number, y: number): TextClip | null => {
      const textY = RULER_H + TRACK_ORDER.length * TRACK_H;
      if (y < textY || y >= textY + TEXT_LANE_H) return null;
      for (const t of project.texts) {
        const tx = xOf(t.start_ms);
        const tw = Math.max(2, (t.end_ms - t.start_ms) * pxPerMs);
        if (x >= tx && x <= tx + tw) return t;
      }
      return null;
    },
    [project.texts, pxPerMs, xOf],
  );

  // ---- pointer handlers ----------------------------------------------------------
  const onPointerDown = (e: React.PointerEvent) => {
    const rect = canvasRef.current!.getBoundingClientRect();
    const x = e.clientX - rect.left;
    const y = e.clientY - rect.top;

    const hit = clipAt(x, y);
    if (hit) {
      const { clip, edge } = hit;
      select(clip.id, null);
      setDrag({
        kind: edge === "left" ? "trimL" : edge === "right" ? "trimR" : "move",
        id: clip.id,
        grabOffsetMs: edge ? 0 : msOf(x) - clip.start_ms,
        origStart: clip.start_ms,
        origEnd: clipEnd(clip),
        origIn: clip.in_ms,
        origOut: clip.out_ms,
      });
      (e.target as HTMLElement).setPointerCapture(e.pointerId);
      return;
    }
    const t = textAt(x, y);
    if (t) {
      select(null, t.id);
      setDrag({
        kind: "text",
        id: t.id,
        grabOffsetMs: msOf(x) - t.start_ms,
        origStart: t.start_ms,
        origEnd: t.end_ms,
        origIn: 0,
        origOut: 0,
      });
      (e.target as HTMLElement).setPointerCapture(e.pointerId);
      return;
    }
    // empty area: scrub playhead
    select(null, null);
    setPlayhead(Math.max(0, Math.round(msOf(x))));
    setDrag(null);
  };

  const patchClip = (id: string, patch: Partial<Clip>) => {
    useStore.setState((st) => ({
      project: {
        ...st.project,
        clips: st.project.clips.map((c) => (c.id === id ? { ...c, ...patch } : c)),
      },
    }));
  };

  // stable neighbor bounds based on the clip's ORIGINAL positions
  const neighbors = (p: Project, id: string, trackId: string, origStart: number, origEnd: number) => {
    const others = p.clips.filter((c) => c.track_id === trackId && c.id !== id);
    const prevEnd = Math.max(0, ...others.filter((c) => clipEnd(c) <= origStart).map(clipEnd));
    const nextStart = Math.min(
      Infinity,
      ...others.filter((c) => c.start_ms >= origEnd).map((c) => c.start_ms),
    );
    return { prevEnd, nextStart };
  };

  const onPointerMove = (e: React.PointerEvent) => {
    const rect = canvasRef.current!.getBoundingClientRect();
    const x = e.clientX - rect.left;
    const y = e.clientY - rect.top;

    if (!drag) {
      // cursor feedback
      const hit = clipAt(x, y);
      const t = textAt(x, y);
      const el = canvasRef.current!;
      if (hit?.edge) el.style.cursor = "ew-resize";
      else if (hit || t) el.style.cursor = "grab";
      else if (y <= RULER_H) el.style.cursor = "col-resize";
      else el.style.cursor = "default";
      if (e.buttons === 1 && y <= RULER_H) {
        setPlayhead(Math.max(0, Math.round(msOf(x))));
      }
      return;
    }

    if (drag.kind === "text") {
      const ms = snapMs(msOf(x) - drag.grabOffsetMs, drag.id);
      const dur = drag.origEnd - drag.origStart;
      useStore.setState((st) => ({
        project: {
          ...st.project,
          texts: st.project.texts.map((t) =>
            t.id === drag.id ? { ...t, start_ms: ms, end_ms: ms + dur } : t,
          ),
        },
      }));
      return;
    }

    const live = useStore.getState().project.clips.find((c) => c.id === drag.id);
    if (!live) return;
    const assetDur =
      useStore.getState().project.assets.find((a) => a.id === live.asset_id)?.duration_ms ?? Infinity;
    const { prevEnd, nextStart } = neighbors(
      useStore.getState().project,
      drag.id,
      live.track_id,
      drag.origStart,
      drag.origEnd,
    );

    if (drag.kind === "move") {
      const start = snapMs(msOf(x) - drag.grabOffsetMs, drag.id);
      patchClip(drag.id, { start_ms: Math.max(0, start) });
      return;
    }

    if (drag.kind === "trimL") {
      let left = snapMs(msOf(x), drag.id);
      const maxLeft = Math.min(drag.origEnd - MIN_CLIP_MS, nextStart);
      const minLeft = Math.max(prevEnd, drag.origStart - drag.origIn); // in ≥ 0
      left = Math.min(Math.max(left, minLeft), maxLeft);
      const delta = left - drag.origStart;
      patchClip(drag.id, { start_ms: left, in_ms: drag.origIn + delta });
      return;
    }

    if (drag.kind === "trimR") {
      let right = snapMs(msOf(x), drag.id);
      const minRight = drag.origStart + MIN_CLIP_MS;
      const maxRight = Math.min(nextStart, drag.origStart + (assetDur - drag.origIn));
      right = Math.min(Math.max(right, minRight), maxRight);
      patchClip(drag.id, { out_ms: drag.origIn + (right - drag.origStart) });
      return;
    }
  };

  const onPointerUp = async () => {
    if (!drag) return;
    const d = drag;
    setDrag(null);
    const live = useStore.getState().project.clips.find((c) => c.id === d.id);
    if (!live) return;

    // if the backend rejects (overlap etc.), pull the authoritative state back
    const rollback = (why: string) => {
      console.warn(why);
      invoke<Project>("get_project").then((p) => useStore.getState().setProject(p));
    };

    if (d.kind === "move" && live.start_ms !== d.origStart) {
      await tool("clip_move", { clip_id: d.id, start_ms: live.start_ms }).catch((e) =>
        rollback(`move rejected: ${e}`),
      );
    } else if (d.kind === "trimL" || d.kind === "trimR") {
      const changed =
        live.in_ms !== d.origIn || live.out_ms !== d.origOut || live.start_ms !== d.origStart;
      if (changed) {
        await tool("clip_trim", {
          clip_id: d.id,
          in_ms: live.in_ms,
          out_ms: live.out_ms,
          start_ms: live.start_ms,
        }).catch((e) => rollback(`trim rejected: ${e}`));
      }
    } else if (d.kind === "text") {
      const t = useStore.getState().project.texts.find((t) => t.id === d.id);
      if (t && (t.start_ms !== d.origStart || t.end_ms !== d.origEnd)) {
        await tool("text_update", {
          id: d.id,
          start_ms: t.start_ms,
          end_ms: t.end_ms,
        }).catch((e) => rollback(`text move rejected: ${e}`));
      }
    }
  };

  // ---- keyboard ---------------------------------------------------------------
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const tag = (e.target as HTMLElement)?.tagName;
      if (tag === "INPUT" || tag === "TEXTAREA" || tag === "SELECT") return;

      const st = useStore.getState();
      const frame = Math.round(1000 / Math.max(1, st.project.fps));
      if (e.code === "Space") {
        e.preventDefault();
        setPlaying(!playing);
      } else if (e.key === "ArrowLeft") {
        e.preventDefault();
        setPlayhead(Math.max(0, st.playhead.ms - (e.shiftKey ? frame * 10 : frame)));
      } else if (e.key === "ArrowRight") {
        e.preventDefault();
        setPlayhead(st.playhead.ms + (e.shiftKey ? frame * 10 : frame));
      } else if ((e.key === "s" || e.key === "S") && st.selectedClipId) {
        e.preventDefault();
        const clip = st.project.clips.find((c) => c.id === st.selectedClipId);
        if (clip && st.playhead.ms > clip.start_ms && st.playhead.ms < clipEnd(clip)) {
          tool("clip_cut", { clip_id: clip.id, at_ms: st.playhead.ms });
        }
      } else if (e.key === "Delete" || e.key === "Backspace") {
        e.preventDefault();
        if (st.selectedClipId) {
          tool("clip_remove", { clip_id: st.selectedClipId, ripple: e.shiftKey });
        } else if (st.selectedTextId) {
          tool("text_remove", { id: st.selectedTextId });
        }
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [playing, setPlaying, setPlayhead, tool]);

  const onWheel = (e: React.WheelEvent) => {
    if (e.ctrlKey || e.metaKey) {
      const rect = canvasRef.current!.getBoundingClientRect();
      const x = e.clientX - rect.left;
      const msAtCursor = msOf(x);
      const factor = e.deltaY < 0 ? 1.2 : 1 / 1.2;
      const next = Math.min(2.0, Math.max(0.01, pxPerMs * factor));
      const newScroll = msAtCursor - (x - PAD_LEFT) / next;
      setPxPerMs(next);
      setScrollX(Math.max(0, newScroll));
    } else if (e.shiftKey) {
      setScrollX((s) => Math.max(0, s + e.deltaY / pxPerMs));
    } else {
      setScrollX((s) => Math.max(0, s + e.deltaX));
    }
  };

  return (
    <div className="timeline-wrap" ref={wrapRef}>
      <canvas
        ref={canvasRef}
        onPointerDown={onPointerDown}
        onPointerMove={onPointerMove}
        onPointerUp={onPointerUp}
        onWheel={onWheel}
        style={{ touchAction: "none", display: "block" }}
      />
      <div className="timeline-hint">
        drag move · drag edges trim · S split · Del remove · Shift+Del ripple-del · ␣ play · arrows step · ctrl+wheel zoom
      </div>
    </div>
  );
}
