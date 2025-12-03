# Noridoc: lmstudio

Path: @/codex-rs/lmstudio

### Overview

The `codex-lmstudio` crate provides client utilities for communicating with LM Studio, a local LLM application. It handles API communication using LM Studio's OpenAI-compatible endpoint.

### How it fits into the larger codebase

LM Studio is used by common for OSS provider support:

- **Common** `oss` module uses for provider readiness checks
- **Core** model provider configuration references LM Studio
- **Enables** `codex --oss` with LM Studio backend

### Core Implementation

`client.rs` provides:
- API client for LM Studio's OpenAI-compatible API
- Model listing via `/v1/models`
- Health/availability checking

### Things to Know

**Default Port:**

LM Studio runs on port 1234 by default (`DEFAULT_LMSTUDIO_PORT` in core).

**API Compatibility:**

LM Studio exposes an OpenAI-compatible API at `/v1/*`, allowing Codex to use standard chat completions format.

Created and maintained by Nori.
