use crate::client_common::tools::ToolSpec;
use crate::error::Result;
use crate::model_family::ModelFamily;
use codex_apply_patch::APPLY_PATCH_TOOL_INSTRUCTIONS;
use codex_protocol::models::ResponseItem;
use futures::Stream;
use serde::Deserialize;
use serde_json::Value;
use std::borrow::Cow;
use std::collections::HashSet;
use std::ops::Deref;
use std::pin::Pin;
use std::task::Context;
use std::task::Poll;
use tokio::sync::mpsc;

// Placeholder: ModelClient was deleted (HTTP backend removed).
// This stub exists so existing code can compile, but methods panic if called.
// Nori uses ACP backend exclusively and should never instantiate this.
#[derive(Debug, Clone)]
pub struct ModelClient;

impl ModelClient {
    #[allow(dead_code)]
    pub(crate) fn new(
        _config: std::sync::Arc<crate::config::Config>,
        _auth_manager: Option<std::sync::Arc<crate::AuthManager>>,
        _otel: codex_otel::otel_event_manager::OtelEventManager,
        _provider: crate::config::ModelProviderInfo,
        _reasoning_effort: Option<codex_protocol::config_types::ReasoningEffort>,
        _reasoning_summary: codex_protocol::config_types::ReasoningSummary,
        _conversation_id: codex_protocol::ConversationId,
        _session_source: codex_protocol::protocol::SessionSource,
    ) -> Self {
        ModelClient
    }

    #[allow(dead_code)]
    pub(crate) fn get_model(&self) -> String {
        panic!("ModelClient is a stub - HTTP backend was removed. Nori uses ACP backend.")
    }

    #[allow(dead_code)]
    pub(crate) fn get_session_source(&self) -> codex_protocol::protocol::SessionSource {
        panic!("ModelClient is a stub - HTTP backend was removed. Nori uses ACP backend.")
    }

    #[allow(dead_code)]
    pub(crate) fn provider(&self) -> &crate::config::ModelProviderInfo {
        panic!("ModelClient is a stub - HTTP backend was removed. Nori uses ACP backend.")
    }

    #[allow(dead_code)]
    pub(crate) fn config(&self) -> &std::sync::Arc<crate::config::Config> {
        panic!("ModelClient is a stub - HTTP backend was removed. Nori uses ACP backend.")
    }

    #[allow(dead_code)]
    pub(crate) fn get_model_context_window(&self) -> Option<i64> {
        panic!("ModelClient is a stub - HTTP backend was removed. Nori uses ACP backend.")
    }

    #[allow(dead_code)]
    pub(crate) async fn make_request(&self, _prompt: Prompt) -> Result<ResponseStream> {
        panic!("ModelClient is a stub - HTTP backend was removed. Nori uses ACP backend.")
    }

    #[allow(dead_code)]
    pub(crate) fn get_otel_event_manager(
        &self,
    ) -> &codex_otel::otel_event_manager::OtelEventManager {
        panic!("ModelClient is a stub - HTTP backend was removed. Nori uses ACP backend.")
    }

    #[allow(dead_code)]
    pub(crate) fn get_auto_compact_token_limit(&self) -> Option<i64> {
        panic!("ModelClient is a stub - HTTP backend was removed. Nori uses ACP backend.")
    }

    #[allow(dead_code)]
    pub(crate) fn get_model_family(&self) -> &crate::model_family::ModelFamily {
        panic!("ModelClient is a stub - HTTP backend was removed. Nori uses ACP backend.")
    }

    #[allow(dead_code)]
    pub(crate) fn get_provider(&self) -> crate::config::ModelProviderInfo {
        panic!("ModelClient is a stub - HTTP backend was removed. Nori uses ACP backend.")
    }

    #[allow(dead_code)]
    pub(crate) fn get_reasoning_effort(
        &self,
    ) -> Option<codex_protocol::config_types::ReasoningEffort> {
        panic!("ModelClient is a stub - HTTP backend was removed. Nori uses ACP backend.")
    }

    #[allow(dead_code)]
    pub(crate) fn get_reasoning_summary(&self) -> codex_protocol::config_types::ReasoningSummary {
        panic!("ModelClient is a stub - HTTP backend was removed. Nori uses ACP backend.")
    }

    #[allow(dead_code)]
    pub(crate) async fn stream(&self, _prompt: Prompt) -> Result<ResponseStream> {
        panic!("ModelClient is a stub - HTTP backend was removed. Nori uses ACP backend.")
    }

    #[allow(dead_code)]
    pub(crate) async fn compact_conversation_history(
        &self,
        _context: &[codex_protocol::models::ResponseItem],
        _summary_text: &str,
    ) -> Result<Vec<codex_protocol::models::ResponseItem>> {
        panic!("ModelClient is a stub - HTTP backend was removed. Nori uses ACP backend.")
    }
}

