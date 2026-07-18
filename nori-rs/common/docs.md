# Noridoc: codex-common

Path: @/nori-rs/common

### Overview

The common crate provides small shared utilities for the Nori CLI and TUI, including CLI argument types, fuzzy matching, approval presets, and sandbox summaries.

### How it fits into the larger codebase

Used by:
- `@/nori-rs/tui/` - for CLI argument parsing, approval presets, sandbox summaries, and fuzzy matching
- `@/nori-rs/cli/` - for the shared raw `-c` override argument surface

### Core Implementation

**CLI Argument Types** (feature-gated by `cli`):
- `ApprovalModeCliArg` - CLI arg for approval mode selection
- `SandboxModeCliArg` - CLI arg for sandbox mode selection
- `CliConfigOverrides` - Command-line overrides for config values

**Fuzzy Matching** (`fuzzy_match.rs`): Provides fuzzy string matching utilities for TUI selection popups.

**Model Presets** (`model_presets.rs`): Defines inherited model compatibility metadata such as:
- Display metadata and model slugs
- Supported and default reasoning effort

These inherited presets are compatibility data; Nori's runtime agent and default-model choices come from `nori-config` and ACP session configuration rather than a migration flow in the TUI.

**Approval Presets** (`approval_presets.rs`): Combines approval mode and sandbox policy into coherent presets.

**Sandbox Summary** (`sandbox_summary.rs`, feature-gated by `sandbox_summary`): Generates human-readable summaries of sandbox policies.

**Elapsed Time** (`elapsed.rs`, feature-gated by `elapsed`): Utilities for formatting elapsed time displays.

### Things to Know

- Most functionality is feature-gated to allow selective inclusion
- The `cli` feature pulls in `clap` derive macros
- The fuzzy matcher is used for file picker and agent picker interfaces

Created and maintained by Nori.
