use nori_cli::conversation::{parse_jsonl_event, render_event, ConversationEvent};

#[test]
fn test_parse_assistant_message_event() {
    let jsonl = r#"{"type":"assistant","message":{"content":[{"type":"text","text":"Hello world"}]}}"#;
    let event = parse_jsonl_event(jsonl).unwrap();

    match event {
        ConversationEvent::AssistantMessage { text } => {
            assert_eq!(text, "Hello world");
        }
        _ => panic!("Expected AssistantMessage variant"),
    }
}

#[test]
fn test_parse_system_init_event() {
    let jsonl = r#"{"type":"system","subtype":"init","cwd":"/path"}"#;
    let event = parse_jsonl_event(jsonl).unwrap();

    match event {
        ConversationEvent::SystemEvent { subtype, details } => {
            assert_eq!(subtype, "init");
            assert!(details.is_some());
        }
        _ => panic!("Expected SystemEvent variant"),
    }
}

#[test]
fn test_parse_result_success_event() {
    let jsonl = r#"{"type":"result","subtype":"success","result":"Done"}"#;
    let event = parse_jsonl_event(jsonl).unwrap();

    match event {
        ConversationEvent::ResultSummary { success, details } => {
            assert!(success);
            assert_eq!(details, "Done");
        }
        _ => panic!("Expected ResultSummary variant"),
    }
}

#[test]
fn test_parse_malformed_json() {
    let jsonl = r#"{"type":"assistant"invalid"#;
    let event = parse_jsonl_event(jsonl);
    assert!(event.is_none());
}

#[test]
fn test_parse_unknown_type() {
    let jsonl = r#"{"type":"unknown_type","data":"something"}"#;
    let event = parse_jsonl_event(jsonl).unwrap();

    match event {
        ConversationEvent::UnknownEvent { raw } => {
            assert!(raw.contains("unknown_type"));
        }
        _ => panic!("Expected UnknownEvent variant"),
    }
}

#[test]
fn test_render_assistant_message_as_plain_text() {
    let event = ConversationEvent::AssistantMessage {
        text: "Test content".to_string(),
    };
    let line = render_event(&event);

    // Verify line contains the text (Line to String conversion for testing)
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
