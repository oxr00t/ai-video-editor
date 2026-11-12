---
name: captions-basic
description: How to add readable title cards and captions with text_add — sizing, placement, timing and readability rules
---

# Captions & Titles

Text overlays live on their own lane and render on top of the video. The
preview shows them exactly where export puts them.

## Placement (x/y are CENTER of the frame, 0..1)

| Use | x | y | font_size (1080p) |
|---|---|---|---|
| Main title | 0.5 | 0.42 | 90–120 |
| Lower-third name/tag | 0.5 | 0.85 | 48–64 |
| Caption line | 0.5 | 0.88 | 44–56 |
| Small corner note | 0.5 | 0.94 | 32–40 |

- Keep text inside 0.05–0.95 on both axes — edges get cropped on phones.
- font_size is in output pixels (the project renders at project.height, e.g. 1080).

## Timing

- Titles: hold at least `words × 0.3s + 1s`.
- Captions: sync to speech; max ~42 characters per line, split longer lines
  into consecutive text clips rather than one long one.
- Never let a caption outlive its shot by more than ~300ms.

## Readability

- White (#FFFFFF) on footage; yellow (#FFD400) also reads well.
- Over busy footage prefer larger sizes — there is no outline in v0.
- All-caps for titles, sentence case for captions.

## Recipe: caption a segment

```
project_get                        # find shot boundaries
text_add("In this video…", 0, 2800, x=0.5, y=0.42, font_size=96)
text_add("step one — prep",  2800, 6300, x=0.5, y=0.88, font_size=48)
text_add("step two — edit",  6300, 9800, x=0.5, y=0.88, font_size=48)
```

Adjust start/end to actual clip boundaries from project_get, not guesses.
