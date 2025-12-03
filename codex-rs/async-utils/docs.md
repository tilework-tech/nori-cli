# Noridoc: async-utils

Path: @/codex-rs/async-utils

### Overview

The `codex-async-utils` crate provides async utility functions and types used across the Codex workspace. It contains helpers for common async patterns in tokio-based code.

### How it fits into the larger codebase

Async utils is a shared dependency for async code patterns used throughout the workspace.

### Core Implementation

`lib.rs` exports async utility functions and types for:
- Task coordination
- Async patterns
- Error handling in async contexts

### Things to Know

Designed with minimal dependencies as a lightweight utility crate.

Created and maintained by Nori.
