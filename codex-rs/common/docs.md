# Noridoc: common

Path: @/codex-rs/common

### Overview

The `codex-common` crate provides shared utilities used across multiple Codex crates. It includes CLI argument types, configuration summary generation, sandbox policy display, fuzzy matching, model presets, and OSS provider utilities.

### How it fits into the larger codebase

Common is a utility dependency for TUI, exec, and CLI:

- **CLI parsing**: `CliConfigOverrides`, `ApprovalModeCliArg`, `SandboxModeCliArg`
- **Config display**: `create_config_summary_entries()` for status displays
- **Model selection**: `model_presets` for available models
- **OSS support**: `oss` module for Ollama/LM Studio integration

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
| `oss` | always | OSS provider utilities |
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
- Claude ACP preset added with display_name "Claude" and description "Anthropic's Claude via Agent Context Protocol" to make Claude model visible in TUI model selection

**Approval Presets:**

`approval_presets` provides named combinations like "full-auto" that set both approval policy and sandbox mode together.

**OSS Provider Utilities:**

The `oss` module handles:
- Provider detection (Ollama vs LM Studio)
- Model availability checking
- Default model selection per provider
- Provider health verification (`ensure_oss_provider_ready()`)

**Format Env Display:**

`format_env_display` provides utilities for formatting environment variables in status displays.

Created and maintained by Nori.
