# AGENTS.md — for coding agents working in this repo

## What this is

Media Agent: a Tauri 2 app. Rust backend (`src-tauri/`), React+TS frontend
(`src/`). The core concept: the project is a JSON EDL; **every** mutation goes
through `src-tauri/src/tools/` so undo, validation, and live UI updates apply
to all clients (UI, internal agent, MCP bridge).

## Commands

- `npm run tauri dev` — run the app in dev
- `cargo check` / `cargo test` (in `src-tauri/`) — backend checks/tests;
  tests exercise real ffmpeg against `test-media/` clips
- `npx tsc --noEmit` — frontend type check
- `npm run build` — frontend production build

## Rules

- New editing capabilities = a new tool in `tools/` (handler + ToolDef with a
  clear description and JSON schema). Never mutate the project outside
  `tools::mutate` (it snapshots for undo, validates, broadcasts, saves).
- Keep the Rust core GUI-free: `edl/`, `engine/`, `tools/`, `control.rs`,
  `mcp/` must stay testable headless (`cargo test` runs without a window).
- All times in the EDL are integer **milliseconds**. Track ids: `v1`, `v2`
  (video, v2 renders on top), `a1` (audio). Text positions are relative
  0..1 centers.
- `mcp_bridge` (`--mcp` flag) must stay dependency-light and never touch
  Tauri APIs — it runs as a plain stdio process next to the GUI app.
- Export compiles ONE ffmpeg command (`engine/export.rs`). Keep that property;
  don't render frame-by-frame in Rust.
- When changing tool signatures, update: ToolDef schema, the internal agent
  system prompt (`agent/mod.rs`), and README's tool list.

## Skills directory

`skills/<name>/SKILL.md` — technique guides agents read at runtime
(`skills_list` / `skill_read`). Frontmatter needs `description:`; the agent
uses it to pick what to read.
