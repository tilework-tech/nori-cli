use super::*;

#[test]
fn wrapped_navigation_with_newlines_and_spaces() {
    // Include spaces and an explicit newline to exercise boundaries
    let mut t = ta_with("word1  word2\nword3");
    // Width 6 will wrap "word1  " and then "word2" before the newline
    let _ = t.desired_height(6);

    // Put cursor on the second wrapped line before the newline, at column 1 of "word2"
    let start_word2 = t.text().find("word2").unwrap();
    t.set_cursor(start_word2 + 1);

    // Up should go to first wrapped line, column 1 -> index 1
    t.move_cursor_up();
    assert_eq!(t.cursor(), 1);

    // Down should return to the same visual column on "word2"
    t.move_cursor_down();
    assert_eq!(t.cursor(), start_word2 + 1);

    // Down again should cross the logical newline to the next visual line ("word3"), clamped to its length if needed
    t.move_cursor_down();
    let start_word3 = t.text().find("word3").unwrap();
    assert!(t.cursor() >= start_word3 && t.cursor() <= start_word3 + "word3".len());
}

#[test]
fn wrapped_navigation_with_wide_graphemes() {
    // Four thumbs up, each of display width 2, with width 3 to force wrapping inside grapheme boundaries
    let mut t = ta_with("👍👍👍👍");
    let _ = t.desired_height(3);

    // Put cursor after the second emoji (which should be on first wrapped line)
    t.set_cursor("👍👍".len());

    // Move down should go to the start of the next wrapped line (same column preserved but clamped)
    t.move_cursor_down();
    // We expect to land somewhere within the third emoji or at the start of it
    let pos_after_down = t.cursor();
    assert!(pos_after_down >= "👍👍".len());

    // Moving up should take us back to the original position
    t.move_cursor_up();
    assert_eq!(t.cursor(), "👍👍".len());
}

