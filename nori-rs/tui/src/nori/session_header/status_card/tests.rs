use super::*;

use nori_harness::ConversationId;
use nori_harness::TranscriptTokenUsage;
use nori_protocol::acp::v1 as acp;
use pretty_assertions::assert_eq;
use std::path::PathBuf;

use crate::history_cell::HistoryCell;
use crate::nori::agent_config_state::AgentConfigState;
use crate::nori::session_header::AgentStatus;
use crate::nori::session_header::AgentStatusHandle;
use crate::nori::session_header::CloudSessionInfo;
use crate::nori::session_header::DisplayMode;
use crate::nori::session_header::NoriSessionHeaderCell;
use crate::nori::session_header::SkillsetStatus;
use crate::nori::session_header::new_nori_status_output;
use crate::nori::token_count::TokenCount;

/// A model with a fixed version and no discovered local context, so view tests
/// never depend on the machine they run on.
fn model(agent: &str, directory: &str) -> StatusViewModel {
    let mut model = StatusViewModel::new(
        AgentStatusHandle::new(AgentStatus::new(agent)),
        PathBuf::from(directory),
    );
    model.version = "test";
    model
}

fn select(
    id: &str,
    name: &str,
    current: &str,
    values: &[(&str, &str)],
) -> acp::SessionConfigOption {
    acp::SessionConfigOption::select(
        id.to_string(),
        name.to_string(),
        current.to_string(),
        values
            .iter()
            .map(|(value, label)| {
                acp::SessionConfigSelectOption::new(value.to_string(), label.to_string())
            })
            .collect::<Vec<_>>(),
    )
}

fn boolean(id: &str, name: &str, current: bool) -> acp::SessionConfigOption {
    acp::SessionConfigOption::new(
        id.to_string(),
        name.to_string(),
        acp::SessionConfigKind::Boolean(acp::SessionConfigBoolean::new(current)),
    )
}

/// The configuration a Claude-style agent advertises: a mode selector, a model
/// selector, a thought-level selector, and a boolean toggle.
fn claude_options(fast_mode: bool) -> Vec<acp::SessionConfigOption> {
    vec![
        select(
            "mode",
            "Mode",
            "plan",
            &[("plan", "Plan"), ("build", "Build")],
        )
        .category(acp::SessionConfigOptionCategory::Mode),
        select(
            "model",
            "Model",
            "opus-5",
            &[("opus-5", "Opus 5"), ("sonnet-5", "Sonnet 5")],
        )
        .category(acp::SessionConfigOptionCategory::Model),
        select("effort", "Effort", "xhigh", &[("xhigh", "xhigh")])
            .category(acp::SessionConfigOptionCategory::ThoughtLevel),
        boolean("fast-mode", "Fast mode", fast_mode),
    ]
}

fn configure(model: &StatusViewModel, agent: &str, options: &[acp::SessionConfigOption]) {
    model.agent.set(AgentStatus::from_config(
        agent,
        &AgentConfigState::from_options(options),
    ));
}

fn render(model: StatusViewModel, display_mode: DisplayMode) -> String {
    let cell = NoriSessionHeaderCell::new(model).with_display_mode(display_mode);
    render_lines(&cell.display_lines(80))
}

