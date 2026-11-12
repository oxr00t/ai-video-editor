---
name: cuts-and-pace
description: Cutting and pacing technique for assembly edits — when to cut, how long shots should last, and how to build a rhythm with the clip tools
---

# Cuts & Pace

You are editing a timeline where **every cut is visible to the user**. Good pacing
is the difference between "a slideshow" and "an edit".

## Baseline shot lengths

| Content | Duration |
|---|---|
| Establishing / wide shot | 3–6 s |
| Normal dialogue or action beat | 2–4 s |
| Detail / insert / B-roll punch | 0.8–1.5 s |
| Title card | 2–3 s (or as long as it takes to read: ~0.3 s per word + 1 s) |

When in doubt, cut on **action or motion**, never mid-static-shot.

## Workflow with the tools

1. `project_get` — see what you have.
2. Import if needed (`asset_import`), then either:
   - `assemble_sequence` for a fast rough cut (full-length, back to back), **then refine**, or
   - `clip_add` per shot with explicit `in_ms`/`out_ms` for a deliberate cut.
3. Refine the rough cut:
   - `clip_cut` at timeline positions to split clips,
   - `clip_remove` the parts that drag,
   - `clip_move` to reorder.
4. Tighten: most first cuts are too long. Trim each clip's dead head/tail by
   re-adding with tighter in/out points (remove + add at the same slot), or cut
   and remove the slack segment.

## Rules of thumb

- Open with your strongest 3 seconds.
- A static shot longer than 5 s without motion needs a cut, text, or removal.
- End the sequence on a held beat: last shot 1.5–2× the average shot length.
- Total duration = story length, not asset length. Cutting is editing.

## Worked example

User: "make a snappy 20s cut from these 4 clips"

```
project_get
assemble_sequence([a, b, c, d])        # rough cut
clip_cut(clip_b, at=8500) → remove second half if slack
clip_cut(clip_c, at=12000) → remove first half
text_add("THE GOOD PART", 0, 2500, y=0.18, font_size=96)
```

State durations to the user in plain language when you finish.
