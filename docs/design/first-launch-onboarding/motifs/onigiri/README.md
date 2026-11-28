# onigiri (v2)

The code now lives in `nori-rs/tui-components/src/onigiri/` (implementation,
tests and the demo runner). This folder keeps the design notes and renders.

A rice ball with a square nori band. Grains gather, get pressed, and get wrapped,
then a light circles the ball for as long as the welcome is on screen. The
`nori` wordmark beside it is drawn in rounded box-drawing lines (style 9 from
`../../wordmark/`).

## Timeline

Times below are wall-clock. The constants in `timeline.rs` and
`word/motion.rs` are on the timeline's own clock, and the intro plays `SPEED`
(1.15) times faster than that until `SETTLED_AT` (3000), which is 2.61 s of
real time. After that the scene runs in real time, so the light keeps its 6 s turn.

| ms | What happens |
|---|---|
| 130–850 | Four pens write `n`, `o`, `r`, `i` at once (52 ms apart) in half-cell steps, leaving unlit glass. |
| per letter | Each finished letter ignites with a neon stutter (290 ms), glows, then cools over 0.8 s to a faint resting glow. |
| 610–1360 | As the pens finish, grains (Braille dots) fly into frame, overshoot their spot by about 12% and spring back. |
| 1360–1450 | Press: the solid ball grows from the centre and squashes 10%. |
| 1450–1680 | Hop: the ball springs up stretched and falls back; the green i dot falls at the same time. |
| 1680 | Ball and dot land on the same frame: the ball squashes and wobbles out over 0.45 s, the dot squashes, bounces and ripples down the stem. In the TUI the setup form starts fading in here. |
| 2130–2460 | The nori band wraps up from the base, one row at a time. |
| 2130→ | The light eases up to speed over about 1 s, then turns once every 6 s. |
| 3010→ | A shimmer runs across the wordmark and back once per turn of the light. |

## Wordmark

Code in `tui-components/src/onigiri/word/`. Every timing and strength is a
constant in `word/motion.rs`; letter proportions are in `word/letters.rs`.

- Letter height is 0.28 of the ball height (at least 2 rows); widths, gaps and
  the i dot size scale from it.
- The shimmer's position is the light's own left/right direction, so it is in
  sync with the rice by construction. Its leading edge is short and white, its
  tail long and green, and it leans by `SLANT`.
- Glow is a background tint around lit strokes, baked per size. It assumes the
  dark background it was designed on. ANSI 16 has no glow; there the resting
  wordmark uses the theme's own foreground and the shimmer is green and bold.
- 256 colours step the glow through hand-picked greys and green (233/234/22).
- Renders: `img/wordmark/` (100x30 in all three tiers, and `tiers.png`).

## Lighting

- The rice is a puffy surface built from the ball's outline, slightly blurred so
  the triangle's creases soften. It has diffuse light, a specular highlight,
  and ambient light.
- About half the Braille dots belong to a grain (pairs of dots with their own
  tilt). Grains facing the light glint, and grains facing away show as pits,
  so the texture moves with the light while the shape stays still.
- Cells without a grain are split with `▀`, which gives the shading two pixels
  of height per cell. The outline uses quadrant blocks.
- The band stays a flat square, kept clean for a future QR code, with a faint
  gloss that follows the light.

## Scaling

The ball size comes from the terminal size (`layout()` in
`tui-components/src/onigiri/layout.rs`):

| Ball height | Behaviour |
|---|---|
| ≥ 11 rows (about 70x18 and up) | Full intro with particles, then the light loop. |
| 7–10 rows | Settled ball with grain texture and the light loop. No intro. |
| 4–6 rows | Settled ball with smooth shading only. |
| < 4 rows | One-line `nori cli v…` header. |

The ball sits beside the block wordmark by default. It moves above a one-line
text wordmark only when stacking gives a much bigger ball (`img/sizes.png`).

## Colour tiers (`--tier`, otherwise `COLORTERM` / `TERM`)

- **truecolor:** cool rice greys from the Nori palette up to white, and nori
  from the verdant greens.
- **256:** the nearest xterm-256 match of the same RGB values. Nori uses a
  hand-picked ramp (233/234/236/22), because a nearest match turns it plain grey.
- **ANSI 16:** rice is Gray with White highlights; no DarkGray, which reads
  as mud. The grain texture carries the shading: White dots on Gray rice and
  Gray dots in the White highlight. The band is still DarkGray.

## Performance

`cargo run --release -p nori-tui-components --example onigiri -- --bench`
prints the table below; `-- --live` plays it in your terminal with an fps,
frame-time and KB/s overlay. Numbers are from one core of a cloud EPYC.

| 160x50 truecolor | Prototype | Now |
|---|---|---|
| Light loop, render per frame | 2.2 ms | 0.1 ms |
| Intro, render per frame (avg) | 1.1 ms | 0.48 ms |
| Frame rate | 25 (fixed) | 60 intro, 30 loop |
| Light loop output | 31 KB/frame | 27 KB/frame, 0.8 MB/s |

- Everything that depends only on the terminal size (coverage, normals, grain
  tilts, wordmark glyphs, grain flight paths) is baked once per size. A loop
  frame only computes lighting. The first frame at a new size pays for the
  bake (about 4 ms at 160x50).
- The light moves on a 30 Hz grid, so the loop draws at 30 fps. It turns 2° a
  frame; 60 fps would not look smoother but would double the output.
- Brightness goes through a 64-step palette. Steps are at most 2 RGB levels
  apart, which is invisible, and cells whose step did not change are not
  rewritten. 32 steps would cut loop output by another third.
- Grains fade in and out with how strongly they catch the light instead of
  switching on at a threshold.
- The remaining cost is the press (860–1350 ms of timeline time), which is still computed live
  because the ball squashes every frame, and ratatui's own diff and colour
  encoding, which now costs more than drawing.

## Open

- The production TUI bans `Color::Rgb` through clippy. The onigiri module has
  one scoped exception, on its colour mapping.
- A QR code (version 1 is 21x21 modules) needs about 25x13 cells with a quiet
  zone. That is far bigger than the band at any normal size, so the QR moment
  needs its own layout, for example the band unfolding into a sheet.
- Light terminals and the narrow small-size art are still untuned.

Casts are in `casts/`. Record them with
`cargo run --release -p nori-tui-components --example onigiri -- --cast FILE`
and render them with `../../scripts/render.sh`.