fn render_lines(lines: &[Line<'static>]) -> String {
    lines
        .iter()
        .map(|line| {
            line.spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn instruction_files() -> Vec<InstructionFile> {
    vec![
        InstructionFile {
            path: PathBuf::from("/home/user/.claude/CLAUDE.md"),
            active: true,
            token_count: Some(TokenCount {
                count: 1200,
                approximate: true,
            }),
        },
        InstructionFile {
            path: PathBuf::from("/home/user/project/CLAUDE.md"),
            active: true,
            token_count: Some(TokenCount {
                count: 800,
                approximate: true,
            }),
        },
        InstructionFile {
            path: PathBuf::from("/home/user/project/AGENTS.md"),
            active: false,
            token_count: None,
        },
    ]
}

// =========================================================================
// Compact welcome block
// =========================================================================

#[test]
fn compact_agent_row_is_provider_only_before_configuration() {
    let rendered = render(
        model("claude-code", "/home/user/project"),
        DisplayMode::Compact,
    );

    assert!(
        rendered.contains("Agent        Claude\n") || rendered.ends_with("Agent        Claude"),
        "an agent that has advertised nothing must show its provider alone, got:\n{rendered}"
    );
}

#[test]
fn compact_agent_row_orders_model_thought_level_then_advertised_order() {
    let model = model("claude-code", "/home/user/project");
    configure(&model, "claude-code", &claude_options(true));

    let rendered = render(model, DisplayMode::Compact);

    assert!(
        rendered.contains("Agent        Claude · Opus 5 · xhigh · Plan · Fast mode"),
        "agent row must read provider, model, thought level, then advertised order, got:\n{rendered}"
    );
}

#[test]
fn compact_agent_row_drops_toggles_that_are_off() {
    let model = model("claude-code", "/home/user/project");
    configure(&model, "claude-code", &claude_options(false));

    let rendered = render(model, DisplayMode::Compact);

    assert!(
        !rendered.contains("Fast mode"),
        "a boolean toggle reads by presence on the compact row, got:\n{rendered}"
    );
    assert!(rendered.contains("Claude · Opus 5 · xhigh · Plan"));
}

#[test]
#[allow(clippy::disallowed_methods)]
fn only_the_provider_name_carries_colour() {
    let view = model("claude-code", "/home/user/project");
    configure(&view, "claude-code", &claude_options(true));
    let cell = NoriSessionHeaderCell::new(view).with_display_mode(DisplayMode::Compact);

    let lines = cell.display_lines(80);
    let agent_row = lines
        .iter()
        .find(|line| line.to_string().contains("Claude"))
        .expect("agent row");

    let provider = agent_row
        .spans
        .iter()
        .find(|span| span.content.as_ref() == "Claude")
        .expect("provider span");
    assert_eq!(provider.style.fg, Some(Color::Rgb(255, 158, 100)));

    for span in &agent_row.spans {
        if span.content.as_ref() == "Claude" {
            continue;
        }
        assert_eq!(
            span.style.fg, None,
            "only the provider name may be coloured, found colour on {:?}",
            span.content
        );
    }
}

#[test]
fn compact_block_uses_the_prompt_accent_and_plain_labels() {
    let cell = NoriSessionHeaderCell::new(model("claude-code", "/home/user/project"))
        .with_display_mode(DisplayMode::Compact);

    let lines = cell.display_lines(80);
    assert_eq!(lines[0].spans[0].content, "  › ");
    assert_eq!(lines[0].spans[0].style.fg, Some(Color::Green));

    let system = lines
        .iter()
        .find(|line| line.to_string().contains("System"))
        .expect("System row");
    assert!(system.to_string().starts_with("  System       "));
    assert!(!system.to_string().contains(':'));
}

#[test]
fn compact_block_summarizes_system_context() {
    let mut view = model("claude-code", "/home/user/project");
    view.approval_mode_label = Some("Agent".to_string());
    view.skillset.name = Some("senior-swe".to_string());

    let rendered = render(view, DisplayMode::Compact);

    assert!(
        rendered.contains("System       /home/user/project · Agent approvals · senior-swe"),
        "compact System row must summarize location, approvals, and skillset, got:\n{rendered}"
    );
}

#[test]
fn compact_block_omits_the_instruction_outline() {
    let mut view = model("claude-code", "/home/user/project");
    view.instruction_files = instruction_files();

    let rendered = render(view, DisplayMode::Compact);

    assert!(
        !rendered.contains("CLAUDE.md") && !rendered.contains("AGENTS.md"),
        "compact mode omits the instruction outline, got:\n{rendered}"
    );
    assert!(
        !rendered.contains("~1,200 tokens") && !rendered.contains("~800 tokens"),
        "compact mode omits instruction token counts, got:\n{rendered}"
    );
}

#[test]
fn compact_block_on_a_cloud_session_shows_the_session_not_the_local_cwd() {
    let mut view = model("claude-code", "/home/user/local-only-checkout");
    view.skillset.name = Some("senior-swe".to_string());
    view.cloud_session = Some(CloudSessionInfo {
        id: "nori-fast-kazunoko-aac8".to_string(),
        title: Some("Fix login flakes".to_string()),
    });

    let rendered = render(view, DisplayMode::Compact);

    assert!(
        rendered.contains("nori-fast-kazunoko-aac8"),
        "cloud welcome card must name the cloud session id, got:\n{rendered}"
    );
    assert!(
        !rendered.contains("local-only-checkout"),
        "cloud welcome card must not present the local cwd as the session directory, got:\n{rendered}"
    );
}

#[test]
fn compact_mode_snapshot() {
    let mut view = model("claude-code", "/home/user/project");
    view.version = "0.1.0";
    view.approval_mode_label = Some("Agent".to_string());
    view.skillset.name = Some("senior-swe".to_string());
    view.instruction_files = instruction_files();
    configure(&view, "claude-code", &claude_options(true));

    insta::assert_snapshot!(render(view, DisplayMode::Compact));
}

// =========================================================================
// Full `/status` card
// =========================================================================

#[test]
fn full_card_lists_every_advertised_option_in_the_agent_block() {
    let view = model("claude-code", "/home/user/project");
    configure(&view, "claude-code", &claude_options(false));

    let rendered = render(view, DisplayMode::Full);

    assert!(
        rendered.contains("  Agent        Claude\n"),
        "the agent row names the provider alone, got:\n{rendered}"
    );
    for row in [
        "               Model      Opus 5",
        "               Effort     xhigh",
        "               Mode       Plan",
        "               Fast mode  Off",
    ] {
        assert!(
            rendered.contains(row),
            "agent block must contain {row:?}, got:\n{rendered}"
        );
    }
}

#[test]
fn full_card_is_provider_only_before_configuration() {
    let rendered = render(
        model("claude-code", "/home/user/project"),
        DisplayMode::Full,
    );

    assert!(
        rendered.contains("Agent        Claude"),
        "status must name the provider, got:\n{rendered}"
    );
    assert!(
        !rendered.contains("Model"),
        "status must not invent configuration rows, got:\n{rendered}"
    );
}

#[test]
fn full_card_shows_session_identity_rows() {
    let mut view = model("claude-code", "/tmp/project");
    view.conversation_id = ConversationId::from_string("11111111-1111-1111-1111-111111111111").ok();
    view.forked_from = ConversationId::from_string("22222222-2222-2222-2222-222222222222").ok();
    view.session_title = Some("Fix login flakes".to_string());
    view.prompt_summary = Some("Fix authentication bug".to_string());
    view.approval_mode_label = Some("Agent".to_string());

    let rendered = render(view, DisplayMode::Full);

    for expected in [
        "Directory    /tmp/project",
        "Session ID   11111111-1111-1111-1111-111111111111",
        "Forked from  22222222-2222-2222-2222-222222222222",
        "Title        Fix login flakes",
        "Summary      Fix authentication bug",
        "Approvals    Agent",
    ] {
        assert!(
            rendered.contains(expected),
            "status must contain {expected:?}, got:\n{rendered}"
        );
    }
}

#[test]
fn full_card_truncates_a_long_summary() {
    let mut view = model("claude-code", "/tmp/project");
    view.prompt_summary = Some(
        "This is an extremely long task summary that goes on and on describing what the user wants"
            .to_string(),
    );

    let rendered = render(view, DisplayMode::Full);

    assert!(
        rendered.contains("This is an extremely long task summary that goe..."),
        "long summaries are truncated to one row, got:\n{rendered}"
    );
}

#[test]
fn full_card_renders_the_git_row() {
    let mut view = model("claude-code", "/tmp/project");
    view.git = GitStatus {
        branch: Some("feat/status-card".to_string()),
        is_worktree: true,
        worktree_name: Some("good-ash-20260205".to_string()),
        lines_added: Some(42),
        lines_removed: Some(7),
        has_untracked: true,
    };

    let rendered = render(view, DisplayMode::Full);

    assert!(
        rendered.contains("Git          ⎇ feat/status-card (worktree: good-ash-20260205) +42 -7 !"),
        "git row must show branch, worktree, stats, and the untracked marker, got:\n{rendered}"
    );
}

#[test]
fn full_card_renders_a_single_consolidated_context_row() {
    let mut view = model("claude-code", "/tmp/project");
    view.context = ContextStatus {
        tokens: Some(43_000),
        window_tokens: Some(272_000),
        // 16% used of the window is rendered as 84% left.
        percent_used: Some(16),
    };

    let rendered = render(view, DisplayMode::Full);

    assert!(
        rendered.contains("Context      84% left (43.0K used / 272K)"),
        "context row must use the codex-style percent-left form, got:\n{rendered}"
    );
    assert_eq!(
        rendered.matches("Context").count(),
        1,
        "context should render as a single row, got:\n{rendered}"
    );
}

#[test]
fn full_card_renders_context_without_a_token_breakdown() {
    let mut view = model("claude-code", "/tmp/project");
    view.context = ContextStatus {
        percent_used: Some(42),
        ..ContextStatus::default()
    };

    let rendered = render(view, DisplayMode::Full);

    // 42% used is rendered as its complement: 58% left.
    assert!(
        rendered.contains("Context      58% left"),
        "context percentage renders without token counts, got:\n{rendered}"
    );
}

#[test]
fn full_card_renders_the_tokens_row_only_when_tokens_were_used() {
    let mut view = model("claude-code", "/tmp/project");
    view.token_breakdown = Some(TranscriptTokenUsage {
        input_tokens: 45_000,
        output_tokens: 78_000,
        cached_tokens: 32_000,
        last_context_tokens: None,
    });

    let rendered = render(view, DisplayMode::Full);
    assert!(
        rendered.contains("Tokens       123K total (32.0K cached)"),
        "tokens row must show the total and cached counts, got:\n{rendered}"
    );

    let mut empty = model("claude-code", "/tmp/project");
    empty.token_breakdown = Some(TranscriptTokenUsage {
        input_tokens: 0,
        output_tokens: 0,
        cached_tokens: 0,
        last_context_tokens: None,
    });
    let rendered = render(empty, DisplayMode::Full);
    assert!(
        !rendered.contains("Tokens"),
        "an unused session has no tokens row, got:\n{rendered}"
    );
}

#[test]
fn full_card_skillset_row_shows_the_name_and_version() {
    let mut view = model("claude-code", "/tmp/test");
    view.skillset = SkillsetStatus {
        name: Some("senior-swe".to_string()),
        version: Some("1.2.3".to_string()),
        version_source: Some(NoriVersionSource::Skillsets),
    };

    let rendered = render(view, DisplayMode::Full);

    assert!(
        rendered.contains("Skillset     senior-swe (Skillsets v1.2.3)"),
        "skillset row must append the skillsets version, got:\n{rendered}"
    );
}

#[test]
fn full_card_skillset_row_reads_none_when_unset() {
    let rendered = render(model("claude-code", "/tmp/test"), DisplayMode::Full);

    assert!(
        rendered.contains("Skillset     (none)"),
        "an unset skillset reads (none), got:\n{rendered}"
    );
}

#[test]
fn full_card_outlines_active_instruction_files_with_counts() {
    let mut view = model("claude-code", "/home/user/project");
    view.instruction_files = instruction_files();

    let rendered = render(view, DisplayMode::Full);

    assert!(
        !rendered.contains("AGENTS.md"),
        "only files the active agent loads belong in the outline, got:\n{rendered}"
    );
    for expected in [
        "Instructions",
        "~1,200 tokens",
        "~800 tokens",
        "2 files · ~2,000 tokens",
    ] {
        assert!(
            rendered.contains(expected),
            "instruction outline must contain {expected:?}, got:\n{rendered}"
        );
    }
}

#[test]
fn full_card_shows_exact_token_counts_without_a_tilde() {
    let mut view = model("codex", "/home/user/project");
    view.instruction_files = vec![InstructionFile {
        path: PathBuf::from("/home/user/project/AGENTS.md"),
        active: true,
        token_count: Some(TokenCount {
            count: 750,
            approximate: false,
        }),
    }];

    let rendered = render(view, DisplayMode::Full);

    assert!(
        rendered.contains("750 tokens"),
        "exact counts render as-is, got:\n{rendered}"
    );
    assert!(
        !rendered.contains("~750"),
        "an exact count must not be marked approximate, got:\n{rendered}"
    );
}

#[test]
fn full_card_on_a_cloud_session_shows_the_broker_title_and_skips_local_instructions() {
    let mut view = model("claude-code", "/home/user/local-only-checkout");
    view.conversation_id = ConversationId::from_string("aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee").ok();
    view.instruction_files = instruction_files();
    view.cloud_session = Some(CloudSessionInfo {
        id: "nori-fast-kazunoko-aac8".to_string(),
        title: Some("Fix login flakes".to_string()),
    });

    let rendered = render(view, DisplayMode::Full);

    assert!(
        rendered.contains("Session ID   aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee (Fix login flakes)"),
        "cloud status names the conversation id and broker title, got:\n{rendered}"
    );
    assert!(
        rendered.contains("Directory") && rendered.contains("local-only-checkout"),
        "cloud status still shows the local directory, got:\n{rendered}"
    );
    assert!(
        !rendered.contains("Instructions"),
        "cloud status must not describe locally discovered instruction files, got:\n{rendered}"
    );
}

#[test]
fn status_output_echoes_the_command_and_the_nori_heading() {
    let status_output = new_nori_status_output(model("claude-code", "/tmp/project"));

    let rendered = render_lines(&status_output.display_lines(80));

    assert!(
        rendered.starts_with("/status"),
        "status output echoes the command, got:\n{rendered}"
    );
    assert!(
        rendered.contains("› Nori CLI vtest"),
        "status output carries the Nori heading, got:\n{rendered}"
    );
}

#[test]
fn status_card_full_snapshot() {
    let mut view = model("claude-code", "/home/user/project");
    view.version = "0.1.0";
    view.approval_mode_label = Some("Agent".to_string());
    view.prompt_summary = Some("Fix auth bug".to_string());
    view.session_title = Some("Fix login flakes".to_string());
    view.conversation_id = ConversationId::from_string("11111111-2222-3333-4444-555555555555").ok();
    view.forked_from = ConversationId::from_string("22222222-2222-2222-2222-222222222222").ok();
    view.skillset = SkillsetStatus {
        name: Some("senior-swe".to_string()),
        version: Some("1.2.3".to_string()),
        version_source: Some(NoriVersionSource::Skillsets),
    };
    view.git = GitStatus {
        branch: Some("main".to_string()),
        has_untracked: true,
        ..GitStatus::default()
    };
    view.context = ContextStatus {
        tokens: Some(164_000),
        window_tokens: Some(1_000_000),
        percent_used: Some(16),
    };
    view.token_breakdown = Some(TranscriptTokenUsage {
        input_tokens: 91_000,
        output_tokens: 32_000,
        cached_tokens: 32_000,
        last_context_tokens: None,
    });
    view.instruction_files = vec![
        InstructionFile {
            path: PathBuf::from("/home/user/.claude/CLAUDE.md"),
            active: true,
            token_count: Some(TokenCount {
                count: 2950,
                approximate: true,
            }),
        },
        InstructionFile {
            path: PathBuf::from("/home/user/project/CLAUDE.md"),
            active: true,
            token_count: Some(TokenCount {
                count: 2164,
                approximate: true,
            }),
        },
    ];
    configure(&view, "claude-code", &claude_options(false));

    insta::assert_snapshot!(render(view, DisplayMode::Full));
}

#[test]
fn truncate_summary_handles_multibyte_utf8() {
    // Multi-byte chars: each CJK character is 3 bytes in UTF-8. Byte slicing
    // would split a multi-byte sequence and panic.
    let summary = "修复认证错误的问题在这里需要更多的文字来触发截断";
    let result = truncate_summary(summary, 10);

    assert!(
        result.ends_with("..."),
        "should end with an ellipsis, got: {result}"
    );
    assert_eq!(result.chars().count(), 10);
}
