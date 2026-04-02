use codex_protocol::models::ContentItem;

pub const SUMMARIZATION_PROMPT: &str = include_str!("../templates/compact/prompt.md");
pub const SUMMARY_PREFIX: &str = include_str!("../templates/compact/summary_prefix.md");

pub fn content_items_to_text(content: &[ContentItem]) -> Option<String> {
    let mut pieces = Vec::new();
    for item in content {
        match item {
            ContentItem::InputText { text } | ContentItem::OutputText { text } => {
                if !text.is_empty() {
                    pieces.push(text.as_str());
                }
            }
            ContentItem::InputImage { .. } => {}
        }
    }
    if pieces.is_empty() {
        None
    } else {
        Some(pieces.join("\n"))
    }
}

#[cfg(test)]
mod tests {

    use super::*;
    use crate::truncate::TruncationPolicy;
    use crate::truncate::approx_token_count;
    use crate::truncate::truncate_text;
    use codex_protocol::items::TurnItem;
    use codex_protocol::models::ResponseItem;
    use pretty_assertions::assert_eq;

    const COMPACT_USER_MESSAGE_MAX_TOKENS: usize = 20_000;

    fn collect_user_messages(items: &[ResponseItem]) -> Vec<String> {
        items
            .iter()
            .filter_map(|item| match crate::event_mapping::parse_turn_item(item) {
                Some(TurnItem::UserMessage(user)) => {
                    if is_summary_message(&user.message()) {
                        None
                    } else {
                        Some(user.message())
                    }
                }
                _ => None,
            })
            .collect()
    }

    fn is_summary_message(message: &str) -> bool {
        message.starts_with(format!("{SUMMARY_PREFIX}\n").as_str())
    }

    fn build_compacted_history(
        initial_context: Vec<ResponseItem>,
        user_messages: &[String],
        summary_text: &str,
    ) -> Vec<ResponseItem> {
        build_compacted_history_with_limit(
            initial_context,
            user_messages,
            summary_text,
            COMPACT_USER_MESSAGE_MAX_TOKENS,
        )
    }

    fn build_compacted_history_with_limit(
        mut history: Vec<ResponseItem>,
        user_messages: &[String],
        summary_text: &str,
        max_tokens: usize,
    ) -> Vec<ResponseItem> {
        let mut selected_messages: Vec<String> = Vec::new();
        if max_tokens > 0 {
            let mut remaining = max_tokens;
            for message in user_messages.iter().rev() {
                if remaining == 0 {
                    break;
                }
                let tokens = approx_token_count(message);
                if tokens <= remaining {
                    selected_messages.push(message.clone());
                    remaining = remaining.saturating_sub(tokens);
                } else {
                    let truncated = truncate_text(message, TruncationPolicy::Tokens(remaining));
                    selected_messages.push(truncated);
                    break;
                }
            }
            selected_messages.reverse();
        }

        for message in &selected_messages {
            history.push(ResponseItem::Message {
                id: None,
                role: "user".to_string(),
                content: vec![ContentItem::InputText {
                    text: message.clone(),
                }],
            });
        }

        let summary_text = if summary_text.is_empty() {
            "(no summary available)".to_string()
        } else {
            summary_text.to_string()
        };

        history.push(ResponseItem::Message {
            id: None,
            role: "user".to_string(),
            content: vec![ContentItem::InputText { text: summary_text }],
        });

        history
    }

    #[test]
    fn content_items_to_text_joins_non_empty_segments() {
        let items = vec![
            ContentItem::InputText {
                text: "hello".to_string(),
            },
            ContentItem::OutputText {
                text: String::new(),
            },
            ContentItem::OutputText {
                text: "world".to_string(),
            },
        ];

        let joined = content_items_to_text(&items);

        assert_eq!(Some("hello\nworld".to_string()), joined);
    }

    #[test]
    fn content_items_to_text_ignores_image_only_content() {
        let items = vec![ContentItem::InputImage {
            image_url: "file://image.png".to_string(),
        }];

        let joined = content_items_to_text(&items);

        assert_eq!(None, joined);
    }

    #[test]
    fn collect_user_messages_extracts_user_text_only() {
        let items = vec![
            ResponseItem::Message {
                id: Some("assistant".to_string()),
                role: "assistant".to_string(),
                content: vec![ContentItem::OutputText {
                    text: "ignored".to_string(),
                }],
            },
            ResponseItem::Message {
                id: Some("user".to_string()),
                role: "user".to_string(),
                content: vec![ContentItem::InputText {
                    text: "first".to_string(),
                }],
            },
            ResponseItem::Other,
        ];

        let collected = collect_user_messages(&items);

        assert_eq!(vec!["first".to_string()], collected);
    }

    #[test]
    fn collect_user_messages_filters_session_prefix_entries() {
        let items = vec![
            ResponseItem::Message {
                id: None,
                role: "user".to_string(),
                content: vec![ContentItem::InputText {
                    text: "# AGENTS.md instructions for project\n\n<INSTRUCTIONS>\ndo things\n</INSTRUCTIONS>"
                        .to_string(),
                }],
            },
            ResponseItem::Message {
                id: None,
                role: "user".to_string(),
                content: vec![ContentItem::InputText {
                    text: "<ENVIRONMENT_CONTEXT>cwd=/tmp</ENVIRONMENT_CONTEXT>".to_string(),
                }],
            },
            ResponseItem::Message {
                id: None,
                role: "user".to_string(),
                content: vec![ContentItem::InputText {
                    text: "real user message".to_string(),
                }],
            },
        ];

        let collected = collect_user_messages(&items);

        assert_eq!(vec!["real user message".to_string()], collected);
    }

    #[test]
    fn build_token_limited_compacted_history_truncates_overlong_user_messages() {
        let max_tokens = 16;
        let big = "word ".repeat(200);
        let history = build_compacted_history_with_limit(
            Vec::new(),
            std::slice::from_ref(&big),
            "SUMMARY",
            max_tokens,
        );
        assert_eq!(history.len(), 2);

        let truncated_message = &history[0];
        let summary_message = &history[1];

        let truncated_text = match truncated_message {
            ResponseItem::Message { role, content, .. } if role == "user" => {
                content_items_to_text(content).unwrap_or_default()
            }
            other => panic!("unexpected item in history: {other:?}"),
        };

        assert!(
            truncated_text.contains("tokens truncated"),
            "expected truncation marker in truncated user message"
        );
        assert!(
            !truncated_text.contains(&big),
            "truncated user message should not include the full oversized user text"
        );

        let summary_text = match summary_message {
            ResponseItem::Message { role, content, .. } if role == "user" => {
                content_items_to_text(content).unwrap_or_default()
            }
            other => panic!("unexpected item in history: {other:?}"),
        };
        assert_eq!(summary_text, "SUMMARY");
    }

    #[test]
    fn build_token_limited_compacted_history_appends_summary_message() {
        let initial_context: Vec<ResponseItem> = Vec::new();
        let user_messages = vec!["first user message".to_string()];
        let summary_text = "summary text";

        let history = build_compacted_history(initial_context, &user_messages, summary_text);
        assert!(
            !history.is_empty(),
            "expected compacted history to include summary"
        );

        let last = history.last().expect("history should have a summary entry");
        let summary = match last {
            ResponseItem::Message { role, content, .. } if role == "user" => {
                content_items_to_text(content).unwrap_or_default()
            }
            other => panic!("expected summary message, found {other:?}"),
        };
        assert_eq!(summary, summary_text);
    }
}
