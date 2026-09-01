mod e2e;

use anyhow::Result;
use e2e::Screen;
use pretty_assertions::assert_eq;

#[test]
fn text_capture_preserves_unicode_spacing_and_rows_without_style_sequences() -> Result<()> {
    let ansi = "\x1b[1;38;2;20;40;60m  Café │ 日本語\x1b[0m  \n\n  next row\n";
    let screen = Screen::from_ansi(ansi.to_owned())?;
    assert_eq!(screen.text, "  Café │ 日本語  \n\n  next row\n");
    assert_eq!(screen.ansi, ansi);
    Ok(())
}

#[test]
fn text_grid_restores_blank_cells_using_display_width() -> Result<()> {
    let screen = Screen::from_ansi("\n\x1b[31m日e\u{301}\x1b[0m\nx  \n\n".to_owned())?;
    assert_eq!(
        screen.text_grid(8)?,
        "        \n日e\u{301}     \nx       \n        \n"
    );
    Ok(())
}

#[test]
fn text_grid_rejects_invalid_width_instead_of_truncating_content() -> Result<()> {
    let screen = Screen::from_ansi("12345\n".to_owned())?;
    for cols in [-1, 0, 4] {
        assert!(screen.text_grid(cols).is_err());
    }
    assert_eq!(screen.text_grid(5)?, "12345\n");
    Ok(())
}

#[test]
fn ansi_snapshot_distinguishes_escape_sequences_from_literal_backslashes() -> Result<()> {
    let screen = Screen::from_ansi("\x1b[31mred\x1b[0m \\x1b[31m\n".to_owned())?;
    assert_eq!(screen.snapshot_ansi(), "\\x1b[31mred\\x1b[0m \\\\x1b[31m\n");
    Ok(())
}

#[test]
fn replay_hides_cursor_and_removes_only_the_final_row_separator() -> Result<()> {
    let screen = Screen::from_ansi("\x1b[31mfirst\x1b[0m\n\nlast\n\n".to_owned())?;
    assert_eq!(
        screen.replay_ansi(),
        "\x1b[?25l\x1b[31mfirst\x1b[0m\n\nlast\n"
    );
    Ok(())
}

#[test]
fn unexpected_terminal_controls_fail_instead_of_corrupting_the_text_snapshot() {
    for ansi in [
        "\x1b[2Jtext",
        "unfinished\x1b[31",
        "\x1b]0;title\x07",
        "overwrite\r",
        "back\x08",
        "bell\x07",
    ] {
        assert!(Screen::from_ansi(ansi.to_owned()).is_err(), "{ansi:?}");
    }
}

#[test]
#[ignore = "requires built storybooks and TUI_PUPPETEERING_DIR; run scripts/storybook-e2e.sh"]
fn sessions_are_isolated_and_cleaned_up_on_unwind() -> Result<()> {
    use e2e::TuiSession;
    let survivor = TuiSession::start("component_storybook", 120, 40)?;
    survivor.expect("Component storybook")?;
    let other = TuiSession::start("markdown_storybook", 48, 30)?;
    other.expect("Markdown storybook")?;
    let name = other.session_name().to_owned();
    let result = std::panic::catch_unwind(move || {
        let _guard = other;
        panic!("exercise cleanup when a snapshot assertion panics");
    });
    assert!(result.is_err());
    let scripts = std::env::var("TUI_PUPPETEERING_DIR")?;
    let status = std::process::Command::new(format!("{scripts}/tmux-isolated"))
        .args(["has-session", "-t", &name])
        .output()?
        .status;
    assert!(!status.success(), "failed test leaked its terminal session");
    survivor.expect("Component storybook")?;
    Ok(())
}