/// Response event placeholder (was previously from codex_api)
#[derive(Debug, Clone)]
pub enum ResponseEvent {
    Created,
    OutputItemDone(codex_protocol::models::ResponseItem),
    OutputItemAdded(codex_protocol::models::ResponseItem),
    RateLimits(crate::protocol::RateLimitSnapshot),
    Completed { cancel_reason: Option<String> },
    OutputTextDelta { text: String },
    ReasoningContentDelta { text: String },
}

/// API request payload for a single model turn
#[derive(Default, Debug, Clone)]
pub struct Prompt {
    /// Conversation context input items.
    pub input: Vec<ResponseItem>,

    /// Tools available to the model, including additional tools sourced from
    /// external MCP servers.
    pub(crate) tools: Vec<ToolSpec>,

    /// Whether parallel tool calls are permitted for this prompt.
    pub(crate) parallel_tool_calls: bool,

    /// Optional override for the built-in BASE_INSTRUCTIONS.
    pub base_instructions_override: Option<String>,

    /// Optional the output schema for the model's response.
    pub output_schema: Option<Value>,
}

impl Prompt {
    pub(crate) fn get_full_instructions<'a>(&'a self, model: &'a ModelFamily) -> Cow<'a, str> {
        let base = self
            .base_instructions_override
            .as_deref()
            .unwrap_or(model.base_instructions.deref());
        // When there are no custom instructions, add apply_patch_tool_instructions if:
        // - the model needs special instructions (4.1)
        // AND
        // - there is no apply_patch tool present
        let is_apply_patch_tool_present = self.tools.iter().any(|tool| match tool {
            ToolSpec::Function(f) => f.name == "apply_patch",
            ToolSpec::Freeform(f) => f.name == "apply_patch",
            _ => false,
        });
        if self.base_instructions_override.is_none()
            && model.needs_special_apply_patch_instructions
            && !is_apply_patch_tool_present
        {
            Cow::Owned(format!("{base}\n{APPLY_PATCH_TOOL_INSTRUCTIONS}"))
        } else {
            Cow::Borrowed(base)
        }
    }

    pub(crate) fn get_formatted_input(&self) -> Vec<ResponseItem> {
        let mut input = self.input.clone();

        // when using the *Freeform* apply_patch tool specifically, tool outputs
        // should be structured text, not json. Do NOT reserialize when using
        // the Function tool - note that this differs from the check above for
        // instructions. We declare the result as a named variable for clarity.
        let is_freeform_apply_patch_tool_present = self.tools.iter().any(|tool| match tool {
            ToolSpec::Freeform(f) => f.name == "apply_patch",
            _ => false,
        });
        if is_freeform_apply_patch_tool_present {
            reserialize_shell_outputs(&mut input);
        }

        input
    }
}

fn reserialize_shell_outputs(items: &mut [ResponseItem]) {
    let mut shell_call_ids: HashSet<String> = HashSet::new();

    items.iter_mut().for_each(|item| match item {
        ResponseItem::LocalShellCall { call_id, id, .. } => {
            if let Some(identifier) = call_id.clone().or_else(|| id.clone()) {
                shell_call_ids.insert(identifier);
            }
        }
        ResponseItem::CustomToolCall {
            id: _,
            status: _,
            call_id,
            name,
            input: _,
        } => {
            if name == "apply_patch" {
                shell_call_ids.insert(call_id.clone());
            }
        }
        ResponseItem::CustomToolCallOutput { call_id, output } => {
            if shell_call_ids.remove(call_id)
                && let Some(structured) = parse_structured_shell_output(output)
            {
                *output = structured
            }
        }
        ResponseItem::FunctionCall { name, call_id, .. }
            if is_shell_tool_name(name) || name == "apply_patch" =>
        {
            shell_call_ids.insert(call_id.clone());
        }
        ResponseItem::FunctionCallOutput { call_id, output } => {
            if shell_call_ids.remove(call_id)
                && let Some(structured) = parse_structured_shell_output(&output.content)
            {
                output.content = structured
            }
        }
        _ => {}
    })
}

fn is_shell_tool_name(name: &str) -> bool {
    matches!(name, "shell" | "container.exec")
}

#[derive(Deserialize)]
struct ExecOutputJson {
    output: String,
    metadata: ExecOutputMetadataJson,
}

#[derive(Deserialize)]
struct ExecOutputMetadataJson {
    exit_code: i32,
    duration_seconds: f32,
}

fn parse_structured_shell_output(raw: &str) -> Option<String> {
    let parsed: ExecOutputJson = serde_json::from_str(raw).ok()?;
    Some(build_structured_output(&parsed))
}

fn build_structured_output(parsed: &ExecOutputJson) -> String {
    let mut sections = Vec::new();
    sections.push(format!("Exit code: {}", parsed.metadata.exit_code));
    sections.push(format!(
        "Wall time: {} seconds",
        parsed.metadata.duration_seconds
    ));

    let mut output = parsed.output.clone();
    if let Some((stripped, total_lines)) = strip_total_output_header(&parsed.output) {
        sections.push(format!("Total output lines: {total_lines}"));
        output = stripped.to_string();
    }

    sections.push("Output:".to_string());
    sections.push(output);

    sections.join("\n")
}

