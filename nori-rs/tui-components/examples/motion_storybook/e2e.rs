#[path = "../support/e2e.rs"]
mod e2e;
#[allow(dead_code)]
mod story;

use std::time::Duration;

use anyhow::Result;
use e2e::TuiSession;
use e2e::assert_screen;
use pretty_assertions::assert_eq;
use ratatui::Terminal;
use ratatui::backend::TestBackend;

#[test]
fn bare_zoom_snapshots() {
    for (width, height) in [(100, 34), (36, 20)] {
        for muster in [false, true] {
            for (name, time) in [
                ("dots", 0),
                ("zoom", 2400),
                ("braille", 3000),
                ("return_zoom", 3600),
            ] {
                let mut story = story::Story::new(muster);
                story.advance(Duration::from_millis(time));
                let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
                terminal
                    .draw(|frame| frame.render_widget(story.widget(), frame.area()))
                    .unwrap();
                let variant = if muster { "muster" } else { "platoon" };
                insta::assert_snapshot!(
                    format!("{variant}_{name}_{width}x{height}"),
                    terminal.backend().to_string()
                );
            }
        }
    }
}

#[test]
fn loop_preserves_overshoot_and_reverses_at_both_endpoints() {
    let mut story = story::Story::new(false);
    story.advance(Duration::from_secs(3));
    assert_eq!((story.phase(), story.zooming_in), (1.0, false));
    story.advance(Duration::from_secs(3));
    assert_eq!((story.phase(), story.zooming_in), (0.0, true));
    story.advance(Duration::from_secs(7));
    assert_eq!((story.phase(), story.zooming_in), (1.0 / 3.0, true));
}

#[test]
#[ignore = "requires built storybooks and TUI_PUPPETEERING_DIR; run scripts/storybook-e2e.sh"]
fn bare_zoom_terminal() -> Result<()> {
    let tui = TuiSession::start("motion_storybook", 100, 34)?;
    tui.expect("·")?;
    tui.key("s")?;
    tui.expect("⠂")?;
    assert_screen!(tui, "platoon_zoom_100x34");
    Ok(())
}

// Fixed story time includes both transition progress and ambient motion, so the
// outward 80% frame differs from the inward one. No wall-clock sleeps select it.
macro_rules! terminal_fixture {
    ($test:ident, $name:literal, $cols:literal, $rows:literal, $time:literal, $glyph:literal $(, $arg:literal)?) => {
        #[test]
        #[ignore = "requires built storybooks and TUI_PUPPETEERING_DIR; run scripts/storybook-e2e.sh"]
        fn $test() -> Result<()> {
            let tui = TuiSession::start_with_args(
                "motion_storybook", $cols, $rows, &["--at-ms", $time $(, $arg)?],
            )?;
            tui.expect($glyph)?;
            assert_screen!(tui, $name);
            Ok(())
        }
    };
}

terminal_fixture!(platoon_dots, "platoon_dots_100x34", 100, 34, "0", "·");
terminal_fixture!(
    platoon_braille,
    "platoon_braille_100x34",
    100,
    34,
    "3000",
    "⠝"
);
terminal_fixture!(
    platoon_return_zoom,
    "platoon_return_zoom_100x34",
    100,
    34,
    "3600",
    "⠂"
);
terminal_fixture!(platoon_narrow, "platoon_zoom_36x20", 36, 20, "2400", "⠁");
terminal_fixture!(
    muster_dots,
    "muster_dots_100x34",
    100,
    34,
    "0",
    "·",
    "--muster"
);
terminal_fixture!(
    muster_zoom,
    "muster_zoom_100x34",
    100,
    34,
    "2400",
    "⠂",
    "--muster"
);
terminal_fixture!(
    muster_braille,
    "muster_braille_100x34",
    100,
    34,
    "3000",
    "⠝",
    "--muster"
);
terminal_fixture!(
    muster_return_zoom,
    "muster_return_zoom_100x34",
    100,
    34,
    "3600",
    "⠂",
    "--muster"
);
terminal_fixture!(
    muster_narrow,
    "muster_zoom_36x20",
    36,
    20,
    "2400",
    "⠁",
    "--muster"
);
