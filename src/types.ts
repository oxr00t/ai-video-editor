// Shared types mirroring the Rust EDL (serde camelCase-free field names).

export interface Asset {
  id: string;
  name: string;
  path: string;
  proxy_path: string | null;
  duration_ms: number;
  width: number;
  height: number;
  fps: number;
  has_audio: boolean;
}

export interface Track {
  id: string;
  kind: string;
  name: string;
}

export interface Clip {
  id: string;
  asset_id: string;
  track_id: string;
  start_ms: number;
  in_ms: number;
  out_ms: number;
}

export interface TextClip {
  id: string;
  text: string;
  start_ms: number;
  end_ms: number;
  x: number;
  y: number;
  font_size: number;
  color: string;
}

export interface Project {
  id: string;
  name: string;
  fps: number;
  width: number;
  height: number;
  assets: Asset[];
  tracks: Track[];
  clips: Clip[];
  texts: TextClip[];
}

export interface AgentEvent {
  type: "message" | "status" | "tool" | "tool_result" | "error" | "user";
  text?: string;
  name?: string;
  args?: unknown;
  ok?: boolean;
  result?: unknown;
}

export function formatTime(ms: number): string {
  const total_s = Math.floor(ms / 1000);
  const m = Math.floor(total_s / 60);
  const s = total_s % 60;
  const cs = Math.floor((ms % 1000) / 10);
  return `${String(m).padStart(2, "0")}:${String(s).padStart(2, "0")}.${String(cs).padStart(2, "0")}`;
}
