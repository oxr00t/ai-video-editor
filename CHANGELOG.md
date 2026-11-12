# Changelog

All notable changes to this project are documented here. Format loosely
follows [Keep a Changelog](https://keepachangelog.com/).

## [0.1.0] — 2026-11-12

First public release — the walking skeleton plus the talking-head pipeline.

### Core
- EDL data model (`Project` / `Track` / `Clip` / `TextClip`) — the timeline
  is JSON, sources are never modified
- Snapshot-based undo/redo shared by humans, the internal agent, and MCP
  clients (single mutation authority: snapshot → validate → broadcast)
- Canvas timeline: drag clips, drag edges to trim, split at playhead,
  ripple delete, magnetic snapping, keyboard shortcuts (Space/S/Del/arrows)
- Preview via 360p proxies, text overlays rendered over the preview
- ffmpeg export compiled as a single command: topmost-wins video segments,
  drawtext overlays, per-clip punch-in zoom (lanczos + crop), 30 ms audio
  fades at every cut, EBU R128 loudnorm (project `loudness_lufs`)

### Agents
- Internal BYOM agent: OpenAI-compatible endpoints (OpenAI, Ollama,
  OpenRouter, LM Studio, 9Router, …) and Anthropic, with SSE-tolerant
  response parsing and prompt caching on the Anthropic path
- MCP stdio bridge (`app --mcp`) — Claude Code / ZCode / Cursor edit the
  same live project over a local WebSocket control server
- Vision: `extract_frames` with `with_images=true` attaches frames to the
  model conversation for visual QA; old images are compacted out of context
- Token economy: compact tool results, 8-char ids, history squashing,
  compact JSON on the MCP wire
- 26 tools, incl. recipes for small models (`assemble_sequence`,
  `remove_ranges`, `mask_jump_cuts`) and agent skills
  (`talking-head-recut`, `cuts-and-pace`, `captions-basic`)
- Live transcript quality warnings (hallucination loops, low coverage,
  model/language mismatch) and bounded cut snapping + a >60% removal guard

### Speech pipeline
- Local Whisper transcription (word-level, `initial_prompt`, UTF-8-safe
  child-process environment) and ffmpeg `silencedetect`

[0.1.0]: https://github.com/YOUR_USER/media-agent/releases/tag/v0.1.0
