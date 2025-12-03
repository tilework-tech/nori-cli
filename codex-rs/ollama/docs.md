# Noridoc: ollama

Path: @/codex-rs/ollama

### Overview

The `codex-ollama` crate provides client utilities for communicating with Ollama, a local LLM server. It handles API communication for model listing and availability checking.

### How it fits into the larger codebase

Ollama is used by common for OSS provider support:

- **Common** `oss` module uses for provider readiness checks
- **Core** model provider configuration references Ollama
- **Enables** `codex --oss` with Ollama backend

### Core Implementation

`client.rs` provides:
- API client for Ollama HTTP endpoints
- Model listing
- Health/availability checking

### Things to Know

**Default Port:**

Ollama runs on port 11434 by default (`DEFAULT_OLLAMA_PORT` in core).

**Model Format:**

Ollama models use format like `llama3.2` without provider prefix. The `gpt-oss:*` prefix is added by Codex configuration.

Created and maintained by Nori.
