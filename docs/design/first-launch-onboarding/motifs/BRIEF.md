# Motif drafts: shared brief

Five terminal motifs for the Nori CLI first launch, each a minimum lovable
draft: one beautiful polished still and a short motion segment. Cliff judges
from these whether the idea is worth finishing. This is design exploration, not
production code.

## The five motifs

| Slug | Idea |
|---|---|
| `onigiri` | Logo: a rice ball (onigiri) with a nori wrap, drawn as terminal art. |
| `noren` | Concept: setup opens behind a split noren curtain printed with `nori`; a key parts it. |
| `conveyor` | Wordmark: the `nori` letters ride in on a conveyor belt and settle. |
| `unmixing` | Wordmark: rows shear until `nori` smears to a swirl, then the shear runs exactly backwards and it reassembles crisp. |
| `moire` | Wordmark: two drifting Braille dot gratings make interference bands; `nori` half-appears in the bands, then both gratings snap to one angle and the word locks solid. |

## Brand context

- The wordmark is lowercase `nori`: heavy, tight-set, rounded. The current
  study draws it in three rows of half blocks with a green i dot; see
  `../README.md` and `../img/settled-80x24.png`. Cliff says those letterforms
  still need work, so you may draw larger or different letterforms. Keep them
  lowercase, heavy and legible.
- Jiro's face is `〉_ ▐`, all green. Use it only if it helps.
- Accent is ANSI green. Everything else uses the default foreground, DIM, BOLD,
  and other ANSI 16 colors sparingly. The production TUI forbids `Color::Rgb`
  (clippy), so stay in ANSI 16 colors plus modifiers. If the motif truly needs
  256-color shading, say so in your README and justify it.
- Useful glyph sets: half blocks `▀▄█▌▐`, quadrant blocks `▖▗▘▝▚▞▙▛▜▟`, Braille
  `U+2800–U+28FF` (2×4 dots per cell), shades `░▒▓`, box drawing. Remember one
  cell is about twice as tall as it is wide.

## Rules

- Original art and writing only. Do not copy website assets or any other
  product's art. Synthetic content only (project `~/tidepool`, version
  `v0.42.0`).
- Keys must work from the first frame in a real version; note how your motif
  would handle a keypress mid-animation and what the static / reduced-motion
  frame is (one sentence each).
- Note the narrow-terminal fallback (one sentence).
- Animation should be bounded: about 0.5–3 s, then settle and stop.

## How to build

- Work only inside `motifs/<slug>/`. Do not edit anything else, and do not
  commit or push; the coordinator commits.
- Recommended: a standalone Rust crate using `ratatui = "0.30"` like
  `../prototype/` (its own `[workspace]` table, package name
  `motif-<slug>`). Copy what helps from `../prototype/src/main.rs`, especially
  the in-memory terminal and the `write_cast` asciicast writer, which records
  the exact bytes ratatui emits on a deterministic clock. Make the effect a pure
  function of elapsed milliseconds so stills can be taken at exact times.
- Build with `CARGO_TARGET_DIR=/tmp/motiftarget cargo build --release -j 2`.
  The target dir is shared and prewarmed with ratatui; cargo serializes on its
  lock, so waiting a little is normal. Do not run other heavy builds.
- Render with `bash ../../scripts/render.sh gif in.cast out.gif` and
  `bash ../../scripts/render.sh still in.cast out.png T` (T in seconds). It
  uses agg, JetBrains Mono, DejaVu Sans for Braille, and Noto Sans Mono CJK JP.
- Look at your own PNGs with the Read tool and iterate until the frame is
  genuinely beautiful. Check Braille and block glyphs render without gaps that
  ruin the picture. Polish matters more than features.

## Deliverables in `motifs/<slug>/`

- `img/still.png`: the hero frame at 100x30 cells (or 120x40 if the art needs
  it).
- `img/motion.gif`: the motion segment, 100x30 (or 80x24), under 6 s and under
  5 MB.
- `img/still-narrow.png` (optional): the fallback at about 40x16.
- `README.md`: under 40 lines. The concept in two sentences, the timeline, the
  keypress / static / narrow notes, and anything you would change next.
- The crate source and the `.cast` files.

Report back: the file paths, a two-line description of what the still and GIF
show, and an honest one-line verdict on whether the idea is lovable.
