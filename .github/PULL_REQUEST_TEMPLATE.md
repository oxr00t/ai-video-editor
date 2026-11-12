## What

<!-- one or two sentences: what does this PR change? -->

## Why

<!-- link the issue, or describe the motivation -->

## Checklist

- [ ] `cargo test` passes (src-tauri/)
- [ ] `npx tsc --noEmit` passes
- [ ] Tests added for new engine/tool logic
- [ ] If tools changed: `ToolDef` schema + agent system prompt + README table updated
- [ ] No project mutations outside `tools::mutate`
