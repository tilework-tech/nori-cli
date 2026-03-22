use super::*;
use pretty_assertions::assert_eq;

#[test]
fn attach_image_and_submit_includes_image_paths() {
    let (tx, _rx) = unbounded_channel::<AppEvent>();
    let sender = AppEventSender::new(tx);
    let mut composer = ChatComposer::new(
        true,
        sender,
        false,
        "Ask Nori to do anything".to_string(),
        false,
    );
    let path = PathBuf::from("/tmp/image1.png");
    composer.attach_image(path.clone(), 32, 16, "PNG");
    composer.handle_paste(" hi".into());
    let (result, _) = composer.handle_key_event(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    match result {
        InputResult::Submitted(text) => assert_eq!(text, "[image1.png 32x16] hi"),
        _ => panic!("expected Submitted"),
    }
    let imgs = composer.take_recent_submission_images();
    assert_eq!(vec![path], imgs);
}

#[test]
fn attach_image_without_text_submits_empty_text_and_images() {
    let (tx, _rx) = unbounded_channel::<AppEvent>();
    let sender = AppEventSender::new(tx);
    let mut composer = ChatComposer::new(
        true,
        sender,
        false,
        "Ask Nori to do anything".to_string(),
        false,
    );
    let path = PathBuf::from("/tmp/image2.png");
    composer.attach_image(path.clone(), 10, 5, "PNG");
    let (result, _) = composer.handle_key_event(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    match result {
        InputResult::Submitted(text) => assert_eq!(text, "[image2.png 10x5]"),
        _ => panic!("expected Submitted"),
    }
    let imgs = composer.take_recent_submission_images();
    assert_eq!(imgs.len(), 1);
    assert_eq!(imgs[0], path);
    assert!(composer.attached_images.is_empty());
}

#[test]
fn image_placeholder_backspace_behaves_like_text_placeholder() {
    let (tx, _rx) = unbounded_channel::<AppEvent>();
    let sender = AppEventSender::new(tx);
    let mut composer = ChatComposer::new(
        true,
        sender,
        false,
        "Ask Nori to do anything".to_string(),
        false,
    );
    let path = PathBuf::from("/tmp/image3.png");
    composer.attach_image(path.clone(), 20, 10, "PNG");
    let placeholder = composer.attached_images[0].placeholder.clone();

    // Case 1: backspace at end
    composer.textarea.move_cursor_to_end_of_line(false);
    composer.handle_key_event(KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE));
    assert!(!composer.textarea.text().contains(&placeholder));
    assert!(composer.attached_images.is_empty());

    // Re-add and test backspace in middle: should break the placeholder string
    // and drop the image mapping (same as text placeholder behavior).
    composer.attach_image(path, 20, 10, "PNG");
    let placeholder2 = composer.attached_images[0].placeholder.clone();
    // Move cursor to roughly middle of placeholder
    if let Some(start_pos) = composer.textarea.text().find(&placeholder2) {
        let mid_pos = start_pos + (placeholder2.len() / 2);
        composer.textarea.set_cursor(mid_pos);
        composer.handle_key_event(KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE));
        assert!(!composer.textarea.text().contains(&placeholder2));
        assert!(composer.attached_images.is_empty());
    } else {
        panic!("Placeholder not found in textarea");
    }
}

#[test]
fn backspace_with_multibyte_text_before_placeholder_does_not_panic() {
    use crossterm::event::KeyCode;
    use crossterm::event::KeyEvent;
    use crossterm::event::KeyModifiers;

    let (tx, _rx) = unbounded_channel::<AppEvent>();
    let sender = AppEventSender::new(tx);
    let mut composer = ChatComposer::new(
        true,
        sender,
        false,
        "Ask Nori to do anything".to_string(),
        false,
    );

    // Insert an image placeholder at the start
    let path = PathBuf::from("/tmp/image_multibyte.png");
    composer.attach_image(path, 10, 5, "PNG");
    // Add multibyte text after the placeholder
    composer.textarea.insert_str("日本語");

    // Cursor is at end; pressing backspace should delete the last character
    // without panicking and leave the placeholder intact.
    composer.handle_key_event(KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE));

    assert_eq!(composer.attached_images.len(), 1);
    assert!(
        composer
            .textarea
            .text()
            .starts_with("[image_multibyte.png 10x5]")
    );
}

