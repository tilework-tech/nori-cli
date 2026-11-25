use super::*;
use pretty_assertions::assert_eq;

#[test]
fn scroll_matches_the_full_wrapped_history_at_each_width() {
    let mut state = AttachState::default();
    let history = &mut state.conversations.entry("test".into()).or_default().parts;
    history.extend([
        Part::Message {
            stream: MessageStream::User,
            id: None,
            text: "Review the items in the inventory".into(),
        },
        Part::Notice("The synthetic inventory has been updated".into()),
        Part::Message {
            stream: MessageStream::Reasoning,
            id: None,
            text: "Hidden reasoning".into(),
        },
        Part::Ended(Some("End of inventory".into())),
    ]);
    state.thoughts_in_history = false;
    for width in [20, 64] {
        state.scroll = 0;
        let reference_area = Rect::new(0, 0, width, 80);
        let mut reference = Buffer::empty(reference_area);
        render_conversation(&state, "test", reference_area, &mut reference);
        let last_content = reference
            .content
            .chunks(usize::from(width))
            .rposition(|row| row.iter().any(|cell| cell.symbol() != " "))
            .unwrap();
        let lines = last_content + 2; // Last part also has a blank separator.
        for height in [2, 5, 18] {
            let area = Rect::new(0, 0, width, height);
            for scroll in [0, 1, 4, lines, usize::MAX] {
                state.scroll = scroll;
                let mut actual = Buffer::empty(area);
                render_conversation(&state, "test", area, &mut actual);
                let top = lines
                    .saturating_sub(usize::from(height))
                    .saturating_sub(scroll);
                let start = top * usize::from(width);
                assert_eq!(
                    actual.content,
                    reference.content[start..start + actual.content.len()]
                );
            }
        }
    }
}

#[test]
fn tail_window_leaves_older_parts_unformatted() {
    let sections = (0..10_000).map(|index| {
        assert!(index >= 9_998, "formatting exceeded the visible history");
        vec![Line::from(format!("section {index}")), Line::default()]
    });
    let visible = history_window(sections, 2, 1);
    assert_eq!(visible, vec![Line::default(), Line::from("section 9999")]);
}

#[test]
fn ready_prompt_preserves_blank_transcript_boundaries() {
    let mut state = AttachState::default();
    state.conversations.entry("test".into()).or_default();
    let area = Rect::new(0, 0, 60, 10);
    let ready = |state: &AttachState| {
        let mut buffer = Buffer::empty(area);
        render_conversation(state, "test", area, &mut buffer);
        buffer.content.chunks(usize::from(area.width)).any(|row| {
            row.iter()
                .map(|cell| cell.symbol())
                .collect::<String>()
                .contains("Ready. Send a prompt")
        })
    };
    assert!(ready(&state));
    state
        .conversations
        .get_mut("test")
        .unwrap()
        .parts
        .push(Part::Ended(Some(String::new())));
    assert!(ready(&state));
    state
        .conversations
        .get_mut("test")
        .unwrap()
        .parts
        .push(Part::Notice(String::new()));
    assert!(!ready(&state));
}
