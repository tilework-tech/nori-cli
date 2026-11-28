# onigiri (v2)

The code now lives in `nori-rs/tui-components/src/onigiri/` (implementation,
tests and the demo runner). This folder keeps the design notes and renders.

A rice ball with a square nori band. Grains gather, get pressed, and get wrapped,
then a light circles the ball for as long as the welcome is on screen. The
wordmark beside it is the v1 placeholder and gets redrawn next.

## Timeline

| ms | What happens |
|---|---|
| 0–860 | Grains (Braille dots) fly in from a looser cloud of the same shape. |
| 860–980 | Press: the solid ball grows from the centre, squashes 10%, and rebounds. |
| 1080–1460 | The nori band wraps up from the base, one row at a time. |
| 1320–1980 | Placeholder wordmark sets in, then the green i dot drops. |
| 1900→ | The light eases up to speed over 1.2 s, then turns once every 6 s. |

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
- **ANSI 16:** rice is Gray, with White for highlights and DarkGray for
  shadow. Glints are White, there are no pits, and the band is DarkGray.

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
- The remaining cost is the press (860–1350 ms), which is still computed live
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
