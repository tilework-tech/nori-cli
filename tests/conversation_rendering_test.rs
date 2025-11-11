use nori_cli::conversation::{ConversationEvent, render_event};

// Rendering tests - testing the display logic, not parsing

#[test]
fn test_render_assistant_message_as_plain_text() {
    let event = ConversationEvent::AssistantMessage {
        text: "Test content".to_string(),
    };
    let line = render_event(&event);

    let line_text = format!("{:?}", line);
    assert!(line_text.contains("Test content"));
}

#[test]
fn test_render_system_event_with_prefix() {
    let event = ConversationEvent::SystemEvent {
        subtype: "init".to_string(),
        details: Some("session started".to_string()),
    };
    let line = render_event(&event);

    let line_text = format!("{:?}", line);
    assert!(line_text.contains("[system]"));
    assert!(line_text.contains("init"));
}

#[test]
fn test_render_result_summary() {
    let event = ConversationEvent::ResultSummary {
        success: true,
        details: "Completed".to_string(),
    };
    let line = render_event(&event);

    let line_text = format!("{:?}", line);
    assert!(line_text.contains("[done]"));
    assert!(line_text.contains("Completed"));
}

#[test]
fn test_render_stderr_output() {
    let event = ConversationEvent::StderrOutput {
        line: "Error message".to_string(),
    };
    let line = render_event(&event);

    let line_text = format!("{:?}", line);
    assert!(line_text.contains("Error message"));
}

#[test]
fn test_render_multiline_assistant_message() {
    let event = ConversationEvent::AssistantMessage {
        text: "Line 1\nLine 2\nLine 3".to_string(),
    };
    let line = render_event(&event);

    let line_text = format!("{:?}", line);
    assert!(line_text.contains("Line 1"));
}

#[test]
fn test_render_unknown_event() {
    let event = ConversationEvent::UnknownEvent {
        raw: "some unknown data".to_string(),
    };
    let line = render_event(&event);

    let line_text = format!("{:?}", line);
    assert!(line_text.contains("[unknown]"));
    assert!(line_text.contains("some unknown data"));
}

#[test]
fn test_render_user_message() {
    let event = ConversationEvent::UserMessage {
        text: "What is the weather?".to_string(),
    };
    let line = render_event(&event);

    let line_text = format!("{:?}", line);
    assert!(line_text.contains("[user]"));
    assert!(line_text.contains("What is the weather?"));
}

#[test]
fn test_render_status_message() {
    let event = ConversationEvent::StatusMessage {
        text: "Debug logs are now enabled".to_string(),
    };
    let line = render_event(&event);

    let line_text = format!("{:?}", line);
    assert!(line_text.contains("[status]"));
    assert!(line_text.contains("Debug logs are now enabled"));
}
