# Noridoc: common

Path: @/codex-rs/common

### Overview

The `codex-common` crate provides shared utilities used across multiple Codex crates. It includes CLI argument types, configuration summary generation, sandbox policy display, fuzzy matching, model presets, and approval presets.

### How it fits into the larger codebase

Common is a utility dependency for TUI and CLI:

- **CLI parsing**: `CliConfigOverrides`, `ApprovalModeCliArg`, `SandboxModeCliArg`
- **Config display**: `create_config_summary_entries()` for status displays
- **Model selection**: `model_presets` for available models
- **Approval mode display**: `approval_mode_label()` for TUI status line

### Core Implementation

**Modules:**

| Module | Feature | Purpose |
|--------|---------|---------|
| `approval_mode_cli_arg` | `cli` | Clap-compatible approval mode enum |
| `sandbox_mode_cli_arg` | `cli` | Clap-compatible sandbox mode enum |
| `config_override` | `cli` | `-c key=value` override parsing |
| `config_summary` | always | Format config for display |
| `sandbox_summary` | `sandbox_summary` | Format sandbox policy |
| `fuzzy_match` | always | Nucleo-based fuzzy matching |
| `model_presets` | always | Available model definitions |
| `approval_presets` | always | Approval + sandbox combinations |
| `elapsed` | `elapsed` | Duration formatting |

### Things to Know

**Config Overrides:**

`CliConfigOverrides` parses `-c key=value` flags:
```rust
pub struct CliConfigOverrides {
    pub raw_overrides: Vec<String>,
}
// Parses to Vec<(String, toml::Value)>
```

**Fuzzy Matching:**

`fuzzy_match` wraps the `nucleo-matcher` crate for fast fuzzy string matching used in TUI selection popups.

**Model Presets:**

`model_presets` in `@/codex-rs/common/src/model_presets.rs` defines available models by provider with capabilities:
- Default reasoning effort levels (set to Medium for all models)
- Summary generation support
- Tool capabilities
- Claude ACP preset with display_name "Claude" and description "Anthropic's Claude via Agent Context Protocol"

**Approval Presets:**

`approval_presets` provides named combinations like "full-auto" that set both approval policy and sandbox mode together. The module includes:

- `builtin_approval_presets()`: Returns the list of preset combinations (Read Only, Agent, Full Access)
- `approval_mode_label()`: Maps current approval policy and sandbox policy back to a display label for status line display

The `approval_mode_label()` function matches current config against builtin presets using fuzzy sandbox matching (ignores `writable_roots` differences for `WorkspaceWrite` policies). Returns `None` if no preset matches.

**Format Env Display:**

`format_env_display` provides utilities for formatting environment variables in status displays.

Created and maintained by Nori.
