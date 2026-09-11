# Full-screen motion background

`nori-tui-components::MotionBackground` is a presentation-only Ratatui layer
inspired by `norimotionsuite.html`, the supplied Nori Motion Suite reference.
It is intended for caller-owned alternate-screen onboarding and login flows.
The component does not initialize a terminal, collect credentials, launch a
browser, contact a provider, or store tokens. Those remain application concerns.
This change supplies the component and specimens; production onboarding is a
separate consumer integration.

## Compose it

The renderer accepts an explicit scene, ambient time, and transition progress.
It owns no playback state and does not borrow a controller. The caller chooses
transition duration, easing, and any seek/reversal policy:

```rust
use std::time::Duration;
use nori_tui_components::{MotionBackground, MotionPalette, MotionScene};
use ratatui::layout::Rect;

let elapsed = Duration::from_secs(12);
let progress = 0.8; // Supply your timeline's progress, optionally already eased.

terminal.draw(|frame| {
    let form = Rect::new(25, 8, 50, 16);
    frame.render_widget(
        MotionBackground::new(MotionScene::Dots, elapsed)
            .transition_to(MotionScene::Formations, progress)
            .palette(MotionPalette::nori())
            .quiet_area(form),
        frame.area(),
    );
    // Render your foreground widgets into `form` now.
});
```

For simple linear playback, keep an optional `MotionState` and call
`motion.background()` to get an owned frame snapshot. This replaces the original
`MotionBackground::new(&motion)` API:

```rust
use std::time::Duration;
use nori_tui_components::{MotionScene, MotionState};

let mut motion = MotionState::new(MotionScene::Dots);
motion.set_scene(MotionScene::Creatures);
motion.advance(Duration::from_millis(40));
let background = motion.background();
// Render `background`, optionally adding palette, quiet_area, ascii, or formation.
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
- `transition_to(target, progress)` clamps progress to `[0, 1]`; NaN means zero.
  Both endpoints match their static scenes. It interpolates from the widget's
  current position, so a captured controller frame can also be redirected.
  Recreate the widget from your chosen origin when seeking an absolute progress.
- On the optional controller, `set_scene` starts or reverses a transition at the current position. Each
  adjacent phase takes three seconds. Skipping phases traverses intermediate
  phases; reaching any destination takes at most twelve seconds.
- Supply ambient time directly to `new`, or advance the optional controller. Render is pure:
  repeated state/geometry/palette produces the same frame. There is no RNG or
  hidden wall clock. The scenery clock wraps after 24 hours to keep numerical
  inputs bounded; a continuously displayed background can change at that boundary.
- Pause by stopping `advance`; discard elapsed paused time when resuming.
  The controller's `set_reduced_motion(true)` freezes its existing ambient time
  and makes scene changes immediate. The stateless widget's `.reduced_motion(true)`
  instead renders its target at time zero, independent of the supplied timeline.
  Both stop blinking and formation changes. Consumers own preference detection.
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
  explicit contrasting colors. Custom palettes expose
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

## Implementation boundaries

- `motion/mod.rs`: public scenes, frame description, clipping, and Ratatui painting.
- `motion/state.rs`: optional playback controller and its state invariants.
- `motion/palette.rs`: theme integration and scenery intensity styles.
- `motion/fields.rs`: frame context, scene dispatch, and cell/braille sampling.
- `motion/fields/zoom.rs`: dot islands and formation geometry.
- `motion/fields/tiles.rs`: sandbox, creature, and blueprint geometry.
- `motion/fields/math.rs`: shared deterministic noise/hash/interpolation.

The storybook owns its six-second ping-pong timeline and supplies explicit
progress to the renderer. Profiling instruments the example's draw boundary;
there are no benchmark timers in the reusable renderer. The API cleanup retains
existing field formulas, palette, and sampling schedule so subsequent performance
changes can be measured separately.

## Measured optimization pass

The renderer prepares styles and bounded geometry caches once per frame. Island
motion, noise corners, and muster depth rows are reused while sampling, with no
retained cache across calls. Palette, time, viewport, and clipping changes take
effect immediately. Opaque formations omit invisible ambient work.

The storybook terminal uses a locked, buffered stdout writer beneath Ratatui's
existing incremental diff. The widget itself owns no output stream. See the
[baseline and optimization results](../performance/motion-low-hanging/README.md)
for measured gains and remaining large-terminal limitations.