#[test]
fn fuzz_textarea_randomized() {
    // Deterministic seed for reproducibility
    // Seed the RNG based on the current day in Pacific Time (PST/PDT). This
    // keeps the fuzz test deterministic within a day while still varying
    // day-to-day to improve coverage.
    let pst_today_seed: u64 = (chrono::Utc::now() - chrono::Duration::hours(8))
        .date_naive()
        .and_hms_opt(0, 0, 0)
        .unwrap()
        .and_utc()
        .timestamp() as u64;
    let mut rng = rand::rngs::StdRng::seed_from_u64(pst_today_seed);

    for _case in 0..500 {
        let mut ta = TextArea::new();
        let mut state = TextAreaState::default();
        // Track element payloads we insert. Payloads use characters '[' and ']' which
        // are not produced by rand_grapheme(), avoiding accidental collisions.
        let mut elem_texts: Vec<String> = Vec::new();
        let mut next_elem_id: usize = 0;
        // Start with a random base string
        let base_len = rng.random_range(0..30);
        let mut base = String::new();
        for _ in 0..base_len {
            base.push_str(&rand_grapheme(&mut rng));
        }
        ta.set_text(&base);
        // Choose a valid char boundary for initial cursor
        let mut boundaries: Vec<usize> = vec![0];
        boundaries.extend(ta.text().char_indices().map(|(i, _)| i).skip(1));
        boundaries.push(ta.text().len());
        let init = boundaries[rng.random_range(0..boundaries.len())];
        ta.set_cursor(init);

        let mut width: u16 = rng.random_range(1..=12);
        let mut height: u16 = rng.random_range(1..=4);

        for _step in 0..60 {
            // Mostly stable width/height, occasionally change
            if rng.random_bool(0.1) {
                width = rng.random_range(1..=12);
            }
            if rng.random_bool(0.1) {
                height = rng.random_range(1..=4);
            }

            // Pick an operation
            match rng.random_range(0..18) {
                0 => {
                    // insert small random string at cursor
                    let len = rng.random_range(0..6);
                    let mut s = String::new();
                    for _ in 0..len {
                        s.push_str(&rand_grapheme(&mut rng));
                    }
                    ta.insert_str(&s);
                }
                1 => {
                    // replace_range with small random slice
                    let mut b: Vec<usize> = vec![0];
                    b.extend(ta.text().char_indices().map(|(i, _)| i).skip(1));
                    b.push(ta.text().len());
                    let i1 = rng.random_range(0..b.len());
                    let i2 = rng.random_range(0..b.len());
                    let (start, end) = if b[i1] <= b[i2] {
                        (b[i1], b[i2])
                    } else {
                        (b[i2], b[i1])
                    };
                    let insert_len = rng.random_range(0..=4);
                    let mut s = String::new();
                    for _ in 0..insert_len {
                        s.push_str(&rand_grapheme(&mut rng));
                    }
                    let before = ta.text().len();
                    // If the chosen range intersects an element, replace_range will expand to
                    // element boundaries, so the naive size delta assertion does not hold.
                    let intersects_element = elem_texts.iter().any(|payload| {
                        if let Some(pstart) = ta.text().find(payload) {
                            let pend = pstart + payload.len();
                            pstart < end && pend > start
                        } else {
                            false
                        }
                    });
                    ta.replace_range(start..end, &s);
                    if !intersects_element {
                        let after = ta.text().len();
                        assert_eq!(
                            after as isize,
                            before as isize + (s.len() as isize) - ((end - start) as isize)
                        );
                    }
                }
                2 => ta.delete_backward(rng.random_range(0..=3)),
                3 => ta.delete_forward(rng.random_range(0..=3)),
                4 => ta.delete_backward_word(),
                5 => ta.kill_to_beginning_of_line(),
                6 => ta.kill_to_end_of_line(),
                7 => ta.move_cursor_left(),
                8 => ta.move_cursor_right(),
                9 => ta.move_cursor_up(),
                10 => ta.move_cursor_down(),
                11 => ta.move_cursor_to_beginning_of_line(true),
                12 => ta.move_cursor_to_end_of_line(true),
                13 => {
                    // Insert an element with a unique sentinel payload
                    let payload =
                        format!("[[EL#{}:{}]]", next_elem_id, rng.random_range(1000..9999));
                    next_elem_id += 1;
                    ta.insert_element(&payload);
                    elem_texts.push(payload);
                }
                14 => {
                    // Try inserting inside an existing element (should clamp to boundary)
                    if let Some(payload) = elem_texts.choose(&mut rng).cloned()
                        && let Some(start) = ta.text().find(&payload)
                    {
                        let end = start + payload.len();
                        if end - start > 2 {
                            let pos = rng.random_range(start + 1..end - 1);
                            let ins = rand_grapheme(&mut rng);
                            ta.insert_str_at(pos, &ins);
                        }
                    }
                }
                15 => {
                    // Replace a range that intersects an element -> whole element should be replaced
                    if let Some(payload) = elem_texts.choose(&mut rng).cloned()
                        && let Some(start) = ta.text().find(&payload)
                    {
                        let end = start + payload.len();
                        // Create an intersecting range [start-δ, end-δ2)
                        let mut s = start.saturating_sub(rng.random_range(0..=2));
                        let mut e = (end + rng.random_range(0..=2)).min(ta.text().len());
                        // Align to char boundaries to satisfy String::replace_range contract
                        let txt = ta.text();
                        while s > 0 && !txt.is_char_boundary(s) {
                            s -= 1;
                        }
                        while e < txt.len() && !txt.is_char_boundary(e) {
                            e += 1;
                        }
                        if s < e {
                            // Small replacement text
                            let mut srep = String::new();
                            for _ in 0..rng.random_range(0..=2) {
                                srep.push_str(&rand_grapheme(&mut rng));
                            }
                            ta.replace_range(s..e, &srep);
                        }
                    }
                }
                16 => {
                    // Try setting the cursor to a position inside an element; it should clamp out
                    if let Some(payload) = elem_texts.choose(&mut rng).cloned()
                        && let Some(start) = ta.text().find(&payload)
                    {
                        let end = start + payload.len();
                        if end - start > 2 {
                            let pos = rng.random_range(start + 1..end - 1);
                            ta.set_cursor(pos);
                        }
                    }
                }
                _ => {
                    // Jump to word boundaries
                    if rng.random_bool(0.5) {
                        let p = ta.beginning_of_previous_word();
                        ta.set_cursor(p);
                    } else {
                        let p = ta.end_of_next_word();
                        ta.set_cursor(p);
                    }
                }
            }

            // Sanity invariants
            assert!(ta.cursor() <= ta.text().len());

            // Element invariants
            for payload in &elem_texts {
                if let Some(start) = ta.text().find(payload) {
                    let end = start + payload.len();
                    // 1) Text inside elements matches the initially set payload
                    assert_eq!(&ta.text()[start..end], payload);
                    // 2) Cursor is never strictly inside an element
                    let c = ta.cursor();
                    assert!(
                        c <= start || c >= end,
                        "cursor inside element: {start}..{end} at {c}"
                    );
                }
            }

            // Render and compute cursor positions; ensure they are in-bounds and do not panic
            let area = Rect::new(0, 0, width, height);
            // Stateless render into an area tall enough for all wrapped lines
            let total_lines = ta.desired_height(width);
            let full_area = Rect::new(0, 0, width, total_lines.max(1));
            let mut buf = Buffer::empty(full_area);
            ratatui::widgets::WidgetRef::render_ref(&(&ta), full_area, &mut buf);

            // cursor_pos: x must be within width when present
            let _ = ta.cursor_pos(area);

            // cursor_pos_with_state: always within viewport rows
            let (_x, _y) = ta
                .cursor_pos_with_state(area, state)
                .unwrap_or((area.x, area.y));

            // Stateful render should not panic, and updates scroll
            let mut sbuf = Buffer::empty(area);
            ratatui::widgets::StatefulWidgetRef::render_ref(&(&ta), area, &mut sbuf, &mut state);

            // After wrapping, desired height equals the number of lines we would render without scroll
            let total_lines = total_lines as usize;
            // state.scroll must not exceed total_lines when content fits within area height
            if (height as usize) >= total_lines {
                assert_eq!(state.scroll, 0);
            }
        }
    }
}

