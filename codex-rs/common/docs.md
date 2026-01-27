# Noridoc: codex-common

Path: @/codex-rs/common

### Overview

The common crate provides shared utilities used across multiple Nori components. It includes CLI argument types, fuzzy matching, model presets, and configuration summary generation.

### How it fits into the larger codebase

Used by:
- `@/codex-rs/tui/` - for CLI argument parsing, model presets, fuzzy matching
- `@/codex-rs/core/` - (indirectly via config types)
- `@/codex-rs/acp/` - for model presets

### Core Implementation

**CLI Argument Types** (feature-gated by `cli`):
- `ApprovalModeCliArg` - CLI arg for approval mode selection
- `SandboxModeCliArg` - CLI arg for sandbox mode selection
- `CliConfigOverrides` - Command-line overrides for config values

**Fuzzy Matching** (`fuzzy_match.rs`): Provides fuzzy string matching utilities for TUI selection popups.

**Model Presets** (`model_presets.rs`): Defines available model configurations with metadata like:
- Model family (OpenAI, Anthropic, etc.)
- Reasoning effort support
- Upgrade paths between model versions

**Approval Presets** (`approval_presets.rs`): Combines approval mode and sandbox policy into coherent presets.

**Sandbox Summary** (`sandbox_summary.rs`, feature-gated by `sandbox_summary`): Generates human-readable summaries of sandbox policies.

**Elapsed Time** (`elapsed.rs`, feature-gated by `elapsed`): Utilities for formatting elapsed time displays.

### Things to Know

- Most functionality is feature-gated to allow selective inclusion
- The `cli` feature pulls in `clap` derive macros
- Model presets define upgrade paths used by the TUI migration prompts
- The fuzzy matcher is used for file picker and agent picker interfaces

Created and maintained by Nori.
