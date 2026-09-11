# Full-screen motion background

`nori-tui-components::MotionBackground` is a presentation-only Ratatui layer
inspired by `norimotionsuite.html`, the supplied Nori Motion Suite reference.
It is intended for caller-owned alternate-screen onboarding and login flows.
The component does not initialize a terminal, collect credentials, launch a
browser, contact a provider, or store tokens. Those remain application concerns.
This change supplies the component and specimens; production onboarding is a
separate consumer integration.

## Compose it

```rust
use std::time::Duration;
use nori_tui_components::{MotionBackground, MotionPalette, MotionScene, MotionState};
use ratatui::layout::Rect;

let mut motion = MotionState::new(MotionScene::Dots);
motion.advance(Duration::from_millis(40)); // elapsed time supplied by your loop
motion.set_scene(MotionScene::Creatures);

// In your application's terminal draw call:
terminal.draw(|frame| {
    let form = Rect::new(25, 8, 50, 16);
    frame.render_widget(
        MotionBackground::new(&motion)
            .palette(MotionPalette::nori())
            .quiet_area(form),
        frame.area(),
    );
    // Render your foreground widgets into `form` now.
});
```

`quiet_area` uses absolute terminal coordinates. It blanks the content rectangle
and fades the surrounding scenery over six horizontal or three vertical cells.
It clears old content, so render the background first on every frame. The widget
clips to the intersection of its rectangle and the buffer, including non-zero
origins, empty areas, and tiny viewports. Foreground layout is entirely caller-owned.

## Motion and rendering policies

- `MotionScene::ALL` orders Dots, Formations, Sandboxes, Creatures, Blueprint.
  Dots zoom into ordered-dither braille. Formations cycle through moving lanes,
  argyle, orbiting paired tiles, and a perspective muster. A fade separates the
  formation field from sandboxes; those grow into blinking creatures and then
  braille wireframes. Canvas strokes are approximated with terminal cells.
- `set_scene` starts or reverses a transition at the current position. Each
  adjacent phase takes three seconds. Skipping phases traverses intermediate
  phases; reaching any destination takes at most twelve seconds.
- The caller supplies elapsed `Duration` through `advance`. Render is pure:
  repeated state/geometry/palette produces the same frame. There is no RNG or
  hidden wall clock. The scenery clock wraps after 24 hours to keep numerical
  inputs bounded; a continuously displayed background can change at that boundary.
- Pause by stopping `advance`; discard elapsed paused time when resuming.
  `set_reduced_motion(true)` freezes all scenery, including blinking and formation
  changes, and makes scene changes immediate. Consumers own preference detection.
- Choose a measured frame budget for the intended terminal size, and redraw only
  for input or resize when paused/reduced. The example uses a 40 ms frame budget
  and blocking input polling when idle. Rendering costs O(visible cells), with
  at most eight procedural field samples per cell and no retained frame history.
- Unicode uses single-column geometric glyphs and 2×4 braille sampling. Use
  `.ascii(true)` for fonts or terminals without those glyphs. This policy belongs
  to the consumer and is independent of the palette.
- The default palette uses `Theme::default().surface` and muted foreground.
  `MotionPalette::from_theme(theme)` accepts caller-provided terminal styles;
  it does not invent a background. `MotionPalette::nori()` explicitly opts into
  the reference's RGB ink `#0a0f0c`, green `#7dd6a0`, and pale highlights. Only
  use that art palette when true color is suitable; compose foreground copy with
  explicit contrasting colors, as the example does. Custom palettes expose
  `surface`, `ink`, and `highlight` styles. RGB colors are blended and quantized
  into sixteen intensity steps; other palettes use their supplied ink and dimming.

## Bare zoom storybook and profiling

The focused example now contains only a continuous Dots ↔ Formations zoom, three
seconds in each direction, with no text or UI. `--muster` selects perspective
muster; otherwise it uses platoon. `MotionBackground::formation` accepts
`MotionFormation::{Cycle, Platoon, Muster}`. The default remains `Cycle`, so
existing consumers keep the ambient sequence. Pinning a formation replaces only
the formation selector/fade; the field formulas remain the same.

From `nori-rs/`:

```console
cargo run --release -p nori-tui-components --example motion_storybook
cargo run --release -p nori-tui-components --example motion_storybook -- --muster
```

Space pauses, `s` freezes at a repeatable 80%-zoom frame, and `q`, Escape, or
Ctrl-C restores the terminal and quits. `--still` starts at the dot endpoint.
`--fps` configures cadence (25 by default). `--frames N --profile PATH` records
live timing without on-screen counters, writing CSV after terminal restoration.
`--bench --size WIDTHxHEIGHT --profile PATH` measures widget, diff/encoding, and
buffer reset without terminal I/O. See the
[profiling guide](../../nori-rs/tui-components/examples/motion_storybook/README.md)
and [measurement report](../performance/motion-background/report.md).

## Verification

`cargo test -p nori-tui-components` covers all five reusable scenes, transitions,
determinism, reduced motion, clipping, quiet regions, styles, glyph widths, and
ASCII fallback. The example snapshots dots, 80% zoom, and braille endpoints for
both fixed variants at 100×34 and 36×20, and tests reversal/overshoot behavior.
Its opt-in tmux case checks the actual full-screen frame through the shared
[storybook capture workflow](../../nori-rs/tui-components/examples/README.md).