fn strip_total_output_header(output: &str) -> Option<(&str, u32)> {
    let after_prefix = output.strip_prefix("Total output lines: ")?;
    let (total_segment, remainder) = after_prefix.split_once('\n')?;
    let total_lines = total_segment.parse::<u32>().ok()?;
    let remainder = remainder.strip_prefix('\n').unwrap_or(remainder);
    Some((remainder, total_lines))
}

pub(crate) mod tools {
    use crate::tools::spec::JsonSchema;
    use serde::Deserialize;
    use serde::Serialize;

    /// When serialized as JSON, this produces a valid "Tool" in the OpenAI
    /// Responses API.
    #[derive(Debug, Clone, Serialize, PartialEq)]
    #[serde(tag = "type")]
    pub(crate) enum ToolSpec {
        #[serde(rename = "function")]
        Function(ResponsesApiTool),
        #[serde(rename = "local_shell")]
        LocalShell {},
        // TODO: Understand why we get an error on web_search although the API docs say it's supported.
        // https://platform.openai.com/docs/guides/tools-web-search?api-mode=responses#:~:text=%7B%20type%3A%20%22web_search%22%20%7D%2C
        #[serde(rename = "web_search")]
        WebSearch {},
        #[serde(rename = "custom")]
        Freeform(FreeformTool),
    }

    impl ToolSpec {
        pub(crate) fn name(&self) -> &str {
            match self {
                ToolSpec::Function(tool) => tool.name.as_str(),
                ToolSpec::LocalShell {} => "local_shell",
                ToolSpec::WebSearch {} => "web_search",
                ToolSpec::Freeform(tool) => tool.name.as_str(),
            }
        }
    }

    #[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
    pub struct FreeformTool {
        pub(crate) name: String,
        pub(crate) description: String,
        pub(crate) format: FreeformToolFormat,
    }

    #[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
    pub struct FreeformToolFormat {
        pub(crate) r#type: String,
        pub(crate) syntax: String,
        pub(crate) definition: String,
    }

    #[derive(Debug, Clone, Serialize, PartialEq)]
    pub struct ResponsesApiTool {
        pub(crate) name: String,
        pub(crate) description: String,
        /// TODO: Validation. When strict is set to true, the JSON schema,
        /// `required` and `additional_properties` must be present. All fields in
        /// `properties` must be present in `required`.
        pub(crate) strict: bool,
        pub(crate) parameters: JsonSchema,
    }
}

pub struct ResponseStream {
    pub(crate) rx_event: mpsc::Receiver<Result<ResponseEvent>>,
}

impl Stream for ResponseStream {
    type Item = Result<ResponseEvent>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.rx_event.poll_recv(cx)
    }
}

#[cfg(test)]
mod tests {
    use crate::model_family::find_family_for_model;
    use pretty_assertions::assert_eq;

    use super::*;

    struct InstructionsTestCase {
        pub slug: &'static str,
        pub expects_apply_patch_instructions: bool,
    }
    #[test]
    fn get_full_instructions_no_user_content() {
        let prompt = Prompt {
            ..Default::default()
        };
        let test_cases = vec![
            InstructionsTestCase {
                slug: "gpt-3.5",
                expects_apply_patch_instructions: true,
            },
            InstructionsTestCase {
                slug: "gpt-4.1",
                expects_apply_patch_instructions: true,
            },
            InstructionsTestCase {
                slug: "gpt-4o",
                expects_apply_patch_instructions: true,
            },
            InstructionsTestCase {
                slug: "gpt-5",
                expects_apply_patch_instructions: true,
            },
            InstructionsTestCase {
                slug: "gpt-5.1",
                expects_apply_patch_instructions: false,
            },
            InstructionsTestCase {
                slug: "codex-mini-latest",
                expects_apply_patch_instructions: true,
            },
            InstructionsTestCase {
                slug: "gpt-oss:120b",
                expects_apply_patch_instructions: false,
            },
            InstructionsTestCase {
                slug: "gpt-5.1-codex",
                expects_apply_patch_instructions: false,
            },
            InstructionsTestCase {
                slug: "gpt-5.1-codex-max",
                expects_apply_patch_instructions: false,
            },
        ];
        for test_case in test_cases {
            let model_family = find_family_for_model(test_case.slug).expect("known model slug");
            let expected = if test_case.expects_apply_patch_instructions {
                format!(
                    "{}\n{}",
                    model_family.clone().base_instructions,
                    APPLY_PATCH_TOOL_INSTRUCTIONS
                )
            } else {
                model_family.clone().base_instructions
            };

            let full = prompt.get_full_instructions(&model_family);
            assert_eq!(full, expected);
        }
    }
}
