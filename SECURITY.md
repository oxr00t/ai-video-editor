# Security Policy

## Reporting a vulnerability

Please don't open public issues for security problems. Use GitHub's
**private vulnerability reporting** (Security → Report a vulnerability) or
contact the maintainers directly.

We aim to respond within a few days.

## Scope notes

- Media Agent is **local-first**: your model API key is stored only in your
  local app data (`…/com.mediaagent.desktop/cache/settings.json`) and sent
  only to the base URL you configure.
- The MCP bridge and WebSocket control server bind to `127.0.0.1` only and
  accept tool calls from your own machine. Don't port-forward them.
- Child processes invoked: `ffmpeg` / `ffprobe` / `python -m whisper` /
  `node` (wave 2). Don't run untrusted skills or projects from people you
  don't trust — skill files are markdown instructions your agent will read.
