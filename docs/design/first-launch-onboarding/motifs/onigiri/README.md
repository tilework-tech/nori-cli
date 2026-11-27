# onigiri

A rice ball wrapped in a nori band is the logo, sitting on the same baseline as
a heavy lowercase `nori` whose green i dot is the only accent. The motion tells
how onigiri are made: loose grains gather, get pressed solid, then wrapped.

## Timeline (2.0 s, then static)

| ms | What happens |
|---|---|
| 0–860 | About 60% of the ball's Braille dots drift in from a looser cloud of the same shape, dim while in flight. |
| 860–980 | Press: the solid ball grows from the centre as a scaled triangle over the dots and squashes 10% flat, then rebounds. |
| 1080–1460 | The nori band wraps up from the base, one row at a time, cell-aligned so its edges stay crisp. |
| 1320–1800 | `n o r i` set in one by one: dim Braille for 100 ms, then solid. |
| 1820–1980 | The green i dot drops onto the stem; `cli v0.42.0 ~/tidepool` appears. |

## Drawing

- Ball and letters are SDFs sampled 3x3 per quadrant; cells assumed 1:2.32.
- Rice is ANSI 7 (bg fill + quadrant edges), sheen on the left shoulder is
  ANSI 15, a few faint ANSI 15 Braille glints give grain. Nori is ANSI 8.
  Wordmark is default foreground, dot is ANSI green. No RGB or 256 colours.
- Everyday header: a 4-row ball beside a bold one-line `nori cli v…`.

## Notes

- Keypress mid-animation: jump straight to the settled frame and handle the key.
- Static / reduced motion: the settled frame (`--static`), drawn on first paint.
- Narrow (< 90x26): a 9-row ball with a text `nori cli v…` line under it
  (`img/still-narrow.png`); below about 30x14 use the existing one-line header.
- Light terminals: ANSI 7 rice nearly vanishes on white; it would need a
  DarkGray outline or swap rice to default foreground with an inverted band.

## Next

- Quadrant stair steps show on the slopes; try sextants (U+1FB00) where fonts allow.
- The nori band is a flat grey slab; a faint texture or green glint would help.
- Letter corners are still blocky; the o counter could be rounder.

Casts: `motif-onigiri --cast motion.cast`, `--cast still.cast --static [--size 40x16]`.