#[test]
fn deleting_one_of_duplicate_image_placeholders_removes_matching_entry() {
    let (tx, _rx) = unbounded_channel::<AppEvent>();
    let sender = AppEventSender::new(tx);
    let mut composer = ChatComposer::new(
        true,
        sender,
        false,
        "Ask Nori to do anything".to_string(),
        false,
    );

    let path1 = PathBuf::from("/tmp/image_dup1.png");
    let path2 = PathBuf::from("/tmp/image_dup2.png");

    composer.attach_image(path1, 10, 5, "PNG");
    // separate placeholders with a space for clarity
    composer.handle_paste(" ".into());
    composer.attach_image(path2.clone(), 10, 5, "PNG");

    let placeholder1 = composer.attached_images[0].placeholder.clone();
    let placeholder2 = composer.attached_images[1].placeholder.clone();
    let text = composer.textarea.text().to_string();
    let start1 = text.find(&placeholder1).expect("first placeholder present");
    let end1 = start1 + placeholder1.len();
    composer.textarea.set_cursor(end1);

    // Backspace should delete the first placeholder and its mapping.
    composer.handle_key_event(KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE));

    let new_text = composer.textarea.text().to_string();
    assert_eq!(
        0,
        new_text.matches(&placeholder1).count(),
        "first placeholder removed"
    );
    assert_eq!(
        1,
        new_text.matches(&placeholder2).count(),
        "second placeholder remains"
    );
    assert_eq!(
        vec![AttachedImage {
            path: path2,
            placeholder: "[image_dup2.png 10x5]".to_string()
        }],
        composer.attached_images,
        "one image mapping remains"
    );
}

#[test]
fn pasting_filepath_attaches_image() {
    let tmp = tempdir().expect("create TempDir");
    let tmp_path: PathBuf = tmp.path().join("nori_tui_test_paste_image.png");
    let img: ImageBuffer<Rgba<u8>, Vec<u8>> =
        ImageBuffer::from_fn(3, 2, |_x, _y| Rgba([1, 2, 3, 255]));
    img.save(&tmp_path).expect("failed to write temp png");

    let (tx, _rx) = unbounded_channel::<AppEvent>();
    let sender = AppEventSender::new(tx);
    let mut composer = ChatComposer::new(
        true,
        sender,
        false,
        "Ask Nori to do anything".to_string(),
        false,
    );

    let needs_redraw = composer.handle_paste(tmp_path.to_string_lossy().to_string());
    assert!(needs_redraw);
    assert!(
        composer
            .textarea
            .text()
            .starts_with("[nori_tui_test_paste_image.png 3x2] ")
    );

    let imgs = composer.take_recent_submission_images();
    assert_eq!(imgs, vec![tmp_path]);
}