// ===== Configurable hotkey tests =====

#[test]
fn test_configurable_ctrl_a_moves_to_line_start() {
    use nori_acp::config::HotkeyConfig;
    let mut t = ta_with("hello");
    t.set_hotkey_config(HotkeyConfig::default());
    // Cursor is at end (5) after insert
    pretty_assertions::assert_eq!(t.cursor(), 5);
    // Ctrl+A should move to beginning of line
    t.input(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::CONTROL));
    pretty_assertions::assert_eq!(t.cursor(), 0);
}

#[test]
fn test_configurable_ctrl_e_moves_to_line_end() {
    use nori_acp::config::HotkeyConfig;
    let mut t = ta_with("hello");
    t.set_hotkey_config(HotkeyConfig::default());
    t.set_cursor(0);
    // Ctrl+E should move to end of line
    t.input(KeyEvent::new(KeyCode::Char('e'), KeyModifiers::CONTROL));
    pretty_assertions::assert_eq!(t.cursor(), 5);
}

#[test]
fn test_rebound_move_backward_char() {
    use nori_acp::config::HotkeyBinding;
    use nori_acp::config::HotkeyConfig;
    let config = HotkeyConfig {
        move_backward_char: HotkeyBinding::from_str("alt+x"),
        ..HotkeyConfig::default()
    };
    let mut t = ta_with("abcd");
    t.set_hotkey_config(config);
    // Cursor at end (4)
    pretty_assertions::assert_eq!(t.cursor(), 4);

    // Alt+X (the rebound key) should move cursor left
    t.input(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::ALT));
    pretty_assertions::assert_eq!(t.cursor(), 3);

    // Ctrl+B should no longer move cursor (it's no longer bound to move backward)
    t.input(KeyEvent::new(KeyCode::Char('b'), KeyModifiers::CONTROL));
    pretty_assertions::assert_eq!(t.cursor(), 3);
}

#[test]
fn test_unbound_editing_action_falls_through() {
    use nori_acp::config::HotkeyBinding;
    use nori_acp::config::HotkeyConfig;
    let config = HotkeyConfig {
        kill_to_end_of_line: HotkeyBinding::none(),
        ..HotkeyConfig::default()
    };
    let mut t = ta_with("hello");
    t.set_hotkey_config(config);
    t.set_cursor(2);
    // Ctrl+K should not kill because the action is unbound
    t.input(KeyEvent::new(KeyCode::Char('k'), KeyModifiers::CONTROL));
    pretty_assertions::assert_eq!(t.text(), "hello");
    pretty_assertions::assert_eq!(t.cursor(), 2);
}

#[test]
fn test_configurable_kill_and_yank() {
    use nori_acp::config::HotkeyConfig;
    let mut t = ta_with("hello world");
    t.set_hotkey_config(HotkeyConfig::default());
    t.set_cursor(5);
    // Ctrl+K should kill to end of line
    t.input(KeyEvent::new(KeyCode::Char('k'), KeyModifiers::CONTROL));
    pretty_assertions::assert_eq!(t.text(), "hello");
    pretty_assertions::assert_eq!(t.cursor(), 5);
    // Ctrl+Y should yank back
    t.input(KeyEvent::new(KeyCode::Char('y'), KeyModifiers::CONTROL));
    pretty_assertions::assert_eq!(t.text(), "hello world");
    pretty_assertions::assert_eq!(t.cursor(), 11);
}

#[test]
fn test_configurable_word_movement() {
    use nori_acp::config::HotkeyConfig;
    let mut t = ta_with("foo bar baz");
    t.set_hotkey_config(HotkeyConfig::default());
    t.set_cursor(0);
    // Alt+F should move forward past "foo"
    t.input(KeyEvent::new(KeyCode::Char('f'), KeyModifiers::ALT));
    pretty_assertions::assert_eq!(t.cursor(), 3);
    // Alt+B should move backward to start of "foo"
    t.input(KeyEvent::new(KeyCode::Char('b'), KeyModifiers::ALT));
    pretty_assertions::assert_eq!(t.cursor(), 0);
}
