# onigiri (v2)

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

The ball size comes from the terminal size (`layout()` in `src/main.rs`):

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

## Open

- The production TUI bans `Color::Rgb` through clippy. The truecolor tier
  would need a scoped exception.
- A QR code (version 1 is 21x21 modules) needs about 25x13 cells with a quiet
  zone. That is far bigger than the band at any normal size, so the QR moment
  needs its own layout, for example the band unfolding into a sheet.
- Light terminals and the narrow small-size art are still untuned.

Casts are in `casts/`. Render them with `../../scripts/render.sh`. Build with
`CARGO_TARGET_DIR=/tmp/motiftarget cargo build --release`.
