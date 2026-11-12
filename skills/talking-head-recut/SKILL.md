---
name: talking-head-recut
description: The talking-head cleanup flow — transcribe, decide cuts (fillers, silences, bad takes), apply with remove_ranges, mask jump cuts, verify by looking at frames
---

# Talking-Head Recut

Turn a raw single-speaker video into a tight, professional cut. The decisions
are yours; the media work (transcription, silence analysis, cutting, fades,
zoom) is done by local tools.

## The flow

```
asset_import → transcribe → silence_detect → [decide cut table]
→ remove_ranges (ONE call) → mask_jump_cuts → extract_frames → verify → export
```

## 1. Transcribe
- `transcribe {asset_id, language, model: "medium", initial_prompt: "..."}`
- Put names/jargon in initial_prompt — it fixes domain words.
- medium is slow on CPU (minutes) — tell the user before starting.

## 2. Decide the cut table
Mark for removal, with reasons:
- **Fillers** («خب، عه، اِم، یعنی، ببین» or the user's list) — only when spoken
  from hesitation, NOT when a natural sentence opener.
- **Silences** — trim >0.7s down to ~0.3s: remove the middle of the gap, keep
  ~300ms of it. Don't zero silences; rhythm needs air.
- **Mouth noises** — coughs, throat clears, sudden noise.
- **Repeated takes** — same sentence said multiple times: keep the best
  (complete > fluent > correct > energetic; tie → the later one).

Cut ONLY between words — `remove_ranges` snaps to word boundaries for you.
Never let a cut change the meaning of a sentence.

## 3. Apply
- `remove_ranges {asset_id, ranges: [{start_ms, end_ms}, …]}` — one call for
  the whole table. Durations shrink, pieces butt together, export adds 30ms
  audio fades at every cut (no clicks).
- Then `mask_jump_cuts {track_id: "v1"}` — alternating 1.08 punch-in hides
  the face-jumps.

## 4. Verify (don't skip)
- `extract_frames {asset_id, every_ms: 5000}` and LOOK at the frames: face
  framing consistent, zoom level alternating, no frozen-looking artifacts.
- Present to the user: what was removed (table: from→to, reason, ms saved),
  final duration vs original, anything you were unsure about.

## Rules of thumb
- First 3 seconds decide the video: if the opening rambles, cut to the point.
- Typical raw→clean shrink: 15–35%.
- If a segment is unintelligible, remove it entirely — never guess in
  subtitles or captions.
- Show the cut table BEFORE applying when the user seems cautious
  (stage approval); apply directly when they said "go".