#[test]
fn selecting_custom_prompt_without_args_submits_content() {
    let prompt_text = "Hello from saved prompt";

    let (tx, _rx) = unbounded_channel::<AppEvent>();
    let sender = AppEventSender::new(tx);
    let mut composer = ChatComposer::new(
        true,
        sender,
        false,
        "Ask Nori to do anything".to_string(),
        false,
    );

    // Inject prompts as if received via event.
    composer.set_custom_prompts(vec![CustomPrompt {
        name: "my-prompt".to_string(),
        path: "/tmp/my-prompt.md".to_string().into(),
        content: prompt_text.to_string(),
        description: None,
        argument_hint: None,
        kind: Default::default(),
    }]);

    type_chars_humanlike(
        &mut composer,
        &[
            '/', 'p', 'r', 'o', 'm', 'p', 't', 's', ':', 'm', 'y', '-', 'p', 'r', 'o', 'm', 'p',
            't',
        ],
    );

    let (result, _needs_redraw) =
        composer.handle_key_event(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

    assert_eq!(InputResult::Submitted(prompt_text.to_string()), result);
    assert!(composer.textarea.is_empty());
}

#[test]
fn custom_prompt_submission_expands_arguments() {
    let (tx, _rx) = unbounded_channel::<AppEvent>();
    let sender = AppEventSender::new(tx);
    let mut composer = ChatComposer::new(
        true,
        sender,
        false,
        "Ask Nori to do anything".to_string(),
        false,
    );

    composer.set_custom_prompts(vec![CustomPrompt {
        name: "my-prompt".to_string(),
        path: "/tmp/my-prompt.md".to_string().into(),
        content: "Review $USER changes on $BRANCH".to_string(),
        description: None,
        argument_hint: None,
        kind: Default::default(),
    }]);

    composer
        .textarea
        .set_text("/prompts:my-prompt USER=Alice BRANCH=main");

    let (result, _needs_redraw) =
        composer.handle_key_event(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

    assert_eq!(
        InputResult::Submitted("Review Alice changes on main".to_string()),
        result
    );
    assert!(composer.textarea.is_empty());
}

#[test]
fn custom_prompt_submission_accepts_quoted_values() {
    let (tx, _rx) = unbounded_channel::<AppEvent>();
    let sender = AppEventSender::new(tx);
    let mut composer = ChatComposer::new(
        true,
        sender,
        false,
        "Ask Nori to do anything".to_string(),
        false,
    );

    composer.set_custom_prompts(vec![CustomPrompt {
        name: "my-prompt".to_string(),
        path: "/tmp/my-prompt.md".to_string().into(),
        content: "Pair $USER with $BRANCH".to_string(),
        description: None,
        argument_hint: None,
        kind: Default::default(),
    }]);

    composer
        .textarea
        .set_text("/prompts:my-prompt USER=\"Alice Smith\" BRANCH=dev-main");

    let (result, _needs_redraw) =
        composer.handle_key_event(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

    assert_eq!(
        InputResult::Submitted("Pair Alice Smith with dev-main".to_string()),
        result
    );
    assert!(composer.textarea.is_empty());
}

#[test]
fn custom_prompt_with_large_paste_expands_correctly() {
    use crossterm::event::KeyCode;
    use crossterm::event::KeyEvent;
    use crossterm::event::KeyModifiers;

    let (tx, _rx) = unbounded_channel::<AppEvent>();
    let sender = AppEventSender::new(tx);
    let mut composer = ChatComposer::new(
        true,
        sender,
        false,
        "Ask Nori to do anything".to_string(),
        false,
    );

    // Create a custom prompt with positional args (no named args like $USER)
    composer.set_custom_prompts(vec![CustomPrompt {
        name: "code-review".to_string(),
        path: "/tmp/code-review.md".to_string().into(),
        content: "Please review the following code:\n\n$1".to_string(),
        description: None,
        argument_hint: None,
        kind: Default::default(),
    }]);

    // Type the slash command
    let command_text = "/prompts:code-review ";
    composer.textarea.set_text(command_text);
    composer.textarea.set_cursor(command_text.len());

    // Paste large content (>3000 chars) to trigger placeholder
    let large_content = "x".repeat(LARGE_PASTE_CHAR_THRESHOLD + 3000);
    composer.handle_paste(large_content.clone());

    // Verify placeholder was created
    let placeholder = format!("[Pasted Content {} chars]", large_content.chars().count());
    assert_eq!(
        composer.textarea.text(),
        format!("/prompts:code-review {}", placeholder)
    );
    assert_eq!(composer.pending_pastes.len(), 1);
    assert_eq!(composer.pending_pastes[0].0, placeholder);
    assert_eq!(composer.pending_pastes[0].1, large_content);

    // Submit by pressing Enter
    let (result, _needs_redraw) =
        composer.handle_key_event(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

    // Verify the custom prompt was expanded with the large content as positional arg
    match result {
        InputResult::Submitted(text) => {
            // The prompt should be expanded, with the large content replacing $1
            assert_eq!(
                text,
                format!("Please review the following code:\n\n{}", large_content),
                "Expected prompt expansion with large content as $1"
            );
        }
        _ => panic!("expected Submitted, got: {result:?}"),
    }
    assert!(composer.textarea.is_empty());
    assert!(composer.pending_pastes.is_empty());
}

#[test]
fn slash_path_input_submits_without_command_error() {
    use crossterm::event::KeyCode;
    use crossterm::event::KeyEvent;
    use crossterm::event::KeyModifiers;

    let (tx, mut rx) = unbounded_channel::<AppEvent>();
    let sender = AppEventSender::new(tx);
    let mut composer = ChatComposer::new(
        true,
        sender,
        false,
        "Ask Nori to do anything".to_string(),
        false,
    );

    composer
        .textarea
        .set_text("/Users/example/project/src/main.rs");

    let (result, _needs_redraw) =
        composer.handle_key_event(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

    if let InputResult::Submitted(text) = result {
        assert_eq!(text, "/Users/example/project/src/main.rs");
    } else {
        panic!("expected Submitted");
    }
    assert!(composer.textarea.is_empty());
    match rx.try_recv() {
        Ok(event) => panic!("unexpected event: {event:?}"),
        Err(tokio::sync::mpsc::error::TryRecvError::Empty) => {}
        Err(err) => panic!("unexpected channel state: {err:?}"),
    }
}

#[test]
fn slash_with_leading_space_submits_as_text() {
    use crossterm::event::KeyCode;
    use crossterm::event::KeyEvent;
    use crossterm::event::KeyModifiers;

    let (tx, mut rx) = unbounded_channel::<AppEvent>();
    let sender = AppEventSender::new(tx);
    let mut composer = ChatComposer::new(
        true,
        sender,
        false,
        "Ask Nori to do anything".to_string(),
        false,
    );

    composer.textarea.set_text(" /this-looks-like-a-command");

    let (result, _needs_redraw) =
        composer.handle_key_event(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

    if let InputResult::Submitted(text) = result {
        assert_eq!(text, "/this-looks-like-a-command");
    } else {
        panic!("expected Submitted");
    }
    assert!(composer.textarea.is_empty());
    match rx.try_recv() {
        Ok(event) => panic!("unexpected event: {event:?}"),
        Err(tokio::sync::mpsc::error::TryRecvError::Empty) => {}
        Err(err) => panic!("unexpected channel state: {err:?}"),
    }
}

#[test]
fn custom_prompt_invalid_args_reports_error() {
    let (tx, mut rx) = unbounded_channel::<AppEvent>();
    let sender = AppEventSender::new(tx);
    let mut composer = ChatComposer::new(
        true,
        sender,
        false,
        "Ask Nori to do anything".to_string(),
        false,
    );

    composer.set_custom_prompts(vec![CustomPrompt {
        name: "my-prompt".to_string(),
        path: "/tmp/my-prompt.md".to_string().into(),
        content: "Review $USER changes".to_string(),
        description: None,
        argument_hint: None,
        kind: Default::default(),
    }]);

    composer
        .textarea
        .set_text("/prompts:my-prompt USER=Alice stray");

    let (result, _needs_redraw) =
        composer.handle_key_event(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

    assert_eq!(InputResult::None, result);
    assert_eq!(
        "/prompts:my-prompt USER=Alice stray",
        composer.textarea.text()
    );

    let mut found_error = false;
    while let Ok(event) = rx.try_recv() {
        if let AppEvent::InsertHistoryCell(cell) = event {
            let message = cell
                .display_lines(80)
                .into_iter()
                .map(|line| line.to_string())
                .collect::<Vec<_>>()
                .join("\n");
            assert!(message.contains("expected key=value"));
            found_error = true;
            break;
        }
    }
    assert!(found_error, "expected error history cell to be sent");
}
