---
name: Bug report
about: Something broke or the agent misbehaved
labels: bug
---

**Describe the bug**
A clear description of what happened vs. what you expected.

**To reproduce**
Steps (or the exact chat message you sent to the agent).

**What the agent did**
Paste the relevant 🔧 tool lines and ✓/✗ results from the chat panel
(they include the tool args).

**Environment**
- OS:
- Model + provider (e.g. claude-sonnet-4-5 via 9Router):
- Build: `npm run tauri dev` or a release build

**Whisper involved?**
If a transcribe/remove_ranges step was involved, paste the tail of:
`%LOCALAPPDATA%\com.mediaagent.desktop\cache\transcripts\whisper_last_run.log`

**Screenshots**
If relevant (timeline state, chat errors).
