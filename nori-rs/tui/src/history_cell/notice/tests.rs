use super::fixtures::NoticeSpecimen;
use super::*;
use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyEventKind;
use crossterm::event::KeyModifiers;
use pretty_assertions::assert_eq;

const EMPTY: PromptState = PromptState {
    is_empty: true,
    is_busy: false,
};
const DRAFT: PromptState = PromptState {
    is_empty: false,
    is_busy: false,
};
const BUSY: PromptState = PromptState {
    is_empty: true,
    is_busy: true,
};

fn text(lines: &[Line<'static>]) -> Vec<String> {
    lines
        .iter()
        .map(|line| {
            line.spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect::<String>()
        })
        .collect()
}

fn stderr(output: &str) -> Vec<String> {
    let notice = Notice::from_stderr("Codex adapter", output).expect("visible output");
    let mut rows = text(&notice.display_lines(300));
    rows.remove(0);
    rows.pop();
    rows
}

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

fn slot_with(specimens: &[NoticeSpecimen]) -> NoticeSlot {
    let mut slot = NoticeSlot::default();
    for specimen in specimens {
        slot.offer(specimen.notice());
    }
    slot
}

/// Every specimen in every state a slot can leave it in.
fn every_state(specimen: NoticeSpecimen) -> Vec<(&'static str, Notice)> {
    let mut states = vec![("active", specimen.notice())];
    let mut slot = slot_with(&[specimen]);
    if let NoticeKey::Settled { notice, .. } = slot.handle_key(key(KeyCode::Tab), EMPTY) {
        states.push(("after Tab", notice));
    }
    let mut slot = slot_with(&[specimen]);
    states.extend(slot.on_submit().into_iter().map(|n| ("after a prompt", n)));
    states
}

#[test]
fn specimens_snapshot() {
    let mut out = String::new();
    for width in [80_u16, 40, 24] {
        for specimen in NoticeSpecimen::ALL {
            for (state, notice) in every_state(specimen) {
                out.push_str(&format!(
                    "── {} · width {width} · {state}\n",
                    specimen.label()
                ));
                for line in text(&notice.display_lines(width)) {
                    out.push_str(&format!("{line}\n"));
                }
            }
        }
    }
    insta::assert_snapshot!(out);
}

#[test]
fn every_row_keeps_the_rail_and_fits_down_to_24_columns() {
    for width in [80_u16, 56, 40, 32, 24] {
        for specimen in NoticeSpecimen::ALL {
            for (state, notice) in every_state(specimen) {
                for row in text(&notice.display_lines(width)) {
                    assert!(
                        row.starts_with("  ▎ "),
                        "{state} row lost the rail: {row:?}"
                    );
                    assert!(
                        unicode_width::UnicodeWidthStr::width(row.as_str()) <= usize::from(width),
                        "{state} row overflows {width}: {row:?}"
                    );
                }
            }
        }
    }
}

#[test]
fn commands_are_never_split_at_their_hyphens() {
    let mut slot = slot_with(&[NoticeSpecimen::CliUpdate]);
    let settled = text(&slot.on_submit()[0].display_lines(32));

    assert!(
        settled.iter().all(|row| !row.ends_with('-')),
        "{settled:#?}"
    );
    assert!(
        settled
            .iter()
            .any(|row| row.ends_with("nori-ai-cli@latest")),
        "{settled:#?}"
    );
}

#[test]
fn notices_use_no_foreground_or_background_colors() {
    for specimen in NoticeSpecimen::ALL {
        for (state, notice) in every_state(specimen) {
            for span in notice
                .display_lines(80)
                .iter()
                .flat_map(|l| l.spans.clone())
            {
                assert_eq!(
                    (span.style.fg, span.style.bg),
                    (None, None),
                    "{} {state}: {:?}",
                    specimen.label(),
                    span.content
                );
            }
        }
    }
}

#[test]
fn stderr_text_is_shown_as_inert_text() {
    assert_eq!(
        stderr(
            "\u{1b}[1;31mNot signed in.\u{1b}[0m Run claude /login in a shell.\r\n\
             \u{1b}]8;;https://example.invalid\u{7}link\u{1b}]8;;\u{1b}\\\tdone\u{8}\n\
             \u{1b}(Bcharset\u{1b}OP keys \u{1b}P1$r\u{1b}\\dcs\n\
             right\u{202e}to\u{2066}left\u{200b}zero\u{feff}width\n\
             \u{9b}2Jc1"
        ),
        vec![
            "  ▎ Not signed in. Run claude /login in a shell.",
            "  ▎ link    done",
            "  ▎ charset keys dcs",
            "  ▎ righttoleftzerowidth",
            "  ▎ c1",
        ]
    );
}

#[test]
fn progress_rewrites_become_separate_rows() {
    assert_eq!(
        stderr("Indexing 50%\rIndexing 100%\r\n"),
        vec!["  ▎ Indexing 50%", "  ▎ Indexing 100%"]
    );
}

#[test]
fn stderr_trims_blank_rows_and_trailing_spaces() {
    assert_eq!(
        stderr("\n  \nfirst   \n\nsecond\t\n\n"),
        vec!["  ▎ first", "  ▎", "  ▎ second"]
    );
}

#[test]
fn stderr_shows_five_rows_in_full_and_caps_longer_output() {
    let rows = |n: i32| {
        (1..=n)
            .map(|i| format!("line {i}"))
            .collect::<Vec<_>>()
            .join("\n")
    };

    assert_eq!(stderr(&rows(5)).len(), 5);
    assert_eq!(
        stderr(&rows(9)),
        vec![
            "  ▎ line 1",
            "  ▎ line 2",
            "  ▎ line 3",
            "  ▎ line 4",
            "  ▎ +5 more lines",
        ]
    );
}

#[test]
fn a_long_stderr_line_is_cut_before_it_floods_the_transcript() {
    let rows = stderr(&"x".repeat(5_000));

    assert_eq!(
        rows,
        vec![
            format!("  ▎ {}…", "x".repeat(STDERR_LINE_CHARS)),
            format!("  ▎ +{} more characters", 5_000 - STDERR_LINE_CHARS),
        ]
    );
}

#[test]
fn stderr_without_visible_text_makes_no_notice() {
    assert_eq!(
        Notice::from_stderr("Codex adapter", " \n\u{1b}[0m\r\n"),
        None
    );
}

#[test]
fn stderr_notices_cannot_carry_an_action() {
    let notice = Notice::from_stderr("Claude Code adapter", "Signed in as someone")
        .expect("visible output")
        .with_action(NoticeAction::new("x", "sign in", "done"));
    let mut slot = NoticeSlot::default();
    slot.offer(notice);

    assert_eq!(
        slot.handle_key(key(KeyCode::Tab), EMPTY),
        NoticeKey::PassThrough
    );
}

#[test]
fn typing_passes_through_to_the_composer_while_a_notice_is_active() {
    let mut slot = slot_with(&[NoticeSpecimen::CliUpdate]);

    for code in [
        KeyCode::Char('u'),
        KeyCode::Char(' '),
        KeyCode::Backspace,
        KeyCode::Left,
        KeyCode::Enter,
    ] {
        assert_eq!(slot.handle_key(key(code), EMPTY), NoticeKey::PassThrough);
    }
    assert_eq!(slot.active(), Some(&NoticeSpecimen::CliUpdate.notice()));
}

#[test]
fn tab_and_esc_belong_to_the_prompt_unless_it_is_empty_and_idle() {
    let mut slot = slot_with(&[NoticeSpecimen::CliUpdate]);
    let shift_tab = KeyEvent::new(KeyCode::BackTab, KeyModifiers::SHIFT);
    let ctrl_esc = KeyEvent::new(KeyCode::Esc, KeyModifiers::CONTROL);
    let mut release = key(KeyCode::Tab);
    release.kind = KeyEventKind::Release;

    for (key, prompt) in [
        (key(KeyCode::Tab), DRAFT),
        (key(KeyCode::Esc), DRAFT),
        (key(KeyCode::Tab), BUSY),
        (key(KeyCode::Esc), BUSY),
        (shift_tab, EMPTY),
        (ctrl_esc, EMPTY),
        (release, EMPTY),
    ] {
        assert_eq!(
            slot.handle_key(key, prompt),
            NoticeKey::PassThrough,
            "{key:?}"
        );
    }
    assert_eq!(slot.active(), Some(&NoticeSpecimen::CliUpdate.notice()));
}

#[test]
fn tab_passes_through_when_the_notice_has_no_action() {
    let mut slot = slot_with(&[NoticeSpecimen::Tip]);

    assert_eq!(
        slot.handle_key(key(KeyCode::Tab), EMPTY),
        NoticeKey::PassThrough
    );
    assert_eq!(slot.active(), Some(&NoticeSpecimen::Tip.notice()));
}

#[test]
fn keys_pass_through_when_no_notice_is_active() {
    let mut slot = NoticeSlot::default();

    assert_eq!(
        slot.handle_key(key(KeyCode::Esc), EMPTY),
        NoticeKey::PassThrough
    );
    assert_eq!(
        slot.handle_key(key(KeyCode::Tab), EMPTY),
        NoticeKey::PassThrough
    );
}

#[test]
fn sending_a_prompt_settles_every_waiting_notice_in_order_once() {
    let mut slot = slot_with(&[NoticeSpecimen::CliUpdate, NoticeSpecimen::Tip]);

    let settled = slot.on_submit();

    assert_eq!(
        settled.iter().map(Notice::source).collect::<Vec<_>>(),
        vec!["nori CLI", "nori"]
    );
    assert_eq!(slot.active(), None);
    assert_eq!(slot.on_submit(), Vec::new());
}

#[test]
fn tab_on_an_empty_prompt_invokes_the_action_once() {
    let mut slot = slot_with(&[NoticeSpecimen::CliUpdate]);

    let NoticeKey::Settled { invoked, .. } = slot.handle_key(key(KeyCode::Tab), EMPTY) else {
        panic!("Tab should invoke the action");
    };

    assert_eq!(invoked, Some("cli.update"));
    assert_eq!(slot.active(), None);
    assert_eq!(
        slot.handle_key(key(KeyCode::Tab), EMPTY),
        NoticeKey::PassThrough
    );
}

#[test]
fn esc_dismisses_like_a_sent_prompt_and_shows_the_next_notice() {
    let mut slot = slot_with(&[NoticeSpecimen::CliUpdate, NoticeSpecimen::Tip]);
    let mut sent = slot_with(&[NoticeSpecimen::CliUpdate]);

    let dismissed = slot.handle_key(key(KeyCode::Esc), EMPTY);

    assert_eq!(
        dismissed,
        NoticeKey::Settled {
            invoked: None,
            notice: sent.on_submit().remove(0),
        }
    );
    assert_eq!(slot.active(), Some(&NoticeSpecimen::Tip.notice()));
}
