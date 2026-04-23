# Noridoc: codex-app-server-protocol

Path: @/nori-rs/app-server-protocol

### Overview

This crate defines the JSON-RPC protocol for external app server communication. It provides type definitions for server-to-client messaging and includes code generation utilities for TypeScript bindings.

### How it fits into the larger codebase

Used by:
- `@/nori-rs/core/` - for auth mode definitions
- `@/nori-rs/tui/` - for auth mode handling
- `@/nori-rs/acp/` - for auth types

The crate supports both v1 and v2 protocol versions and exports a JSON-RPC lite implementation.

### Core Implementation

**Common Types** (`protocol/common.rs`): Shared types across protocol versions including `AuthMode` (ApiKey, ChatGPT, None).

**Protocol V1/V2** (`protocol/v1.rs`, `protocol/v2.rs`): Version-specific message types for server communication.

**JSON-RPC** (`jsonrpc_lite.rs`): Lightweight JSON-RPC 2.0 implementation for request/response handling.

**Code Generation** (`export.rs`): Functions to generate JSON schemas and TypeScript type definitions from Rust types.

### Things to Know

- The `AuthMode` enum is used throughout the codebase to determine authentication behavior
- TypeScript bindings can be generated for web client integration
- Protocol versioning allows for backward-compatible evolution

Created and maintained by Nori.
