//! Curated read-only context exposed by the backend-owned `nori-client` MCP server.
//!
//! This module intentionally serves a fixed catalog. It is not a filesystem API;
//! the ACP agent receives only Nori-owned harness facts and source guidance.

use rmcp::ErrorData as McpError;
use rmcp::model::AnnotateAble;
use rmcp::model::GetPromptResult;
use rmcp::model::ListPromptsResult;
use rmcp::model::ListResourcesResult;
use rmcp::model::Prompt;
use rmcp::model::PromptMessage;
use rmcp::model::PromptMessageRole;
use rmcp::model::RawResource;
use rmcp::model::ReadResourceResult;
use rmcp::model::Resource;
use rmcp::model::ResourceContents;

const TEXT_MARKDOWN: &str = "text/markdown";

struct NoriResourceSpec {
    uri: &'static str,
    name: &'static str,
    description: &'static str,
    text: &'static str,
}

struct NoriPromptSpec {
    name: &'static str,
    description: &'static str,
    text: &'static str,
}

const RESOURCE_SPECS: &[NoriResourceSpec] = &[
    NoriResourceSpec {
        uri: "nori://context/cli",
        name: "Nori CLI operating context",
        description: "Concise facts about the current Nori CLI ACP harness.",
        text: "You are running inside Nori CLI, the terminal harness for agent sessions and user interaction. ACP is the JSON-RPC wire protocol between Nori and the underlying agent, and implementation details live in the Nori CLI source repo at https://github.com/tilework-tech/nori-cli. nori-client is an internal-only, backend-owned MCP server for common harness-owned tools and curated context; it is not a user-configured MCP server.",
    },
    NoriResourceSpec {
        uri: "nori://context/repo",
        name: "Answering with Nori CLI source",
        description: "Compact source map for answering Nori CLI implementation questions.",
        text: "Use this resource for answering Nori CLI questions from source instead of guessing.\n\nStart in the smallest relevant area:\n\n- nori-rs/acp: ACP backend, session runtime, agent connection, nori-client MCP server, transcript handling.\n- nori-rs/tui: terminal UI, slash commands, composer, popups, session rendering.\n- nori-rs/nori-protocol and nori-rs/protocol: client events, ops, session runtime views, shared protocol types.\n- nori-rs/acp/src/config: Nori config, custom agent definitions, and agent registry normalization.\n- nori-rs/rmcp-client and nori-rs/mcp-types: external MCP client support and MCP wire types.\n- docs/followups: durable behavior specs and follow-up design notes.\n\nCite the concrete source files you used.",
    },
    NoriResourceSpec {
        uri: "nori://help/custom-acp-agent",
        name: "Custom ACP agent registration help",
        description: "Minimal instructions for registering local ACP agents in Nori.",
        text: "Register custom ACP agents in `~/.nori/cli/config.toml` with `[[agents]]` entries. Each ACP agent communicates over JSON-RPC 2.0 on stdin/stdout. Each entry needs `name`, `slug`, and exactly one distribution block.\n\nFor a local command:\n\n```toml\n[[agents]]\nname = \"ElizACP\"\nslug = \"elizacp\"\n\n[agents.distribution.local]\ncommand = \"elizacp\"\nargs = [\"acp\"]\nenv = { \"EXAMPLE_ENV\" = \"value\" }\n```\n\nPackage-manager distributions use the same shape under `[agents.distribution.npx]`, `[agents.distribution.bunx]`, `[agents.distribution.pipx]`, or `[agents.distribution.uvx]` with `package` and optional `args`. Preserve existing config entries, use `/agent` to switch to the new agent, and verify it starts an ACP session.",
    },
    NoriResourceSpec {
        uri: "nori://debug/acp-wire",
        name: "ACP wire debugging help",
        description: "Minimal instructions for debugging underlying agents with ACP JSONL logs.",
        text: "Enable ACP wire recording when you need to debug the JSON-RPC messages between Nori and the underlying agent.\n\nIn `~/.nori/cli/config.toml`:\n\n```toml\n[acp_proxy]\nenabled = true\n```\n\nYou can also toggle recording for future agent subprocesses from the `/agent` picker with `Shift-Tab`. New ACP subprocesses write JSONL files under `$NORI_HOME/acp-wire` or `~/.nori/cli/acp-wire`; filenames are `{timestamp_ms}-{child_pid}-{agent_slug}.jsonl`. Each record includes `ts_ms`, `direction`, `agent`, `child_pid`, and either parsed `message` or `raw_line` plus `parse_error`. Debug by sorting records by timestamp, following `client_to_agent` and `agent_to_client` pairs, and identifying the first request, response, or notification where the agent and Nori diverge.",
    },
];

const PROMPT_SPECS: &[NoriPromptSpec] = &[
    NoriPromptSpec {
        name: "register_custom_acp_agent",
        description: "Register or try a custom ACP agent in Nori.",
        text: "Read nori://help/custom-acp-agent, then help the user register or verify the requested custom ACP agent. Preserve existing configuration and verify the agent can start over ACP.",
    },
    NoriPromptSpec {
        name: "debug_acp_wire_protocol",
        description: "Debug an underlying ACP agent with wire protocol JSONL.",
        text: "Read nori://debug/acp-wire, then use the ACP wire JSONL to build a request/response timeline. Identify the first divergent protocol message before proposing a fix.",
    },
    NoriPromptSpec {
        name: "answer_nori_cli_question",
        description: "Answer a Nori CLI implementation question from curated source context.",
        text: "Read nori://context/repo, then inspect the relevant Nori CLI source files before answering. Cite the concrete files or modules you used.",
    },
];

pub(super) fn list_resources() -> ListResourcesResult {
    ListResourcesResult {
        meta: None,
        resources: RESOURCE_SPECS.iter().map(resource_from_spec).collect(),
        next_cursor: None,
    }
}

pub(super) fn read_resource(uri: String) -> Result<ReadResourceResult, McpError> {
    let Some(resource) = RESOURCE_SPECS.iter().find(|resource| resource.uri == uri) else {
        return Err(McpError::resource_not_found(
            "Nori resource not found",
            None,
        ));
    };
    Ok(ReadResourceResult {
        contents: vec![ResourceContents::TextResourceContents {
            uri: resource.uri.to_string(),
            mime_type: Some(TEXT_MARKDOWN.to_string()),
            text: resource.text.to_string(),
            meta: None,
        }],
    })
}

pub(super) fn list_prompts() -> ListPromptsResult {
    ListPromptsResult {
        meta: None,
        prompts: PROMPT_SPECS.iter().map(prompt_from_spec).collect(),
        next_cursor: None,
    }
}

pub(super) fn get_prompt(name: String) -> Result<GetPromptResult, McpError> {
    let Some(prompt) = PROMPT_SPECS.iter().find(|prompt| prompt.name == name) else {
        return Err(McpError::invalid_params("Nori prompt not found", None));
    };
    Ok(GetPromptResult {
        description: Some(prompt.description.to_string()),
        messages: vec![PromptMessage::new_text(
            PromptMessageRole::User,
            prompt.text,
        )],
    })
}

fn resource_from_spec(spec: &NoriResourceSpec) -> Resource {
    RawResource {
        uri: spec.uri.to_string(),
        name: spec.name.to_string(),
        title: None,
        description: Some(spec.description.to_string()),
        mime_type: Some(TEXT_MARKDOWN.to_string()),
        size: None,
        icons: None,
        meta: None,
    }
    .no_annotation()
}

fn prompt_from_spec(spec: &NoriPromptSpec) -> Prompt {
    Prompt::new(spec.name, Some(spec.description), None)
}
