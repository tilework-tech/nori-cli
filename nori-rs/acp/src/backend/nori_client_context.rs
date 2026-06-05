//! Curated read-only context exposed by the backend-owned `nori-client` MCP server.
//!
//! This module intentionally serves a fixed catalog. It is not a filesystem API;
//! the ACP agent receives only Nori-owned harness facts and reusable workflows.

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
        text: "You are operating inside Nori CLI over ACP.\n\nnori-client is Nori's backend-owned, harness-side MCP channel for structured client context and Nori-owned live state. The open source implementation lives at https://github.com/tilework-tech/nori-cli.",
    },
    NoriResourceSpec {
        uri: "nori://context/repo",
        name: "Nori CLI repo source map",
        description: "Compact map of the Nori CLI source areas an agent most often needs.",
        text: "Nori CLI source map:\n\n- nori-rs/acp: ACP backend, session runtime, agent connection, nori-client MCP server, transcript handling.\n- nori-rs/tui: terminal UI, slash commands, composer, popups, session rendering.\n- nori-rs/nori-protocol and nori-rs/protocol: client events, ops, session runtime views, shared protocol types.\n- nori-rs/acp/src/config and registry modules: agent registry and config normalization.\n- nori-rs/rmcp-client and nori-rs/mcp-types: external MCP client support and MCP wire types.\n- docs/followups: durable follow-up behavior specs, including nori-client MCP.",
    },
    NoriResourceSpec {
        uri: "nori://help/custom-acp-agent",
        name: "Custom ACP agent registration help",
        description: "Workflow guidance for registering local ACP agents in Nori.",
        text: "Use this workflow when the user wants to register or try a custom ACP agent in Nori.\n\nCheck the requested agent command, add or update the Nori agent config, preserve existing user config, and verify the agent can start an ACP session. Prefer local commands the user explicitly chose. Do not mutate third-party API state.",
    },
    NoriResourceSpec {
        uri: "nori://debug/acp-wire",
        name: "ACP wire debugging help",
        description: "Workflow guidance for debugging ACP protocol traffic and session issues.",
        text: "Use this workflow when diagnosing ACP wire, session, or connectivity behavior.\n\nBuild an evidence timeline from broker logs, worker logs, ACP JSON-RPC messages, backend events, and TUI-visible symptoms. Identify the first divergent boundary before changing code, then verify the fix at the protocol boundary.",
    },
    NoriResourceSpec {
        uri: "nori://source/nori-cli-map",
        name: "Nori CLI source Q&A map",
        description: "Curated source map for answering Nori CLI implementation questions.",
        text: "For Nori CLI implementation questions, start from the relevant source area instead of guessing:\n\n- ACP backend: nori-rs/acp/src/backend\n- ACP connection and capability forwarding: nori-rs/acp/src/connection\n- TUI command handling and rendering: nori-rs/tui/src\n- Shared protocol events and ops: nori-rs/protocol/src and nori-rs/nori-protocol/src\n- MCP client support: nori-rs/rmcp-client\n\nStable public source reference: https://github.com/tilework-tech/nori-cli.",
    },
    NoriResourceSpec {
        uri: "nori://skills/workflows",
        name: "Nori workflow chooser",
        description: "Guidance for choosing Nori-specific workflows and skills.",
        text: "Choose Nori workflows from the task boundary:\n\n- For code changes, use test-driven development and update noridocs when architecture changes.\n- For bugs, use systematic debugging and trace back to the first invalid state.\n- For TUI behavior, verify through focused tests and end-to-end TUI driving when practical.\n- For ACP agent setup, use the custom ACP agent registration workflow.\n- For ACP connectivity, use the ACP wire debugging workflow.",
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
        description: "Debug ACP wire protocol or session connectivity behavior.",
        text: "Read nori://debug/acp-wire, then build an evidence-backed timeline of ACP messages, backend events, and user-visible symptoms. Identify the first divergent boundary before proposing a fix.",
    },
    NoriPromptSpec {
        name: "answer_nori_cli_question",
        description: "Answer a Nori CLI implementation question from curated source context.",
        text: "Read nori://source/nori-cli-map, then inspect the relevant Nori CLI source files before answering. Cite the concrete files or modules you used.",
    },
    NoriPromptSpec {
        name: "choose_nori_workflow",
        description: "Choose the right Nori workflow for the current task.",
        text: "Read nori://skills/workflows, then select the smallest relevant workflow for the user's task. Explain any uncertainty before proceeding.",
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
