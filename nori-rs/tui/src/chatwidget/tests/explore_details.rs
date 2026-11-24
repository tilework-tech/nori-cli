use super::*;
use nori_protocol::acp::v1 as acp;
use pretty_assertions::assert_eq;

fn notification(update: acp::SessionUpdate) -> nori_protocol::SessionEvent {
    nori_protocol::SessionEvent::Acp(nori_protocol::AcpEvent::Notification(
        acp::AgentNotification::SessionNotification(acp::SessionNotification::new(
            "session", update,
        )),
    ))
}

fn send(chat: &mut ChatWidget, updates: Vec<acp::SessionUpdate>) {
    let generation = chat.session_generation;
    for update in updates {
        chat.handle_session_event(generation, notification(update));
    }
}

/// A read whose observed line count arrives in a metadata-only update after
/// the call completes, the way the Claude adapter reports its tool results.
#[test]
fn late_read_metadata_reaches_the_explored_row() {
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual();
    let file = chat.config.cwd.join("src/lib.rs");
    let id = acp::ToolCallId::new("read-1");
    send(
        &mut chat,
        vec![
            acp::SessionUpdate::ToolCall(
                acp::ToolCall::new(id.clone(), "Read src/lib.rs")
                    .kind(acp::ToolKind::Read)
                    .status(acp::ToolCallStatus::InProgress)
                    .raw_input(serde_json::json!({ "file_path": file })),
            ),
            acp::SessionUpdate::ToolCallUpdate(acp::ToolCallUpdate::new(
                id.clone(),
                acp::ToolCallUpdateFields::new().status(acp::ToolCallStatus::Completed),
            )),
            acp::SessionUpdate::ToolCallUpdate(
                acp::ToolCallUpdate::new(id, acp::ToolCallUpdateFields::new()).meta(
                    serde_json::json!({
                        "claudeCode": {
                            "toolName": "Read",
                            "toolResponse": {
                                "type": "text",
                                "file": { "startLine": 1, "numLines": 120, "totalLines": 120 }
                            }
                        }
                    })
                    .as_object()
                    .cloned(),
                ),
            ),
            acp::SessionUpdate::AgentMessageChunk(acp::ContentChunk::new(acp::ContentBlock::Text(
                acp::TextContent::new("Done."),
            ))),
        ],
    );

    let history = drain_insert_history(&mut rx)
        .iter()
        .map(|lines| lines_to_single_string(lines))
        .collect::<String>();
    let row = history
        .lines()
        .find(|line| line.contains("Read src/lib.rs"))
        .unwrap_or_default()
        .trim_start_matches([' ', '└', '•'])
        .to_string();
    assert_eq!(row, "Read src/lib.rs · 120 lines", "{history}");
}
