# Contributing to Media Agent

Thanks for helping! This is a young project — small, focused PRs land fastest.

## Dev setup

```bash
npm install
npm run tauri dev      # run the app
cargo test             # in src-tauri/ — real ffmpeg tests (needs ffmpeg on PATH)
npx tsc --noEmit       # frontend types
npm run build          # frontend build
```

Optional (speech tools): `pip install openai-whisper`.

## Ground rules

- **Every editing capability is a tool.** New feature = new entry in
  `src-tauri/src/tools/` (handler + `ToolDef` with schema + description).
  Never mutate the project outside `tools::mutate` — it snapshots for undo,
  validates, broadcasts, and autosaves.
- **GUI-free core.** `edl/`, `engine/`, `tools/`, `control.rs`, `mcp/` must
  stay testable headless (`cargo test` runs without a window).
- **Times are integer milliseconds.** Tracks: `v1`, `v2` (video, v2 on top),
  `a1` (audio). Text positions are relative 0..1 centers.
- **Export compiles ONE ffmpeg command** (`engine/export.rs`). Don't
  introduce frame-by-frame rendering in Rust.
- **Token economy matters.** Tool results should be compact; use the shared
  serializers in `tools/ops_media.rs`. Don't add file paths or redundant
  fields to model-facing output.
- When changing tool signatures, update in the same PR: the `ToolDef` schema,
  the agent system prompt (`agent/mod.rs`), and the README tool table.

## Adding a skill

`skills/<name>/SKILL.md` — a technique guide agents read at runtime via
`skills_list` / `skill_read`. Frontmatter needs a `description:`. Keep them
opinionated and practical (see `talking-head-recut` for the house style).

## PR checklist

- [ ] `cargo test` passes (in `src-tauri/`)
- [ ] `npx tsc --noEmit` passes
- [ ] Tests for new engine/tool logic (ffmpeg-based tests live in
      `src-tauri/tests/` and run against real binaries)
- [ ] README updated if the tool surface changed

## Reporting bugs

Use the issue templates. For agent-behavior bugs, include: which model +
provider, the chat/tool output (the 🔧 lines show args), and if Whisper was
involved, the tail of
`%LOCALAPPDATA%\com.mediaagent.desktop\cache\transcripts\whisper_last_run.log`.

## Coding agents

This repo ships `AGENTS.md` — open it in Claude Code / ZCode and ask for a
feature. Agent-authored PRs are welcome under the same rules.
