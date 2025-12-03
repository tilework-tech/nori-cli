# Noridoc: chatgpt

Path: @/codex-rs/chatgpt

### Overview

The `codex-chatgpt` crate provides integration with ChatGPT-specific features, including the `codex apply` command for applying diffs from Codex Cloud tasks and client utilities for the ChatGPT backend.

### How it fits into the larger codebase

ChatGPT crate is used by CLI and cloud tasks:

- **CLI** `codex apply` command uses `run_apply_command()`
- **Cloud tasks** uses `get_task` for fetching task details
- **Handles** ChatGPT-specific API endpoints

### Core Implementation

**Key Files:**

- `apply_command.rs`: Apply diff from cloud task to local repo
- `chatgpt_client.rs`: ChatGPT-specific API client
- `chatgpt_token.rs`: Token management for ChatGPT auth
- `get_task.rs`: Fetch task details from backend

### Things to Know

**Apply Command:**

`codex apply` fetches the latest diff from a Codex Cloud task and applies it locally using `git apply`. This enables synchronization between cloud and local development.

**Authentication:**

Uses ChatGPT OAuth tokens, not direct API keys, for authenticated requests to ChatGPT-specific endpoints.

Created and maintained by Nori.
